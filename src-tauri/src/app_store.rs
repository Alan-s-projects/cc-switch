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
    let store = match app
        .store_builder("app_paths.json")
        .disable_auto_save()
        .build()
    {
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

            if !path.is_dir() {
                log::warn!(
                    "The configured app-data path is not a directory: {path:?}\n\
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
    let path = prepare_app_config_dir(path)?;
    let store = app
        .store_builder("app_paths.json")
        .disable_auto_save()
        .build()
        .map_err(|e| AppError::Message(format!("Could not open the app-path store: {e}")))?;

    update_and_save_override(
        store.get(STORE_KEY_APP_CONFIG_DIR),
        path.map(Value::String),
        |value| match value {
            Some(value) => store.set(STORE_KEY_APP_CONFIG_DIR, value),
            None => {
                store.delete(STORE_KEY_APP_CONFIG_DIR);
            }
        },
        || {
            store
                .save()
                .map_err(|e| AppError::Message(format!("Could not save the app-path store: {e}")))
        },
    )
}

fn update_and_save_override(
    previous: Option<Value>,
    next: Option<Value>,
    update: impl Fn(Option<Value>),
    save: impl FnOnce() -> Result<(), AppError>,
) -> Result<(), AppError> {
    update(next);
    if let Err(error) = save() {
        // The plugin shares its cache between readers. A rejected save must
        // not become the apparent saved value on the next Settings visit.
        update(previous);
        return Err(error);
    }
    Ok(())
}

/// Prepare a chosen data folder before saving it for the next launch.
fn prepare_app_config_dir(path: Option<&str>) -> Result<Option<String>, AppError> {
    let Some(raw) = path.map(str::trim).filter(|path| !path.is_empty()) else {
        return Ok(None);
    };
    let resolved = resolve_path(raw);
    if !resolved.is_absolute() {
        return Err(AppError::Message(
            "Choose an absolute path for the app-data directory.".into(),
        ));
    }
    std::fs::create_dir_all(&resolved).map_err(|error| AppError::io(&resolved, error))?;
    Ok(Some(raw.to_string()))
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
    fn failed_directory_save_restores_the_previous_cached_value() {
        use std::cell::RefCell;

        for (previous, next) in [
            (None, Some(Value::String("new".into()))),
            (Some(Value::String("old".into())), None),
            (
                Some(Value::String("old".into())),
                Some(Value::String("new".into())),
            ),
        ] {
            let cache = RefCell::new(previous.clone());
            let result = update_and_save_override(
                previous.clone(),
                next.clone(),
                |value| *cache.borrow_mut() = value,
                || {
                    assert_eq!(*cache.borrow(), next);
                    Err(AppError::Message("Disk write failed".into()))
                },
            );
            assert!(result.is_err());
            assert_eq!(*cache.borrow(), previous);
        }
    }

    #[test]
    fn new_data_folder_is_ready_for_the_next_launch() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("new").join("data");
        let raw = destination.to_string_lossy().into_owned();

        let saved = prepare_app_config_dir(Some(&format!("  {raw}  "))).unwrap();
        assert_eq!(saved.as_deref(), Some(raw.as_str()));
        assert!(resolve_path(saved.as_deref().unwrap()).is_dir());
        assert_eq!(std::fs::read_dir(&destination).unwrap().count(), 0);

        let marker = destination.join("settings.json");
        std::fs::write(&marker, b"existing preferences").unwrap();
        assert_eq!(prepare_app_config_dir(Some(&raw)).unwrap(), saved);
        assert_eq!(std::fs::read(&marker).unwrap(), b"existing preferences");
    }

    #[test]
    fn invalid_data_folder_is_rejected_without_modifying_existing_files() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("existing.txt");
        std::fs::write(&file, b"keep this file").unwrap();
        assert!(prepare_app_config_dir(Some(&file.to_string_lossy())).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"keep this file");
        assert!(prepare_app_config_dir(Some("relative-data")).is_err());
        assert_eq!(prepare_app_config_dir(None).unwrap(), None);
        assert_eq!(prepare_app_config_dir(Some(" \t ")).unwrap(), None);
    }

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
