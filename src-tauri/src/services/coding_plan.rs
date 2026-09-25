//! Token Plan / 编程套餐额度查询服务
//!
//! 支持 Kimi For Coding、智谱 GLM、MiniMax、ZenMux、火山方舟、OpenCode Go
//! 的套餐额度查询。复用 subscription 模块的 SubscriptionQuota / QuotaTier 类型。

use super::subscription::{
    CredentialStatus, QuotaTier, SubscriptionQuota, TIER_FIVE_HOUR, TIER_MONTHLY, TIER_WEEKLY_LIMIT,
};
use std::time::{SystemTime, UNIX_EPOCH};

// ── 供应商检测 ──────────────────────────────────────────────

enum CodingPlanProvider {
    Kimi,
    ZhipuCn,
    ZhipuEn,
    MiniMaxCn,
    MiniMaxEn,
    ZenMux,
    /// 火山方舟 Agent Plan / Coding Plan（base_url 形如
    /// `https://ark.cn-beijing.volces.com/api/plan[/v3]`（Agent Plan）
    /// 或 `/api/coding[/v3]`（Coding Plan））。
    Volcengine,
    /// OpenCode Go（$10/月订阅，美元额度三时间窗口）。base_url 分两档：
    /// `https://opencode.ai/zen/go`（claude/claude-desktop 直连 /messages）
    /// 与 `https://opencode.ai/zen/go/v1`（codex/opencode/pi 走 Chat）。
    OpencodeGo,
}

fn detect_provider(base_url: &str) -> Option<CodingPlanProvider> {
    let url = base_url.to_lowercase();
    if url.contains("api.kimi.com/coding") {
        Some(CodingPlanProvider::Kimi)
    } else if url.contains("open.bigmodel.cn") || url.contains("bigmodel.cn") {
        Some(CodingPlanProvider::ZhipuCn)
    } else if url.contains("api.z.ai") {
        Some(CodingPlanProvider::ZhipuEn)
    } else if crate::codex_config::codex_url_host_matches_any(
        base_url,
        &["api.minimaxi.com", "api.minimax.cn"],
    ) {
        Some(CodingPlanProvider::MiniMaxCn)
    } else if crate::codex_config::codex_url_host_matches_any(base_url, &["api.minimax.io"]) {
        Some(CodingPlanProvider::MiniMaxEn)
    } else if url.contains("zenmux") {
        Some(CodingPlanProvider::ZenMux)
    } else if url.contains("opencode.ai/zen/go") {
        // 同时覆盖 /zen/go 与 /zen/go/v1 两档 base；Zen 按量版（/zen/v1）
        // 没有任何用量/余额 API（实测 404），刻意不命中。
        Some(CodingPlanProvider::OpencodeGo)
    } else if url.contains("volces.com/api/plan") || url.contains("volces.com/api/coding") {
        // 仅匹配 Agent Plan（/api/plan[/v3]）与 Coding Plan（/api/coding[/v3]）
        // 入口；DouBaoSeed 按量付费走 /api/v3 与 /api/compatible，没有套餐
        // 额度，不在此命中。用量探测本身是双 plan 自动探测（GetAFPUsage →
        // GetCodingPlanUsage），无需在此区分两种订阅。
        Some(CodingPlanProvider::Volcengine)
    } else {
        None
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn millis_to_iso8601(ms: i64) -> Option<String> {
    let secs = ms / 1000;
    let nsecs = ((ms % 1000) * 1_000_000) as u32;
    chrono::DateTime::from_timestamp(secs, nsecs).map(|dt| dt.to_rfc3339())
}

/// 从 JSON 值提取重置时间，兼容字符串和数字格式
/// - 字符串：直接返回（ISO 8601）
/// - 数字：自动判断秒/毫秒并转为 ISO 8601
fn extract_reset_time(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    if let Some(n) = value.as_i64() {
        // 0/负时间戳（如火山 session 无活跃窗口回 -1）视为无重置时间
        if n <= 0 {
            return None;
        }
        // 区分秒和毫秒：秒级时间戳 < 1e12，毫秒 >= 1e12
        let ms = if n < 1_000_000_000_000 { n * 1000 } else { n };
        return millis_to_iso8601(ms);
    }
    None
}

/// 解析 JSON 值为 f64，兼容数字和字符串格式（如 `100` 和 `"100"`）
fn parse_f64(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

fn make_error(msg: String) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: false,
        tiers: vec![],
        extra_usage: None,
        error: Some(msg),
        queried_at: Some(now_millis()),
    }
}

// ── Kimi For Coding ─────────────────────────────────────────

async fn query_kimi(api_key: &str) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get("https://api.kimi.com/coding/v1/usages")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    let mut tiers = Vec::new();

    // 5 小时窗口限额（优先显示）
    if let Some(limits) = body.get("limits").and_then(|v| v.as_array()) {
        for limit_item in limits {
            if let Some(detail) = limit_item.get("detail") {
                let limit = detail.get("limit").and_then(parse_f64).unwrap_or(1.0);
                let remaining = detail.get("remaining").and_then(parse_f64).unwrap_or(0.0);
                let resets_at = detail.get("resetTime").and_then(extract_reset_time);

                let used = (limit - remaining).max(0.0);
                let utilization = if limit > 0.0 {
                    (used / limit) * 100.0
                } else {
                    0.0
                };
                tiers.push(QuotaTier {
                    name: "five_hour".to_string(),
                    utilization,
                    resets_at,
                    used_value_usd: None,
                    max_value_usd: None,
                });
            }
        }
    }

    // 总体用量（周限额）
    if let Some(usage) = body.get("usage") {
        let limit = usage.get("limit").and_then(parse_f64).unwrap_or(1.0);
        let remaining = usage.get("remaining").and_then(parse_f64).unwrap_or(0.0);
        let resets_at = usage.get("resetTime").and_then(extract_reset_time);

        let used = (limit - remaining).max(0.0);
        let utilization = if limit > 0.0 {
            (used / limit) * 100.0
        } else {
            0.0
        };
        tiers.push(QuotaTier {
            name: "weekly_limit".to_string(),
            utilization,
            resets_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }

    Ok(SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

// ── 智谱 GLM ────────────────────────────────────────────────

/// 智谱 TOKENS_LIMIT 条目按 `unit` 字段的显式窗口分类。
enum ZhipuWindow {
    FiveHour,
    Weekly,
}

/// 按 `unit` 字段判定 TOKENS_LIMIT 条目所属窗口。
///
/// 实测形态（bigmodel.cn 与 z.ai 共用同一后端，字段一致）：
/// - `unit: 3, number: 5` → 5 小时滚动窗口（老/新套餐均有）
/// - `unit: 6, number: 7` 与 `unit: 6, number: 1` → 每周窗口（两种取值都被
///   实测过，故只锚定 `unit`、不绑 `number`）
///
/// `unit` 缺失或值不认识时返回 None，由调用方走重置时间启发式兜底。
fn classify_zhipu_window(item: &serde_json::Value) -> Option<ZhipuWindow> {
    match item.get("unit").and_then(|v| v.as_i64()) {
        Some(3) => Some(ZhipuWindow::FiveHour),
        Some(6) => Some(ZhipuWindow::Weekly),
        _ => None,
    }
}

/// 把智谱 `data` 里的 `limits[]` 解析成 tier 列表。
///
/// 分类优先级：
/// 1. 显式字段：`unit` 标识窗口类型（见 [`classify_zhipu_window`]）。不能按
///    `nextResetTime` 排序代替——周期末尾每周窗口会比 5 小时窗口更早重置
///    （issue #3036），时间排序在该场景必然把两桶标反。
/// 2. 兜底启发式（`unit` 缺失或不识别）：无 `nextResetTime` 的条目优先归
///    five_hour（5 小时桶在 0% 等状态下可能没有 reset），其余按 reset 升序
///    依次填入仍空缺的槽位。
///
/// 老套餐（2026-02-12 前订阅）只回 1 条
/// `TOKENS_LIMIT`，自然降级为仅展示 `five_hour`；新套餐回 2 条。
fn parse_zhipu_token_tiers(data: &serde_json::Value) -> Vec<QuotaTier> {
    type Entry = (Option<i64>, f64, Option<String>);
    let mut five_hour: Option<Entry> = None;
    let mut weekly: Option<Entry> = None;
    let mut unclassified: Vec<Entry> = Vec::new();

    if let Some(limits) = data.get("limits").and_then(|v| v.as_array()) {
        for limit_item in limits {
            let limit_type = limit_item
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // 大小写不敏感比较：上游若把 "TOKENS_LIMIT" 改成小写或驼峰，依然能识别
            if !(limit_type.eq_ignore_ascii_case("TOKENS_LIMIT")
                || limit_type.eq_ignore_ascii_case("CREDIT_LIMIT"))
            {
                continue;
            }
            let percentage = limit_item
                .get("percentage")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let reset_ms = limit_item.get("nextResetTime").and_then(|v| v.as_i64());
            let reset_iso = reset_ms.and_then(millis_to_iso8601);
            let entry = (reset_ms, percentage, reset_iso);
            match classify_zhipu_window(limit_item) {
                Some(ZhipuWindow::FiveHour) if five_hour.is_none() => five_hour = Some(entry),
                Some(ZhipuWindow::Weekly) if weekly.is_none() => weekly = Some(entry),
                _ => unclassified.push(entry),
            }
        }
    }

    unclassified.sort_by_key(|(reset, _, _)| (reset.is_some(), reset.unwrap_or(i64::MIN)));
    for entry in unclassified {
        if five_hour.is_none() {
            five_hour = Some(entry);
        } else if weekly.is_none() {
            weekly = Some(entry);
        }
        // 智谱当前最多两条 TOKENS_LIMIT，多余的忽略
    }

    let mut tiers = Vec::new();
    for (name, slot) in [(TIER_FIVE_HOUR, five_hour), (TIER_WEEKLY_LIMIT, weekly)] {
        if let Some((_, percentage, resets_at)) = slot {
            tiers.push(QuotaTier {
                name: name.to_string(),
                utilization: percentage,
                resets_at,
                used_value_usd: None,
                max_value_usd: None,
            });
        }
    }
    tiers
}

/// Resolve the Zhipu quota endpoint from the user's configured `base_url`.
///
/// Zhipu ships as two distinct presets (Zhipu GLM = `open.bigmodel.cn`,
/// Zhipu GLM en = `api.z.ai`) that share the same quota path and JSON shape.
/// The quota endpoint lives on the same host as the user's coding endpoint,
/// so we route by `base_url` and let the caller's existing reachability
/// (they're already using this host to run coding) determine success — no
/// cross-host fallback, no auth-error heuristics.
fn zhipu_quota_base(base_url: &str) -> &'static str {
    if base_url.to_lowercase().contains("bigmodel.cn") {
        "https://open.bigmodel.cn"
    } else {
        "https://api.z.ai"
    }
}

async fn query_zhipu(base_url: &str, api_key: &str) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();
    let url = format!(
        "{}/api/monitor/usage/quota/limit",
        zhipu_quota_base(base_url)
    );

    let resp = client
        .get(&url)
        .header("Authorization", api_key) // 注意：智谱不加 Bearer 前缀
        .header("Content-Type", "application/json")
        .header("Accept-Language", "en-US,en")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    Ok(zhipu_quota_from_body(&body))
}

/// 解析智谱额度响应体（个人版与团队版共用同一 shape）。
/// 仅在 HTTP 成功、body 已完整读取并解析为 JSON 后调用——本函数不做任何网络 IO，
/// 故无瞬时失败通道，确定性失败直接落进 `Ok(success:false)`。
fn zhipu_quota_from_body(body: &serde_json::Value) -> SubscriptionQuota {
    // 检查业务级别错误
    if body.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = body
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        return make_error(format!("API error: {msg}"));
    }

    let data = match body.get("data") {
        Some(d) => d,
        None => return make_error("Missing 'data' field in response".to_string()),
    };

    let tiers = parse_zhipu_token_tiers(data);

    // 套餐等级存入 credential_message
    let level = data
        .get("level")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: level,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    }
}

// ── MiniMax ─────────────────────────────────────────────────

async fn query_minimax(api_key: &str, is_cn: bool) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    // 额度接口只在 api.minimaxi.com / api.minimax.io 有公开出处；国内新推理域名
    // api.minimax.cn 未见该接口文档，沿用旧域名（同一账号体系与 Key）
    let api_domain = if is_cn {
        "api.minimaxi.com"
    } else {
        "api.minimax.io"
    };
    let url = format!("https://{api_domain}/v1/api/openplatform/coding_plan/remains");

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    // 检查业务级别错误
    if let Some(base_resp) = body.get("base_resp") {
        let status_code = base_resp
            .get("status_code")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if status_code != 0 {
            let msg = base_resp
                .get("status_msg")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return Ok(make_error(format!("API error (code {status_code}): {msg}")));
        }
    }

    // 提取纯函数便于无 mock 单元测试;新接口直接给"剩余百分比",反转为已用百分比
    let tiers = parse_minimax_tiers(&body);

    Ok(SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

// ── ZenMux ──────────────────────────────────────────────────

async fn query_zenmux(base_url: &str, api_key: &str) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    let resp = client
        .get(base_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    // 检查业务级别错误
    if body.get("success").and_then(|v| v.as_bool()) != Some(true) {
        let msg = body
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        return Ok(make_error(format!("API error: {msg}")));
    }

    let data = match body.get("data") {
        Some(d) => d,
        None => return Ok(make_error("Missing 'data' field in response".to_string())),
    };

    let mut tiers = Vec::new();

    // 5 小时窗口限额
    if let Some(q5h) = data.get("quota_5_hour") {
        let usage_pct = q5h
            .get("usage_percentage")
            .and_then(parse_f64)
            .unwrap_or(0.0);
        let resets_at = q5h
            .get("resets_at")
            .and_then(|v| v.as_str())
            .map(String::from);
        let used_usd = q5h.get("used_value_usd").and_then(parse_f64);
        let max_usd = q5h.get("max_value_usd").and_then(parse_f64);
        tiers.push(QuotaTier {
            name: "five_hour".to_string(),
            utilization: usage_pct * 100.0,
            resets_at,
            used_value_usd: used_usd,
            max_value_usd: max_usd,
        });
    }

    // 7 天窗口限额
    if let Some(q7d) = data.get("quota_7_day") {
        let usage_pct = q7d
            .get("usage_percentage")
            .and_then(parse_f64)
            .unwrap_or(0.0);
        let resets_at = q7d
            .get("resets_at")
            .and_then(|v| v.as_str())
            .map(String::from);
        let used_usd = q7d.get("used_value_usd").and_then(parse_f64);
        let max_usd = q7d.get("max_value_usd").and_then(parse_f64);
        tiers.push(QuotaTier {
            name: "weekly_limit".to_string(),
            utilization: usage_pct * 100.0,
            resets_at,
            used_value_usd: used_usd,
            max_value_usd: max_usd,
        });
    }

    // 套餐等级和账户状态存入 credential_message
    let plan_tier = data
        .get("plan")
        .and_then(|p| p.get("tier"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let account_status = data
        .get("account_status")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let plan_info = if !plan_tier.is_empty() {
        format!("{plan_tier} ({account_status})")
    } else {
        String::new()
    };

    Ok(SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: if plan_info.is_empty() {
            None
        } else {
            Some(plan_info)
        },
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

/// 从 `/coding_plan/remains` 响应中解析 MiniMax 编程套餐的额度 tier。
///
/// 新接口语义:`current_*_remaining_percent` 是"剩余百分比"(0-100),
/// `model_remains` 数组里有 `general`(编程套餐)和 `video` 等其他模型,
/// 这里只取 `general`,跳过 video。
///
/// 5h 桶始终存在;周桶并非所有套餐都有,靠 `current_weekly_status == 1`
/// 判定激活(无周限额套餐该字段为 3,`remaining_percent` 恒为 100,不应展示)。
fn parse_minimax_tiers(body: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();

    let Some(model_remains) = body.get("model_remains").and_then(|v| v.as_array()) else {
        return tiers;
    };

    // 只取 model_name == "general" 的条目,跳过 video 等非编程模型
    let Some(item) = model_remains.iter().find(|item| {
        item.get("model_name")
            .and_then(|v| v.as_str())
            .map(|s| s == "general")
            .unwrap_or(false)
    }) else {
        return tiers;
    };

    // 5h 桶:剩余百分比 → 已用百分比
    if let Some(remain_pct) = item
        .get("current_interval_remaining_percent")
        .and_then(|v| v.as_f64())
    {
        let resets_at = item
            .get("end_time")
            .and_then(|v| v.as_i64())
            .and_then(millis_to_iso8601);
        tiers.push(QuotaTier {
            name: TIER_FIVE_HOUR.to_string(),
            utilization: 100.0 - remain_pct,
            resets_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }

    // 周桶:仅当 status=1 时激活;status=3 等表示该套餐无周限额,跳过
    if item.get("current_weekly_status").and_then(|v| v.as_i64()) == Some(1) {
        if let Some(remain_pct) = item
            .get("current_weekly_remaining_percent")
            .and_then(|v| v.as_f64())
        {
            let resets_at = item
                .get("weekly_end_time")
                .and_then(|v| v.as_i64())
                .and_then(millis_to_iso8601);
            tiers.push(QuotaTier {
                name: TIER_WEEKLY_LIMIT.to_string(),
                utilization: 100.0 - remain_pct,
                resets_at,
                used_value_usd: None,
                max_value_usd: None,
            });
        }
    }

    tiers
}

// ── OpenCode Go ─────────────────────────────────────────────

/// 解析 OpenCode Go usage 端点响应为 tier 列表。
///
/// 响应形态（上游 `packages/console/app/src/routes/zen/go/v1/usage.ts`）：
/// `{"usage":{"rolling"|"weekly"|"monthly":{"status":"ok"|"rate-limited",
/// "percent":0-100 已用整数,"resetsAt":ISO8601}}}`，三窗口对应文档口径
/// $12/5h、$30/周、$60/月（端点不回传金额，仅百分比）。
///
/// 该端点是第一方但未文档化的路由，上线当天（2026-08-11）就改过一次形态
/// （旧扁平 `rollingUsage/usagePercent/resetInSec` 已作废），故逐窗口防御
/// 解析：缺失或 percent 不可解析的窗口跳过，不整体失败。
///
/// `status=="rate-limited"` 时上游已把 percent 钉在 100，无需特判；
/// percent 为 0 时上游的 `resetsAt` 是「now+窗口时长」的占位值（滚动窗按
/// 最后记账时间整窗清零，此时窗口早已过期），丢弃不展示倒计时。
fn parse_opencode_go_tiers(body: &serde_json::Value) -> Vec<QuotaTier> {
    const WINDOWS: [(&str, &str); 3] = [
        ("rolling", TIER_FIVE_HOUR),
        ("weekly", TIER_WEEKLY_LIMIT),
        ("monthly", TIER_MONTHLY),
    ];
    let Some(usage) = body.get("usage") else {
        return Vec::new();
    };
    let mut tiers = Vec::new();
    for (key, tier_name) in WINDOWS {
        let Some(window) = usage.get(key) else {
            continue;
        };
        let Some(percent) = window.get("percent").and_then(parse_f64) else {
            continue;
        };
        let resets_at = if percent > 0.0 {
            window.get("resetsAt").and_then(extract_reset_time)
        } else {
            None
        };
        tiers.push(QuotaTier {
            name: tier_name.to_string(),
            utilization: percent,
            resets_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }
    tiers
}

async fn query_opencode_go(api_key: &str) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();

    // 用量端点只认 `Authorization: Bearer`——与推理侧 /messages 只认
    // x-api-key 正好相反，不能互换。
    let resp = client
        .get("https://opencode.ai/zen/go/v1/usage")
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    // 403 EntitlementError：key 本身有效（Zen 与 Go 共用同一把 workspace
    // API key），但该 workspace 没有 Go 订阅——与 401 认证失败分开提示。
    if status == reqwest::StatusCode::FORBIDDEN {
        return Ok(make_error(
            "API key is valid but has no OpenCode Go subscription (HTTP 403)".to_string(),
        ));
    }
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    let tiers = parse_opencode_go_tiers(&body);
    // 三个窗口一个都没解析出来 = 响应形态不认识（未文档化端点可能再次
    // 变形），明确报错而不是渲染一张空卡片。
    if tiers.is_empty() {
        return Ok(make_error("Unexpected usage response shape".to_string()));
    }

    Ok(SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: None,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    })
}

// ── 火山方舟 Agent Plan / Coding Plan ───────────────────────
//
// 与 Kimi/MiniMax（数据面 Bearer 余额接口）不同，火山用量接口是**控制面
// OpenAPI**：统一网关 `open.volcengineapi.com`（**不是**数据面推理域名
// `ark.cn-beijing.volces.com`），形如
// `POST https://open.volcengineapi.com/?Action=...&Version=2024-01-01&Region=cn-beijing`，
// **强制火山引擎签名 V4（AK/SK）**——实测复用推理 Bearer Key 会被网关以
// `400 InvalidAuthorization` 拒绝（格式层拒绝，非权限问题）。因此用户需在用量查询
// 里另填火山账号的 AccessKey ID + Secret（与推理 Key 是两套凭据）。两个 plan 用
// 同一份 AK/SK，故鉴权类错误直接停、不再试另一个 plan。
//
// 自动探测：先调 `GetAFPUsage`（Agent Plan，回绝对额度 Quota/Used），未订阅再调
// `GetCodingPlanUsage`（Coding Plan，回百分比）。

/// 控制面 OpenAPI 统一网关（区别于数据面推理域名 ark.cn-beijing.volces.com）。
const VOLCENGINE_OPENAPI_HOST: &str = "open.volcengineapi.com";
const VOLCENGINE_API_VERSION: &str = "2024-01-01";
/// ark 控制面 OpenAPI 的默认 Region（Agent/Coding Plan 目前在 cn-beijing）。
const VOLCENGINE_DEFAULT_REGION: &str = "cn-beijing";

/// 单次 OpenAPI 调用的归类结果。
enum VolcCall {
    /// 2xx 且 JSON 可解析、无 OpenAPI 级错误（业务 Result 仍可能为空=未订阅）。
    Body(serde_json::Value),
    /// 硬鉴权失败（HTTP 401/403 或 AccessDenied/Signature 等错误码）——两个 plan
    /// 共用凭据，命中即停。
    Auth(String),
    /// 非鉴权 HTTP 错误 / 响应体非法 JSON——记录后可继续尝试另一个 plan。
    Soft(String),
    /// 瞬时传输失败（网络/超时/读体中断）——同 host 的另一个 plan 大概率同样
    /// 失败，调用方应立即以 `Err` 传播（前端 reject → retry + 保留上次成功值）。
    Transient(String),
}

/// 从数据面 base_url 提取控制面 OpenAPI 所需的 Region（如
/// `ark.cn-beijing.volces.com` → `cn-beijing`）；无法识别时回落 cn-beijing。
/// 控制面 Host 是固定网关（`VOLCENGINE_OPENAPI_HOST`），不随 base_url 变化。
fn volcengine_region(base_url: &str) -> String {
    let host = base_url
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(base_url)
        .split('/')
        .next()
        .unwrap_or("");
    host.split('.')
        .find(|p| p.starts_with("cn-") || p.starts_with("ap-"))
        .map(|p| p.to_string())
        .unwrap_or_else(|| VOLCENGINE_DEFAULT_REGION.to_string())
}

/// 判断 OpenAPI 错误码是否属于鉴权类（需要硬停并提示换 AK/SK）。
fn volcengine_is_auth_error_code(code: &str) -> bool {
    let c = code.to_lowercase();
    c.contains("auth")
        || c.contains("signature")
        || c.contains("accessdenied")
        || c.contains("denied")
        || c.contains("unauthorized")
        || c.contains("forbidden")
        || c.contains("credential")
        || c.contains("token")
}

/// 提取火山 OpenAPI 响应里的 `ResponseMetadata.Error`（或顶层 `Error`）。
fn volcengine_response_error(body: &serde_json::Value) -> Option<(String, String)> {
    let err = body
        .get("ResponseMetadata")
        .and_then(|m| m.get("Error"))
        .or_else(|| body.get("Error"))?;
    let code = err
        .get("Code")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let msg = err
        .get("Message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if code.is_empty() && msg.is_empty() {
        None
    } else {
        Some((code, msg))
    }
}

/// 鉴权失败时的引导文案，附加在错误后。
const VOLCENGINE_AKSK_HINT: &str =
    "Check the AccessKey ID / Secret are correct and the account has Ark usage-query (OpenAPI) permission.";

// ── 火山引擎签名 V4（AK/SK）─────────────────────────────────
//
// 算法是 AWS SigV4 的火山变体（对照官方 volc-openapi-demos/signature/java/Sign.java）。
// **两处致命差异，照搬 s3.rs 的标准 SigV4 会签名失败**：
//   1. canonical headers 与 SignedHeaders 用**固定顺序**
//      `host;x-date;x-content-sha256;content-type`（**不按字母序**，s3.rs 是字母序）；
//   2. algorithm 串 `HMAC-SHA256`（无 `AWS4` 前缀）、credential scope 结尾 `request`
//      （非 `aws4_request`）、签名密钥 `kDate=HMAC(SK, date)`（SK 不加 `AWS4` 前缀）。
// canonical query 仍按 key 字母序（与标准 SigV4 一致）；service=`ark`、POST、空 body。

const VOLCENGINE_SERVICE: &str = "ark";
const VOLCENGINE_CONTENT_TYPE: &str = "application/json; charset=utf-8";
const VOLCENGINE_SIGNED_HEADERS: &str = "host;x-date;x-content-sha256;content-type";

fn volc_hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn volc_sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(data))
}

/// RFC3986 unreserved 之外全部按 `%XX` 编码（用于 canonical query string）。
fn volc_uri_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// 构造按 key 字母序排序、逐段 URL 编码的 canonical query string。
/// 同一份字符串既用于签名也用于实际请求 URL，保证两者完全一致。
fn volcengine_canonical_query(action: &str, region: &str) -> String {
    let mut pairs = [
        ("Action", action),
        ("Region", region),
        ("Version", VOLCENGINE_API_VERSION),
    ];
    pairs.sort_by(|a, b| a.0.cmp(b.0));
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", volc_uri_encode(k), volc_uri_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// 生成火山引擎签名 V4 的鉴权头，返回 `(Authorization, X-Date, X-Content-Sha256)`，
/// 三者都要塞进请求头；`canonical_query` 必须与实际请求 URL 的 query 完全一致。
/// `now` 作参数传入便于写确定性单测。
fn volcengine_sign(
    access_key_id: &str,
    secret_access_key: &str,
    region: &str,
    canonical_query: &str,
    body: &[u8],
    now: chrono::DateTime<chrono::Utc>,
) -> (String, String, String) {
    let x_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let short_date = now.format("%Y%m%d").to_string();
    let x_content_sha256 = volc_sha256_hex(body);

    // 固定顺序 canonical headers（火山特有，**不排序**）。
    let canonical_headers = format!(
        "host:{VOLCENGINE_OPENAPI_HOST}\nx-date:{x_date}\nx-content-sha256:{x_content_sha256}\ncontent-type:{VOLCENGINE_CONTENT_TYPE}\n"
    );
    let canonical_request = format!(
        "POST\n/\n{canonical_query}\n{canonical_headers}\n{VOLCENGINE_SIGNED_HEADERS}\n{x_content_sha256}"
    );

    let credential_scope = format!("{short_date}/{region}/{VOLCENGINE_SERVICE}/request");
    let string_to_sign = format!(
        "HMAC-SHA256\n{x_date}\n{credential_scope}\n{}",
        volc_sha256_hex(canonical_request.as_bytes())
    );

    // 签名密钥派生：kDate=HMAC(SK, date)（SK **不加** AWS4 前缀），终止串 `request`。
    let k_date = volc_hmac_sha256(secret_access_key.as_bytes(), short_date.as_bytes());
    let k_region = volc_hmac_sha256(&k_date, region.as_bytes());
    let k_service = volc_hmac_sha256(&k_region, VOLCENGINE_SERVICE.as_bytes());
    let k_signing = volc_hmac_sha256(&k_service, b"request");
    let signature: String = volc_hmac_sha256(&k_signing, string_to_sign.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    let authorization = format!(
        "HMAC-SHA256 Credential={access_key_id}/{credential_scope}, SignedHeaders={VOLCENGINE_SIGNED_HEADERS}, Signature={signature}"
    );
    (authorization, x_date, x_content_sha256)
}

async fn volcengine_openapi_call(
    region: &str,
    access_key_id: &str,
    secret_access_key: &str,
    action: &str,
) -> VolcCall {
    let client = crate::proxy::http_client::get();
    // canonical query 同时用于签名与实际 URL，确保两者逐字一致（否则签名不匹配）。
    let canonical_query = volcengine_canonical_query(action, region);
    let url = format!("https://{VOLCENGINE_OPENAPI_HOST}/?{canonical_query}");
    let body: &[u8] = b"";
    let (authorization, x_date, x_content_sha256) = volcengine_sign(
        access_key_id,
        secret_access_key,
        region,
        &canonical_query,
        body,
        chrono::Utc::now(),
    );

    let resp = client
        .post(&url)
        .header("X-Date", x_date)
        .header("X-Content-Sha256", x_content_sha256)
        .header("Content-Type", VOLCENGINE_CONTENT_TYPE)
        .header("Authorization", authorization)
        .body(body.to_vec())
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return VolcCall::Transient(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return VolcCall::Auth(format!(
            "Authentication failed (HTTP {status}). {VOLCENGINE_AKSK_HINT}"
        ));
    }
    if !status.is_success() {
        // 火山 OpenAPI 网关对签名/凭据类错误常返 4xx（多为 HTTP 400）并携带与 200
        // 路径相同的 ResponseMetadata.Error 信封，而非 401/403。这里也解析信封，让
        // Bearer 被拒时仍能给出 AK/SK 引导并标记凭据失效，而不是当成普通 API 错误。
        let raw = resp.text().await.unwrap_or_default();
        if let Ok(body) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some((code, msg)) = volcengine_response_error(&body) {
                if volcengine_is_auth_error_code(&code) {
                    return VolcCall::Auth(format!(
                        "Authentication failed (HTTP {status}, {code}): {msg}. {VOLCENGINE_AKSK_HINT}"
                    ));
                }
                return VolcCall::Soft(format!("API error (HTTP {status}, {code}): {msg}"));
            }
        }
        return VolcCall::Soft(format!("API error (HTTP {status}): {raw}"));
    }

    // 同 Bearer 路径：先 bytes() 再解析——读体失败是瞬时（Transient），解析失败
    // 是确定性（Soft）。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return VolcCall::Transient(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return VolcCall::Soft(format!("Failed to parse response: {e}")),
    };

    // 火山 OpenAPI 业务错误常以 200 + ResponseMetadata.Error 返回。
    if let Some((code, msg)) = volcengine_response_error(&body) {
        if volcengine_is_auth_error_code(&code) {
            return VolcCall::Auth(format!(
                "Authentication failed ({code}): {msg}. {VOLCENGINE_AKSK_HINT}"
            ));
        }
        return VolcCall::Soft(format!("API error ({code}): {msg}"));
    }

    VolcCall::Body(body)
}

/// 解析 `GetAFPUsage` 的 `Result` 为 tier 列表。
///
/// 展示 5h / 周 / 月三个窗口（与控制台一致）；`AFPDaily` 被官方控制台隐藏
/// （其 Quota 常高于周上限，属历史默认值而非强制限额），故跳过。
/// `Quota`/`Used` 是绝对 AFP 值，已用百分比 = Used/Quota×100；`Quota<=0` 视为
/// 该窗口未订阅/未启用，跳过——也用于把"已鉴权但无 Agent Plan"识别为空结果，
/// 从而回落到 Coding Plan 探测。
fn parse_afp_tiers(result: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();
    for (key, name) in [
        ("AFPFiveHour", TIER_FIVE_HOUR),
        ("AFPWeekly", TIER_WEEKLY_LIMIT),
        ("AFPMonthly", TIER_MONTHLY),
    ] {
        let Some(win) = result.get(key) else { continue };
        let quota = win.get("Quota").and_then(parse_f64).unwrap_or(0.0);
        if quota <= 0.0 {
            continue;
        }
        let used = win.get("Used").and_then(parse_f64).unwrap_or(0.0);
        // 已用百分比；不做范围裁剪，与 parse_zhipu_token_tiers/parse_minimax_tiers
        // 的约定一致（下游渲染层负责显示策略）。
        let utilization = used / quota * 100.0;
        let resets_at = win.get("ResetTime").and_then(extract_reset_time);
        tiers.push(QuotaTier {
            name: name.to_string(),
            utilization,
            resets_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }
    tiers
}

/// 把 `GetCodingPlanUsage` 的 window 标签归一到 tier 名。
fn volcengine_coding_window(label: &str) -> Option<&'static str> {
    match label.to_lowercase().as_str() {
        "session" | "5h" | "fivehour" | "five_hour" | "rolling_5h" => Some(TIER_FIVE_HOUR),
        "weekly" | "week" | "7d" => Some(TIER_WEEKLY_LIMIT),
        "monthly" | "month" => Some(TIER_MONTHLY),
        _ => None,
    }
}

/// 解析 `GetCodingPlanUsage` 的 `Result` 为 tier 列表（防御式）。
///
/// 该接口官方文档未给出逐字段规格，依据官方 ark-cli 描述：回 session/weekly/
/// monthly 窗口、**只给百分比**（已用）、重置时间是秒级。这里宽松匹配
/// `QuotaUsage`/`Usages`/`Details` 数组及多种字段名，命中即用、未命中跳过。
fn parse_coding_plan_tiers(result: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();
    let arr = result
        .get("QuotaUsage")
        .and_then(|v| v.as_array())
        .or_else(|| result.get("Usages").and_then(|v| v.as_array()))
        .or_else(|| result.get("Details").and_then(|v| v.as_array()));
    let Some(arr) = arr else { return tiers };

    for item in arr {
        // 真实字段是 `Level`（实测 2026-06-21：session/weekly/monthly）；其余作防御式 fallback。
        let label = item
            .get("Level")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("Type").and_then(|v| v.as_str()))
            .or_else(|| item.get("Period").and_then(|v| v.as_str()))
            .or_else(|| item.get("Label").and_then(|v| v.as_str()))
            .or_else(|| item.get("Window").and_then(|v| v.as_str()))
            .unwrap_or("");
        let Some(name) = volcengine_coding_window(label) else {
            continue;
        };
        let utilization = item
            .get("Percent")
            .and_then(parse_f64)
            .or_else(|| item.get("UsedPercent").and_then(parse_f64))
            .or_else(|| item.get("UsagePercent").and_then(parse_f64))
            .unwrap_or(0.0);
        // 兼容秒/毫秒/字符串（extract_reset_time 内部已区分秒与毫秒）。
        let resets_at = item
            .get("ResetTime")
            .or_else(|| item.get("ResetTimestamp"))
            .and_then(extract_reset_time);
        tiers.push(QuotaTier {
            name: name.to_string(),
            utilization,
            resets_at,
            used_value_usd: None,
            max_value_usd: None,
        });
    }
    tiers
}

fn volcengine_success(tiers: Vec<QuotaTier>, plan: Option<String>) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Valid,
        credential_message: plan,
        success: true,
        tiers,
        extra_usage: None,
        error: None,
        queried_at: Some(now_millis()),
    }
}

fn volcengine_auth_error(detail: String) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::Expired,
        credential_message: Some("Invalid API key".to_string()),
        success: false,
        tiers: vec![],
        extra_usage: None,
        error: Some(detail),
        queried_at: Some(now_millis()),
    }
}

async fn query_volcengine(
    base_url: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> Result<SubscriptionQuota, String> {
    let region = volcengine_region(base_url);
    let mut soft_errors: Vec<String> = Vec::new();
    // 2xx + 无 Error 信封但解析不出额度时，截断原始响应用于诊断（区分"真没订阅"
    // 与"字段名/包裹层猜错"）。签名若不通会走 Auth/Soft 分支，到不了这里。
    let mut empty_responses: Vec<String> = Vec::new();
    let summarize = |action: &str, body: &serde_json::Value| -> String {
        let raw: String = body.to_string().chars().take(700).collect();
        format!("{action}={raw}")
    };

    // 1) Agent Plan：GetAFPUsage
    match volcengine_openapi_call(&region, access_key_id, secret_access_key, "GetAFPUsage").await {
        VolcCall::Auth(detail) => return Ok(volcengine_auth_error(detail)),
        VolcCall::Transient(detail) => return Err(format!("GetAFPUsage: {detail}")),
        VolcCall::Soft(detail) => soft_errors.push(format!("GetAFPUsage: {detail}")),
        VolcCall::Body(body) => {
            let result = body.get("Result").unwrap_or(&body);
            let tiers = parse_afp_tiers(result);
            if !tiers.is_empty() {
                let plan = result
                    .get("PlanType")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("Agent Plan {s}"));
                return Ok(volcengine_success(tiers, plan));
            }
            empty_responses.push(summarize("GetAFPUsage", &body));
        }
    }

    // 2) Coding Plan：GetCodingPlanUsage
    match volcengine_openapi_call(
        &region,
        access_key_id,
        secret_access_key,
        "GetCodingPlanUsage",
    )
    .await
    {
        VolcCall::Auth(detail) => return Ok(volcengine_auth_error(detail)),
        VolcCall::Transient(detail) => return Err(format!("GetCodingPlanUsage: {detail}")),
        VolcCall::Soft(detail) => soft_errors.push(format!("GetCodingPlanUsage: {detail}")),
        VolcCall::Body(body) => {
            let result = body.get("Result").unwrap_or(&body);
            let tiers = parse_coding_plan_tiers(result);
            if !tiers.is_empty() {
                return Ok(volcengine_success(tiers, Some("Coding Plan".to_string())));
            }
            empty_responses.push(summarize("GetCodingPlanUsage", &body));
        }
    }

    if !soft_errors.is_empty() {
        Ok(make_error(soft_errors.join("; ")))
    } else if !empty_responses.is_empty() {
        // 签名已通过、请求到达业务层，但响应里没有可解析的额度。带上原始响应，
        // 便于核对真实字段名/包裹层，或确认确实未订阅。
        Ok(make_error(format!(
            "No active subscription found (signature OK). Raw: {}",
            empty_responses.join(" || ")
        )))
    } else {
        Ok(make_error(
            "No active Agent Plan or Coding Plan subscription found for this credential"
                .to_string(),
        ))
    }
}

// ── 公开入口 ────────────────────────────────────────────────

/// 构造"凭据缺失 / 域名未命中"的失败结果（NotFound 状态 + 明确错误文案）。
fn coding_plan_not_found(error: &str) -> SubscriptionQuota {
    SubscriptionQuota {
        tool: "coding_plan".to_string(),
        credential_status: CredentialStatus::NotFound,
        credential_message: None,
        success: false,
        tiers: vec![],
        extra_usage: None,
        error: Some(error.to_string()),
        queried_at: None,
    }
}

// ── 智谱团队套餐（Team Plan）──────────────────────────────────
//
// 与个人版的差异仅在请求构造（参考 token-monitor/src/shared/zaiTeamLimits.js）：
// - 固定走国内站 open.bigmodel.cn（团队版仅存在于国内站，z.ai 国际站无 team 档）
// - 同一 quota 路径加 `?type=2`
// - 额外请求头 bigmodel-organization / bigmodel-project（两者 + api_key 缺一不可）
// 响应 shape 与个人版完全一致 → 复用 zhipu_quota_from_body / parse_zhipu_token_tiers。
const ZHIPU_TEAM_QUOTA_URL: &str = "https://open.bigmodel.cn/api/monitor/usage/quota/limit";

async fn query_zhipu_team(
    api_key: &str,
    organization_id: &str,
    project_id: &str,
) -> Result<SubscriptionQuota, String> {
    query_zhipu_team_at(ZHIPU_TEAM_QUOTA_URL, api_key, organization_id, project_id).await
}

/// 团队版额度查询。`quota_url_base` 为不含 query 的 quota 端点；团队版与个人版同路径，
/// 靠 `?type=2` 区分（在此拼上）。拆出 url 参数便于用本地 server 测试请求形状。
async fn query_zhipu_team_at(
    quota_url_base: &str,
    api_key: &str,
    organization_id: &str,
    project_id: &str,
) -> Result<SubscriptionQuota, String> {
    let client = crate::proxy::http_client::get();
    let url = format!("{quota_url_base}?type=2");

    let resp = client
        .get(&url)
        .header("Authorization", api_key) // 与个人版一致：智谱不加 Bearer 前缀
        .header("bigmodel-organization", organization_id)
        .header("bigmodel-project", project_id)
        .header("Content-Type", "application/json")
        .header("Accept-Language", "en-US,en")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => return Err(format!("Network error: {e}")),
    };

    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Ok(SubscriptionQuota {
            tool: "coding_plan".to_string(),
            credential_status: CredentialStatus::Expired,
            credential_message: Some("Invalid API key".to_string()),
            success: false,
            tiers: vec![],
            extra_usage: None,
            error: Some(format!("Authentication failed (HTTP {status})")),
            queried_at: Some(now_millis()),
        });
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Ok(make_error(format!("API error (HTTP {status}): {body}")));
    }

    // 先 bytes() 再解析：读体失败（超时/连接中断）是瞬时 → Err；拿到完整响应体
    // 后解析失败才是确定性。reqwest 的 json() 把读体错误也包成 decode，无法区分。
    let raw = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {e}")),
    };
    let body: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return Ok(make_error(format!("Failed to parse response: {e}"))),
    };

    Ok(zhipu_quota_from_body(&body))
}

/// 查询编程套餐额度。瞬时传输失败（网络/超时/读体中断）返回 `Err`（前端 reject →
/// retry + 保留上次成功值）；确定性失败（凭据缺失/未知域名/鉴权/非 2xx/业务错误）
/// 返回 `Ok(success:false)` 立即透出文案。判定按 reqwest 错误种类在折叠点完成。
///
/// `coding_plan_provider` 显式标识用于无法靠 base_url 区分的供应商（当前为智谱团队版
/// `zhipu_team`——其 base_url 与个人版智谱相同）；其余情况走 `detect_provider`。
pub async fn get_coding_plan_quota(
    base_url: &str,
    api_key: &str,
    access_key_id: Option<&str>,
    secret_access_key: Option<&str>,
    coding_plan_provider: Option<&str>,
    team_organization_id: Option<&str>,
    team_project_id: Option<&str>,
) -> Result<SubscriptionQuota, String> {
    // 智谱团队版：base_url 与个人版智谱（open.bigmodel.cn）相同，detect_provider 无法
    // 区分，必须靠显式 coding_plan_provider == "zhipu_team" 路由。需 api_key + 组织 ID
    // + 项目 ID 三者齐全，缺任一返回 NotFound 引导补全。
    if coding_plan_provider
        .map(|p| p.eq_ignore_ascii_case("zhipu_team"))
        .unwrap_or(false)
    {
        let organization_id = team_organization_id.unwrap_or("").trim();
        let project_id = team_project_id.unwrap_or("").trim();
        if api_key.trim().is_empty() || organization_id.is_empty() || project_id.is_empty() {
            return Ok(coding_plan_not_found(
                "Zhipu team plan needs the API key + organization ID + project ID",
            ));
        }
        return query_zhipu_team(api_key, organization_id, project_id).await;
    }

    let provider = match detect_provider(base_url) {
        Some(p) => p,
        // 域名未命中已知套餐供应商（如第三方中转站）：给出明确错误而非静默失败
        None => return Ok(coding_plan_not_found("Unknown coding plan provider")),
    };

    // 火山方舟走控制面 AK/SK 签名（区别于其他供应商的数据面 Bearer api_key），凭据
    // 校验与查询路径都不同，单独分支提前处理。
    if let CodingPlanProvider::Volcengine = provider {
        let ak = access_key_id.unwrap_or("").trim();
        let sk = secret_access_key.unwrap_or("").trim();
        if ak.is_empty() || sk.is_empty() {
            return Ok(coding_plan_not_found(
                "Volcengine usage query needs the account AccessKey ID + Secret (not the inference API key)",
            ));
        }
        return query_volcengine(base_url, ak, sk).await;
    }

    // 其余供应商：数据面 Bearer api_key。
    // 与 balance::get_balance 一致：给出明确错误，避免 footer 显示无信息的失败
    if api_key.trim().is_empty() {
        return Ok(coding_plan_not_found("API key is empty"));
    }

    match provider {
        CodingPlanProvider::Kimi => query_kimi(api_key).await,
        CodingPlanProvider::ZhipuCn | CodingPlanProvider::ZhipuEn => {
            query_zhipu(base_url, api_key).await
        }
        CodingPlanProvider::MiniMaxCn => query_minimax(api_key, true).await,
        CodingPlanProvider::MiniMaxEn => query_minimax(api_key, false).await,
        CodingPlanProvider::ZenMux => query_zenmux(base_url, api_key).await,
        CodingPlanProvider::OpencodeGo => query_opencode_go(api_key).await,
        // 火山已在上面的 AK/SK 分支提前返回，此处不可达。
        CodingPlanProvider::Volcengine => {
            unreachable!("volcengine handled via AK/SK branch above")
        }
    }
}
