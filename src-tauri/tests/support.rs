use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use cc_switch_lib::{update_settings, AppSettings, AppState, Database};

/// 为测试设置隔离的 HOME 目录，避免污染真实用户数据。
pub fn ensure_test_home() -> &'static Path {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let base =
            std::env::temp_dir().join(format!("cc-switch-atlas-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).expect("create test home");
        // Windows 上 `dirs::home_dir()` 不受 HOME/USERPROFILE 影响（走 Known Folder API），
        // 用 CC_SWITCH_TEST_HOME 显式覆盖，以确保测试不会污染真实用户目录。
        std::env::set_var("CC_SWITCH_TEST_HOME", &base);
        std::env::set_var("HOME", &base);
        #[cfg(windows)]
        std::env::set_var("USERPROFILE", &base);
        // Claude Desktop 的配置目录在 Windows 上只读 LOCALAPPDATA（见 claude_desktop_config.rs
        // 的 windows_local_app_data_dir），既不认 CC_SWITCH_TEST_HOME 也不认 HOME。不覆盖它，
        // 涉及 Claude Desktop 供应商切换的测试会写进开发者真实的桌面版配置。
        #[cfg(windows)]
        std::env::set_var("LOCALAPPDATA", base.join("AppData").join("Local"));
        let app_dir = base.join(".cc-switch");
        std::fs::create_dir_all(&app_dir).expect("create isolated app data");
        std::fs::write(app_dir.join("cc-switch.db"), []).expect("disable legacy home fallback");
        base
    })
    .as_path()
}

/// 清理测试目录中生成的配置文件与缓存。
pub fn reset_test_fs() {
    let home = ensure_test_home();
    for sub in [
        ".claude",
        ".codex",
        ".cc-switch",
        ".gemini",
        ".grok",
        ".config",
        ".openclaw",
        "profiles",
    ] {
        let path = home.join(sub);
        if path.exists() {
            assert!(path
                .canonicalize()
                .unwrap()
                .starts_with(home.canonicalize().unwrap()));
            if let Err(err) = std::fs::remove_dir_all(&path) {
                eprintln!("failed to clean {}: {}", path.display(), err);
            }
        }
    }
    let claude_json = home.join(".claude.json");
    if claude_json.exists() {
        let _ = std::fs::remove_file(&claude_json);
    }

    let app_data = home.join(".cc-switch");
    std::fs::create_dir_all(&app_data).expect("create isolated app data");
    std::fs::write(app_data.join("cc-switch.db"), []).expect("disable legacy home fallback");

    // 重置内存中的设置缓存，确保测试环境不受上一次调用影响
    let _ = update_settings(AppSettings::default());
}

/// 全局互斥锁，避免多测试并发写入相同的 HOME 目录。
pub fn test_mutex() -> &'static Mutex<()> {
    static MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    MUTEX.get_or_init(|| Mutex::new(()))
}

/// 创建测试用的 AppState，包含一个空的数据库
#[allow(dead_code)]
pub fn create_test_state() -> Result<AppState, Box<dyn std::error::Error>> {
    let db = Arc::new(Database::init()?);
    Ok(AppState::new(db))
}
