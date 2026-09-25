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
    db.save_provider("codex", provider)?;
    if current(db)? == provider.id {
        select(db, &provider.id)?;
    }
    Ok(())
}

pub fn delete(db: &Database, id: &str) -> Result<(), AppError> {
    if let Some(provider) = db.get_provider_by_id(id, "codex")? {
        require_copilot(&provider)?;
    }
    db.delete_provider("codex", id)?;
    let next = current(db)?;
    if next.is_empty() {
        crate::settings::set_current_provider(&AppType::Codex, None)?;
    } else {
        select(db, &next)?;
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
    }
    Ok(())
}

/// Keep legacy DB rows and backups for rollback, but never activate their writers.
pub fn initialize(state: &AppState) -> Result<(), AppError> {
    let current = current(&state.db)?;
    if !current.is_empty() {
        select(&state.db, &current)?;
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
}

fn setup_suggestion(
    current_text: &str,
    path: &Path,
    endpoint: &str,
    catalog: Option<&Path>,
) -> Result<CodexSetupSuggestion, AppError> {
    // Return only generated connection fields; never return secrets from the input.
    let current = current_text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| {
            AppError::Message(
                "Codex config.toml is invalid. Fix it in your editor; no files were changed."
                    .into(),
            )
        })?;
    let active_provider = current.get("model_provider").and_then(|item| item.as_str());
    let provider_id = active_provider
        .filter(|id| !matches!(*id, "openai" | "ollama" | "lmstudio"))
        .unwrap_or("copilot");
    let configured = active_provider == Some(provider_id)
        && current
            .get("model_providers")
            .and_then(|item| item.get(provider_id))
            .and_then(|item| item.get("base_url"))
            .and_then(|item| item.as_str())
            .is_some_and(|url| url.trim_end_matches('/') == endpoint.trim_end_matches('/'));
    let mut suggested = toml_edit::DocumentMut::new();
    suggested["model_provider"] = toml_edit::value(provider_id);
    if let Some(catalog) = catalog {
        suggested["model_catalog_json"] = toml_edit::value(catalog.to_string_lossy().as_ref());
    }
    let mut provider = toml_edit::Table::new();
    provider["name"] = toml_edit::value("GitHub Copilot");
    provider["base_url"] = toml_edit::value(endpoint);
    provider["wire_api"] = toml_edit::value("responses");
    provider["requires_openai_auth"] = toml_edit::value(false);
    provider["experimental_bearer_token"] = toml_edit::value("PROXY_MANAGED");
    let mut providers = toml_edit::Table::new();
    providers.set_implicit(true);
    providers[provider_id] = toml_edit::Item::Table(provider);
    suggested["model_providers"] = toml_edit::Item::Table(providers);
    Ok(CodexSetupSuggestion {
        config_path: path.to_string_lossy().into_owned(),
        suggestion: suggested.to_string(),
        endpoint: endpoint.to_owned(),
        configured,
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
            assert!(result.suggestion.contains("model_provider = \"copilot\""));
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
        save(&db, &provider).unwrap();
        initialize(&state).unwrap();
        assert!(catalog_path().starts_with(&app_dir));
        assert!(catalog_path().exists());
        provider.name = "Renamed Copilot".into();
        save(&db, &provider).unwrap();
        let mut other = provider.clone();
        other.id = "copilot-b".into();
        save(&db, &other).unwrap();
        select(&db, &other.id).unwrap();
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
        state
            .proxy_service
            .hot_switch_provider("codex", &provider.id)
            .await
            .unwrap();
        let mut updated = state.proxy_service.get_config().await.unwrap();
        updated.listen_port = 0;
        state.proxy_service.update_config(&updated).await.unwrap();
        state.proxy_service.stop().await.unwrap();
        delete(&db, &other.id).unwrap();
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
