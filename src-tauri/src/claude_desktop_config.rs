use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "macos", windows, target_os = "linux"))]
use crate::config::get_home_dir;
use crate::config::{atomic_write, delete_file, read_json_file, write_json_file};
use crate::database::Database;
use crate::database::CLAUDE_DESKTOP_OFFICIAL_PROVIDER_ID;
use crate::error::AppError;
use crate::provider::{ClaudeDesktopMode, Provider};

pub const PROFILE_ID: &str = "00000000-0000-4000-8000-000000157210";
pub const PROFILE_NAME: &str = "CC Switch";

#[cfg(any(target_os = "macos", windows, target_os = "linux", test))]
const CONFIG_FILE: &str = "claude_desktop_config.json";
#[cfg(any(target_os = "macos", windows, target_os = "linux", test))]
const CONFIG_LIBRARY_DIR: &str = "configLibrary";
const GATEWAY_TOKEN_SETTING_KEY: &str = "claude_desktop_gateway_token";
const CLAUDE_DESKTOP_PROXY_PREFIX: &str = "/claude-desktop";
const DEFAULT_CREATED_AT: &str = "2024-01-01T00:00:00Z";
const MIMO_REDACTED_THINKING_PLACEHOLDER: &str = "[redacted thinking]";
const MIMO_TOOL_CALL_THINKING_PLACEHOLDER: &str = "tool call";

/// Claude Desktop 模型菜单识别的 route ID 前缀。
pub const CLAUDE_ROUTE_PREFIX: &str = "claude-";
/// 替代前缀（与前端 `ANTHROPIC_CLAUDE_ROUTE_PREFIX` 一致）。
pub const ANTHROPIC_CLAUDE_ROUTE_PREFIX: &str = "anthropic/claude-";
/// Claude Code env 中通过 `[1M]` 后缀声明 1M 上下文能力（匹配用 `eq_ignore_ascii_case`）。
/// Claude Desktop schema 不接受此后缀，import 边界翻译为 `supports1m` 字段。
pub const ONE_M_CONTEXT_MARKER: &str = "[1m]";

const CURRENT_OPUS_ROUTE_ID: &str = "claude-opus-5";
const LEGACY_OPUS_ROUTE_ID: &str = "claude-opus-4-8";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopDefaultRoute {
    pub route_id: &'static str,
    pub env_key: &'static str,
    #[serde(rename = "supports1m")]
    pub supports_1m: bool,
}

pub const DEFAULT_PROXY_ROUTES: &[ClaudeDesktopDefaultRoute] = &[
    ClaudeDesktopDefaultRoute {
        route_id: "claude-sonnet-5",
        env_key: "ANTHROPIC_DEFAULT_SONNET_MODEL",
        supports_1m: true,
    },
    ClaudeDesktopDefaultRoute {
        route_id: CURRENT_OPUS_ROUTE_ID,
        env_key: "ANTHROPIC_DEFAULT_OPUS_MODEL",
        supports_1m: true,
    },
    ClaudeDesktopDefaultRoute {
        route_id: "claude-haiku-4-5",
        env_key: "ANTHROPIC_DEFAULT_HAIKU_MODEL",
        supports_1m: true,
    },
    // fable 置于末尾：next_catalog_safe_route_id 给非安全品牌 route 借用合法
    // 角色名时仍按 sonnet→opus→haiku 顺序分配（向后兼容既有 catalog），不会把
    // 无关品牌模型借用成 fable 顶配档名。UI 行序由前端 ROLE_ORDER 独立控制为
    // Sonnet/Opus/Fable/Haiku（所有 proxy 路径都经 normalizeProxyRows 重排），
    // 与此处物理顺序无关。
    ClaudeDesktopDefaultRoute {
        route_id: "claude-fable-5",
        env_key: "ANTHROPIC_DEFAULT_FABLE_MODEL",
        supports_1m: true,
    },
];

#[derive(Debug, Clone)]
struct ClaudeDesktopPaths {
    normal_config_path: PathBuf,
    threep_config_path: PathBuf,
    config_library_path: PathBuf,
    profile_path: PathBuf,
    meta_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectGatewayCredentials {
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Clone)]
struct FileSnapshot {
    path: PathBuf,
    content: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopStatus {
    pub supported: bool,
    pub configured: bool,
    pub applied_id: Option<String>,
    pub profile_path: Option<String>,
    pub config_library_path: Option<String>,
    pub mode: Option<ClaudeDesktopMode>,
    pub expected_base_url: Option<String>,
    pub actual_base_url: Option<String>,
    pub proxy_running: bool,
    pub stale_raw_models: bool,
    pub missing_route_mappings: bool,
    pub gateway_token_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelRoute {
    pub route_id: String,
    pub upstream_model: String,
    pub label_override: Option<String>,
    pub supports_1m: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InferenceModelSpec {
    name: String,
    label_override: Option<String>,
    supports_1m: bool,
}

pub fn is_claude_safe_model_id(model: &str) -> bool {
    let normalized = model.trim().to_ascii_lowercase();
    if normalized.contains(ONE_M_CONTEXT_MARKER) {
        return false;
    }

    let Some(route_tail) = normalized
        .strip_prefix(ANTHROPIC_CLAUDE_ROUTE_PREFIX)
        .or_else(|| normalized.strip_prefix(CLAUDE_ROUTE_PREFIX))
    else {
        return false;
    };

    // 角色前缀后必须还有实际模型标识，拒绝 claude-sonnet- 这类退化值
    // （否则会写入 profile 并触发 Claude Desktop fail-all 拒收整组）。
    // Claude Desktop 1.12603.1+ 的 fail-all validator 角色白名单已纳入 fable
    // （app.asar 内 ["sonnet","opus","haiku","fable","mythos"]），故 claude-fable-*
    // 可安全写入 profile。mythos 官方未公开发布，暂不暴露给用户。
    ["sonnet-", "opus-", "haiku-", "fable-"]
        .iter()
        .any(|prefix| {
            route_tail
                .strip_prefix(prefix)
                .is_some_and(|rest| !rest.is_empty())
        })
}

pub fn proxy_model_routes(provider: &Provider) -> Result<Vec<ResolvedModelRoute>, AppError> {
    let routes = provider
        .meta
        .as_ref()
        .map(|meta| &meta.claude_desktop_model_routes)
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.routes_missing",
                "Claude Desktop 本地路由模式缺少模型路由映射",
                "Claude Desktop proxy mode is missing model route mappings",
            )
        })?;

    let reserved_route_ids = routes
        .keys()
        .map(|route_id| route_id.trim())
        .filter(|route_id| is_claude_safe_model_id(route_id))
        .map(str::to_string)
        .collect::<std::collections::HashSet<_>>();
    let mut result = Vec::new();
    let mut entries = routes.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(left, _)| *left);
    for (route_id, route) in entries {
        let supports_1m = route.supports_1m.unwrap_or(false);
        let route_id = route_id.trim();
        let upstream_model = route.model.trim();
        if route_id.is_empty() || upstream_model.is_empty() {
            continue;
        }
        let repaired_route_id = if is_claude_safe_model_id(route_id) {
            route_id.to_string()
        } else {
            next_catalog_safe_route_id(&result, &reserved_route_ids)
        };
        result.push(ResolvedModelRoute {
            route_id: repaired_route_id,
            upstream_model: upstream_model.to_string(),
            label_override: route
                .label_override
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    (!is_claude_safe_model_id(route_id)).then(|| upstream_model.to_string())
                }),
            supports_1m,
        });
    }

    result.sort_by(|a, b| a.route_id.cmp(&b.route_id));
    result.dedup_by(|a, b| a.route_id == b.route_id);

    if result.is_empty() {
        return Err(AppError::localized(
            "claude_desktop.provider.routes_missing",
            "Claude Desktop 本地路由模式至少需要一个模型路由映射",
            "Claude Desktop proxy mode requires at least one model route mapping",
        ));
    }

    Ok(result)
}

fn next_catalog_safe_route_id(
    existing: &[ResolvedModelRoute],
    reserved: &std::collections::HashSet<String>,
) -> String {
    if let Some(default_route) = DEFAULT_PROXY_ROUTES
        .iter()
        .map(|route| route.route_id)
        .find(|route_id| {
            !reserved.contains(*route_id)
                && !existing.iter().any(|route| route.route_id == *route_id)
        })
    {
        return default_route.to_string();
    }

    let mut index = 2usize;
    loop {
        let route_id = format!("{}-r{index}", DEFAULT_PROXY_ROUTES[0].route_id);
        if !reserved.contains(&route_id) && !existing.iter().any(|route| route.route_id == route_id)
        {
            return route_id;
        }
        index += 1;
    }
}

pub fn map_proxy_request_model(mut body: Value, provider: &Provider) -> Result<Value, AppError> {
    let requested_raw = body
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.model_missing",
                "Claude Desktop 请求缺少 model 字段",
                "Claude Desktop request is missing the model field",
            )
        })?;
    let requested = strip_one_m_suffix_for_route_lookup(&requested_raw);

    let routes = proxy_model_routes(provider)?;
    let upstream_model = routes
        .iter()
        .find(|r| r.route_id == requested)
        .or_else(|| {
            routes
                .iter()
                .find(|r| is_compatible_opus_route_alias(&r.route_id, requested))
        })
        .map(|route| route.upstream_model.clone())
        .or_else(|| legacy_raw_route_upstream_model(provider, requested))
        .or_else(|| {
            // 角色关键词回落:Claude Desktop 的部分调用(如子 agent)会请求带发布
            // 日期后缀的完整官方名(claude-haiku-4-5-20251001),与 manifest 暴露的
            // 简短 route_id(claude-haiku-4-5)不精确相等。按 opus/haiku/fable/sonnet
            // 归类到同档已配置路由,对齐 Claude Code model_mapper 的宽松匹配。
            // 匹配前已剥离本地 [1m] 标记；这里仍只对 Claude Desktop 认可的
            // 安全模型名回落，避免非 Claude route 被误映射。
            if !is_claude_safe_model_id(requested) {
                return None;
            }
            let role = claude_role_keyword(requested)?;
            routes
                .iter()
                .find(|route| claude_role_keyword(&route.route_id) == Some(role))
                // 老用户只配了 Sonnet/Opus/Haiku 三档时，fable 请求降级到 opus 档，
                // 与官方安全分类器的降级方向一致，避免 route_unknown 硬错误。
                // 用户一旦显式配置 fable 档，上面的精确角色匹配会优先命中。
                .or_else(|| {
                    (role == "fable")
                        .then(|| {
                            routes
                                .iter()
                                .find(|route| claude_role_keyword(&route.route_id) == Some("opus"))
                        })
                        .flatten()
                })
                .map(|route| route.upstream_model.clone())
        })
        .ok_or_else(|| {
            AppError::localized(
                "claude_desktop.provider.route_unknown",
                format!("Claude Desktop 模型路由未配置: {requested_raw}"),
                format!("Claude Desktop model route is not configured: {requested_raw}"),
            )
        })?;

    body["model"] = json!(upstream_model);
    if should_normalize_mimo_anthropic_thinking_history(provider, &upstream_model) {
        normalize_mimo_anthropic_thinking_history(&mut body);
    }
    Ok(body)
}

fn strip_one_m_suffix_for_route_lookup(model: &str) -> &str {
    let trimmed = model.trim();
    let marker = ONE_M_CONTEXT_MARKER.as_bytes();
    let bytes = trimmed.as_bytes();
    if bytes.len() >= marker.len()
        && bytes[bytes.len() - marker.len()..].eq_ignore_ascii_case(marker)
    {
        return trimmed[..trimmed.len() - marker.len()].trim_end();
    }
    trimmed
}

fn legacy_raw_route_upstream_model(provider: &Provider, requested: &str) -> Option<String> {
    provider
        .meta
        .as_ref()?
        .claude_desktop_model_routes
        .iter()
        .find(|(route_id, _)| route_id.trim() == requested)
        .and_then(|(_, route)| {
            let upstream_model = route.model.trim();
            (!upstream_model.is_empty()).then(|| upstream_model.to_string())
        })
}

fn is_compatible_opus_route_alias(route_id: &str, requested: &str) -> bool {
    matches!(
        (route_id, requested),
        (CURRENT_OPUS_ROUTE_ID, LEGACY_OPUS_ROUTE_ID)
            | (LEGACY_OPUS_ROUTE_ID, CURRENT_OPUS_ROUTE_ID)
    )
}

/// 按角色关键词(opus / haiku / fable / sonnet)归类一个 Claude 模型名/route_id。
/// 仅在命中明确角色词时返回 Some,未知模型返回 None(不回落,保持精确报错语义)。
/// 与前端 `routeRoleFromId` 同序(opus → haiku → fable → sonnet)。
fn claude_role_keyword(model: &str) -> Option<&'static str> {
    let normalized = model.to_ascii_lowercase();
    if normalized.contains("opus") {
        Some("opus")
    } else if normalized.contains("haiku") {
        Some("haiku")
    } else if normalized.contains("fable") {
        Some("fable")
    } else if normalized.contains("sonnet") {
        Some("sonnet")
    } else {
        None
    }
}

fn should_normalize_mimo_anthropic_thinking_history(
    provider: &Provider,
    upstream_model: &str,
) -> bool {
    if !provider_uses_anthropic_messages_format(provider) {
        return false;
    }

    is_mimo_identifier(upstream_model) || provider_has_mimo_endpoint(provider)
}

fn provider_uses_anthropic_messages_format(provider: &Provider) -> bool {
    let api_format = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.api_format.as_deref())
        .or_else(|| {
            provider
                .settings_config
                .get("api_format")
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .unwrap_or("anthropic");

    api_format.is_empty() || api_format == "anthropic"
}

fn provider_has_mimo_endpoint(provider: &Provider) -> bool {
    let settings = &provider.settings_config;
    [
        settings
            .get("env")
            .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
            .and_then(Value::as_str),
        settings.get("base_url").and_then(Value::as_str),
        settings.get("baseURL").and_then(Value::as_str),
        settings.get("apiEndpoint").and_then(Value::as_str),
    ]
    .into_iter()
    .flatten()
    .any(is_mimo_identifier)
}

fn is_mimo_identifier(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("mimo") || value.contains("xiaomimimo")
}

fn normalize_mimo_anthropic_thinking_history(body: &mut Value) {
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };

    for message in messages {
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }

        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        if !content
            .iter()
            .any(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
        {
            continue;
        }

        let mut has_thinking = false;
        for block in content.iter_mut() {
            match block.get("type").and_then(Value::as_str) {
                Some("thinking") => {
                    let has_non_empty_thinking = block
                        .get("thinking")
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty());
                    if let Some(obj) = block.as_object_mut() {
                        obj.remove("signature");
                    }
                    if has_non_empty_thinking {
                        has_thinking = true;
                    } else if let Some(obj) = block.as_object_mut() {
                        obj.insert(
                            "thinking".to_string(),
                            json!(MIMO_TOOL_CALL_THINKING_PLACEHOLDER),
                        );
                        has_thinking = true;
                    }
                }
                Some("redacted_thinking") => {
                    *block = json!({
                        "type": "thinking",
                        "thinking": MIMO_REDACTED_THINKING_PLACEHOLDER
                    });
                    has_thinking = true;
                }
                _ => {}
            }
        }

        if !has_thinking {
            content.insert(
                0,
                json!({
                    "type": "thinking",
                    "thinking": MIMO_TOOL_CALL_THINKING_PLACEHOLDER
                }),
            );
        }
    }
}

/// Flatpak keeps an app's XDG_CONFIG_HOME inside its sandbox. Claude Desktop
/// installed natively uses the host's ~/.config by default, which must be
/// exposed through the Flatpak filesystem permissions.
///
/// This is intentionally the host *default* configuration directory. We do
/// not expose a directory override or attempt to recover a host-custom
/// XDG_CONFIG_HOME: Flatpak replaces that variable with its private path, so
/// its original host value is not available reliably from the sandbox. Users
/// with a custom host XDG_CONFIG_HOME should run the native CC Switch package.
#[cfg(target_os = "linux")]
fn linux_config_dir() -> PathBuf {
    let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
    linux_config_dir_from_home(&get_home_dir(), xdg_config_home.as_deref(), is_flatpak())
}

#[cfg(any(target_os = "linux", all(test, unix)))]
fn linux_config_dir_from_home(
    home: &Path,
    xdg_config_home: Option<&Path>,
    running_in_flatpak: bool,
) -> PathBuf {
    if running_in_flatpak {
        return home.join(".config");
    }

    xdg_config_home
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| home.join(".config"))
}

#[cfg(target_os = "linux")]
fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").is_file()
}

#[cfg(target_os = "linux")]
fn linux_paths_from_config_dir(config_dir: &Path) -> ClaudeDesktopPaths {
    paths_from_dirs(config_dir.join("Claude"), config_dir.join("Claude-3p"))
}

#[cfg(target_os = "macos")]
fn macos_paths_from_home(home: &Path) -> ClaudeDesktopPaths {
    let app_support = home.join("Library").join("Application Support");
    paths_from_dirs(app_support.join("Claude"), app_support.join("Claude-3p"))
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
fn unsupported_platform_error() -> AppError {
    AppError::localized(
        "claude_desktop.unsupported_platform",
        "当前平台暂不支持 Claude Desktop 3P 配置。支持的平台：macOS、Windows 和 Linux。",
        "Claude Desktop 3P configuration is not supported on this platform yet. Supported platforms: macOS, Windows, and Linux.",
    )
}
