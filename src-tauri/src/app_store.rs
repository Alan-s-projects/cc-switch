use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri_plugin_store::StoreExt;

use crate::error::AppError;

/// Store 中的键名
const STORE_KEY_APP_CONFIG_DIR: &str = "app_config_dir_override";

/// The database, settings and backups must share one directory for the entire
/// process lifetime. A saved override takes effect only on the next launch.
static APP_CONFIG_DIR_OVERRIDE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// 获取缓存中的 app_config_dir 覆盖路径
pub fn get_app_config_dir_override() -> Option<PathBuf> {
    APP_CONFIG_DIR_OVERRIDE.get().cloned().flatten()
}

pub fn read_override_from_store(app: &tauri::AppHandle) -> Option<PathBuf> {
    let store = match app.store_builder("app_paths.json").build() {
        Ok(store) => store,
        Err(e) => {
            log::warn!("Could not open the app-path store: {e}");
            return None;
        }
    };

    match store.get(STORE_KEY_APP_CONFIG_DIR) {
        Some(Value::String(path_str)) => {
            let path_str = path_str.trim();
            if path_str.is_empty() {
                return None;
            }

            let path = resolve_path(path_str);

            if !path.exists() {
                log::warn!(
                    "The configured app-data directory does not exist: {path:?}\n\
                     Using the default directory."
                );
                return None;
            }

            log::info!("Using the configured app-data directory: {path:?}");
            Some(path)
        }
        Some(_) => {
            log::warn!("The stored {STORE_KEY_APP_CONFIG_DIR} must be a string");
            None
        }
        None => None,
    }
}

fn initialize_override(
    cache: &OnceLock<Option<PathBuf>>,
    read: impl FnOnce() -> Option<PathBuf>,
) -> Option<PathBuf> {
    cache.get_or_init(read).clone()
}

pub fn initialize_app_config_dir_override(app: &tauri::AppHandle) -> Option<PathBuf> {
    initialize_override(&APP_CONFIG_DIR_OVERRIDE, || read_override_from_store(app))
}

/// 写入 app_config_dir 到 Tauri Store
pub fn set_app_config_dir_to_store(
    app: &tauri::AppHandle,
    path: Option<&str>,
) -> Result<(), AppError> {
    let store = app
        .store_builder("app_paths.json")
        .build()
        .map_err(|e| AppError::Message(format!("Could not open the app-path store: {e}")))?;

    match path {
        Some(p) => {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                store.set(STORE_KEY_APP_CONFIG_DIR, Value::String(trimmed.to_string()));
                log::info!("Saved the app-data directory override: {trimmed}");
            } else {
                store.delete(STORE_KEY_APP_CONFIG_DIR);
                log::info!("Removed the app-data directory override");
            }
        }
        None => {
            store.delete(STORE_KEY_APP_CONFIG_DIR);
            log::info!("Removed the app-data directory override");
        }
    }

    store
        .save()
        .map_err(|e| AppError::Message(format!("Could not save the app-path store: {e}")))?;

    Ok(())
}

/// 解析路径，支持 ~ 开头的相对路径
fn resolve_path(raw: &str) -> PathBuf {
    if raw == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    } else if let Some(stripped) = raw.strip_prefix("~\\") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }

    PathBuf::from(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_directory_changes_cannot_redirect_active_settings_or_backup_paths() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("active");
        let destination = root.path().join("next-launch");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&destination).unwrap();
        let destination_settings = destination.join("settings.json");
        std::fs::write(&destination_settings, b"destination preferences").unwrap();
        let cache = OnceLock::new();
        assert_eq!(
            initialize_override(&cache, || Some(source.clone())),
            Some(source.clone())
        );
        // Saving another choice or resetting it is a next-launch preference.
        for pending in [Some(destination.clone()), None] {
            let active = initialize_override(&cache, || pending.clone()).unwrap();
            assert_eq!(active, source);
            std::fs::write(active.join("settings.json"), b"active preferences").unwrap();
            assert_eq!(
                std::fs::read(&destination_settings).unwrap(),
                b"destination preferences"
            );
            assert_eq!(active.join("backups"), source.join("backups"));
        }
        let next_process = OnceLock::new();
        assert_eq!(
            initialize_override(&next_process, || Some(destination.clone())),
            Some(destination)
        );
    }
}
