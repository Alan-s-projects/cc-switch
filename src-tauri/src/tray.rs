use crate::{AppError, AppState};
use tauri::menu::{Menu, MenuBuilder};
use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

pub const TRAY_ID: &str = "cc-switch";

pub fn create_tray_menu(
    app: &tauri::AppHandle,
    _state: &AppState,
) -> Result<Menu<tauri::Wry>, AppError> {
    MenuBuilder::new(app)
        .text("show_main", "Open CC Switch Atlas")
        .text("open_website", "GitHub repository")
        .separator()
        .text(
            "lightweight_mode",
            if crate::lightweight::is_lightweight_mode() {
                "Open window"
            } else {
                "Lightweight mode"
            },
        )
        .text("quit", "Quit")
        .build()
        .map_err(|error| AppError::Message(error.to_string()))
}

pub fn refresh_tray_menu(app: &tauri::AppHandle) {
    if let (Some(state), Some(tray)) = (app.try_state::<AppState>(), app.tray_by_id(TRAY_ID)) {
        if let Ok(menu) = create_tray_menu(app, &state) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

pub fn schedule_tray_refresh(app: &tauri::AppHandle) {
    refresh_tray_menu(app);
}

pub fn handle_tray_menu_event(app: &tauri::AppHandle, id: &str) {
    match id {
        "show_main" => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_skip_taskbar(false);
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            } else if let Err(error) = crate::lightweight::exit_lightweight_mode(app) {
                log::error!("Opening the window failed: {error}");
            }
        }
        "open_website" => {
            let _ = app.opener().open_url(
                "https://github.com/Alan-s-projects/cc-switch",
                None::<String>,
            );
        }
        "lightweight_mode" => {
            let result = if crate::lightweight::is_lightweight_mode() {
                crate::lightweight::exit_lightweight_mode(app)
            } else {
                crate::lightweight::enter_lightweight_mode(app)
            };
            if let Err(error) = result {
                log::error!("Changing window mode failed: {error}");
            }
        }
        "quit" => app.exit(0),
        _ => {}
    }
}
