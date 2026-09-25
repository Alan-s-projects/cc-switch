//! Grok Build (Grok CLI) 会话用量追踪
//!
//! 从 `~/.grok/{sessions,archived_sessions}/<enc-cwd>/<session-id>/updates.jsonl`
//! 的 `turn_completed` 事件中提取用量，写入 proxy_request_logs，实现官方
//! OAuth 直连态（无代理数据）下的用量统计。
//!
//! ## 数据流
//! ```text
//! updates.jsonl（逐轮 turn_completed） → 沉降窗/接管守卫 → 费用计算 → proxy_request_logs
//! ```
//!
//! ## 事件口径（2026-07-23 单进程双 prompt 实测 + CLI 二进制逆向双重确证）
//! - `sessionUpdate == "turn_completed"` 事件的 usage 是【该 user prompt 一轮
//!   的独立总量】：轮内跨 inference loop 累加（`modelCalls`/`numTurns` = 本轮
//!   loop 数），下一轮从零起算。【不是】进程或会话累计——进程累计走 CLI 内
//!   另一条独立通道（`GetSessionUsage`，"since start or last resume"），不落
//!   updates.jsonl。🔴 勿改回相邻事件差分：那是把每轮总量误当累计快照，会把
//!   第二轮记成两轮之差造成巨量漏记（曾犯，实测单进程双 prompt 证伪）。
//! - 逐事件按面值入账即为正确的逐轮记录；两轮数值完全相同 = 两笔真实用量，
//!   照常都入账。
//! - `reasoningTokens` ⊂ `outputTokens`（totalTokens = input + output，且
//!   costUsdTicks 反推 output 未加计 reasoning），不参与计费。
//! - `costUsdTicks`（1 tick = 1e-10 USD）是 CLI 自报的本轮精确成本，6 个实测
//!   样本与本地定价 grok-4.5-build 2/6/0.30 分毫不差。**有自报且完整时
//!   total_cost 以自报为准**（回填只补 total<=0 的行、不修正错价，入账后无
//!   修复路径，所以定价漂移窗口不能押在本地价上）；本地定价负责分项成本与
//!   漂移告警。`costIsPartial` 标记自报为下界：有本地价回退本地全额复算并
//!   抑制漂移告警，无价才用下界入账（分项记 0）。
//! - 防接管态双算不用指纹去重：接管态下 CLI 照写 updates.jsonl，但轮事件是
//!   聚合值（多 loop 求和），与代理逐请求行结构性不相等。改用「沉降窗 +
//!   接管活动时间窗守卫」：只导入足够旧的事件（届时接管态的代理行必已
//!   落库），插入前按事件时刻查询附近是否存在代理直录行（见
//!   `has_recent_grokbuild_proxy_activity`）。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::proxy::usage::calculator::CostCalculator;
use crate::proxy::usage::parser::TokenUsage;
use crate::services::session_usage::{
    metadata_modified_nanos, update_sync_state, SessionSyncResult,
};
use crate::services::sql_helpers::INPUT_TOKEN_SEMANTICS_TOTAL;
use crate::services::usage_stats::{
    find_model_pricing, has_recent_grokbuild_proxy_activity, SESSION_PROXY_DEDUP_WINDOW_SECONDS,
};
use rust_decimal::Decimal;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 事件沉降窗：只导入早于「现在 − 窗口」的事件。
///
/// 接管态下 CLI 照写 updates.jsonl，同一请求代理已逐请求记账；代理行与
/// 会话事件几乎同时产生，若导入抢在代理行落库前运行，接管守卫会因查不到
/// 代理行而放行，双算永久留存。让事件先「沉降」再导入后，守卫查询必然
/// 能看到已落库的代理行，竞态从源头消除。代价：官方态用量最多延迟约一个
/// 窗口 + 一次后台同步周期（60s）上屏。
const SETTLE_WINDOW_SECONDS: i64 = SESSION_PROXY_DEDUP_WINDOW_SECONDS;

/// 单个模型的本轮用量（从 `modelUsage` 或顶层 usage 提取，均为逐轮口径）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GrokCounters {
    input: u64,
    output: u64,
    cached: u64,
    api_ms: u64,
    model_calls: u64,
    /// CLI 自报本轮成本，1 tick = 1e-10 USD；0 = 上游未提供
    cost_ticks: u64,
    /// 上游标记 cost_ticks 只是部分费用（`costIsPartial`）：此时它是下界
    cost_partial: bool,
}

impl GrokCounters {
    fn is_zero(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cached == 0
    }

    fn reported_cost_usd(&self) -> Option<Decimal> {
        (self.cost_ticks > 0)
            .then(|| Decimal::from(self.cost_ticks) / Decimal::from(10_000_000_000u64))
    }
}

/// 一条 `turn_completed` 用量事件
#[derive(Debug)]
struct GrokUsageEvent {
    created_at: i64,
    prompt_id: String,
    /// 事件级 `costIsPartial`（顶层 usage 上观测到的位置；对本事件全部模型生效）
    cost_is_partial: bool,
    per_model: Vec<(String, GrokCounters)>,
}

/// 同步 Grok Build 使用数据（从 updates.jsonl 会话日志）
pub fn sync_grokbuild_usage(db: &Database) -> Result<SessionSyncResult, AppError> {
    let files = collect_grok_updates_files();

    let mut result = SessionSyncResult {
        files_scanned: files.len() as u32,
        ..Default::default()
    };

    let cursors = crate::services::session_usage::load_sync_cursors(db)?;

    for file_path in &files {
        match sync_single_grok_file(db, file_path, &cursors) {
            Ok(file_result) => result.merge(file_result),
            Err(e) => {
                let msg = format!("Grok Build 会话文件解析失败 {}: {e}", file_path.display());
                log::warn!("[GROK-SYNC] {msg}");
                result.errors.push(msg);
            }
        }
    }

    if result.imported > 0 {
        log::info!(
            "[GROK-SYNC] 同步完成: 导入 {} 条, 跳过 {} 条, 扫描 {} 个文件, 延后 {} 个文件",
            result.imported,
            result.skipped,
            result.files_scanned,
            result.deferred_files
        );
    }

    Ok(result)
}

/// 收集所有 Grok 会话的 updates.jsonl（含归档会话，与会话浏览器同根）
fn collect_grok_updates_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for root in crate::session_manager::providers::grokbuild::session_roots() {
        collect_files_named(&root, "updates.jsonl", &mut files, 0);
    }
    files
}

/// 单个 updates.jsonl 文件读取上限（50 MiB）。JSONL 单行事件通常几 KiB，
/// 正常活跃会话数月也到不了这个量级；超过则视为异常/恶意文件，跳过。
const MAX_GROK_FILE_BYTES: u64 = 50 * 1024 * 1024;
/// 递归收集 session 日志时的最大目录深度，防止 symlink 循环导致栈溢出。
const MAX_COLLECT_DEPTH: usize = 16;

/// 递归收集目录下指定文件名的文件（容忍布局深度变化，对齐会话浏览器的做法）
fn collect_files_named(root: &Path, name: &str, files: &mut Vec<PathBuf>, depth: usize) {
    if depth > MAX_COLLECT_DEPTH {
        log::warn!(
            "Grok session directory traversal exceeded max depth {} at {}",
            MAX_COLLECT_DEPTH,
            root.display()
        );
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // `entry.metadata()` 不跟随符号链接（不同于 `path.is_dir()`），这里据此
        // **无条件跳过一切 symlink**：目录 symlink 不递归（避免循环），文件
        // symlink 也不收集——同名文件若经 symlink 指向 sessions 根之外，会把用户
        // 意料之外的内容当作会话日志读入。代价：把 sessions 目录整体做成 symlink
        // 的用户会同步不到数据，所以跳过必须留日志，便于排查"用量数据静默缺失"。
        let metadata = entry.metadata();
        if metadata.as_ref().map(|m| m.is_symlink()).unwrap_or(false) {
            log::info!("[GROK-SYNC] 跳过符号链接（不跟随）: {}", path.display());
            continue;
        }
        let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        if is_dir {
            collect_files_named(&path, name, files, depth + 1);
        } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
            files.push(path);
        }
    }
}

/// 同步单个 updates.jsonl 文件。游标来自调用方批量预取。
fn sync_single_grok_file(
    db: &Database,
    file_path: &Path,
    cursors: &std::collections::HashMap<String, crate::services::session_usage::SyncCursor>,
) -> Result<SessionSyncResult, AppError> {
    let file_path_str = file_path.to_string_lossy().to_string();

    let metadata = fs::metadata(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件元数据: {e}")))?;
    let file_modified = metadata_modified_nanos(&metadata);

    // 异常大文件直接跳过，避免一次性读取耗尽内存。
    if metadata.len() > MAX_GROK_FILE_BYTES {
        log::warn!(
            "Grok session log too large ({} bytes), skipping: {}",
            metadata.len(),
            file_path.display()
        );
        return Ok(SessionSyncResult::default());
    }

    let last_modified = cursors.get(&file_path_str).map_or(0, |c| c.last_modified);
    if file_modified <= last_modified {
        return Ok(SessionSyncResult::default());
    }

    // 文件变更时全量重读：UPSERT 幂等使重读无害，且沉降窗延后的事件本就
    // 依赖下一轮重读补入。事件已是逐轮独立值，改 offset 增量读在正确性上
    // 可行（无差分基线依赖），但需另行处理延后事件的 offset 回退，收益
    // （活跃会话每周期省一次 O(N) 解析）暂不值得该复杂度。
    let content = fs::read_to_string(file_path)
        .map_err(|e| AppError::Config(format!("无法读取文件: {e}")))?;
    let events = parse_grok_usage_events(&content);

    // 会话 ID = 会话目录名（与 summary.json 的 info.id 一致）。request_id
    // 唯一性押在该 UUIDv7 全局唯一上：同 ID 的归档/活跃副本经 UPSERT 幂等
    // 收敛（有意），不同 <enc-cwd> 下撞 ID 视为不可能。
    let session_id = file_path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut result = SessionSyncResult::default();
    let mut deferred = false;

    for (idx, event) in events.iter().enumerate() {
        // 沉降窗：事件按 append 顺序时间单调，遇到第一条未沉降的事件即停，
        // 后续事件与它一起等下一轮（保持"文件前缀已导入"的简单不变量）。
        // 已知局限：未来时间戳（时钟误设）会让该文件持续延后并整文件重扫，
        // 墙钟越过 事件时刻+窗口 后自愈；活跃会话每周期全量重读为设计代价。
        if now.saturating_sub(event.created_at) < SETTLE_WINDOW_SECONDS {
            deferred = true;
            break;
        }

        // 接管守卫按事件时刻判定一次，整条事件的所有模型行同进退；
        // 被守卫跳过的 token 已由代理行记账，跳过即终态（同步状态照常
        // 推进）。已知局限：守卫无 session 维度，见 usage_stats.rs 注释。
        let takeover_active = {
            let conn = lock_conn!(db.conn);
            has_recent_grokbuild_proxy_activity(&conn, event.created_at)?
        };

        for (model, turn) in &event.per_model {
            if turn.is_zero() {
                continue;
            }
            if takeover_active {
                // 计入 skipped（对齐 gemini 指纹去重跳过的语义：未入账，代理
                // 行权威）。勿改用 suspected_duplicates——codex 对它的语义相反
                // （已入账待查），而 merge() 会把两义直接求和。
                result.skipped += 1;
                continue;
            }

            // 幂等键锚定上游稳定 ID（prompt_id 是每轮唯一的 UUID），不含文件
            // 内序号：updates.jsonl 前缀被改写（如 rewind 截断）导致事件序号
            // 前移时，幸存轮次仍命中原行不会双算；被移除轮次的行保留——
            // rewind 不退还已消耗的 token，留存即正确记账。若上游对同一
            // prompt_id 写多条 turn_completed（未观测到），UPSERT 取后者，
            // 方向是少记不双算。prompt_id 缺失时回退 "idx{N}"（UUID 形态的
            // prompt_id 不可能与之撞名）。
            let turn_key = if event.prompt_id.is_empty() {
                format!("idx{idx}")
            } else {
                event.prompt_id.clone()
            };
            let request_id = format!("grok_session:{session_id}:{turn_key}:{model}");
            match insert_grok_session_entry(
                db,
                &request_id,
                turn,
                event.cost_is_partial || turn.cost_partial,
                model,
                &session_id,
                event.created_at,
            ) {
                Ok(true) => result.imported += 1,
                Ok(false) => result.skipped += 1,
                Err(e) => {
                    log::warn!("[GROK-SYNC] 插入失败 ({request_id}): {e}");
                    result.skipped += 1;
                }
            }
        }
    }

    if deferred {
        // 不落同步状态：下一轮重读整个文件，把沉降后的事件补入。
        result.deferred_files += 1;
    } else {
        update_sync_state(db, &file_path_str, file_modified, events.len() as i64)?;
    }

    Ok(result)
}

/// 从 updates.jsonl 内容解析出全部逐轮用量事件（保持文件顺序）
fn parse_grok_usage_events(content: &str) -> Vec<GrokUsageEvent> {
    let mut events = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record.get("method").and_then(|v| v.as_str()) != Some("_x.ai/session/update") {
            continue;
        }
        let update = record.get("params").and_then(|p| p.get("update"));
        // 只认 turn_completed（实测全体带 usage 的事件均为此类；判别字段是
        // sessionUpdate，serde internally-tagged）。字段缺失时向后兼容放行，
        // 但显式标为其它类型的事件即使带 usage 也不导入——中途快照若与轮末
        // 事件并存，双导会双算。
        let kind = update
            .and_then(|u| u.get("sessionUpdate"))
            .and_then(|v| v.as_str());
        if kind.is_some() && kind != Some("turn_completed") {
            continue;
        }
        let Some(usage) = update
            .and_then(|u| u.get("usage"))
            .filter(|u| u.is_object())
        else {
            continue;
        };
        // 沉降窗与接管守卫都依赖事件时刻，没有时间戳的事件无法安全导入。
        let Some(created_at) = parse_event_timestamp(record.get("timestamp")) else {
            continue;
        };

        let prompt_id = update
            .and_then(|u| u.get("prompt_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let mut per_model: Vec<(String, GrokCounters)> = usage
            .get("modelUsage")
            .and_then(|m| m.as_object())
            .map(|map| {
                map.iter()
                    .map(|(model, counters)| (model.clone(), parse_grok_counters(counters)))
                    .collect()
            })
            .unwrap_or_default();
        if per_model.is_empty() {
            // 缺 modelUsage 时退回顶层逐轮值；模型名未知，交由查价层兜底。
            per_model.push(("unknown".to_string(), parse_grok_counters(usage)));
        }
        // modelUsage 是 JSON object，遍历序不保证稳定；排序保证插入顺序
        // 与日志在多次重扫间确定。
        per_model.sort_by(|a, b| a.0.cmp(&b.0));

        events.push(GrokUsageEvent {
            created_at,
            prompt_id,
            cost_is_partial: usage
                .get("costIsPartial")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            per_model,
        });
    }

    events
}

fn parse_grok_counters(value: &serde_json::Value) -> GrokCounters {
    let get = |key: &str| value.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
    GrokCounters {
        input: get("inputTokens"),
        output: get("outputTokens"),
        cached: get("cachedReadTokens"),
        api_ms: get("apiDurationMs"),
        model_calls: get("modelCalls"),
        cost_ticks: get("costUsdTicks"),
        cost_partial: value
            .get("costIsPartial")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

/// updates.jsonl 顶层 `timestamp` 实测为数字 epoch 秒（勿与 summary.json 的
/// RFC3339 字符串混淆）；字符串形态仅作防御性兜底。
fn parse_event_timestamp(value: Option<&serde_json::Value>) -> Option<i64> {
    let value = value?;
    if let Some(n) = value.as_i64() {
        // 防未来毫秒形态：超过 1e11 视作毫秒
        return Some(if n > 100_000_000_000 { n / 1000 } else { n });
    }
    value
        .as_str()
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|dt| dt.timestamp())
}

/// 插入单条 Grok 会话记录到 proxy_request_logs
fn insert_grok_session_entry(
    db: &Database,
    request_id: &str,
    turn: &GrokCounters,
    cost_is_partial: bool,
    model: &str,
    session_id: &str,
    created_at: i64,
) -> Result<bool, AppError> {
    let conn = lock_conn!(db.conn);

    let clamp = |v: u64| v.min(u32::MAX as u64) as u32;
    let usage = TokenUsage {
        input_tokens: clamp(turn.input),
        output_tokens: clamp(turn.output),
        cache_read_tokens: clamp(turn.cached),
        cache_creation_tokens: 0,
        model: Some(model.to_string()),
        message_id: None,
    };

    let pricing = find_model_pricing(&conn, model);
    let multiplier = Decimal::from(1);
    let reported = turn.reported_cost_usd();
    // 插入成功（changed）后才发，避免重扫时重复刷日志
    let mut deferred_warn: Option<String> = None;

    // total_cost 取值优先级（🔴 回填机制只补 total<=0 的行、从不修正已有正值，
    // 见 backfill_missing_usage_costs；本导入器 UPSERT 也不因 cost 单独变化而
    // 更新——所以入账时就必须写对，事后没有修复路径）：
    // 1. 有自报且完整 → 以自报为准（上游 ground truth，定价漂移窗口内也准确；
    //    本地定价负责分项与漂移告警，漂移时分项与 total 允许暂不自洽）；
    // 2. 自报不完整（costIsPartial）→ 有本地价用本地全额复算（token 数完整），
    //    并抑制此时无意义的漂移告警；无价则仍用自报下界（好过记 0）；
    // 3. 无自报 → 本地复算；彻底无价才整单记 0。
    let (input_cost, output_cost, cache_read_cost, cache_creation_cost, total_cost) = match pricing
    {
        Some(p) => {
            let cost = CostCalculator::calculate_for_app("grokbuild", &usage, &p, multiplier);
            let total = match reported {
                Some(reported) if !cost_is_partial => {
                    // 偏差超 1%（微额下限 1e-6）即本地定价漂移——xAI 调价时
                    // 最早的可观测信号，提醒更新 seed/repair。
                    let tolerance = (reported * Decimal::new(1, 2)).max(Decimal::new(1, 6));
                    if (cost.total_cost - reported).abs() > tolerance {
                        deferred_warn = Some(format!(
                            "本地定价与 CLI 自报成本偏差超阈值，total 已以自报为准，请更新本地定价: model={model} local={} reported={reported} request_id={request_id}",
                            cost.total_cost
                        ));
                    }
                    reported
                }
                _ => cost.total_cost,
            };
            (
                cost.input_cost.to_string(),
                cost.output_cost.to_string(),
                cost.cache_read_cost.to_string(),
                cost.cache_creation_cost.to_string(),
                total.to_string(),
            )
        }
        None => {
            // 未 seed 的新别名：token 照常入账；有自报成本时直接采用（分项
            // 记 0），彻底无价才整单记 0。xAI 内部别名会周期性变动
            // （grok-4.5-build 即先例），两种情况都要留下可排查的痕迹。
            let total = match reported {
                Some(reported) => {
                    if model != "unknown" {
                        let partial_note = if cost_is_partial {
                            "（上游标记为部分费用，实际为下界）"
                        } else {
                            ""
                        };
                        deferred_warn = Some(format!(
                            "模型定价未找到，采用 CLI 自报成本入账{partial_note}: model={model} total={reported} request_id={request_id}"
                        ));
                    }
                    reported.to_string()
                }
                None => {
                    if model != "unknown" {
                        deferred_warn = Some(format!(
                            "模型定价未找到且无自报成本，成本记 0: model={model} request_id={request_id}"
                        ));
                    }
                    "0".to_string()
                }
            };
            (
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                "0".to_string(),
                total,
            )
        }
    };

    // UPSERT：重扫幂等；解析口径修正后重扫时更新既有行（token/成本/
    // latency；created_at 保持首插值不动，避免行在沉降窗与 rollup 边界间漂移）。
    // WHERE 的 data_source 守卫是纵深防御：request_id 前缀命名空间已隔离，
    // 万一撞上非本导入器的行也绝不改写它。
    // input_token_semantics 显式写 TOTAL——xAI 口径 inputTokens 含 cache read，
    // 与代理路径的 grokbuild 行（logger）保持同一语义，勿依赖列默认值。
    conn.execute(
        "INSERT INTO proxy_request_logs (
            request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            input_cost_usd, output_cost_usd, cache_read_cost_usd, cache_creation_cost_usd, total_cost_usd,
            latency_ms, first_token_ms, status_code, error_message, session_id,
            provider_type, is_streaming, cost_multiplier, created_at, data_source,
            input_token_semantics
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)
        ON CONFLICT(request_id) DO UPDATE SET
            model = excluded.model,
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            input_cost_usd = excluded.input_cost_usd,
            output_cost_usd = excluded.output_cost_usd,
            cache_read_cost_usd = excluded.cache_read_cost_usd,
            cache_creation_cost_usd = excluded.cache_creation_cost_usd,
            total_cost_usd = excluded.total_cost_usd,
            latency_ms = excluded.latency_ms
        WHERE data_source = 'grok_session'
          AND (input_tokens != excluded.input_tokens
           OR output_tokens != excluded.output_tokens
           OR cache_read_tokens != excluded.cache_read_tokens
           OR latency_ms != excluded.latency_ms
           OR model != excluded.model)",
        rusqlite::params![
            request_id,
            "_grok_session",     // provider_id
            "grokbuild",         // app_type
            model,
            model,               // request_model = model
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_tokens,
            0i64,                // cache_creation_tokens
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
            total_cost,
            turn.api_ms.min(i64::MAX as u64) as i64, // latency_ms（本轮 API 时长）
            Option::<i64>::None, // first_token_ms
            200i64,              // status_code
            Option::<String>::None, // error_message
            session_id,
            Some("grok_session"), // provider_type
            1i64,                // is_streaming
            "1.0",               // cost_multiplier
            created_at,
            "grok_session",      // data_source
            INPUT_TOKEN_SEMANTICS_TOTAL,
        ],
    )
    .map_err(|e| AppError::Database(format!("插入 Grok Build 会话日志失败: {e}")))?;

    // changes() > 0 表示新插入或已更新，== 0 表示值完全相同（无实际变更）
    let changed = conn.changes() > 0;
    if changed {
        if let Some(msg) = deferred_warn {
            log::warn!("[GROK-SYNC] {msg}");
        }
    }
    Ok(changed)
}
