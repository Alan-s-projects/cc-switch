use copilot_bridge_atlas_lib::{update_settings, AppSettings, AppState, Database};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub fn ensure_test_home() -> &'static Path {
    static TEST_HOME: OnceLock<PathBuf> = OnceLock::new();
    TEST_HOME
        .get_or_init(|| {
            let base = std::env::temp_dir().join(format!(
                "copilot-bridge-atlas-tests-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&base).expect("create isolated test directory");
            std::env::set_var("COPILOT_BRIDGE_ATLAS_TEST_HOME", &base);
            base
        })
        .as_path()
}

pub fn reset_test_fs() {
    let root = ensure_test_home();
    for name in [".codex", ".copilot-bridge-atlas"] {
        let target = root.join(name);
        if target.exists() {
            assert!(target
                .canonicalize()
                .unwrap()
                .starts_with(root.canonicalize().unwrap()));
            std::fs::remove_dir_all(&target).expect("remove isolated test data");
        }
    }
    update_settings(AppSettings::default()).expect("reset isolated settings");
}

pub fn test_mutex() -> &'static Mutex<()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

pub fn create_test_state() -> Result<AppState, Box<dyn std::error::Error>> {
    Ok(AppState::new(Arc::new(Database::init()?)))
}
