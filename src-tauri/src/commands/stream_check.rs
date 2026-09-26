use crate::commands::copilot::CopilotAuthState;
use crate::error::AppError;
use crate::services::stream_check::{StreamCheckConfig, StreamCheckResult, StreamCheckService};
use crate::store::AppState;
use std::time::Duration;
use tauri::State;

#[tauri::command]
pub async fn stream_check_provider(
    state: State<'_, AppState>,
    copilot_state: State<'_, CopilotAuthState>,
    app_type: String,
    provider_id: String,
) -> Result<StreamCheckResult, AppError> {
    crate::copilot_bridge::require_codex(&app_type)?;
    let config = StreamCheckConfig::default();
    let provider = state
        .db
        .get_provider_by_id(&provider_id, "codex")?
        .ok_or_else(|| AppError::Message("Copilot provider not found".into()))?;
    crate::copilot_bridge::require_copilot(&provider)?;
    let endpoint = resolve_copilot_endpoint(
        &provider,
        &copilot_state,
        Duration::from_secs(config.timeout_secs),
    )
    .await?;
    let result = StreamCheckService::check_with_retry(&endpoint, &config).await?;
    let _ = state
        .db
        .save_stream_check_log(&provider.id, &provider.name, "codex", &result);
    Ok(result)
}

async fn resolve_copilot_endpoint(
    provider: &crate::provider::Provider,
    copilot_state: &CopilotAuthState,
    timeout: Duration,
) -> Result<String, AppError> {
    tokio::time::timeout(timeout, async {
        let manager = copilot_state.0.read().await;
        let account_id = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("github_copilot"));
        match account_id.as_deref() {
            Some(id) => manager.get_api_endpoint(id).await,
            None => manager.get_default_api_endpoint().await,
        }
    })
    .await
    .map_err(|_| AppError::Message("Copilot endpoint discovery timed out".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Provider, ProviderMeta};
    use crate::proxy::providers::copilot_auth::CopilotAuthManager;
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn endpoint_discovery_respects_timeout_and_recovers_without_network() {
        let directory = tempfile::tempdir().unwrap();
        let state = CopilotAuthState(Arc::new(RwLock::new(CopilotAuthManager::new(
            directory.path().to_path_buf(),
        ))));
        let mut provider = Provider::with_id("p1".into(), "Copilot".into(), json!({}));
        provider.meta = Some(ProviderMeta {
            provider_type: Some("github_copilot".into()),
            ..Default::default()
        });
        let lock = state.0.write().await;
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            resolve_copilot_endpoint(&provider, &state, Duration::from_millis(20)),
        )
        .await
        .expect("discovery must be bounded while waiting for the account lock");
        assert_eq!(
            result.unwrap_err().to_string(),
            "Copilot endpoint discovery timed out"
        );
        drop(lock);
        assert_eq!(
            resolve_copilot_endpoint(&provider, &state, Duration::from_secs(1))
                .await
                .unwrap(),
            "https://api.githubcopilot.com",
        );
    }
}
