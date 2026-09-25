use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::{get_home_dir, path_is_within, read_json_file};
use crate::error::AppError;
use crate::model_capabilities::{image_input_capability_from_modalities, ImageInputCapability};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use toml_edit::DocumentMut;

pub const CC_SWITCH_CODEX_MODEL_PROVIDER_ID: &str = "custom";
/// Temporary model-provider id used while the built-in `codex-official`
/// provider is routed through CC Switch.  A dedicated id is an ownership
/// marker: unlike a generic localhost `base_url`, it can be detected and
/// cleaned up without mistaking a user's own local provider for takeover.
pub const CC_SWITCH_CODEX_OFFICIAL_PROXY_PROVIDER_ID: &str = "cc-switch-official";
pub const CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME: &str = "cc-switch-model-catalog.json";
const CODEX_PROXY_AUTH_PLACEHOLDER: &str = "PROXY_MANAGED";

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// Generating a ProxyChat catalog only needs one stable Codex model template per
// process. Without this cache every provider switch/takeover can start the
// Codex CLI again, which is especially expensive for npm-installed `codex.cmd`
// on Windows. Tests deliberately bypass the global cache because they isolate
// CODEX_HOME and seed different model templates.
#[cfg(not(test))]
static CODEX_MODEL_CATALOG_TEMPLATE_CACHE: OnceCell<Value> = OnceCell::new();

/// Top-level `config.toml` key that controls Codex's built-in web-search tool.
pub(crate) const CODEX_WEB_SEARCH_FIELD: &str = "web_search";
/// Value that disables the web-search tool. Some native `/responses` gateways
/// reject a `web_search` tool with `responses_feature_not_supported` ("tool type
/// 'web_search' is not supported by this gateway phase"), so for those we write
/// this per the vendors' official Codex docs. Also doubles as cc-switch's
/// ownership sentinel: we only ever remove a `web_search` key whose value equals
/// this string, never a user's own setting.
pub(crate) const CODEX_WEB_SEARCH_DISABLED: &str = "disabled";

/// Native `/responses` gateways whose first-party models do NOT support the Codex
/// `web_search` hosted tool. A BLACKLIST (default-on): everything not listed keeps
/// Codex's default, so relays/aggregators fronting real GPT — and any unknown
/// provider — are never touched. This avoids a whitelist's dangerous failure mode
/// (a fragile "is this GPT?" heuristic wrongly keeping web_search ON → hard 400);
/// the blacklist's failure mode is the safe, recoverable one (a not-yet-listed
/// broken gateway errors once → add it here).
///
/// Matched two ways so an aggregator (e.g. SiliconFlow) fronting these vendors'
/// models is also caught:
/// - `base_url` host substring, and
/// - the model id's brand prefix (after stripping any `vendor/` path segment).
///
/// Verified 2026-06-28 doc audit — reject: MiMo (hard 400), LongCat (official
/// config ships `web_search = "disabled"`), MiniMax (tool-type enum `['function']`
/// only), and Qwen3-Coder models (百炼 marks built-in tools unsupported for
/// the coder series). Deliberately NOT listed by host: 火山方舟豆包, general
/// 阿里百炼 Qwen models that support built-in web_search, and GPT-native relays.
const CODEX_WEB_SEARCH_REJECT_HOSTS: &[&str] = &[
    "xiaomimimo.com", // Xiaomi MiMo (api.xiaomimimo.com, token-plan-cn.xiaomimimo.com)
    "longcat.chat",   // Meituan LongCat (api.longcat.chat)
    "minimax.io",     // MiniMax global (api.minimax.io)
    "minimax.cn",     // MiniMax CN (current official endpoint)
    "minimaxi.com",   // MiniMax CN (legacy endpoint)
    // StepFun Responses API currently supports only `function` tools:
    // platform.stepfun.com/docs/zh/api-reference/responses/responses-create
    "stepfun.com",
    "stepfun.ai",
    // Conservative (unverified, not a confirmed reject): Baidu Qianfan's
    // pay-as-you-go Responses guide documents only `function` / `mcp` tools
    // (cloud.baidu.com/doc/qianfan-docs/s/4mi400l1m). Host-exact; Qianfan's
    // Chat plans on the same domain are ProxyChat and never consult this list.
    "qianfan.baidubce.com",
    // Conservative (unverified): iFlytek Astron Coding Plan fronts third-party
    // models behind one Responses gateway with no documented hosted-tool
    // support (www.xfyun.cn/doc/spark/CodingPlan.html).
    "xf-yun.com",
    // Zhipu GLM CN / global (open.bigmodel.cn, api.z.ai): the native Responses
    // gateway's tool-type enum is `function | web_search_preview |
    // code_interpreter | mcp` (verbatim from the #6944 400 body) — Codex's
    // `web_search` hosted tool is not in it. Matched on host labels (see
    // `codex_url_host_matches_any`), so `xyz.ai` never collides with `z.ai`.
    "bigmodel.cn",
    "z.ai",
];

/// Brand prefixes of models whose native gateways reject `web_search`, matched
/// against the model id's last `/`-segment so aggregator ids like
/// `MiniMaxAI/MiniMax-M3` are caught. Exact brand names (not a fuzzy heuristic),
/// so a supporting gateway is never wrongly matched.
const CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES: &[&str] =
    &["mimo", "longcat", "minimax", "qwen3-coder", "glm"];

const CODEX_MODEL_CATALOG_TEMPLATE_SLUG: &str = "gpt-5.5";

/// Which Codex tool surface the generated model catalog should target.
///
/// - `ProxyChat`: cc-switch's proxy takes over and converts Responses<->Chat,
///   so the catalog keeps Codex's default tool set (incl. the freeform
///   `apply_patch` custom tool, which the proxy rewrites to a function tool).
/// - `NativeResponses`: Codex talks directly to a provider's native
///   `/responses` endpoint (no proxy). Such gateways (e.g. Xiaomi MiMo,
///   MiniMax) reject `type=="custom"` tools, so the catalog must suppress the
///   freeform `apply_patch` and rely on `shell_type="shell_command"` for edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexCatalogToolProfile {
    ProxyChat,
    /// Copilot selects Responses or Chat per model, but both paths run through
    /// the local proxy. Keep proxy-compatible tools while disabling Codex's
    /// hosted web-search endpoint, which Copilot does not expose.
    Copilot,
    NativeResponses,
    /// Codex talks (through cc-switch's proxy) to a native Anthropic Messages
    /// gateway. Like `NativeResponses` it must suppress Codex's freeform custom
    /// tools — the Responses→Anthropic transform keeps only `function` tools.
    /// Additionally the Codex `web_search` hosted tool is unusable on this path
    /// (the transform drops it), so it is always disabled — see
    /// `prepare_codex_config_text_with_model_catalog`.
    Anthropic,
}

impl CodexCatalogToolProfile {
    /// Pick the catalog tool profile from a provider's `apiFormat` meta value.
    ///
    /// Prefer [`crate::proxy::providers::codex::resolve_codex_catalog_tool_profile`],
    /// which also honors settings-level `apiFormat` and the TOML `wire_api` (matching
    /// the proxy router). This string-only mapping is the fallback for non-Anthropic
    /// cases.
    pub fn from_api_format(api_format: Option<&str>) -> Self {
        match api_format {
            Some("anthropic") => CodexCatalogToolProfile::Anthropic,
            // Native (direct) Responses gateways reject Codex's freeform custom
            // tools (apply_patch, etc.); strip them via the NativeResponses profile.
            Some("openai_responses") => CodexCatalogToolProfile::NativeResponses,
            _ => CodexCatalogToolProfile::ProxyChat,
        }
    }
}

/// Reserved built-in provider IDs from OpenAI Codex's config/model-provider
/// catalog. Keep in sync with Codex `RESERVED_MODEL_PROVIDER_IDS` (0.149:
/// exactly these five; 0.148 is the same minus `amazon-bedrock-runtime`).
/// `oss` / `ollama-chat` are NOT reserved on 0.148/0.149 — both load as
/// ordinary custom tables — so listing them here would strand their bearer
/// token in the ignored top level. Mirror: providerConfigUtils.ts.
const CODEX_RESERVED_MODEL_PROVIDER_IDS: &[&str] = &[
    "amazon-bedrock",
    "amazon-bedrock-runtime",
    "openai",
    "ollama",
    "lmstudio",
];

/// 获取 Codex 配置目录路径
pub fn get_codex_config_dir() -> PathBuf {
    if let Some(custom) = crate::settings::get_codex_override_dir() {
        return custom;
    }

    get_home_dir().join(".codex")
}

/// 获取 Codex auth.json 路径
pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

/// 获取 Codex config.toml 路径
pub fn get_codex_config_path() -> PathBuf {
    get_codex_config_dir().join("config.toml")
}

/// 读取 `~/.codex/config.toml`，若不存在返回空字符串
pub fn read_codex_config_text() -> Result<String, AppError> {
    let path = get_codex_config_path();
    if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))
    } else {
        Ok(String::new())
    }
}

fn active_codex_model_provider_id(doc: &DocumentMut) -> Option<String> {
    doc.get("model_provider")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
}

pub(crate) fn is_custom_codex_model_provider_id(id: &str) -> bool {
    // Exact match, mirroring upstream: both the built-in provider lookup and
    // validate_reserved_model_provider_ids are case-sensitive, so `OpenAI`
    // etc. are legitimate custom ids whose tables must receive the token.
    // Keep in sync with the frontend list in src/utils/providerConfigUtils.ts.
    let id = id.trim();
    !id.is_empty() && !CODEX_RESERVED_MODEL_PROVIDER_IDS.contains(&id)
}

pub fn extract_codex_auth_api_key(auth: &Value) -> Option<String> {
    auth.get("OPENAI_API_KEY")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
}

pub fn extract_codex_api_key(auth: Option<&Value>, config_text: Option<&str>) -> Option<String> {
    auth.and_then(extract_codex_auth_api_key)
        .or_else(|| config_text.and_then(extract_codex_experimental_bearer_token))
}

/// Extract the upstream base URL from a Codex `config.toml` string.
///
/// Prefers the active `[model_providers.<model_provider>].base_url`, falling
/// back to a top-level `base_url`. Deliberately never reads a non-active
/// `[model_providers.*]` section — the frontend `extractCodexBaseUrl`
/// (`getRecoverableBaseUrlAssignments`) excludes those too, and a leftover
/// section unrelated to the active provider must not leak into `{{baseUrl}}`.
pub fn extract_codex_base_url(config_text: &str) -> Option<String> {
    let doc = config_text.parse::<toml::Value>().ok()?;

    if let Some(active_provider) = doc.get("model_provider").and_then(|v| v.as_str()) {
        if let Some(base_url) = doc
            .get("model_providers")
            .and_then(|providers| providers.get(active_provider))
            .and_then(|provider| provider.get("base_url"))
            .and_then(|v| v.as_str())
        {
            return Some(base_url.to_string());
        }
    }

    doc.get("base_url")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn parse_codex_positive_u64(value: Option<&Value>) -> Option<u64> {
    match value {
        Some(Value::Number(n)) => n.as_u64().filter(|v| *v > 0),
        Some(Value::String(s)) => s.trim().parse::<u64>().ok().filter(|v| *v > 0),
        _ => None,
    }
}

fn extract_codex_top_level_u64(config_text: &str, field: &str) -> Option<u64> {
    let doc = config_text.parse::<toml::Value>().ok()?;
    doc.get(field)
        .and_then(|value| value.as_integer())
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn codex_catalog_input_modalities(
    model: &str,
    declared_modalities: Option<&[String]>,
) -> Vec<String> {
    let modalities = match image_input_capability_from_modalities(model, declared_modalities) {
        ImageInputCapability::Unsupported => &["text"][..],
        ImageInputCapability::Supported | ImageInputCapability::Unknown => &["text", "image"][..],
    };
    modalities.iter().map(|item| (*item).to_string()).collect()
}

/// Canonical reasoning effort levels Codex understands, with the same
/// descriptions the official gpt-5.5 template uses. `none` disables thinking.
const CODEX_REASONING_LEVEL_DESCRIPTIONS: &[(&str, &str)] = &[
    ("none", "Disable Thinking"),
    ("minimal", "Minimal reasoning"),
    ("low", "Fast responses with lighter reasoning"),
    (
        "medium",
        "Balances speed and reasoning depth for everyday tasks",
    ),
    ("high", "Greater reasoning depth for complex problems"),
    ("xhigh", "Extra high reasoning depth for complex problems"),
    ("max", "Maximum reasoning depth for the hardest problems"),
    ("ultra", "Ultra reasoning depth"),
];

fn codex_reasoning_level_description(effort: &str) -> Option<&'static str> {
    CODEX_REASONING_LEVEL_DESCRIPTIONS
        .iter()
        .find(|(candidate, _)| *candidate == effort)
        .map(|(_, description)| *description)
}

/// User-declared levels reduced to the canonical efforts Codex understands,
/// in canonical (lowest → highest) order regardless of declaration order.
/// Unknown efforts are dropped so a typo can never produce an entry Codex
/// would reject.
fn codex_canonical_efforts(levels: &[String]) -> Vec<&str> {
    CODEX_REASONING_LEVEL_DESCRIPTIONS
        .iter()
        .filter(|(effort, _)| levels.iter().any(|candidate| candidate == effort))
        .map(|(effort, _)| *effort)
        .collect()
}

/// Build a `supported_reasoning_levels` array from user-declared effort values.
fn codex_supported_reasoning_levels(levels: &[String]) -> Value {
    let entries: Vec<Value> = codex_canonical_efforts(levels)
        .into_iter()
        .map(|effort| {
            let description = codex_reasoning_level_description(effort)
                .expect("canonical effort always has a description");
            json!({ "effort": effort, "description": description })
        })
        .collect();
    json!(entries)
}

/// Apply a per-model reasoning-level override onto a catalog entry. Returns
/// true when the override was applied (so callers can skip further work).
/// `template_default` is the base entry's `default_reasoning_level` (from the
/// profile template or an official vendor entry) used as the fallback when the
/// user did not declare one explicitly.
fn apply_codex_reasoning_level_override(
    entry_obj: &mut serde_json::Map<String, Value>,
    template_default: Option<&str>,
    spec: &CodexCatalogModelSpec,
) -> bool {
    let Some(levels) = spec.reasoning_levels.as_deref() else {
        return false;
    };
    let canonical = codex_canonical_efforts(levels);
    if canonical.is_empty() {
        return false;
    }
    let supported = codex_supported_reasoning_levels(levels);
    entry_obj.insert("supported_reasoning_levels".to_string(), supported);

    // Default: explicit user value wins; otherwise keep the base default when
    // it is still supported; otherwise fall back to the highest supported
    // level in canonical order. All candidates are validated against the
    // canonical set so the default can never reference a dropped effort.
    let default_level = spec
        .default_reasoning_level
        .as_deref()
        .filter(|level| canonical.contains(level))
        .or_else(|| template_default.filter(|level| canonical.contains(level)))
        .or_else(|| canonical.last().copied());
    if let Some(default_level) = default_level {
        entry_obj.insert("default_reasoning_level".to_string(), json!(default_level));
    }
    true
}

fn codex_catalog_model_entry(
    template: &Value,
    spec: &CodexCatalogModelSpec,
    priority: usize,
    profile: CodexCatalogToolProfile,
    default_context_window: u64,
) -> Value {
    let mut entry = template.clone();
    let Some(entry_obj) = entry.as_object_mut() else {
        return json!({});
    };

    let display_name = spec.display_name.as_deref().unwrap_or(&spec.model);
    let context_window = spec.context_window.unwrap_or(default_context_window);
    entry_obj.insert("slug".to_string(), json!(spec.model));
    entry_obj.insert("display_name".to_string(), json!(display_name));
    entry_obj.insert("description".to_string(), json!(display_name));
    entry_obj.insert("context_window".to_string(), json!(context_window));
    entry_obj.insert("max_context_window".to_string(), json!(context_window));
    entry_obj.insert("priority".to_string(), json!(1000 + priority));
    entry_obj.insert("additional_speed_tiers".to_string(), json!([]));
    entry_obj.insert("service_tiers".to_string(), json!([]));
    entry_obj.insert("availability_nux".to_string(), Value::Null);
    entry_obj.insert("upgrade".to_string(), Value::Null);

    // Image support is a model capability, not a tool-profile capability.
    // Trust hidden preset metadata first, then the confirmed text-only registry;
    // every unknown model fails open so GPT/relay aliases are never declared
    // text-only merely because a template had a conservative default.
    entry_obj.insert(
        "input_modalities".to_string(),
        json!(codex_catalog_input_modalities(
            &spec.model,
            spec.input_modalities.as_deref(),
        )),
    );

    if !matches!(
        profile,
        CodexCatalogToolProfile::ProxyChat | CodexCatalogToolProfile::Copilot
    ) {
        // Native `/responses` and Anthropic gateways reject / drop Codex's freeform
        // `apply_patch` (type=="custom") tool. Strip any key that would make Codex
        // emit a custom/freeform tool, and rely on shell_type="shell_command" for
        // edits. Defensive even though the native template is already clean
        // (guards against template drift / an accidental gpt-5.5 clone).
        //
        // NOTE: `base_instructions` is NOT stripped — Codex's catalog parser
        // treats it as a REQUIRED field and refuses to load the file without
        // it ("missing field `base_instructions`"). The template carries a
        // neutral identity default; per-vendor official text overrides below.
        for key in [
            "apply_patch_tool_type",
            "web_search_tool_type",
            "tools",
            "model_messages",
        ] {
            entry_obj.remove(key);
        }
        entry_obj.insert("shell_type".to_string(), json!("shell_command"));

        if let Some(base_instructions) = spec
            .base_instructions
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            entry_obj.insert("base_instructions".to_string(), json!(base_instructions));
        }
        if let Some(parallel) = spec.supports_parallel_tool_calls {
            entry_obj.insert("supports_parallel_tool_calls".to_string(), json!(parallel));
        }
    }
    if profile == CodexCatalogToolProfile::Copilot {
        entry_obj.insert(
            "supports_parallel_tool_calls".to_string(),
            json!(spec.supports_parallel_tool_calls.unwrap_or(false)),
        );
    }

    if profile == CodexCatalogToolProfile::ProxyChat {
        // Codex's `original` image detail (full-resolution) is rejected by
        // strict Chat gateways with `400 invalid_request_error`, param
        // `messages.N.content`. Never advertise the capability on the
        // ProxyChat contract so Codex keeps to auto/high.
        entry_obj.insert("supports_image_detail_original".to_string(), json!(false));
    }

    // Per-model reasoning levels override the template's conservative
    // none/high default (e.g. a LiteLLM gateway serving a model that accepts
    // low/medium/high/xhigh/max). Applies to every profile.
    let template_default = template
        .get("default_reasoning_level")
        .and_then(|value| value.as_str());
    apply_codex_reasoning_level_override(entry_obj, template_default, spec);

    entry
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexCatalogModelSpec {
    model: String,
    /// Explicit user value only. Entries fall back to the model id — except
    /// official vendor catalog entries, which keep the vendor's display name.
    display_name: Option<String>,
    /// Explicit user value only. Entries fall back to the config's
    /// `model_context_window` (or 128k) — except official vendor catalog
    /// entries, which keep the vendor's declared window.
    context_window: Option<u64>,
    /// Per-row override for the native template's `supports_parallel_tool_calls`
    /// (e.g. MiniMax=true, MiMo=false). Only consulted for `NativeResponses`.
    supports_parallel_tool_calls: Option<bool>,
    /// Hidden per-row capability declaration from built-in provider metadata.
    /// When omitted, all catalog profiles consult the shared text-only model
    /// registry and otherwise default to `["text", "image"]`.
    input_modalities: Option<Vec<String>>,
    /// Per-row override for the native template's `base_instructions` (the
    /// model identity / system preamble). Carries each vendor's OFFICIAL value
    /// (e.g. MiMo "developed by Xiaomi", MiniMax "based on MiniMax-M3"); falls
    /// back to the template default when absent. Only consulted for
    /// `NativeResponses`.
    base_instructions: Option<String>,
    /// Per-row override for the generated catalog's `supported_reasoning_levels`
    /// (e.g. ["none", "low", "medium", "high", "xhigh", "max"]). When omitted
    /// the template's conservative default (none/high) is kept. Consulted for
    /// every profile; the vendor-catalog path applies it on top of the
    /// official entry.
    reasoning_levels: Option<Vec<String>>,
    /// Per-row override for the generated catalog's `default_reasoning_level`.
    /// Only meaningful together with `reasoning_levels`; when absent the
    /// template default is kept if it is still in the list, otherwise the last
    /// (highest) declared level wins.
    default_reasoning_level: Option<String>,
}

fn codex_catalog_model_specs(settings: &Value) -> Vec<CodexCatalogModelSpec> {
    let Some(models) = settings
        .get("modelCatalog")
        .and_then(|catalog| catalog.get("models"))
        .and_then(|models| models.as_array())
    else {
        return Vec::new();
    };

    let mut seen = std::collections::HashSet::new();
    let mut specs = Vec::new();

    for model_config in models {
        let Some(model) = model_config
            .get("model")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|model| !model.is_empty())
        else {
            continue;
        };

        if !seen.insert(model.to_string()) {
            continue;
        }

        let display_name = model_config
            .get("displayName")
            .or_else(|| model_config.get("display_name"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string);
        let context_window = parse_codex_positive_u64(
            model_config
                .get("contextWindow")
                .or_else(|| model_config.get("context_window")),
        );

        let supports_parallel_tool_calls = model_config
            .get("supportsParallelToolCalls")
            .or_else(|| model_config.get("supports_parallel_tool_calls"))
            .and_then(|value| value.as_bool());
        let input_modalities = model_config
            .get("inputModalities")
            .or_else(|| model_config.get("input_modalities"))
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|items| !items.is_empty());

        let base_instructions = model_config
            .get("baseInstructions")
            .or_else(|| model_config.get("base_instructions"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string);

        let reasoning_levels = model_config
            .get("reasoningLevels")
            .or_else(|| model_config.get("reasoning_levels"))
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(str::trim)
                    .filter(|level| !level.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|levels| !levels.is_empty());
        let default_reasoning_level = model_config
            .get("defaultReasoningLevel")
            .or_else(|| model_config.get("default_reasoning_level"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|level| !level.is_empty())
            .map(str::to_string);

        specs.push(CodexCatalogModelSpec {
            model: model.to_string(),
            display_name,
            context_window,
            supports_parallel_tool_calls,
            input_modalities,
            base_instructions,
            reasoning_levels,
            default_reasoning_level,
        });
    }

    specs
}

fn find_codex_model_template(catalog: &Value) -> Option<Value> {
    catalog
        .get("models")
        .and_then(|models| models.as_array())
        .and_then(|models| {
            models.iter().find(|model| {
                model.get("slug").and_then(|slug| slug.as_str())
                    == Some(CODEX_MODEL_CATALOG_TEMPLATE_SLUG)
            })
        })
        .cloned()
}

fn load_codex_model_template_from_cache() -> Result<Option<Value>, AppError> {
    let path = get_codex_config_dir().join("models_cache.json");
    if !path.exists() {
        return Ok(None);
    }

    let text = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let catalog: Value = serde_json::from_str(&text).map_err(|e| AppError::json(&path, e))?;
    Ok(find_codex_model_template(&catalog))
}

/// Fixed candidates for locating the `codex` CLI when it is not on the process
/// PATH (common in GUI apps launched outside a terminal).
const CODEX_CLI_FIXED_CANDIDATES: &[&str] = &[
    "codex",                                // PATH (all platforms)
    "/opt/homebrew/bin/codex",              // macOS Apple Silicon Homebrew
    "/usr/local/bin/codex",                 // macOS Intel Homebrew / Linux
    "/home/linuxbrew/.linuxbrew/bin/codex", // Linux Homebrew
];

fn load_codex_model_template_static() -> Option<Value> {
    let text = include_str!("resources/gpt5_5_template.json");
    match serde_json::from_str(text) {
        Ok(template) => Some(template),
        Err(e) => {
            log::warn!("Failed to parse bundled gpt-5.5 template: {e}");
            None
        }
    }
}

/// Bundled clean template for native `/responses` providers. Unlike the
/// gpt-5.5 template it carries NO freeform `apply_patch` / `web_search` tool
/// declarations and no GPT-5 base_instructions, so Codex never emits a
/// `type=="custom"` tool that native gateways (MiMo/MiniMax/…) reject. Edits
/// flow through `shell_type="shell_command"` instead. We deliberately do NOT
/// fall back to `models_cache.json` here (that would reintroduce gpt-5.5's
/// freeform apply_patch).
fn load_codex_native_responses_template() -> Value {
    let text = include_str!("resources/codex_native_responses_template.json");
    serde_json::from_str(text).expect("bundled codex native responses template must be valid JSON")
}

/// Hosts whose native `/responses` gateway publishes an OFFICIAL Codex model
/// catalog (models.json) that cc-switch mirrors verbatim. Matched against
/// `base_url` ONLY — deliberately NOT by model brand, unlike
/// `CODEX_WEB_SEARCH_REJECT_MODEL_PREFIXES`: the official entries GRANT
/// capabilities (freeform `apply_patch`, vendor harness), and an aggregator
/// merely hosting the same model may not honor them. The safe failure
/// direction for aggregators is the neutral template (degraded but working);
/// wrongly granting freeform apply_patch would reintroduce the custom-tool
/// rejection bug.
const CODEX_DEEPSEEK_OFFICIAL_CATALOG_HOSTS: &[&str] = &["deepseek.com"];

/// Bundled copy of DeepSeek's official Codex models.json — the exact file
/// their one-click integration script writes (api-docs.deepseek.com →
/// quick_start/agent_integrations/codex): freeform apply_patch, GPT-5 harness
/// base_instructions, low/high/max reasoning levels, web_search supported,
/// 1m context. Declares `minimal_client_version` 0.144.0.
fn load_codex_deepseek_official_catalog_models() -> Vec<Value> {
    let text = include_str!("resources/codex_deepseek_catalog_template.json");
    let catalog: Value =
        serde_json::from_str(text).expect("bundled DeepSeek official catalog must be valid JSON");
    catalog
        .get("models")
        .and_then(|models| models.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Official vendor catalog entries for the provider in `config_text`, if its
/// gateway ships one. Only the `NativeResponses` profile qualifies: ProxyChat
/// runs through cc-switch's converter (gpt-5.5 template contract) and the
/// Anthropic transform drops custom tools, so both must keep their existing
/// templates. Host-driven like the web_search blacklist, so existing providers
/// pick it up on their next switch without a re-save.
fn codex_official_vendor_catalog_models(
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Option<Vec<Value>> {
    if profile != CodexCatalogToolProfile::NativeResponses {
        return None;
    }
    let base_url = extract_codex_base_url(config_text)?.to_ascii_lowercase();
    if CODEX_DEEPSEEK_OFFICIAL_CATALOG_HOSTS
        .iter()
        .any(|host| base_url.contains(host))
    {
        let models = load_codex_deepseek_official_catalog_models();
        if !models.is_empty() {
            return Some(models);
        }
    }
    None
}

/// Build one catalog entry from an official vendor catalog: match the user's
/// model id against the vendor entries by slug; an unknown id clones the
/// vendor's first (flagship) entry so it keeps the gateway's capability
/// profile without impersonating the flagship. The official entry is
/// authoritative — no tool-profile stripping — but explicit per-row user
/// overrides still win.
fn codex_vendor_catalog_model_entry(
    vendor_models: &[Value],
    spec: &CodexCatalogModelSpec,
    priority: usize,
) -> Value {
    let matched = vendor_models.iter().find(|entry| {
        entry
            .get("slug")
            .and_then(|slug| slug.as_str())
            .is_some_and(|slug| slug.eq_ignore_ascii_case(&spec.model))
    });
    let mut entry = match matched {
        Some(found) => found.clone(),
        None => vendor_models.first().cloned().unwrap_or_else(|| json!({})),
    };
    // Capture before the mutable borrow: the vendor entry's own default is the
    // fallback when the user declares reasoning levels without a default.
    let vendor_default = entry
        .get("default_reasoning_level")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let Some(entry_obj) = entry.as_object_mut() else {
        return json!({});
    };

    if matched.is_none() {
        let display_name = spec.display_name.as_deref().unwrap_or(&spec.model);
        entry_obj.insert("slug".to_string(), json!(spec.model));
        entry_obj.insert("display_name".to_string(), json!(display_name));
        entry_obj.insert("description".to_string(), json!(display_name));
        entry_obj.insert("priority".to_string(), json!(1000 + priority));
        // Unknown model: don't inherit the flagship entry's modalities —
        // resolve from the registry/fail-open logic instead, so a vision
        // variant (e.g. deepseek-v4-flash-vision-exp) is not declared
        // text-only merely because the flagship is.
        entry_obj.insert(
            "input_modalities".to_string(),
            json!(codex_catalog_input_modalities(
                &spec.model,
                spec.input_modalities.as_deref(),
            )),
        );
    }

    // Explicit user overrides win over the official entry; absent values keep
    // the vendor's declarations (context window, modalities, harness, ...).
    if let Some(display_name) = spec.display_name.as_deref() {
        entry_obj.insert("display_name".to_string(), json!(display_name));
    }
    if let Some(context_window) = spec.context_window {
        entry_obj.insert("context_window".to_string(), json!(context_window));
        entry_obj.insert("max_context_window".to_string(), json!(context_window));
    }
    if let Some(parallel) = spec.supports_parallel_tool_calls {
        entry_obj.insert("supports_parallel_tool_calls".to_string(), json!(parallel));
    }
    if let Some(modalities) = spec.input_modalities.as_deref() {
        entry_obj.insert("input_modalities".to_string(), json!(modalities));
    }
    if let Some(base_instructions) = spec
        .base_instructions
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        entry_obj.insert("base_instructions".to_string(), json!(base_instructions));
    }

    // Per-model reasoning levels win over the official vendor entry too.
    // The vendor file is the base (its own levels stay when no override is
    // declared); its default_reasoning_level is the fallback.
    apply_codex_reasoning_level_override(entry_obj, vendor_default.as_deref(), spec);

    // Defensive: if a future codex parser requires a field the vendor file
    // predates, backfill only whitelisted parser-required keys.
    fill_template_fields_from_static(&mut entry);
    entry
}

/// Fields Codex's external-catalog parser REQUIRES (no serde default): when
/// one is missing Codex rejects the whole catalog file at startup ("missing
/// field ..."). `base_instructions` is the other known required field; the
/// templates always carry it and `codex_catalog_model_entry` handles it.
/// When Codex requires a new field, add it here AND to the static templates.
const CODEX_CATALOG_PARSER_REQUIRED_FIELDS: &[&str] = &[
    "supports_reasoning_summaries",
    // codex 0.148.0 rejects the catalog without it (#6661); a models_cache.json
    // written by an older build can lack it.
    "supports_parallel_tool_calls",
];

/// `models_cache.json` is shared by every Codex install on the machine (npm
/// CLI, desktop-bundled binary, ...), and each version serializes its own
/// `ModelInfo` shape — the cache's field set follows whichever process wrote
/// it last, so it cannot be assumed to satisfy the current external-catalog
/// schema (observed live: 0.144.5 requires `supports_reasoning_summaries`
/// while a coexisting build kept rewriting the cache without it). Backfill
/// ONLY parser-required fields from the bundled static template: optional
/// capability fields keep their missing-means-default semantics, and existing
/// values always win.
fn fill_template_fields_from_static(template: &mut Value) {
    let Some(static_template) = load_codex_model_template_static() else {
        return;
    };
    let (Some(template_obj), Some(static_obj)) =
        (template.as_object_mut(), static_template.as_object())
    else {
        return;
    };
    for key in CODEX_CATALOG_PARSER_REQUIRED_FIELDS {
        if !template_obj.contains_key(*key) {
            if let Some(value) = static_obj.get(*key) {
                template_obj.insert((*key).to_string(), value.clone());
            }
        }
    }
}

fn load_codex_model_catalog_template_uncached() -> Result<Value, AppError> {
    // ① models_cache.json (created by Codex when it connects to OpenAI)
    if let Some(mut template) = load_codex_model_template_from_cache()? {
        fill_template_fields_from_static(&mut template);
        return Ok(template);
    }
    // ③ Static fallback bundled at compile time
    if let Some(template) = load_codex_model_template_static() {
        return Ok(template);
    }

    Err(AppError::Message(format!(
        "Codex model catalog template `{CODEX_MODEL_CATALOG_TEMPLATE_SLUG}` not found. Please start Codex once so models_cache.json is available, or restore the bundled model template."
    )))
}

fn get_or_load_codex_model_catalog_template<F>(
    cache: &OnceCell<Value>,
    loader: F,
) -> Result<Value, AppError>
where
    F: FnOnce() -> Result<Value, AppError>,
{
    cache.get_or_try_init(loader).cloned()
}

#[cfg(not(test))]
fn load_codex_model_catalog_template() -> Result<Value, AppError> {
    get_or_load_codex_model_catalog_template(
        &CODEX_MODEL_CATALOG_TEMPLATE_CACHE,
        load_codex_model_catalog_template_uncached,
    )
}

#[cfg(test)]
fn load_codex_model_catalog_template() -> Result<Value, AppError> {
    load_codex_model_catalog_template_uncached()
}

fn codex_model_catalog_from_specs(
    specs: &[CodexCatalogModelSpec],
    template: &Value,
    profile: CodexCatalogToolProfile,
    default_context_window: u64,
) -> Value {
    let entries: Vec<Value> = specs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            codex_catalog_model_entry(template, spec, index, profile, default_context_window)
        })
        .collect();

    json!({ "models": entries })
}

pub(crate) fn codex_model_catalog_from_settings(
    settings: &Value,
    config_text: &str,
    profile: CodexCatalogToolProfile,
) -> Result<Option<Value>, AppError> {
    let specs = codex_catalog_model_specs(settings);
    if specs.is_empty() {
        return Ok(None);
    }

    // Vendors that publish an OFFICIAL Codex models.json for their native
    // `/responses` gateway get it mirrored verbatim instead of the neutral
    // template: its freeform apply_patch, vendor harness base_instructions and
    // reasoning levels are load-bearing (the harness tells the model to use
    // apply_patch, so catalog and harness must stay consistent).
    if let Some(vendor_models) = codex_official_vendor_catalog_models(config_text, profile) {
        let entries: Vec<Value> = specs
            .iter()
            .enumerate()
            .map(|(index, spec)| codex_vendor_catalog_model_entry(&vendor_models, spec, index))
            .collect();
        return Ok(Some(json!({ "models": entries })));
    }

    let default_context_window =
        extract_codex_top_level_u64(config_text, "model_context_window").unwrap_or(128_000);

    // Native providers use the bundled clean template (no freeform apply_patch,
    // no cache dependency); proxy-chat providers keep cloning Codex's gpt-5.5
    // entry so the proxy can rewrite custom<->function tools as before.
    let template = match profile {
        CodexCatalogToolProfile::NativeResponses | CodexCatalogToolProfile::Anthropic => {
            load_codex_native_responses_template()
        }
        CodexCatalogToolProfile::ProxyChat | CodexCatalogToolProfile::Copilot => {
            load_codex_model_catalog_template()?
        }
    };
    Ok(Some(codex_model_catalog_from_specs(
        &specs,
        &template,
        profile,
        default_context_window,
    )))
}

/// Reverse of `prepare_codex_config_text_with_model_catalog`: read the
/// cc-switch–maintained catalog file referenced by `~/.codex/config.toml` and
/// convert it back into the simplified shape the frontend table uses:
/// `{ "models": [{ "model", "displayName"?, "contextWindow"?, hidden overrides... }, ...] }`.
///
/// We only reverse-parse catalogs whose `model_catalog_json` path is the
/// cc-switch–generated file (identified by filename
/// `cc-switch-model-catalog.json`). A user-managed external catalog file is
/// left alone — surfacing its richer structure as the simplified table would
/// be a downgrade we can't safely round-trip.
///
/// `displayName`, `contextWindow`, and `inputModalities` are omitted from the
/// returned entry when the on-disk value matches the fallback that
/// `codex_model_catalog_from_settings` injects for unset inputs (slug for
/// display_name, `model_context_window` or 128_000 for context_window, and the
/// shared confirmed-text-only inference for input modalities). This preserves
/// the "user left it blank" intent across round-trip; an unavoidable edge case
/// is that a user-typed value that happens to equal the fallback also collapses
/// to blank, but the next save writes the same fallback so behavior is stable.
///
/// All failure modes (missing file, parse error, no `model_catalog_json`,
/// entries without `slug`) collapse to `Ok(None)` so callers can treat this
/// as best-effort enrichment without making `read_live_settings` brittle.
/// 模型目录文件读取上限（32 MiB）。目录 JSON 正常只有几百 KiB；超过则视为异常，
/// 避免指向外部大文件时耗尽内存。
const MAX_CODEX_CATALOG_BYTES: u64 = 32 * 1024 * 1024;

/// 安全地读取文件为字符串，并在超过字节上限时返回错误。
pub(crate) fn read_limited_string(path: &Path, max_bytes: u64) -> Result<String, AppError> {
    let metadata = fs::metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.len() > max_bytes {
        return Err(AppError::Config(format!(
            "文件 {} 超过大小上限 {} 字节",
            path.display(),
            max_bytes
        )));
    }
    fs::read_to_string(path).map_err(|error| AppError::io(path, error))
}

/// Read the cc-switch Codex model catalog file with a size cap.
pub(crate) fn read_codex_model_catalog_text(path: &Path) -> Result<String, AppError> {
    read_limited_string(path, MAX_CODEX_CATALOG_BYTES)
}

/// Given `config.toml` text, resolve the on-disk path of the cc-switch–owned
/// catalog file (returns `None` if `model_catalog_json` is absent or points at
/// a file we don't own). Relative paths are resolved under `base_dir`;
/// absolute paths must still be inside `base_dir`.
pub(crate) fn resolve_cc_switch_catalog_path(
    config_text: &str,
    base_dir: &Path,
) -> Option<PathBuf> {
    if config_text.trim().is_empty() {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let catalog_path_str = doc
        .get("model_catalog_json")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;

    let referenced_path = Path::new(catalog_path_str);
    let is_cc_switch_owned = referenced_path.file_name().and_then(|name| name.to_str())
        == Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
    if !is_cc_switch_owned {
        return None;
    }

    // 注意（有意的行为变更）：Windows 上 `/…` 形式的旧 WSL 风格 Linux 路径也会
    // 被视为绝对路径，从而在下方的包含性校验中失败——此前这类路径会因无法匹配
    // 生成文件名而回退为按文件名解析、碰巧能工作。可接受：下一次切换供应商时
    // 写入侧会重新落一个裸文件名，配置自愈（见
    // `set_catalog_json_none_removes_cc_switch_owned_by_filename` 的场景注释）。
    let is_unix_absolute = catalog_path_str.starts_with('/');
    let resolved = if referenced_path.is_absolute() || is_unix_absolute {
        referenced_path.to_path_buf()
    } else {
        base_dir.join(referenced_path)
    };

    if !path_is_within(base_dir, &resolved) {
        log::warn!(
            "Codex model_catalog_json 指向配置目录外: {}（允许目录: {}）",
            resolved.display(),
            base_dir.display()
        );
        return None;
    }

    // 词法包含不等于运行时包含：配置目录内的符号链接（如 ~/.codex/link ->
    // /etc）能让 `link/cc-switch-model-catalog.json` 通过上面的检查，读取却
    // 落到目录外。文件存在时把真实路径 canonicalize 出来再校验一次，并把
    // canonical 路径返回给调用方——后续读取不再经过 symlink 组件。
    if resolved.exists() {
        let canonical = match fs::canonicalize(&resolved) {
            Ok(path) => path,
            Err(error) => {
                log::warn!(
                    "Codex model_catalog_json canonicalize 失败: {}: {error}",
                    resolved.display()
                );
                return None;
            }
        };
        // base 同样 canonicalize，保证两侧前缀一致（Windows \\?\、
        // macOS /tmp -> /private/tmp）；base 失败时退回词法 base——
        // 词法 base 与 canonical 路径比较只会误拒（退化为不读），不会误放。
        let canonical_base = fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());
        if !path_is_within(&canonical_base, &canonical) {
            log::warn!(
                "Codex model_catalog_json 经符号链接解析到配置目录外: {} -> {}（允许目录: {}）",
                resolved.display(),
                canonical.display(),
                canonical_base.display()
            );
            return None;
        }
        return Some(canonical);
    }

    Some(resolved)
}

/// Extract a provider-scoped `experimental_bearer_token` from Codex `config.toml`.
///
/// Mobile compat: third-party providers may store the API key inside
/// `[model_providers.<id>].experimental_bearer_token` while keeping the
/// user's ChatGPT login cache intact in `auth.json`. Falls back to the
/// top-level `experimental_bearer_token` when no active model provider is set.
pub fn extract_codex_experimental_bearer_token(config_text: &str) -> Option<String> {
    if !config_text.contains("experimental_bearer_token") {
        return None;
    }
    let doc = config_text.parse::<DocumentMut>().ok()?;
    let provider_id = active_codex_model_provider_id(&doc);

    let top_level_token = || {
        doc.get("experimental_bearer_token")
            .and_then(|item| item.as_str())
    };
    let token = match provider_id.as_deref() {
        // `as_table_like` (not `as_table`): user configs may use inline tables
        // (`model_providers = { foo = {...} }`), which `as_table` rejects.
        Some(id) if is_custom_codex_model_provider_id(id) => doc
            .get("model_providers")
            .and_then(|item| item.as_table_like())
            .and_then(|table| table.get(id))
            .and_then(|item| item.as_table_like())
            .and_then(|table| table.get("experimental_bearer_token"))
            .and_then(|item| item.as_str())
            .or_else(top_level_token),
        Some(_) => top_level_token(),
        None => top_level_token(),
    };

    token
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

/// cc-switch-owned provider id used by the legacy-shape normalization below.
/// Not a Codex reserved id, so an injected token lands inside the table.
const CODEX_MIGRATED_PROVIDER_ID: &str = "cc-switch";

/// The reserved built-in ids whose `[model_providers.<id>]` tables make
/// Codex reject the WHOLE config at load (`validate_reserved_model_provider_ids`,
/// present since 0.148, case-sensitive; the bedrock ids are exempt).
const CODEX_STALE_RESERVED_TABLE_IDS: &[&str] = &["openai", "ollama", "lmstudio"];

fn table_matches_codex_unified_official_provider(table: &toml_edit::Table) -> bool {
    table.len() == 4
        && table.get("name").and_then(|item| item.as_str()) == Some("OpenAI")
        && table
            .get("requires_openai_auth")
            .and_then(|item| item.as_bool())
            == Some(true)
        && table
            .get("supports_websockets")
            .and_then(|item| item.as_bool())
            == Some(true)
        && table.get("wire_api").and_then(|item| item.as_str()) == Some("responses")
}

/// `inject_codex_unified_session_bucket` 的反向操作：从配置文本里剥掉注入的
/// 统一会话路由，保证切换回填不会把它带进数据库的存储配置（关闭开关后
/// 切换即可完全还原）。仅当形态与注入产物完全一致时才剥离；第三方模板和
/// 用户自定义的 `custom` 条目（带 base_url 等差异字段）原样保留。
pub fn strip_codex_unified_session_bucket(config_text: &str) -> Result<String, AppError> {
    if !config_text.contains("model_provider") {
        return Ok(config_text.to_string());
    }
    let mut doc = config_text
        .parse::<DocumentMut>()
        .map_err(|e| AppError::Message(format!("Invalid Codex config.toml: {e}")))?;

    if doc.get("model_provider").and_then(|item| item.as_str())
        != Some(CC_SWITCH_CODEX_MODEL_PROVIDER_ID)
    {
        return Ok(config_text.to_string());
    }
    let matches_injected = doc
        .get("model_providers")
        .and_then(|item| item.as_table())
        .and_then(|providers| providers.get(CC_SWITCH_CODEX_MODEL_PROVIDER_ID))
        .and_then(|item| item.as_table())
        .is_some_and(table_matches_codex_unified_official_provider);
    if !matches_injected {
        return Ok(config_text.to_string());
    }

    doc.as_table_mut().remove("model_provider");
    let providers_empty = doc["model_providers"]
        .as_table_mut()
        .map(|providers| {
            providers.remove(CC_SWITCH_CODEX_MODEL_PROVIDER_ID);
            providers.is_empty()
        })
        .unwrap_or(false);
    if providers_empty {
        doc.as_table_mut().remove("model_providers");
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use serial_test::serial;
    use std::ffi::OsString;

    struct CodexLiveTestHome {
        _dir: tempfile::TempDir,
        original_test_home: Option<OsString>,
    }

    impl CodexLiveTestHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("create isolated Codex live test home");
            let original_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings for isolated test home");

            Self {
                _dir: dir,
                original_test_home,
            }
        }
    }

    impl Drop for CodexLiveTestHome {
        fn drop(&mut self) {
            match &self.original_test_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
            let _ = crate::settings::reload_settings();
        }
    }

    #[derive(Debug, PartialEq)]
    struct CodexLiveTestState {
        auth_bytes: Vec<u8>,
        auth_value: Value,
        config_bytes: Vec<u8>,
        config_value: toml::Value,
        catalog_bytes: Vec<u8>,
        catalog_value: Value,
        marker_bytes: Vec<u8>,
        marker_value: Value,
    }

    #[test]
    fn catalog_tool_profile_from_api_format() {
        assert_eq!(
            CodexCatalogToolProfile::from_api_format(Some("anthropic")),
            CodexCatalogToolProfile::Anthropic
        );
        assert_eq!(
            CodexCatalogToolProfile::from_api_format(Some("openai_responses")),
            CodexCatalogToolProfile::NativeResponses
        );
        assert_eq!(
            CodexCatalogToolProfile::from_api_format(Some("openai_chat")),
            CodexCatalogToolProfile::ProxyChat
        );
        assert_eq!(
            CodexCatalogToolProfile::from_api_format(None),
            CodexCatalogToolProfile::ProxyChat
        );
    }

    #[test]
    fn unified_session_bucket_strip_keeps_third_party_custom_entry() {
        // 第三方模板同样用 custom 路由，但条目带 base_url 等差异字段，
        // 形态不等于注入产物，必须原样保留。
        let third_party = r#"model_provider = "custom"

[model_providers.custom]
name = "Relay"
base_url = "https://relay.example/v1"
wire_api = "responses"
requires_openai_auth = true
"#;
        let untouched = strip_codex_unified_session_bucket(third_party).expect("strip");
        assert_eq!(untouched, third_party);
    }

    #[test]
    fn extract_base_url_prefers_active_provider_section() {
        let input = r#"model_provider = "azure"

[model_providers.azure]
base_url = "https://azure.example.com/v1"

[model_providers.other]
base_url = "https://other.example.com/v1"
"#;

        assert_eq!(
            extract_codex_base_url(input).as_deref(),
            Some("https://azure.example.com/v1")
        );
    }

    #[test]
    fn extract_base_url_falls_back_to_top_level_only() {
        let top_level = r#"base_url = "https://top-level.example.com/v1""#;
        assert_eq!(
            extract_codex_base_url(top_level).as_deref(),
            Some("https://top-level.example.com/v1")
        );
    }

    // Mirrors the frontend extractCodexBaseUrl: a non-active provider section
    // is never a credential source, whether the active provider points
    // elsewhere (e.g. the built-in "openai") or none is selected at all.
    #[test]
    fn extract_base_url_ignores_non_active_provider_sections() {
        let mismatched = r#"model_provider = "openai"

[model_providers.custom]
base_url = "https://leftover.example.com/v1"
"#;
        assert_eq!(extract_codex_base_url(mismatched), None);

        let no_active = r#"[model_providers.any]
base_url = "https://single.example.com/v1"
"#;
        assert_eq!(extract_codex_base_url(no_active), None);
    }

    #[test]
    fn extract_bearer_uses_top_level_token_for_reserved_provider() {
        let input = r#"model_provider = "openai"
experimental_bearer_token = "top-level-key"

[model_providers.openai]
experimental_bearer_token = "stale-table-key"
"#;

        assert_eq!(
            extract_codex_experimental_bearer_token(input).as_deref(),
            Some("top-level-key")
        );
    }

    #[test]
    fn dynamic_template_backfills_parser_required_fields_from_static() {
        // Simulate a template cloned from a models_cache.json written by a
        // Codex build whose ModelInfo lacks parser-side required fields such
        // as `supports_reasoning_summaries` (codex >= 0.144.5 rejects the
        // whole catalog file without it).
        let mut template = json!({
            "slug": "gpt-5.5",
            "context_window": 272_000,
            "supports_parallel_tool_calls": false
        });
        fill_template_fields_from_static(&mut template);

        assert_eq!(
            template
                .get("supports_reasoning_summaries")
                .and_then(Value::as_bool),
            Some(true)
        );
        // Keys already present in the dynamic template are never overwritten.
        assert_eq!(
            template
                .get("supports_parallel_tool_calls")
                .and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            template.get("context_window").and_then(Value::as_u64),
            Some(272_000)
        );
        // Optional capability fields must NOT be backfilled: for the catalog
        // parser "missing" means the parser default, not the static
        // template's value.
        assert!(template.get("supports_search_tool").is_none());
        assert!(template.get("supports_image_detail_original").is_none());
        assert!(template.get("web_search_tool_type").is_none());

        // A cache template missing supports_parallel_tool_calls gets the
        // static gpt-5.5 default backfilled (codex 0.148.0 rejects the
        // catalog without it, #6661).
        let mut stale = json!({ "slug": "gpt-5.5" });
        fill_template_fields_from_static(&mut stale);
        assert_eq!(
            stale
                .get("supports_parallel_tool_calls")
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn proxy_chat_catalog_entries_carry_reasoning_summaries_flag() {
        // End to end: a stale dynamic template, once backfilled, must yield
        // catalog entries codex 0.144.5+ can parse.
        let mut template = json!({ "slug": "gpt-5.5" });
        fill_template_fields_from_static(&mut template);
        let specs = vec![CodexCatalogModelSpec {
            model: "k3".to_string(),
            display_name: Some("Kimi K3".to_string()),
            context_window: Some(262_144),
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        }];
        let catalog = codex_model_catalog_from_specs(
            &specs,
            &template,
            CodexCatalogToolProfile::ProxyChat,
            128_000,
        );
        assert_eq!(
            catalog["models"][0]
                .get("supports_reasoning_summaries")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            catalog["models"][0]
                .get("supports_parallel_tool_calls")
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn codex_model_catalog_uses_provider_models_and_context() {
        let template = json!({
            "slug": "gpt-5.5",
            "display_name": "GPT-5.5",
            "description": "Frontier model",
            "base_instructions": "gpt-5.5 base instructions",
            "model_messages": {
                "instructions_template": "gpt-5.5 instructions template",
                "instructions_variables": {
                    "personality_default": "",
                    "personality_friendly": "",
                    "personality_pragmatic": ""
                }
            },
            "additional_speed_tiers": ["fast"],
            "service_tiers": [
                {
                    "id": "priority",
                    "name": "Fast",
                    "description": "1.5x speed, increased usage"
                }
            ],
            "availability_nux": {
                "message": "GPT-5.5 is now available."
            },
            "upgrade": {
                "target": "gpt-5.5"
            },
            "context_window": 272000,
            "max_context_window": 272000
        });
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash",
                        "displayName": "DeepSeek V4 Flash",
                        "contextWindow": "64000"
                    },
                    {
                        "model": "kimi-k2",
                        "display_name": "Kimi K2"
                    }
                ]
            }
        });
        let specs = codex_catalog_model_specs(&settings);
        let catalog = codex_model_catalog_from_specs(
            &specs,
            &template,
            CodexCatalogToolProfile::ProxyChat,
            128_000,
        );
        let models = catalog
            .get("models")
            .and_then(|value| value.as_array())
            .expect("models should be an array");

        assert_eq!(models.len(), 2);
        assert_eq!(
            models[0].get("slug").and_then(|value| value.as_str()),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            models[0]
                .get("context_window")
                .and_then(|value| value.as_u64()),
            Some(64_000)
        );
        assert_eq!(
            models[1]
                .get("context_window")
                .and_then(|value| value.as_u64()),
            Some(128_000)
        );
        assert!(
            models[0].get("model_messages").is_some(),
            "Codex requires model_messages in custom catalogs"
        );
        assert_eq!(
            models[0]
                .get("base_instructions")
                .and_then(|value| value.as_str()),
            Some("gpt-5.5 base instructions")
        );
        assert_eq!(
            models[0].get("model_messages"),
            template.get("model_messages"),
            "custom catalog entries should keep the gpt-5.5 agent template"
        );
        assert_eq!(
            models[0].get("additional_speed_tiers"),
            Some(&json!([])),
            "generated third-party entries should not inherit OpenAI speed tiers"
        );
        assert!(
            models[0]
                .get("availability_nux")
                .is_some_and(|value| value.is_null()),
            "generated third-party entries should not inherit GPT-5.5 launch messaging"
        );
    }

    #[test]
    fn native_responses_catalog_honors_per_model_reasoning_levels() {
        // The native template only declares none/high. A per-model
        // reasoningLevels override must replace supported_reasoning_levels and
        // pick a sensible default_reasoning_level.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash",
                        "reasoningLevels": ["none", "low", "medium", "high", "xhigh", "max"],
                        "defaultReasoningLevel": "xhigh"
                    },
                    {
                        "model": "no-default-model",
                        "reasoningLevels": ["low", "medium", "high"]
                    },
                    {
                        "model": "template-default-model",
                        "reasoningLevels": ["none", "high", "xhigh"]
                    },
                    {
                        "model": "dirty-levels",
                        "reasoningLevels": ["none", "bogus", "high", ""]
                    },
                    {
                        "model": "unordered-model",
                        "reasoningLevels": ["xhigh", "low", "bogus", "low"],
                        "defaultReasoningLevel": "bogus"
                    }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            "",
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let models = catalog["models"].as_array().expect("models array");
        let efforts = |index: usize| -> Vec<String> {
            models[index]["supported_reasoning_levels"]
                .as_array()
                .expect("supported_reasoning_levels array")
                .iter()
                .filter_map(|level| level.get("effort").and_then(|v| v.as_str()))
                .map(str::to_string)
                .collect()
        };

        // Explicit default wins.
        assert_eq!(
            efforts(0),
            vec!["none", "low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            models[0]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("xhigh")
        );

        // No explicit default: falls back to the last (highest) declared level.
        assert_eq!(efforts(1), vec!["low", "medium", "high"]);
        assert_eq!(
            models[1]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("high")
        );

        // Template default ("high") is kept when it is still in the list.
        assert_eq!(efforts(2), vec!["none", "high", "xhigh"]);
        assert_eq!(
            models[2]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("high")
        );

        // Unknown / empty efforts are dropped; the default still resolves to
        // a supported level (the template default, "high").
        assert_eq!(efforts(3), vec!["none", "high"]);
        assert_eq!(
            models[3]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("high")
        );

        // Declaration order is normalized to canonical order, duplicates and
        // an unknown explicit default are dropped, and the fallback picks the
        // highest supported level in canonical order (not the last declared
        // one, and never an unknown effort).
        assert_eq!(efforts(4), vec!["low", "xhigh"]);
        assert_eq!(
            models[4]
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("xhigh")
        );
    }

    #[test]
    fn vendor_catalog_honors_per_model_reasoning_levels() {
        // The DeepSeek official catalog declares low/high/max; a per-model
        // override must win over the official entry.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash",
                        "reasoningLevels": ["none", "low", "medium", "high", "xhigh", "max"],
                        "defaultReasoningLevel": "xhigh"
                    }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let entry = &catalog["models"][0];
        let efforts: Vec<&str> = entry["supported_reasoning_levels"]
            .as_array()
            .expect("supported_reasoning_levels array")
            .iter()
            .filter_map(|level| level.get("effort").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(
            efforts,
            vec!["none", "low", "medium", "high", "xhigh", "max"]
        );
        assert_eq!(
            entry
                .get("default_reasoning_level")
                .and_then(|v| v.as_str()),
            Some("xhigh")
        );
    }

    #[test]
    fn vendor_catalog_unknown_model_does_not_inherit_flagship_modalities() {
        // A vision variant not in the official DeepSeek catalog must not
        // inherit the flagship entry's text-only modalities; the registry /
        // fail-open logic should resolve it as image-capable instead.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash-vision-exp",
                        "displayName": "DeepSeek V4 Flash Vision Exp"
                    }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let modalities: Vec<&str> = catalog["models"][0]["input_modalities"]
            .as_array()
            .expect("input_modalities array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            modalities,
            vec!["text", "image"],
            "unknown vision model must not inherit the flagship's text-only modalities"
        );
    }

    #[test]
    fn vendor_catalog_unknown_model_explicit_modalities_override() {
        // An explicit user inputModalities declaration must win over the
        // registry/fail-open resolution even for unmatched models.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-flash-vision-exp",
                        "inputModalities": ["text"]
                    }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let modalities: Vec<&str> = catalog["models"][0]["input_modalities"]
            .as_array()
            .expect("input_modalities array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(modalities, vec!["text"]);
    }

    #[test]
    fn vendor_catalog_matched_model_keeps_vendor_modalities() {
        // A model that IS in the official catalog must keep the vendor's
        // declared modalities verbatim (deepseek-v4-pro is text-only there).
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "deepseek-v4-pro",
                        "displayName": "DeepSeek V4 Pro"
                    }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let modalities: Vec<&str> = catalog["models"][0]["input_modalities"]
            .as_array()
            .expect("input_modalities array")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(modalities, vec!["text"]);
    }

    #[test]
    fn native_responses_profile_suppresses_apply_patch_and_keeps_shell() {
        // Native (direct) /responses providers must NOT emit a freeform
        // apply_patch (type=="custom") tool — gateways like MiMo reject it.
        // The native profile uses the bundled clean template and relies on
        // shell_type="shell_command" for edits, plus per-row overrides.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    {
                        "model": "MiniMax-M3",
                        "displayName": "MiniMax-M3",
                        "contextWindow": 1_000_000,
                        "supportsParallelToolCalls": true,
                        "inputModalities": ["text", "image"],
                        "baseInstructions": "You are Codex, a coding agent based on MiniMax-M3."
                    }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            "",
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("native catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let entry = &catalog["models"][0];
        assert_eq!(
            entry.get("slug").and_then(|v| v.as_str()),
            Some("MiniMax-M3")
        );
        assert_eq!(
            entry.get("shell_type").and_then(|v| v.as_str()),
            Some("shell_command"),
            "native entries edit via shell, not the custom apply_patch tool"
        );
        assert!(
            entry.get("apply_patch_tool_type").is_none(),
            "native entries must NOT declare a freeform apply_patch tool"
        );
        // `base_instructions` is REQUIRED by Codex's catalog parser, so it must
        // be present — and the per-row official override must win over the
        // template default.
        assert_eq!(
            entry.get("base_instructions").and_then(|v| v.as_str()),
            Some("You are Codex, a coding agent based on MiniMax-M3."),
            "per-row baseInstructions override must apply (and field must exist)"
        );
        assert!(
            entry.get("model_messages").is_none(),
            "native entries must not carry the gpt-5.5 model_messages persona text"
        );
        assert_eq!(
            entry.get("supports_parallel_tool_calls"),
            Some(&json!(true)),
            "per-row supportsParallelToolCalls override must apply"
        );
        assert_eq!(
            entry.get("input_modalities"),
            Some(&json!(["text", "image"])),
            "per-row inputModalities override must apply"
        );
        assert_eq!(
            entry.get("context_window").and_then(|v| v.as_u64()),
            Some(1_000_000)
        );
    }

    #[test]
    fn catalog_infers_image_input_independently_of_tool_profile() {
        // Start from a deliberately text-only template to prove that every
        // profile overwrites template defaults with shared capability logic.
        let template = json!({
            "input_modalities": ["text"],
            "apply_patch_tool_type": "freeform"
        });
        let specs = vec![
            CodexCatalogModelSpec {
                model: "gpt-5.4".to_string(),
                display_name: Some("GPT 5.4".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: None,
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
            CodexCatalogModelSpec {
                model: "qwen/qwen3-coder-plus".to_string(),
                display_name: Some("Qwen3 Coder Plus".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: None,
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
            CodexCatalogModelSpec {
                model: "glm-5.2v".to_string(),
                display_name: Some("GLM 5.2V".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: None,
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
            CodexCatalogModelSpec {
                model: "deepseek-v4-flash".to_string(),
                display_name: Some("Explicit Visual Override".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: Some(vec!["text".to_string(), "image".to_string()]),
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
            CodexCatalogModelSpec {
                model: "custom-text-alias".to_string(),
                display_name: Some("Explicit Text Override".to_string()),
                context_window: Some(128_000),
                supports_parallel_tool_calls: None,
                input_modalities: Some(vec!["text".to_string()]),
                base_instructions: None,
                reasoning_levels: None,
                default_reasoning_level: None,
            },
        ];

        for profile in [
            CodexCatalogToolProfile::ProxyChat,
            CodexCatalogToolProfile::NativeResponses,
            CodexCatalogToolProfile::Anthropic,
        ] {
            let catalog = codex_model_catalog_from_specs(&specs, &template, profile, 128_000);
            let models = catalog["models"].as_array().expect("models array");
            let modalities = |slug: &str| {
                models
                    .iter()
                    .find(|entry| entry["slug"] == slug)
                    .and_then(|entry| entry.get("input_modalities"))
                    .cloned()
                    .unwrap_or(Value::Null)
            };

            assert_eq!(modalities("gpt-5.4"), json!(["text", "image"]));
            assert_eq!(modalities("qwen/qwen3-coder-plus"), json!(["text"]));
            assert_eq!(modalities("glm-5.2v"), json!(["text", "image"]));
            assert_eq!(
                modalities("deepseek-v4-flash"),
                json!(["text", "image"]),
                "explicit provider metadata must override the text-only registry"
            );
            assert_eq!(modalities("custom-text-alias"), json!(["text"]));
        }
    }

    #[test]
    fn native_responses_catalog_always_carries_base_instructions() {
        // Regression guard for the "missing field `base_instructions`" parse
        // error: Codex refuses to load a model catalog whose entries lack
        // base_instructions. Synthesized presets carry no per-row override, so
        // the entry MUST inherit the template's neutral default rather than
        // dropping the field entirely.
        let settings = json!({
            "modelCatalog": { "models": [{ "model": "qwen3-coder-plus" }] }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            "",
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("native catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let base = catalog["models"][0]
            .get("base_instructions")
            .and_then(|v| v.as_str());
        assert!(
            base.is_some_and(|s| !s.trim().is_empty()),
            "every native entry must carry a non-empty base_instructions (Codex requires it)"
        );
    }

    const DEEPSEEK_NATIVE_CONFIG: &str = r#"model = "deepseek-v4-flash"
model_provider = "custom"

[model_providers.custom]
name = "deepseek"
base_url = "https://api.deepseek.com"
wire_api = "responses"
"#;

    #[test]
    fn deepseek_host_native_catalog_mirrors_official_entries() {
        // DeepSeek publishes an official Codex models.json (freeform
        // apply_patch + GPT-5 harness + low/high/max reasoning levels). For a
        // deepseek.com native provider the generated catalog must mirror it
        // verbatim instead of the stripped neutral template — the harness
        // tells the model to use apply_patch, so stripping the tool while
        // keeping the harness would be self-inconsistent.
        let settings = json!({
            "modelCatalog": {
                "models": [
                    { "model": "deepseek-flash", "displayName": "DeepSeek Flash" },
                    { "model": "deepseek-v4-pro", "contextWindow": 500_000 }
                ]
            }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let flash = &catalog["models"][0];
        assert_eq!(
            flash.get("slug").and_then(|v| v.as_str()),
            Some("deepseek-flash")
        );
        assert_eq!(
            flash.get("apply_patch_tool_type").and_then(|v| v.as_str()),
            Some("freeform"),
            "official DeepSeek entries keep the freeform apply_patch grant"
        );
        assert!(
            flash
                .get("base_instructions")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.starts_with("You are Codex, an agent based on GPT-5")),
            "official GPT-5 harness must survive verbatim"
        );
        let efforts: Vec<&str> = flash["supported_reasoning_levels"]
            .as_array()
            .expect("official reasoning levels array")
            .iter()
            .filter_map(|level| level.get("effort").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(efforts, vec!["low", "high", "max"]);
        // DeepSeek has no tool_search support; `true` makes Codex defer MCP
        // tools behind tool search, so none can ever be called (#6647).
        assert_eq!(flash.get("supports_search_tool"), Some(&json!(false)));
        assert_eq!(
            flash.get("web_search_tool_type").and_then(|v| v.as_str()),
            Some("text")
        );
        assert_eq!(
            flash.get("supports_reasoning_summaries"),
            Some(&json!(true))
        );
        // deepseek-flash accepts image input per the vendor's own catalog and
        // vision guide (api-docs.deepseek.com/guides/vision); the legacy
        // deepseek-v4-flash alias routes to it and must not be gated (#7283).
        assert_eq!(
            flash.get("input_modalities"),
            Some(&json!(["text", "image"]))
        );
        assert!(
            flash.get("model_messages").is_some(),
            "official entries are mirrored verbatim, incl. model_messages"
        );
        // No explicit contextWindow on the row: the official 1m window must
        // survive instead of being clobbered by the 128k default.
        assert_eq!(
            flash.get("context_window").and_then(|v| v.as_u64()),
            Some(1_048_576)
        );
        // Explicit user display name still wins over the official one.
        assert_eq!(
            flash.get("display_name").and_then(|v| v.as_str()),
            Some("DeepSeek Flash")
        );

        let pro = &catalog["models"][1];
        assert_eq!(
            pro.get("slug").and_then(|v| v.as_str()),
            Some("deepseek-v4-pro")
        );
        // Explicit user context window override wins…
        assert_eq!(
            pro.get("context_window").and_then(|v| v.as_u64()),
            Some(500_000)
        );
        assert_eq!(
            pro.get("max_context_window").and_then(|v| v.as_u64()),
            Some(500_000)
        );
        // …while the untouched official display name is kept.
        assert_eq!(
            pro.get("display_name").and_then(|v| v.as_str()),
            Some("DeepSeek-V4-Pro")
        );
    }

    #[test]
    fn deepseek_official_catalog_unknown_model_clones_flagship() {
        // A user-added model id the official file doesn't know keeps the
        // gateway's capability profile (clone of the flagship entry) without
        // impersonating it: own slug/name, demoted priority, and the official
        // context window rather than the 128k synthetic default.
        let settings = json!({
            "modelCatalog": { "models": [{ "model": "deepseek-v4-lite" }] }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let entry = &catalog["models"][0];
        assert_eq!(
            entry.get("slug").and_then(|v| v.as_str()),
            Some("deepseek-v4-lite")
        );
        assert_eq!(
            entry.get("display_name").and_then(|v| v.as_str()),
            Some("deepseek-v4-lite")
        );
        assert!(
            entry
                .get("priority")
                .and_then(|v| v.as_u64())
                .is_some_and(|p| p >= 1000),
            "clones must sort after official entries"
        );
        assert_eq!(
            entry.get("apply_patch_tool_type").and_then(|v| v.as_str()),
            Some("freeform")
        );
        assert_eq!(
            entry.get("context_window").and_then(|v| v.as_u64()),
            Some(1_048_576),
            "absent contextWindow keeps the flagship's official window"
        );
        assert!(entry
            .get("base_instructions")
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty()));
    }

    #[test]
    fn deepseek_official_catalog_legacy_flash_alias_stays_image_capable() {
        // The vendor's catalog now ships `deepseek-flash` only; the legacy
        // `deepseek-v4-flash` id the preset defaulted to is still accepted by
        // the API and routes to the same vision-capable Flash model, so it must
        // clone the flagship and resolve image-capable instead of being gated
        // text-only (#7283).
        let settings = json!({
            "modelCatalog": { "models": [{ "model": "deepseek-v4-flash" }] }
        });

        let catalog = codex_model_catalog_from_settings(
            &settings,
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses,
        )
        .expect("vendor catalog generation should not error")
        .expect("non-empty modelCatalog must yield a catalog");

        let entry = &catalog["models"][0];
        assert_eq!(
            entry.get("slug").and_then(|v| v.as_str()),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            entry.get("input_modalities"),
            Some(&json!(["text", "image"])),
            "the legacy alias routes to the vision-capable Flash model and must fail open"
        );
    }

    #[test]
    fn official_vendor_catalog_gated_by_native_profile_and_host() {
        // The official mirror is a capability GRANT, so the gate must be
        // narrow: native `/responses` profile AND the vendor's own host. Chat
        // runs through the proxy converter (gpt-5.5 contract), the Anthropic
        // transform drops custom tools, and aggregators hosting the same
        // model may reject freeform tools — all of them keep their templates.
        assert!(codex_official_vendor_catalog_models(
            DEEPSEEK_NATIVE_CONFIG,
            CodexCatalogToolProfile::NativeResponses
        )
        .is_some_and(|models| !models.is_empty()));

        for profile in [
            CodexCatalogToolProfile::ProxyChat,
            CodexCatalogToolProfile::Anthropic,
        ] {
            assert!(
                codex_official_vendor_catalog_models(DEEPSEEK_NATIVE_CONFIG, profile).is_none(),
                "only the NativeResponses profile may mirror the official catalog"
            );
        }

        let minimax_config = r#"model = "MiniMax-M3"
model_provider = "custom"

[model_providers.custom]
name = "minimax"
base_url = "https://api.minimaxi.com/v1"
wire_api = "responses"
"#;
        assert!(
            codex_official_vendor_catalog_models(
                minimax_config,
                CodexCatalogToolProfile::NativeResponses
            )
            .is_none(),
            "non-DeepSeek native hosts keep the neutral template"
        );
        assert!(
            codex_official_vendor_catalog_models("", CodexCatalogToolProfile::NativeResponses)
                .is_none()
        );
    }

    #[test]
    fn proxy_chat_profile_still_keeps_apply_patch() {
        // Regression guard for Mode A: the proxy-chat profile must keep the
        // freeform apply_patch tool (the proxy rewrites custom<->function).
        let template = load_codex_native_responses_template();
        let specs = vec![CodexCatalogModelSpec {
            model: "x".to_string(),
            display_name: Some("x".to_string()),
            context_window: Some(128_000),
            supports_parallel_tool_calls: None,
            input_modalities: None,
            base_instructions: None,
            reasoning_levels: None,
            default_reasoning_level: None,
        }];
        // Using a gpt-5.5-shaped template under ProxyChat must NOT strip
        // apply_patch_tool_type. (The native template lacks it, so synthesize
        // one with the field present to prove ProxyChat leaves it intact.)
        let mut proxy_template = template.clone();
        proxy_template["apply_patch_tool_type"] = json!("freeform");
        let catalog = codex_model_catalog_from_specs(
            &specs,
            &proxy_template,
            CodexCatalogToolProfile::ProxyChat,
            128_000,
        );
        assert_eq!(
            catalog["models"][0]
                .get("apply_patch_tool_type")
                .and_then(|v| v.as_str()),
            Some("freeform"),
            "ProxyChat must preserve apply_patch_tool_type (no native stripping)"
        );
    }

    #[test]
    fn resolve_catalog_path_returns_none_when_config_missing_field() {
        let base = PathBuf::from("/tmp/.codex");
        assert!(resolve_cc_switch_catalog_path("", &base).is_none());
        assert!(
            resolve_cc_switch_catalog_path("model = \"gpt-5\"", &base).is_none(),
            "no model_catalog_json field should yield None"
        );
    }

    #[test]
    fn resolve_catalog_path_accepts_cc_switch_owned_file() {
        let base = PathBuf::from("/tmp/.codex");
        let config = r#"model_catalog_json = "/tmp/.codex/cc-switch-model-catalog.json"
"#;
        let resolved = resolve_cc_switch_catalog_path(config, &base).expect("path resolves");
        assert_eq!(resolved, base.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME));
    }

    #[test]
    fn resolve_catalog_path_rejects_user_owned_external_file() {
        let base = PathBuf::from("/tmp/.codex");
        let config = r#"model_catalog_json = "/Users/me/.codex/my-handwritten-catalog.json"
"#;
        assert!(
            resolve_cc_switch_catalog_path(config, &base).is_none(),
            "external catalog files should be left alone"
        );
    }

    #[test]
    fn successful_model_catalog_template_load_is_cached() {
        use std::cell::Cell;

        let cache = OnceCell::new();
        let calls = Cell::new(0);
        let first = get_or_load_codex_model_catalog_template(&cache, || {
            calls.set(calls.get() + 1);
            Ok(json!({ "slug": "first" }))
        })
        .expect("first template load");
        let second = get_or_load_codex_model_catalog_template(&cache, || {
            calls.set(calls.get() + 1);
            Ok(json!({ "slug": "second" }))
        })
        .expect("cached template load");

        assert_eq!(first, json!({ "slug": "first" }));
        assert_eq!(second, first);
        assert_eq!(calls.get(), 1, "successful template should load only once");
    }

    #[test]
    fn failed_model_catalog_template_load_can_retry() {
        use std::cell::Cell;

        let cache = OnceCell::new();
        let calls = Cell::new(0);
        let first = get_or_load_codex_model_catalog_template(&cache, || {
            calls.set(calls.get() + 1);
            Err(AppError::Message("temporary failure".to_string()))
        });
        assert!(first.is_err());

        let second = get_or_load_codex_model_catalog_template(&cache, || {
            calls.set(calls.get() + 1);
            Ok(json!({ "slug": "recovered" }))
        })
        .expect("retry template load");

        assert_eq!(second, json!({ "slug": "recovered" }));
        assert_eq!(calls.get(), 2, "failed loads must not poison the cache");
    }

    #[test]
    fn static_template_is_valid_json_with_slug() {
        let template =
            load_codex_model_template_static().expect("static template must parse as valid JSON");
        assert_eq!(
            template.get("slug").and_then(|v| v.as_str()),
            Some("gpt-5.5"),
            "static template slug must be gpt-5.5"
        );
    }

    #[test]
    fn static_template_has_required_keys() {
        let template =
            load_codex_model_template_static().expect("static template must parse as valid JSON");
        for key in &[
            "model_messages",
            "base_instructions",
            "context_window",
            "display_name",
        ] {
            assert!(
                template.get(key).is_some(),
                "static template must contain key '{key}'"
            );
        }
    }

    #[test]
    fn resolve_catalog_finds_relative_filename() {
        let config_text = r#"model_provider = "custom"
model_catalog_json = "cc-switch-model-catalog.json"
"#;
        let base_dir = PathBuf::from("/home/user/.codex");
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result,
            Some(base_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)),
            "relative filename should resolve under base_dir for file I/O"
        );
    }

    #[test]
    fn resolve_catalog_rejects_absolute_path_outside_config_dir() {
        let config_text = r#"model_catalog_json = "/tmp/secret/cc-switch-model-catalog.json"
"#;
        let base_dir = PathBuf::from("/home/user/.codex");
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result, None,
            "absolute path outside ~/.codex must not be accepted"
        );
    }

    #[test]
    fn resolve_catalog_accepts_absolute_path_inside_config_dir() {
        let config_text = r#"model_catalog_json = "/home/user/.codex/cc-switch-model-catalog.json"
"#;
        let base_dir = PathBuf::from("/home/user/.codex");
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result,
            Some(base_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)),
            "absolute path inside ~/.codex should be accepted"
        );
    }

    #[test]
    fn resolve_catalog_rejects_traversal_to_parent_directory() {
        let config_text = r#"model_catalog_json = "../cc-switch-model-catalog.json"
"#;
        let base_dir = PathBuf::from("/home/user/.codex");
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result, None,
            "relative traversal outside ~/.codex must not be accepted"
        );
    }

    #[test]
    #[cfg_attr(
        windows,
        ignore = "Requires Windows symlink privilege; run with --ignored when available"
    )]
    fn resolve_catalog_rejects_symlink_escaping_config_dir() {
        // 词法包含可被符号链接绕过：~/.codex/link -> 外部目录，
        // "link/cc-switch-model-catalog.json" 词法上在 base 内，真实读取却落到
        // base 外。canonicalize 之后的二次校验必须拒绝。
        let temp = tempfile::tempdir().expect("tempdir");
        let base_dir = temp.path().join("codex");
        let outside_dir = temp.path().join("outside");
        fs::create_dir_all(&base_dir).expect("create base");
        fs::create_dir_all(&outside_dir).expect("create outside");
        let escaped_file = outside_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
        fs::write(&escaped_file, r#"{"models":[]}"#).expect("write escaped catalog");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside_dir, base_dir.join("link")).expect("symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&outside_dir, base_dir.join("link")).expect("symlink");

        let config_text = r#"model_catalog_json = "link/cc-switch-model-catalog.json"
"#;
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        assert_eq!(
            result, None,
            "symlink escaping the config dir must be rejected after canonicalization"
        );
    }

    #[test]
    fn resolve_catalog_accepts_real_file_inside_config_dir() {
        // 存在于 base 内的真实文件：canonical 校验通过后仍应接受
        let temp = tempfile::tempdir().expect("tempdir");
        let base_dir = temp.path().join("codex");
        fs::create_dir_all(&base_dir).expect("create base");
        let catalog_file = base_dir.join(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME);
        fs::write(&catalog_file, r#"{"models":[]}"#).expect("write catalog");

        let config_text = r#"model_catalog_json = "cc-switch-model-catalog.json"
"#;
        let result = resolve_cc_switch_catalog_path(config_text, &base_dir);
        let resolved = result.expect("real file inside config dir should be accepted");
        assert_eq!(
            resolved.file_name().and_then(|n| n.to_str()),
            Some(CC_SWITCH_CODEX_MODEL_CATALOG_FILENAME)
        );
    }

    #[test]
    fn read_limited_string_rejects_oversized_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("huge.json");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(MAX_CODEX_CATALOG_BYTES + 1).expect("set_len");

        let result = read_limited_string(&path, MAX_CODEX_CATALOG_BYTES);
        assert!(
            result.is_err(),
            "file larger than MAX_CODEX_CATALOG_BYTES must be rejected"
        );
    }
}
