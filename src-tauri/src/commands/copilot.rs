//! Renderer-facing model discovery and subscription usage. Credentials stay in
//! the backend; authentication is exposed only through the GitHub device flow.
use crate::proxy::providers::copilot_auth::{
    CopilotAuthManager, CopilotModel, CopilotUsageResponse,
};
use std::sync::Arc;
use tauri::State;
use tokio::sync::RwLock;

pub struct CopilotAuthState(pub Arc<RwLock<CopilotAuthManager>>);

#[tauri::command]
pub async fn copilot_get_models(
    state: State<'_, CopilotAuthState>,
) -> Result<Vec<CopilotModel>, String> {
    state
        .0
        .read()
        .await
        .fetch_models()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_get_models_for_account(
    account_id: String,
    state: State<'_, CopilotAuthState>,
) -> Result<Vec<CopilotModel>, String> {
    state
        .0
        .read()
        .await
        .fetch_models_for_account(&account_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn copilot_get_usage(
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotUsageResponse, String> {
    state
        .0
        .read()
        .await
        .fetch_usage()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn copilot_get_usage_for_account(
    account_id: String,
    state: State<'_, CopilotAuthState>,
) -> Result<CopilotUsageResponse, String> {
    state
        .0
        .read()
        .await
        .fetch_usage_for_account(&account_id)
        .await
        .map_err(|error| error.to_string())
}
