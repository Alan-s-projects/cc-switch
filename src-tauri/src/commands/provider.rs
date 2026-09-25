use crate::{AppState, Provider};
use indexmap::IndexMap;
use tauri::{Manager, State};

#[tauri::command]
pub fn get_providers(
    state: State<'_, AppState>,
    app: String,
) -> Result<IndexMap<String, Provider>, String> {
    crate::copilot_bridge::require_codex(&app).map_err(|e| e.to_string())?;
    crate::copilot_bridge::providers(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_current_provider(state: State<'_, AppState>, app: String) -> Result<String, String> {
    crate::copilot_bridge::require_codex(&app).map_err(|e| e.to_string())?;
    crate::copilot_bridge::current(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_provider(
    app_handle: tauri::AppHandle,
    app: String,
    provider: Provider,
) -> Result<bool, String> {
    crate::copilot_bridge::require_codex(&app).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        crate::copilot_bridge::save(&state.db, &provider)
            .map(|_| true)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Saving Copilot settings failed: {e}"))?
}
