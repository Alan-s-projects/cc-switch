//! The desktop bridge owns its database and generated catalog, never Codex files.
use crate::{AppError, AppState, AppType, Database, Provider};
use indexmap::IndexMap;
use serde::Serialize;
use std::path::Path;

pub fn require_codex(app: &str) -> Result<(), AppError> {
    if app != "codex" {
        return Err(AppError::Message("This build supports Codex only".into()));
    }
    Ok(())
}

pub fn require_copilot(provider: &Provider) -> Result<(), AppError> {
    if !provider.is_github_copilot() {
        return Err(AppError::Message(
            "This build supports GitHub Copilot only".into(),
        ));
    }
    Ok(())
}

pub fn providers(db: &Database) -> Result<IndexMap<String, Provider>, AppError> {
    Ok(db
        .get_all_providers("codex")?
        .into_iter()
        .filter(|(_, provider)| provider.is_github_copilot())
        .collect())
}

pub fn current(db: &Database) -> Result<String, AppError> {
    let providers = providers(db)?;
    let selected = crate::settings::get_effective_current_provider(db, &AppType::Codex)?;
    Ok(selected
        .filter(|id| providers.contains_key(id))
        .or_else(|| providers.keys().next().cloned())
        .unwrap_or_default())
}

pub fn select(db: &Database, id: &str) -> Result<(), AppError> {
    let provider = db
        .get_provider_by_id(id, "codex")?
        .ok_or_else(|| AppError::Message("Copilot provider not found".into()))?;
    require_copilot(&provider)?;
    db.set_current_provider("codex", id)?;
    crate::settings::set_current_provider(&AppType::Codex, Some(id))?;
    refresh_catalog(&provider)
}

pub fn save(db: &Database, provider: &Provider) -> Result<(), AppError> {
    require_copilot(provider)?;
    if current(db)? != provider.id || db.get_provider_by_id(&provider.id, "codex")?.is_none() {
        return Err(AppError::Message(
            "Only the existing Copilot entry can be edited".into(),
        ));
    }
    db.save_provider("codex", provider)?;
    if current(db)? == provider.id {
        select(db, &provider.id)?;
    }
    Ok(())
}

pub fn catalog_path() -> std::path::PathBuf {
    crate::config::get_app_config_dir().join("copilot-model-catalog.json")
}

fn refresh_catalog(provider: &Provider) -> Result<(), AppError> {
    let config = provider
        .settings_config
        .get("config")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if let Some(catalog) = crate::codex_config::codex_model_catalog_from_settings(
        &provider.settings_config,
        config,
        crate::codex_config::CodexCatalogToolProfile::Copilot,
    )? {
        crate::config::write_json_file(&catalog_path(), &catalog)?;
    } else {
        match std::fs::remove_file(catalog_path()) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(AppError::io(&catalog_path(), error)),
        }
    }
    Ok(())
}

fn ensure_copilot_entry(db: &Database) -> Result<(), AppError> {
    if !providers(db)?.is_empty() {
        return Ok(());
    }
    let mut provider = Provider::with_id(
        uuid::Uuid::new_v4().to_string(),
        "GitHub Copilot".into(),
        serde_json::json!({
            "auth": {},
            "config": "model_provider = \"copilot\"\n[model_providers.copilot]\nbase_url = \"https://api.githubcopilot.com\"\nwire_api = \"responses\"\n",
            "modelCatalog": { "models": [] }
        }),
        None,
    );
    provider.category = Some("third_party".into());
    provider.meta = Some(crate::ProviderMeta {
        provider_type: Some("github_copilot".into()),
        ..Default::default()
    });
    db.save_provider("codex", &provider)
}

/// Keep legacy DB rows and backups for rollback, but never activate their writers.
pub fn initialize(state: &AppState) -> Result<(), AppError> {
    ensure_copilot_entry(&state.db)?;
    let current = current(&state.db)?;
    if !current.is_empty() {
        select(&state.db, &current)?;
    }
    Ok(())
}

fn merge_live_capabilities(
    provider: &mut Provider,
    models: &[crate::proxy::providers::copilot_auth::CopilotModel],
) -> bool {
    use serde_json::{json, Value};
    let before = provider.settings_config.clone();
    if let Some(rows) = provider
        .settings_config
        .pointer_mut("/modelCatalog/models")
        .and_then(Value::as_array_mut)
    {
        for row in rows {
            let Some(model) = row
                .get("model")
                .and_then(Value::as_str)
                .and_then(|id| models.iter().find(|m| m.id.eq_ignore_ascii_case(id)))
            else {
                continue;
            };
            if let Some(parallel) = model.supports_parallel_tool_calls {
                row["supportsParallelToolCalls"] = json!(parallel);
            }
            if let Some(vision) = model.supports_vision {
                row["inputModalities"] = if vision {
                    json!(["text", "image"])
                } else {
                    json!(["text"])
                };
            }
            if let Some(limit) = model.context_window {
                let current = row
                    .get("contextWindow")
                    .and_then(|value| {
                        value
                            .as_u64()
                            .or_else(|| value.as_str()?.parse::<u64>().ok())
                    })
                    .filter(|limit| *limit > 0);
                row["contextWindow"] = json!(current.map_or(limit, |current| current.min(limit)));
            }
            if let Some(levels) = &model.reasoning_efforts {
                row["reasoningLevels"] = json!(levels);
                if row
                    .get("defaultReasoningLevel")
                    .and_then(Value::as_str)
                    .is_some_and(|default| !levels.iter().any(|level| level == default))
                {
                    row.as_object_mut().unwrap().remove("defaultReasoningLevel");
                }
            }
        }
    }
    provider.settings_config != before
}

/// Refresh only Atlas's saved model metadata and its own generated catalog.
/// A network failure leaves the last saved capabilities usable offline.
pub async fn refresh_capabilities(
    state: &AppState,
    auth: &crate::proxy::providers::copilot_auth::CopilotAuthManager,
) -> Result<(), AppError> {
    if !auth.is_authenticated().await {
        return Ok(());
    }
    let id = current(&state.db)?;
    let Some(provider) = state.db.get_provider_by_id(&id, "codex")? else {
        return Ok(());
    };
    let account = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("github_copilot"));
    let models = match account.as_deref() {
        Some(id) => auth.fetch_models_for_account(id).await,
        None => auth.fetch_models().await,
    }
    .map_err(|error| AppError::Message(error.to_string()))?;
    // Re-read after the network await so a concurrent settings edit is preserved.
    let Some(mut provider) = state.db.get_provider_by_id(&id, "codex")? else {
        return Ok(());
    };
    let active_account = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.managed_account_id_for("github_copilot"));
    if current(&state.db)? == id
        && active_account == account
        && merge_live_capabilities(&mut provider, &models)
    {
        save(&state.db, &provider)?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSetupSuggestion {
    pub config_path: String,
    pub suggestion: String,
    pub endpoint: String,
    pub configured: bool,
    pub current_provider: String,
    pub copilot_config: String,
    pub copilot_diff: String,
    pub openai_config: String,
    pub openai_diff: String,
}

fn set_connection_value(
    table: &mut dyn toml_edit::TableLike,
    key: &str,
    mut value: toml_edit::Item,
) {
    let existing = table.get(key);
    let unchanged = value
        .as_str()
        .is_some_and(|new| existing.and_then(toml_edit::Item::as_str) == Some(new))
        || value
            .as_bool()
            .is_some_and(|new| existing.and_then(toml_edit::Item::as_bool) == Some(new));
    if unchanged {
        return;
    }
    if let (Some(old), Some(new)) = (
        existing.and_then(toml_edit::Item::as_value),
        value.as_value_mut(),
    ) {
        *new.decor_mut() = old.decor().clone();
    }
    if let Some(existing) = table.get_mut(key) {
        // The key owns leading comments. Replacing its value in place preserves
        // those comments as well as the value decoration copied above.
        *existing = value;
    } else {
        table.insert(key, value);
    }
}

fn provider_table<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    id: &str,
) -> Result<&'a mut dyn toml_edit::TableLike, AppError> {
    if !doc.contains_key("model_providers") {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc["model_providers"] = toml_edit::Item::Table(table);
    }
    let providers = doc["model_providers"]
        .as_table_like_mut()
        .ok_or_else(|| AppError::Message("model_providers must be a TOML table".into()))?;
    if !providers.contains_key(id) {
        providers.insert(id, toml_edit::Item::Table(toml_edit::Table::new()));
    }
    providers
        .get_mut(id)
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or_else(|| AppError::Message("The selected model provider must be a TOML table".into()))
}

fn config_diff(current: &str, proposed: &str) -> String {
    // Line-ending differences must not obscure the actual connection changes.
    let current = current.replace("\r\n", "\n");
    let proposed = proposed.replace("\r\n", "\n");
    similar::TextDiff::from_lines(&current, &proposed)
        .unified_diff()
        .context_radius(3)
        .header("a/config.toml", "b/config.toml")
        .to_string()
}

fn setup_suggestion(
    current_text: &str,
    path: &Path,
    endpoint: &str,
    catalog: Option<&Path>,
) -> Result<CodexSetupSuggestion, AppError> {
    let current = current_text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| {
            AppError::Message(
                "Codex config.toml is invalid. Fix it in your editor; no files were changed."
                    .into(),
            )
        })?;
    let active = current
        .get("model_provider")
        .and_then(toml_edit::Item::as_str)
        .unwrap_or("openai");
    let previous = current
        .get("model_providers")
        .and_then(|items| items.get(active));
    let previous_url = previous
        .and_then(|item| item.get("base_url"))
        .and_then(toml_edit::Item::as_str)
        .or_else(|| {
            (active == "openai")
                .then(|| {
                    current
                        .get("openai_base_url")
                        .and_then(toml_edit::Item::as_str)
                })
                .flatten()
        });
    let current_provider = previous
        .and_then(|item| item.get("name"))
        .and_then(toml_edit::Item::as_str)
        .unwrap_or(active)
        .to_string();
    // Reusing the active custom id avoids changing Codex's provider identity when migrating a proxy.
    let id = if !active.is_empty()
        && !matches!(active, "openai" | "ollama" | "lmstudio" | "amazon-bedrock")
    {
        active.to_string()
    } else {
        current
            .get("model_providers")
            .and_then(toml_edit::Item::as_table_like)
            .and_then(|items| {
                items.iter().find_map(|(id, item)| {
                    if matches!(id, "openai" | "ollama" | "lmstudio" | "amazon-bedrock") {
                        return None;
                    }
                    item.get("base_url")
                        .and_then(toml_edit::Item::as_str)
                        .filter(|url| url.trim_end_matches('/') == endpoint.trim_end_matches('/'))
                        .map(|_| id.to_string())
                })
            })
            .unwrap_or_else(|| "cc-switch".to_string())
    };
    let configured = active == id
        && previous_url
            .is_some_and(|url| url.trim_end_matches('/') == endpoint.trim_end_matches('/'));
    let mut copilot = current.clone();
    set_connection_value(
        copilot.as_table_mut(),
        "model_provider",
        toml_edit::value(&id),
    );
    if let Some(catalog) = catalog {
        set_connection_value(
            copilot.as_table_mut(),
            "model_catalog_json",
            toml_edit::value(catalog.to_string_lossy().as_ref()),
        );
    } else {
        copilot.remove("model_catalog_json");
    }
    for key in ["openai_base_url", "chatgpt_base_url", "forced_login_method"] {
        copilot.remove(key);
    }
    if let Some(providers) = copilot
        .get_mut("model_providers")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        providers.remove("openai");
    }
    let target = provider_table(&mut copilot, &id)?;
    // Codex forbids command-backed auth together with env_key, a direct token, or OpenAI auth.
    // https://developers.openai.com/codex/config-reference/
    for key in ["auth", "env_key", "env_key_instructions"] {
        target.remove(key);
    }
    if !configured {
        for key in ["http_headers", "env_http_headers", "query_params"] {
            target.remove(key);
        }
    }
    for (key, value) in [
        ("name", toml_edit::value("GitHub Copilot")),
        ("base_url", toml_edit::value(endpoint)),
        ("wire_api", toml_edit::value("responses")),
        ("requires_openai_auth", toml_edit::value(false)),
        (
            "experimental_bearer_token",
            toml_edit::value("PROXY_MANAGED"),
        ),
        ("supports_websockets", toml_edit::value(false)),
    ] {
        set_connection_value(target, key, value);
    }
    if target.contains_key("supports_standalone_web_search") {
        set_connection_value(
            target,
            "supports_standalone_web_search",
            toml_edit::value(false),
        );
    }
    let mut openai = current.clone();
    set_connection_value(
        openai.as_table_mut(),
        "model_provider",
        toml_edit::value("openai"),
    );
    set_connection_value(
        openai.as_table_mut(),
        "forced_login_method",
        toml_edit::value("chatgpt"),
    );
    for key in ["model_catalog_json", "openai_base_url", "chatgpt_base_url"] {
        openai.remove(key);
    }
    if let Some(providers) = openai
        .get_mut("model_providers")
        .and_then(toml_edit::Item::as_table_like_mut)
    {
        providers.remove("openai");
        if providers.is_empty() {
            openai.remove("model_providers");
        }
    }
    let mut snippet = toml_edit::DocumentMut::new();
    snippet["model_provider"] = toml_edit::value(&id);
    if let Some(catalog) = catalog {
        snippet["model_catalog_json"] = toml_edit::value(catalog.to_string_lossy().as_ref());
    }
    let snippet_provider = provider_table(&mut snippet, &id)?;
    for (key, value) in [
        ("name", toml_edit::value("GitHub Copilot")),
        ("base_url", toml_edit::value(endpoint)),
        ("wire_api", toml_edit::value("responses")),
        ("requires_openai_auth", toml_edit::value(false)),
        (
            "experimental_bearer_token",
            toml_edit::value("PROXY_MANAGED"),
        ),
        ("supports_websockets", toml_edit::value(false)),
    ] {
        snippet_provider.insert(key, value);
    }
    let copilot_config = copilot.to_string();
    let openai_config = openai.to_string();
    Ok(CodexSetupSuggestion {
        config_path: path.to_string_lossy().into_owned(),
        suggestion: snippet.to_string(),
        endpoint: endpoint.into(),
        configured,
        current_provider,
        copilot_diff: config_diff(current_text, &copilot_config),
        openai_diff: config_diff(current_text, &openai_config),
        copilot_config,
        openai_config,
    })
}

#[tauri::command]
pub async fn get_codex_setup_suggestion(
    state: tauri::State<'_, AppState>,
) -> Result<CodexSetupSuggestion, String> {
    let path = crate::codex_config::get_codex_config_path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => return Err("Cannot read Codex config.toml".into()),
    };
    let config = state
        .db
        .get_global_proxy_config()
        .await
        .map_err(|e| e.to_string())?;
    let address = match config.listen_address.as_str() {
        "0.0.0.0" | "::" => "127.0.0.1",
        address => address,
    };
    let host = if address.contains(':') && !address.starts_with('[') {
        format!("[{address}]")
    } else {
        address.to_string()
    };
    let endpoint = format!("http://{host}:{}/v1", config.listen_port);
    let catalog = catalog_path();
    setup_suggestion(
        &text,
        &path,
        &endpoint,
        catalog.exists().then_some(catalog.as_path()),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_one_copilot_entry_and_preserves_existing_data() {
        let db = Database::memory().unwrap();
        let old = Provider::with_id(
            "legacy".into(),
            "Old provider".into(),
            serde_json::json!({}),
            None,
        );
        db.save_provider("codex", &old).unwrap();
        ensure_copilot_entry(&db).unwrap();
        let first = providers(&db).unwrap();
        assert_eq!(first.len(), 1);
        let provider = first.values().next().unwrap();
        assert_eq!(provider.name, "GitHub Copilot");
        assert_eq!(
            provider.settings_config["modelCatalog"]["models"],
            serde_json::json!([])
        );
        ensure_copilot_entry(&db).unwrap();
        assert_eq!(
            providers(&db).unwrap().keys().collect::<Vec<_>>(),
            first.keys().collect::<Vec<_>>()
        );
        assert!(db.get_provider_by_id("legacy", "codex").unwrap().is_some());
    }

    #[test]
    fn refresh_repairs_stale_flags_without_replacing_models_or_preferences() {
        use crate::proxy::providers::copilot_auth::CopilotModel;
        use serde_json::json;
        let mut provider = Provider::with_id(
            "copilot".into(),
            "Copilot".into(),
            json!({
                "config": "keep the stored template",
                "modelCatalog": {"models": [
                    {"model": "gpt-6-astra", "supportsParallelToolCalls": false, "inputModalities": ["text", "image"], "contextWindow": 1048576},
                    {"model": "gpt-6-luna", "inputModalities": ["text"], "contextWindow": 1000000, "defaultReasoningLevel": "ultra"},
                    {"model": "custom-alias", "inputModalities": ["text"]}
                ]}
            }),
            None,
        );
        let models = [
            CopilotModel {
                id: "gpt-6-astra".into(),
                context_window: Some(1050000),
                supports_parallel_tool_calls: Some(true),
                supports_vision: Some(true),
                ..Default::default()
            },
            CopilotModel {
                id: "gpt-6-luna".into(),
                context_window: Some(872000),
                supports_parallel_tool_calls: Some(true),
                supports_vision: Some(true),
                reasoning_efforts: Some(vec!["low".into(), "medium".into(), "max".into()]),
                ..Default::default()
            },
        ];
        assert!(merge_live_capabilities(&mut provider, &models));
        let rows = &provider.settings_config["modelCatalog"]["models"];
        assert_eq!(rows.as_array().unwrap().len(), 3);
        assert_eq!(rows[0]["supportsParallelToolCalls"], true);
        assert_eq!(rows[0]["contextWindow"], 1048576);
        assert_eq!(rows[1]["contextWindow"], 872000);
        assert_eq!(rows[1]["inputModalities"], json!(["text", "image"]));
        assert!(rows[1].get("defaultReasoningLevel").is_none());
        assert_eq!(
            rows[2],
            json!({"model": "custom-alias", "inputModalities": ["text"]})
        );
        assert_eq!(
            provider.settings_config["config"],
            "keep the stored template"
        );
        assert!(!merge_live_capabilities(&mut provider, &models));
    }

    #[test]
    fn migrates_an_old_proxy_and_restores_openai_without_touching_preferences_or_files() {
        let text = r#"# Keep my preferences
model_provider = "bridge"
model = "gpt-6-astra"
model_reasoning_effort = "high"
model_auto_compact_token_limit = 900000
model_catalog_json = "old-models.json"
openai_base_url = "http://old-openai-proxy/v1"
[model_providers.bridge]
name = "Copilot Bridge"
base_url = "http://localhost:4142/v1"
wire_api = "chat"
env_key = "OLD_KEY"
http_headers = { Authorization = "old-token" }
env_http_headers = { Authorization = "OLD_KEY" }
query_params = { key = "old-key" }
auth = { command = "old-auth-helper" }
[mcp_servers.keep]
command = "unchanged"
"#;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let catalog = dir.path().join("atlas-models.json");
        std::fs::write(&path, text).unwrap();
        let preview =
            setup_suggestion(text, &path, "http://127.0.0.1:15721/v1", Some(&catalog)).unwrap();
        let proposed = preview
            .copilot_config
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(proposed["model_provider"].as_str(), Some("bridge"));
        assert_eq!(
            proposed["model_providers"]["bridge"]["base_url"].as_str(),
            Some("http://127.0.0.1:15721/v1")
        );
        for key in [
            "auth",
            "env_key",
            "http_headers",
            "env_http_headers",
            "query_params",
        ] {
            assert!(
                proposed["model_providers"]["bridge"].get(key).is_none(),
                "{key}"
            );
        }
        for config in [&preview.copilot_config, &preview.openai_config] {
            let doc = config.parse::<toml_edit::DocumentMut>().unwrap();
            assert_eq!(doc["model_reasoning_effort"].as_str(), Some("high"));
            assert_eq!(
                doc["model_auto_compact_token_limit"].as_integer(),
                Some(900000)
            );
            assert_eq!(
                doc["mcp_servers"]["keep"]["command"].as_str(),
                Some("unchanged")
            );
            assert!(config.contains("# Keep my preferences"));
        }
        let official = preview
            .openai_config
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert_eq!(official["model_provider"].as_str(), Some("openai"));
        assert_eq!(official["forced_login_method"].as_str(), Some("chatgpt"));
        assert!(!official.contains_key("model_catalog_json"));
        assert!(!official.contains_key("openai_base_url"));
        assert!(preview.copilot_diff.contains("--- a/config.toml"));
        assert!(preview
            .copilot_diff
            .contains("-base_url = \"http://localhost:4142/v1\""));
        let second = setup_suggestion(
            &preview.copilot_config,
            &path,
            "http://127.0.0.1:15721/v1",
            Some(&catalog),
        )
        .unwrap();
        assert!(second.copilot_diff.is_empty(), "{}", second.copilot_diff);
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }

    #[test]
    fn handles_inline_tables_missing_providers_and_reserved_ids() {
        for text in [
            "model_provider = 'custom'\nmodel_providers = { custom = { base_url = 'http://old/v1', env_key = 'OLD' } }\n",
            "model_provider = 'new'\nmodel_providers = { old = { base_url = 'http://old/v1' } }\n",
            "model_provider = 'missing'\n",
            "model_provider = 'openai'\n[model_providers.openai]\nbase_url = 'http://127.0.0.1:15721/v1'\n",
        ] {
            let preview = setup_suggestion(text, Path::new("config.toml"), "http://127.0.0.1:15721/v1", None).unwrap();
            let doc = preview.copilot_config.parse::<toml_edit::DocumentMut>().unwrap();
            let id = doc["model_provider"].as_str().unwrap();
            assert_ne!(id, "openai");
            assert_eq!(doc["model_providers"][id]["wire_api"].as_str(), Some("responses"));
            assert_eq!(doc["model_providers"][id]["requires_openai_auth"].as_bool(), Some(false));
            assert!(doc["model_providers"].get("openai").is_none());
        }
    }

    #[test]
    fn suggestion_preserves_provider_id_without_copying_preferences_or_secrets() {
        let text = r#"# user comment
model_provider = "my-copilot"
model = "gpt-6-astra"
model_auto_compact_token_limit = 900000
[model_providers.my-copilot]
base_url = "http://127.0.0.1:15721/v1"
experimental_bearer_token = "private-token"
[mcp_servers.example]
command = "keep-me"
"#;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(&path, text).unwrap();
        let result = setup_suggestion(text, &path, "http://127.0.0.1:15721/v1", None).unwrap();
        assert!(result.configured);
        let parsed = result.suggestion.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(parsed["model_provider"].as_str(), Some("my-copilot"));
        assert!(!result.suggestion.contains("private-token"));
        assert!(!result.suggestion.contains("mcp_servers"));
        assert!(!result.suggestion.contains("model_auto_compact"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }

    #[test]
    fn suggestion_handles_fresh_and_official_configs_without_creating_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        for text in ["", "model_provider = \"openai\"\n"] {
            let result = setup_suggestion(text, &path, "http://127.0.0.1:15721/v1", None).unwrap();
            assert!(!result.configured);
            assert!(result.suggestion.contains("model_provider = \"cc-switch\""));
            assert!(!path.exists());
        }
        assert!(setup_suggestion("broken = [", &path, "http://localhost/v1", None).is_err());
        assert!(!path.exists());
        let official = "model_provider = \"openai\"\n[model_providers.copilot]\nbase_url = \"http://127.0.0.1:15721/v1\"\n";
        assert!(
            !setup_suggestion(official, &path, "http://127.0.0.1:15721/v1", None)
                .unwrap()
                .configured
        );
    }

    #[test]
    fn unsupported_clients_and_providers_are_rejected() {
        assert!(require_codex("codex").is_ok());
        for app in ["claude", "gemini", "grokbuild", "opencode"] {
            assert!(require_codex(app).is_err());
        }
        let mut provider =
            Provider::with_id("test".into(), "test".into(), serde_json::json!({}), None);
        assert!(require_copilot(&provider).is_err());
        provider.meta = Some(
            serde_json::from_value(serde_json::json!({
                "providerType": "github_copilot"
            }))
            .unwrap(),
        );
        assert!(require_copilot(&provider).is_ok());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn provider_and_proxy_lifecycle_leave_client_files_unchanged() {
        struct TestHome {
            previous: Option<std::ffi::OsString>,
            _dir: tempfile::TempDir,
        }
        impl Drop for TestHome {
            fn drop(&mut self) {
                if let Some(previous) = &self.previous {
                    std::env::set_var("CC_SWITCH_TEST_HOME", previous);
                } else {
                    std::env::remove_var("CC_SWITCH_TEST_HOME");
                }
                let _ = crate::settings::reload_settings();
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("CC_SWITCH_TEST_HOME");
        std::env::set_var("CC_SWITCH_TEST_HOME", directory.path());
        let fixture = TestHome {
            previous,
            _dir: directory,
        };
        let app_dir = fixture._dir.path().join(".cc-switch");
        std::fs::create_dir_all(&app_dir).unwrap();
        // Prevent the legacy Windows HOME fallback from escaping the test directory.
        std::fs::write(app_dir.join("cc-switch.db"), []).unwrap();
        assert_eq!(crate::config::get_app_config_dir(), app_dir);
        crate::settings::reload_settings().unwrap();
        let client_dir = fixture._dir.path().join(".codex");
        std::fs::create_dir_all(client_dir.join("skills")).unwrap();
        let files = [
            ("config.toml", "# keep comments\nmodel_auto_compact_token_limit = 900000\n[mcp_servers.keep]\ncommand = 'keep'\n"),
            ("auth.json", r#"{"OPENAI_API_KEY":"keep-local-auth"}"#),
            ("AGENTS.md", "Keep my instructions"),
            ("skills/SKILL.md", "Keep my skill"),
        ];
        for (name, contents) in files {
            std::fs::write(client_dir.join(name), contents).unwrap();
        }
        let db = std::sync::Arc::new(Database::memory().unwrap());
        let state = AppState::new(db.clone());
        let mut provider = Provider::with_id(
            "copilot-a".into(),
            "Copilot".into(),
            serde_json::json!({
                "config": "model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"https://api.githubcopilot.com\"\n",
                "modelCatalog": {"models": [{"model": "gpt-6-astra", "contextWindow": 1048576}]}
            }),
            None,
        );
        provider.meta = Some(
            serde_json::from_value(serde_json::json!({
                "providerType": "github_copilot"
            }))
            .unwrap(),
        );
        db.save_provider("codex", &provider).unwrap();
        initialize(&state).unwrap();
        assert!(catalog_path().starts_with(&app_dir));
        assert!(catalog_path().exists());
        provider.name = "Renamed Copilot".into();
        save(&db, &provider).unwrap();
        let mut other = provider.clone();
        other.id = "copilot-b".into();
        db.save_provider("codex", &other).unwrap();
        // Old takeover data must never cause a restore or rewrite in this build.
        db.save_live_backup("codex", r#"{"config":"old configuration"}"#)
            .await
            .unwrap();
        let mut settings = crate::settings::get_settings();
        settings.language = Some("ja".into());
        settings.unify_codex_session_history = true;
        crate::commands::save_settings(settings).await.unwrap();
        assert_eq!(
            crate::settings::get_settings().language.as_deref(),
            Some("en")
        );
        crate::commands::sync_support::run_post_import_sync(&state).unwrap();
        let mut config = db.get_proxy_config().await.unwrap();
        config.listen_port = 0;
        config.live_takeover_active = true;
        db.update_proxy_config(config).await.unwrap();
        let info = state.proxy_service.start().await.unwrap();
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let origin = format!("http://127.0.0.1:{}", info.port);
        assert_eq!(
            client
                .get(format!("{origin}/health"))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        // The client TOML deliberately has no catalog pointer. Discovery must
        // still serve the Atlas-owned catalog, not consult or change that file.
        let models: serde_json::Value = client
            .get(format!("{origin}/v1/models"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(models["models"][0]["slug"], "gpt-6-astra");
        assert_eq!(
            client
                .post(format!("{origin}/claude/v1/messages"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        assert_eq!(
            client
                .post(format!("{origin}/grokbuild/v1/responses"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        assert_eq!(
            client
                .get(format!("{origin}/v1/responses"))
                .send()
                .await
                .unwrap()
                .status(),
            405
        );
        let mut updated = state.proxy_service.get_config().await.unwrap();
        updated.listen_port = 0;
        state.proxy_service.update_config(&updated).await.unwrap();
        state.proxy_service.stop().await.unwrap();
        assert!(save(&db, &other).is_err());
        assert!(db.has_any_live_backup().await.unwrap());
        for (name, contents) in files {
            assert_eq!(
                std::fs::read_to_string(client_dir.join(name)).unwrap(),
                contents,
                "{name}"
            );
        }
        assert!(!client_dir.join("cc-switch-model-catalog.json").exists());
    }
}
