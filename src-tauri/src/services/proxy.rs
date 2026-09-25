//! 代理服务业务逻辑层
//!
//! 提供代理服务器的启动、停止和配置管理

use crate::app_config::AppType;
use crate::config::{get_claude_settings_path, read_json_file, write_json_file};
use crate::database::Database;
use crate::provider::Provider;
use crate::proxy::providers::codex_oauth_auth::{CodexLiveAuthSwitchGuard, CodexOAuthManager};
use crate::proxy::server::ProxyServer;
use crate::proxy::switch_lock::SwitchLockManager;
use crate::proxy::types::*;
use crate::services::provider::{
    build_effective_provider_for_live_with_codex_oauth_manager,
    build_effective_settings_with_common_config,
    write_live_with_common_config_for_codex_oauth_manager,
};
use serde_json::{json, Map, Value};
use std::str::FromStr;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::RwLock;

/// 用于接管 Live 配置时的占位符（避免客户端提示缺少 key，同时不泄露真实 Token）
const PROXY_TOKEN_PLACEHOLDER: &str = "PROXY_MANAGED";

/// 代理接管模式下需要从 Claude Live 配置中移除的"模型覆盖"字段。
///
/// 原因：接管模式下 `*_MODEL` 必须由 CC Switch 写成稳定的 Claude 角色别名，
/// 再由本地代理映射到当前供应商真实模型；`*_MODEL_NAME` 也需要同步接管，
/// 否则 Claude Code 模型菜单会残留上一个供应商的显示名称。
const CLAUDE_MODEL_OVERRIDE_ENV_KEYS: [&str; 12] = [
    "ANTHROPIC_MODEL",
    "ANTHROPIC_REASONING_MODEL", // legacy: 已废弃，但旧配置可能残留
    "ANTHROPIC_DEFAULT_HAIKU_MODEL",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
    "ANTHROPIC_DEFAULT_SONNET_MODEL",
    "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
    "ANTHROPIC_DEFAULT_OPUS_MODEL",
    "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
    "ANTHROPIC_DEFAULT_FABLE_MODEL",
    "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
    "ANTHROPIC_SMALL_FAST_MODEL", // Legacy key (已废弃)：历史版本使用该字段区分 small/fast 模型
    "CLAUDE_CODE_SUBAGENT_MODEL",
];

const CLAUDE_TAKEOVER_HAIKU_MODEL: &str = "claude-haiku-4-5";
const CLAUDE_TAKEOVER_SONNET_MODEL: &str = "claude-sonnet-5";
const CLAUDE_TAKEOVER_OPUS_MODEL: &str = "claude-opus-5";
const CLAUDE_TAKEOVER_FABLE_MODEL: &str = "claude-fable-5";
// 写给 Claude Code 时沿用文档示例的大写形式；解析侧大小写不敏感。
const CLAUDE_ONE_M_MARKER_FOR_CLIENT: &str = "[1M]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeTakeoverAuthPolicy {
    PreserveExistingOrAuthToken,
    ManagedAccount { keep_auth_token: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexAuthFileSnapshot {
    contents: Option<Vec<u8>>,
}

impl CodexAuthFileSnapshot {
    fn capture() -> Result<Self, String> {
        let path = crate::codex_config::get_codex_auth_path();
        match std::fs::read(&path) {
            Ok(contents) => Ok(Self {
                contents: Some(contents),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(Self { contents: None })
            }
            Err(error) => Err(format!(
                "读取 Codex auth 失败 ({}): {error}",
                path.display()
            )),
        }
    }

    fn value(&self) -> Result<Option<Value>, String> {
        self.contents
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .map_err(|error| format!("读取 Codex auth 失败: {error}"))
    }
}

/// Owns the exact auth.json generation observed at restore start.
///
/// A plain compare-then-write is unsafe because Codex can replace auth.json
/// between those two operations. We instead atomically move the current path
/// aside, compare the moved bytes, and only install a replacement if the path
/// is still vacant. A newer Codex login therefore always wins.
struct CodexAuthFileTransaction {
    path: std::path::PathBuf,
    quarantined: Option<std::path::PathBuf>,
    installed: Option<Vec<u8>>,
    finished: bool,
}

impl CodexAuthFileTransaction {
    fn begin(expected: &CodexAuthFileSnapshot) -> Result<Self, String> {
        let path = crate::codex_config::get_codex_auth_path();
        let mut transaction = Self {
            path: path.clone(),
            quarantined: None,
            installed: None,
            finished: false,
        };

        let Some(expected_contents) = expected.contents.as_deref() else {
            return match std::fs::read(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(transaction),
                Ok(_) => Err(Self::changed_error()),
                Err(error) => Err(format!(
                    "读取 Codex auth 失败 ({}): {error}",
                    path.display()
                )),
            };
        };

        // The no-clobber install/rollback protocol below requires hard links.
        // Probe before moving the live credentials so unsupported custom Codex
        // directories fail closed with auth.json still in place.
        let probe = Self::unique_sibling_path(&path, "restore-probe")?;
        match std::fs::hard_link(&path, &probe) {
            Ok(()) => {
                std::fs::remove_file(&probe).map_err(|error| {
                    format!(
                        "清理 Codex auth 事务能力探针失败 ({}): {error}",
                        probe.display()
                    )
                })?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Self::changed_error());
            }
            Err(error) => {
                return Err(format!(
                    "Codex auth 所在文件系统不支持安全恢复，原凭据未修改 ({}): {error}",
                    path.display()
                ));
            }
        }

        let quarantine = Self::unique_sibling_path(&path, "restore-backup")?;
        match std::fs::rename(&path, &quarantine) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Self::changed_error());
            }
            Err(error) => {
                return Err(format!(
                    "认领 Codex auth 失败 ({}): {error}",
                    path.display()
                ));
            }
        }
        transaction.quarantined = Some(quarantine.clone());

        let actual = std::fs::read(&quarantine).map_err(|error| {
            format!(
                "读取已认领的 Codex auth 失败 ({}): {error}",
                quarantine.display()
            )
        })?;
        if actual != expected_contents {
            let restore_result = transaction.restore_quarantined_if_vacant();
            transaction.finished = true;
            return match restore_result {
                Ok(()) => Err(Self::changed_error()),
                Err(restore_error) => Err(format!(
                    "{}; 恢复较新的 Codex auth 失败: {restore_error}",
                    Self::changed_error()
                )),
            };
        }

        Ok(transaction)
    }

    fn install(&mut self, replacement: Option<Vec<u8>>) -> Result<(), String> {
        let Some(contents) = replacement else {
            return Ok(());
        };

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!("创建 Codex auth 目录失败 ({}): {error}", parent.display())
            })?;
        }
        let temporary = Self::unique_sibling_path(&self.path, "restore-new")?;
        let write_result = (|| -> Result<(), String> {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(|error| {
                format!(
                    "创建 Codex auth 临时文件失败 ({}): {error}",
                    temporary.display()
                )
            })?;
            use std::io::Write;
            file.write_all(&contents)
                .and_then(|_| file.flush())
                .map_err(|error| {
                    format!(
                        "写入 Codex auth 临时文件失败 ({}): {error}",
                        temporary.display()
                    )
                })?;
            drop(file);

            match std::fs::hard_link(&temporary, &self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    Err(Self::changed_error())
                }
                Err(error) => Err(format!(
                    "安装 Codex auth 失败 ({}): {error}",
                    self.path.display()
                )),
            }
        })();
        let _ = std::fs::remove_file(&temporary);

        if let Err(error) = write_result {
            // If Codex created a newer path while the expected generation was
            // quarantined, never put the older generation back over it.
            if self.path.exists() {
                self.discard_quarantined();
                self.finished = true;
            }
            return Err(error);
        }

        self.installed = Some(contents);
        Ok(())
    }

    fn commit(mut self) -> Result<(), String> {
        self.discard_quarantined();
        self.finished = true;
        Ok(())
    }

    fn rollback(mut self) -> Result<(), String> {
        let result = self.rollback_inner();
        self.finished = true;
        result
    }

    fn rollback_inner(&mut self) -> Result<(), String> {
        if let Some(installed) = self.installed.take() {
            let replacement_quarantine = Self::unique_sibling_path(&self.path, "restore-rollback")?;
            match std::fs::rename(&self.path, &replacement_quarantine) {
                Ok(()) => {
                    let current = std::fs::read(&replacement_quarantine).map_err(|error| {
                        format!(
                            "读取待回滚 Codex auth 失败 ({}): {error}",
                            replacement_quarantine.display()
                        )
                    })?;
                    if current == installed {
                        std::fs::remove_file(&replacement_quarantine).map_err(|error| {
                            format!(
                                "删除待回滚 Codex auth 失败 ({}): {error}",
                                replacement_quarantine.display()
                            )
                        })?;
                    } else {
                        // Codex replaced our installed generation. Restore that
                        // newer file if the path is still vacant and discard the
                        // old expected generation.
                        Self::restore_file_if_vacant(&replacement_quarantine, &self.path)?;
                        self.discard_quarantined();
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // A concurrent logout removed our generation. Missing auth
                    // is newer state too; do not resurrect the old credentials.
                    self.discard_quarantined();
                    return Ok(());
                }
                Err(error) => {
                    return Err(format!(
                        "回滚 Codex auth 失败 ({}): {error}",
                        self.path.display()
                    ));
                }
            }
        }

        self.restore_quarantined_if_vacant()
    }

    fn restore_quarantined_if_vacant(&mut self) -> Result<(), String> {
        let Some(quarantined) = self.quarantined.take() else {
            return Ok(());
        };
        Self::restore_file_if_vacant(&quarantined, &self.path)
    }

    fn restore_file_if_vacant(
        source: &std::path::Path,
        destination: &std::path::Path,
    ) -> Result<(), String> {
        match std::fs::hard_link(source, destination) {
            Ok(()) => {
                std::fs::remove_file(source).map_err(|error| {
                    format!(
                        "清理 Codex auth 事务文件失败 ({}): {error}",
                        source.display()
                    )
                })?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                // The destination was recreated by Codex and is newer than the
                // quarantined generation.
                std::fs::remove_file(source).map_err(|remove_error| {
                    format!(
                        "清理旧 Codex auth 事务文件失败 ({}): {remove_error}",
                        source.display()
                    )
                })?;
                Ok(())
            }
            Err(error) => Err(format!(
                "恢复 Codex auth 事务文件失败 ({} -> {}): {error}",
                source.display(),
                destination.display()
            )),
        }
    }

    fn discard_quarantined(&mut self) {
        if let Some(path) = self.quarantined.take() {
            if let Err(error) = std::fs::remove_file(&path) {
                log::warn!(
                    "清理旧 Codex auth 事务文件失败 ({}): {error}",
                    path.display()
                );
            }
        }
    }

    fn unique_sibling_path(
        path: &std::path::Path,
        label: &str,
    ) -> Result<std::path::PathBuf, String> {
        let parent = path
            .parent()
            .ok_or_else(|| format!("无效的 Codex auth 路径: {}", path.display()))?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("auth.json");
        Ok(parent.join(format!(
            ".{file_name}.cc-switch-{label}-{}",
            uuid::Uuid::new_v4()
        )))
    }

    fn changed_error() -> String {
        "Codex auth 在恢复期间发生变化；为避免覆盖新凭据，本次恢复已取消，请重试".to_string()
    }
}

impl Drop for CodexAuthFileTransaction {
    fn drop(&mut self) {
        if !self.finished {
            if let Err(error) = self.rollback_inner() {
                log::error!("Codex auth 事务自动回滚失败: {error}");
            }
        }
    }
}

#[derive(Clone)]
pub struct ProxyService {
    db: Arc<Database>,
    codex_oauth_manager: Arc<CodexOAuthManager>,
    server: Arc<RwLock<Option<ProxyServer>>>,
    /// AppHandle，用于传递给 ProxyServer 以支持故障转移时的 UI 更新
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
    switch_locks: SwitchLockManager,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct HotSwitchOutcome {
    pub logical_target_changed: bool,
}

impl ProxyService {
    pub fn new(db: Arc<Database>) -> Self {
        let codex_oauth_manager =
            Arc::new(CodexOAuthManager::new(crate::config::get_app_config_dir()));

        Self::new_with_codex_oauth_manager(db, codex_oauth_manager)
    }

    pub fn new_with_codex_oauth_manager(
        db: Arc<Database>,
        codex_oauth_manager: Arc<CodexOAuthManager>,
    ) -> Self {
        Self {
            db,
            codex_oauth_manager,
            server: Arc::new(RwLock::new(None)),
            app_handle: Arc::new(RwLock::new(None)),
            switch_locks: SwitchLockManager::new(),
        }
    }

    #[cfg(test)]
    fn apply_claude_takeover_fields(config: &mut Value, proxy_url: &str) {
        Self::apply_claude_takeover_fields_with_policy(
            config,
            proxy_url,
            ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken,
        );
    }

    fn apply_claude_takeover_fields_for_provider(
        config: &mut Value,
        proxy_url: &str,
        provider: &Provider,
    ) {
        let auth_policy = if provider.uses_managed_account_auth() {
            // Codex 系（含仅凭 base_url 识别、无 provider_type meta 的）必须保留
            // ANTHROPIC_AUTH_TOKEN 占位符：Claude Code 缺该键会弹登录提示（#3784）。
            // Copilot 默认同样注入 AUTH_TOKEN 占位符：Claude Code（实测 2.1.220）
            // 对 ANTHROPIC_API_KEY 会弹"是否使用该自定义 key"确认框且默认
            // "No (recommended)"，按默认走后占位符被忽略、落入 Not logged in
            // （并非 sk-ant-* 格式校验——headless 下占位符原样出站）；AUTH_TOKEN
            // 作为网关 Bearer 被直接信任，零弹窗。仅当供应商表单显式选择了
            // ANTHROPIC_API_KEY（meta.apiKeyField）时才保留 API_KEY 占位，以规避
            // 与 /login 管理的 key 冲突（#1049）。
            ClaudeTakeoverAuthPolicy::ManagedAccount {
                keep_auth_token: !provider.is_github_copilot()
                    || !provider.claude_uses_api_key_field(),
            }
        } else {
            ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken
        };
        // Copilot/Codex 接管时 live config 可能还是旧供应商；显示模型必须跟随目标 provider。
        let takeover_model_fields = if provider.uses_managed_account_auth() {
            Self::build_claude_takeover_model_fields(&provider.settings_config)
        } else {
            Self::build_claude_takeover_model_fields(config)
        };

        Self::apply_claude_takeover_fields_with_policy_and_models(
            config,
            proxy_url,
            auth_policy,
            takeover_model_fields,
        );
    }

    fn apply_claude_takeover_fields_with_policy(
        config: &mut Value,
        proxy_url: &str,
        auth_policy: ClaudeTakeoverAuthPolicy,
    ) {
        // 必须在 remove/insert 前 snapshot：避免读到自己刚写入的接管别名。
        let takeover_model_fields = Self::build_claude_takeover_model_fields(config);

        Self::apply_claude_takeover_fields_with_policy_and_models(
            config,
            proxy_url,
            auth_policy,
            takeover_model_fields,
        );
    }

    fn apply_claude_takeover_fields_with_policy_and_models(
        config: &mut Value,
        proxy_url: &str,
        auth_policy: ClaudeTakeoverAuthPolicy,
        takeover_model_fields: Vec<(&'static str, String)>,
    ) {
        if !config.is_object() {
            *config = json!({});
        }

        let root = config
            .as_object_mut()
            .expect("Claude config should be normalized to an object");
        let env = root.entry("env".to_string()).or_insert_with(|| json!({}));
        if !env.is_object() {
            *env = json!({});
        }

        let env = env
            .as_object_mut()
            .expect("Claude env should be normalized to an object");
        env.insert("ANTHROPIC_BASE_URL".to_string(), json!(proxy_url));

        for key in CLAUDE_MODEL_OVERRIDE_ENV_KEYS {
            env.remove(key);
        }

        for (key, value) in takeover_model_fields {
            env.insert(key.to_string(), Value::String(value));
        }

        let token_keys = [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ];

        match auth_policy {
            ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken => {
                let mut replaced_any = false;
                for key in token_keys {
                    if env.contains_key(key) {
                        env.insert(key.to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                        replaced_any = true;
                    }
                }

                if !replaced_any {
                    env.insert(
                        "ANTHROPIC_AUTH_TOKEN".to_string(),
                        json!(PROXY_TOKEN_PLACEHOLDER),
                    );
                }
            }
            ClaudeTakeoverAuthPolicy::ManagedAccount { keep_auth_token } => {
                for key in token_keys {
                    env.remove(key);
                }
                // 只注入一个认证键：两者同时存在会触发 Claude Code 的
                // "Both ANTHROPIC_AUTH_TOKEN and ANTHROPIC_API_KEY set" 警告（#4919）。
                // - Codex 系保留 AUTH_TOKEN：缺该键 Claude Code 会弹登录提示（#3784）。
                //   无条件注入而非"已存在才保留"：热切换路径传入的是 provider
                //   settings（预设不含该键），且旧版接管已把存量用户 live 中的键删光。
                // - Copilot 默认 AUTH_TOKEN：API_KEY 占位符会触发 Claude Code 的
                //   自定义 key 确认框（默认 "No (recommended)"），按默认走即
                //   Not logged in；仅当表单显式选择了 ANTHROPIC_API_KEY 时才用
                //   API_KEY 占位以规避 /login key 冲突（#1049）。
                if keep_auth_token {
                    env.insert(
                        "ANTHROPIC_AUTH_TOKEN".to_string(),
                        json!(PROXY_TOKEN_PLACEHOLDER),
                    );
                } else {
                    env.insert(
                        "ANTHROPIC_API_KEY".to_string(),
                        json!(PROXY_TOKEN_PLACEHOLDER),
                    );
                }
            }
        }
    }

    fn build_claude_takeover_model_fields(config: &Value) -> Vec<(&'static str, String)> {
        let Some(env) = config.get("env").and_then(Value::as_object) else {
            return Vec::new();
        };

        let default_model = Self::claude_env_string(env, "ANTHROPIC_MODEL");
        let small_fast_model = Self::claude_env_string(env, "ANTHROPIC_SMALL_FAST_MODEL");
        let haiku_model = Self::claude_env_string(env, "ANTHROPIC_DEFAULT_HAIKU_MODEL")
            .or(small_fast_model)
            .or(default_model);
        let sonnet_model = Self::claude_env_string(env, "ANTHROPIC_DEFAULT_SONNET_MODEL")
            .or(default_model)
            .or(small_fast_model);
        let opus_model = Self::claude_env_string(env, "ANTHROPIC_DEFAULT_OPUS_MODEL")
            .or(default_model)
            .or(small_fast_model);
        // Fable 未配置时不写稳定别名；映射侧会 fable→opus 降级（与官方一致）。
        let fable_model = Self::claude_env_string(env, "ANTHROPIC_DEFAULT_FABLE_MODEL");

        let subagent_model = Self::claude_env_string(env, "CLAUDE_CODE_SUBAGENT_MODEL");

        let mut fields = Vec::with_capacity(9);
        Self::push_claude_takeover_role_fields(
            &mut fields,
            env,
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL_NAME",
            CLAUDE_TAKEOVER_HAIKU_MODEL,
            false,
            haiku_model,
        );
        Self::push_claude_takeover_role_fields(
            &mut fields,
            env,
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL_NAME",
            CLAUDE_TAKEOVER_SONNET_MODEL,
            true,
            sonnet_model,
        );
        Self::push_claude_takeover_role_fields(
            &mut fields,
            env,
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL_NAME",
            CLAUDE_TAKEOVER_OPUS_MODEL,
            true,
            opus_model,
        );
        Self::push_claude_takeover_role_fields(
            &mut fields,
            env,
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_DEFAULT_FABLE_MODEL_NAME",
            CLAUDE_TAKEOVER_FABLE_MODEL,
            true,
            fable_model,
        );
        if let Some(subagent_model) = subagent_model {
            fields.push(("CLAUDE_CODE_SUBAGENT_MODEL", subagent_model.to_string()));
        }
        fields
    }

    fn push_claude_takeover_role_fields(
        fields: &mut Vec<(&'static str, String)>,
        env: &Map<String, Value>,
        model_key: &'static str,
        name_key: &'static str,
        takeover_model: &'static str,
        supports_one_m: bool,
        upstream_model: Option<&str>,
    ) {
        let Some(upstream_model) = upstream_model else {
            return;
        };

        let mut client_model = takeover_model.to_string();
        if supports_one_m && Self::has_claude_one_m_marker(upstream_model) {
            client_model.push_str(CLAUDE_ONE_M_MARKER_FOR_CLIENT);
        }
        fields.push((model_key, client_model));

        let display_name = Self::claude_env_string(env, name_key)
            .map(str::to_string)
            .unwrap_or_else(|| Self::strip_claude_one_m_marker(upstream_model));
        if !display_name.is_empty() {
            fields.push((name_key, display_name));
        }
    }

    fn claude_env_string<'a>(env: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
        env.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    fn has_claude_one_m_marker(model: &str) -> bool {
        model
            .trim_end()
            .to_ascii_lowercase()
            .ends_with(crate::claude_desktop_config::ONE_M_CONTEXT_MARKER)
    }

    fn strip_claude_one_m_marker(model: &str) -> String {
        crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model)
            .trim()
            .to_string()
    }

    fn claude_provider_with_effective_settings(
        &self,
        provider: &Provider,
    ) -> Result<Provider, String> {
        let mut effective_provider = provider.clone();
        effective_provider.settings_config = build_effective_settings_with_common_config(
            self.db.as_ref(),
            &AppType::Claude,
            provider,
        )
        .map_err(|e| format!("构建 claude 有效配置失败: {e}"))?;
        Ok(effective_provider)
    }

    pub async fn sync_claude_live_from_provider_while_proxy_active(
        &self,
        provider: &Provider,
    ) -> Result<(), String> {
        let effective_provider = self.claude_provider_with_effective_settings(provider)?;
        let mut effective_settings = effective_provider.settings_config.clone();
        let (proxy_url, _) = self.build_proxy_urls().await?;

        Self::apply_claude_takeover_fields_for_provider(
            &mut effective_settings,
            &proxy_url,
            &effective_provider,
        );
        self.write_claude_live(&effective_settings)?;
        Ok(())
    }

    pub async fn sync_codex_live_from_provider_while_proxy_active(
        &self,
        provider: &Provider,
    ) -> Result<(), String> {
        self.sync_codex_live_from_provider_while_proxy_active_guarded(provider, None, None)
            .await
    }

    pub(crate) async fn sync_codex_live_from_provider_while_proxy_active_guarded(
        &self,
        provider: &Provider,
        outgoing_managed_account_id: Option<&str>,
        outgoing_guard: Option<&CodexLiveAuthSwitchGuard>,
    ) -> Result<(), String> {
        let existing_live = self.read_codex_live().ok();
        let mut effective_settings = build_effective_provider_for_live_with_codex_oauth_manager(
            self.db.as_ref(),
            &AppType::Codex,
            provider,
            &self.codex_oauth_manager,
        )
        .map_err(|e| format!("构建 codex 有效配置失败: {e}"))?
        .settings_config;
        if let Some(existing_live) = existing_live.as_ref() {
            Self::preserve_toml_mcp_servers_from_existing_config(
                &mut effective_settings,
                existing_live,
            )?;
        }
        let (_, proxy_codex_base_url) = self.build_proxy_urls().await?;

        Self::apply_codex_takeover_fields_for_provider(
            &mut effective_settings,
            &proxy_codex_base_url,
            provider,
        )?;

        if let (Some(account_id), Some(guard)) = (outgoing_managed_account_id, outgoing_guard) {
            guard
                .ensure_unchanged(account_id)
                .map_err(|error| error.to_string())?;
        }

        self.write_codex_takeover_live_for_provider(&effective_settings, Some(provider))?;
        Ok(())
    }

    pub async fn sync_grok_live_from_provider_while_proxy_active(
        &self,
        provider: &Provider,
    ) -> Result<(), String> {
        let existing_live = self.read_grok_live().ok();
        let mut effective_settings = build_effective_settings_with_common_config(
            self.db.as_ref(),
            &AppType::GrokBuild,
            provider,
        )
        .map_err(|e| format!("构建 Grok Build 有效配置失败: {e}"))?;
        if let Some(existing_live) = existing_live.as_ref() {
            Self::preserve_toml_mcp_servers_from_existing_config(
                &mut effective_settings,
                existing_live,
            )?;
        }
        let (proxy_url, _) = self.build_proxy_urls().await?;
        let proxy_grok_base_url = format!("{}/grokbuild/v1", proxy_url.trim_end_matches('/'));
        Self::apply_grok_takeover_fields(&mut effective_settings, &proxy_grok_base_url)?;
        self.write_grok_live(&effective_settings)
    }

    fn get_current_provider_for_app(&self, app_type: &AppType) -> Result<Option<Provider>, String> {
        let Some(current_id) = crate::settings::get_effective_current_provider(&self.db, app_type)
            .map_err(|e| format!("获取 {app_type:?} 当前供应商失败: {e}"))?
        else {
            return Ok(None);
        };

        self.db
            .get_provider_by_id(&current_id, app_type.as_str())
            .map_err(|e| format!("读取 {app_type:?} 当前供应商失败: {e}"))
    }

    fn should_preserve_current_codex_auth(&self) -> Result<bool, String> {
        // Unknown current state is handled conservatively: preserving the live
        // auth file cannot roll a refresh generation back, while restoring an
        // unclassified legacy backup can. A concrete non-official provider is
        // the only case where its stored auth should replace the live file.
        Ok(self
            .get_current_provider_for_app(&AppType::Codex)?
            .as_ref()
            .is_none_or(crate::proxy::providers::is_codex_official_provider))
    }

    /// Official Codex auth is a live, independently rotating login. A takeover
    /// backup may restore config/catalog, but must never freeze refresh tokens
    /// that Codex CLI can advance while the proxy is active.
    fn strip_current_official_codex_auth_from_backup(
        &self,
        config: &mut Value,
    ) -> Result<(), String> {
        if self.should_preserve_current_codex_auth()? {
            if let Some(root) = config.as_object_mut() {
                root.remove("auth");
            }
        }
        Ok(())
    }

    async fn rollback_failed_takeover_activation(
        &self,
        app_type: &AppType,
        codex_snapshot: Option<&crate::codex_config::CodexLiveStateSnapshot>,
    ) -> Result<(), String> {
        if let Some(snapshot) = codex_snapshot {
            return snapshot
                .restore_preserving_newer_same_account_auth()
                .map_err(|error| error.to_string());
        }
        self.restore_live_config_for_app_inner(app_type).await
    }

    async fn refresh_active_target_from_current_provider(&self, app_type: &AppType) {
        let Ok(Some(provider)) = self.get_current_provider_for_app(app_type) else {
            return;
        };
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .set_active_target(app_type.as_str(), &provider.id, &provider.name)
                .await;
        }
    }

    async fn rollback_hot_switch_preparation(
        &self,
        app_type: &AppType,
        previous_backup: Option<&LiveBackup>,
        previous_provider_id: Option<&str>,
        should_sync_backup: bool,
        live_taken_over: bool,
        previous_codex_live_state: Option<&crate::codex_config::CodexLiveStateSnapshot>,
    ) {
        if !should_sync_backup {
            return;
        }

        let rollback_result = match previous_backup {
            Some(backup) => {
                self.db
                    .save_live_backup(app_type.as_str(), &backup.original_config)
                    .await
            }
            None => self.db.delete_live_backup(app_type.as_str()).await,
        };
        if let Err(error) = rollback_result {
            log::error!("{} 热切换失败后恢复原备份失败: {error}", app_type.as_str());
        }

        if let Some(previous_live) = previous_codex_live_state {
            if let Err(error) = previous_live.restore_preserving_newer_same_account_auth() {
                log::error!(
                    "{} 热切换失败后恢复 Codex Live 状态失败: {error}",
                    app_type.as_str()
                );
            }
            return;
        }

        let Some(previous_provider_id) = previous_provider_id else {
            return;
        };
        let Ok(Some(previous_provider)) = self
            .db
            .get_provider_by_id(previous_provider_id, app_type.as_str())
        else {
            return;
        };

        let live_result = if matches!(app_type, AppType::Claude) {
            self.sync_claude_live_from_provider_while_proxy_active(&previous_provider)
                .await
        } else if live_taken_over && matches!(app_type, AppType::Codex) {
            self.sync_codex_live_from_provider_while_proxy_active(&previous_provider)
                .await
        } else if live_taken_over && matches!(app_type, AppType::GrokBuild) {
            self.sync_grok_live_from_provider_while_proxy_active(&previous_provider)
                .await
        } else {
            Ok(())
        };
        if let Err(error) = live_result {
            log::error!(
                "{} 热切换失败后恢复原 Live 配置失败: {error}",
                app_type.as_str()
            );
        }
    }

    fn require_current_provider_for_app(&self, app_type: &AppType) -> Result<Provider, String> {
        self.get_current_provider_for_app(app_type)?
            .ok_or_else(|| format!("{app_type:?} 当前供应商不存在，无法接管 Live 配置"))
    }

    /// 设置 AppHandle（在应用初始化时调用）
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        futures::executor::block_on(async {
            *self.app_handle.write().await = Some(handle);
        });
    }

    pub(crate) async fn lock_switch_for_app(
        &self,
        app_type: &str,
    ) -> tokio::sync::OwnedMutexGuard<()> {
        self.switch_locks.lock_for_app(app_type).await
    }

    /// 该应用是否正有切换 / 接管操作在进行中。见 `SwitchLockManager::is_locked_for_app`。
    pub(crate) async fn is_switch_in_progress_for_app(&self, app_type: &str) -> bool {
        self.switch_locks.is_locked_for_app(app_type).await
    }

    /// 启动代理服务器
    pub async fn start(&self) -> Result<ProxyServerInfo, String> {
        // 1. 启动时自动设置 proxy_enabled = true
        let mut global_config = self
            .db
            .get_global_proxy_config()
            .await
            .map_err(|e| format!("获取全局代理配置失败: {e}"))?;

        if !global_config.proxy_enabled {
            global_config.proxy_enabled = true;
            self.db
                .update_global_proxy_config(global_config.clone())
                .await
                .map_err(|e| format!("更新代理总开关失败: {e}"))?;
        }

        // 2. 获取配置
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 3. 若已在运行：确保持久化状态（如需要）并返回当前信息
        if let Some(server) = self.server.read().await.as_ref() {
            let status = server.get_status().await;
            return Ok(ProxyServerInfo {
                address: status.address,
                port: status.port,
                // 无法精确取回首次启动时间，返回当前时间用于 UI 展示即可
                started_at: chrono::Utc::now().to_rfc3339(),
            });
        }

        // 4. 创建并启动服务器
        let app_handle = self.app_handle.read().await.clone();
        let server = ProxyServer::new(config.clone(), self.db.clone(), app_handle);
        let info = server
            .start()
            .await
            .map_err(|e| format!("启动代理服务器失败: {e}"))?;
        if let Err(e) = self
            .persist_ephemeral_listen_port_if_needed(&config, info.port)
            .await
        {
            let _ = server.stop().await;
            return Err(e);
        }

        // 5. 保存服务器实例
        *self.server.write().await = Some(server);

        log::info!("代理服务器已启动: {}:{}", info.address, info.port);
        Ok(info)
    }

    async fn persist_ephemeral_listen_port_if_needed(
        &self,
        config: &ProxyConfig,
        actual_port: u16,
    ) -> Result<(), String> {
        if config.listen_port != 0 {
            return Ok(());
        }

        // 端口是全局字段，不能通过旧接口回写各应用独立的重试和超时配置。
        let mut resolved_config = self
            .db
            .get_global_proxy_config()
            .await
            .map_err(|e| format!("获取全局代理配置失败: {e}"))?;
        resolved_config.listen_port = actual_port;
        self.db
            .update_global_proxy_config(resolved_config)
            .await
            .map_err(|e| format!("保存动态代理端口失败: {e}"))
    }

    async fn start_before_takeover_if_ephemeral_port(&self) -> Result<bool, String> {
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;
        if config.listen_port != 0 || self.is_running().await {
            return Ok(false);
        }

        self.start().await?;
        Ok(true)
    }

    /// 启动代理服务器（带 Live 配置接管）
    pub async fn start_with_takeover(&self) -> Result<ProxyServerInfo, String> {
        // 1. 备份各应用的 Live 配置
        self.backup_live_configs().await?;

        // 2. 同步 Live 配置中的 Token 到数据库（确保代理能读到最新的 Token）
        if let Err(e) = self.sync_live_to_providers().await {
            // 同步失败时尚未写入接管配置，但备份可能包含敏感信息，尽量清理
            if let Err(clean_err) = self.db.delete_all_live_backups().await {
                log::warn!("清理 Live 备份失败: {clean_err}");
            }
            return Err(e);
        }

        // 端口 0 需要先启动代理拿到 OS 分配的真实端口，否则接管 Live 配置会写出 :0。
        let started_proxy_before_takeover =
            match self.start_before_takeover_if_ephemeral_port().await {
                Ok(started) => started,
                Err(e) => {
                    if let Err(clean_err) = self.db.delete_all_live_backups().await {
                        log::warn!("清理 Live 备份失败: {clean_err}");
                    }
                    return Err(e);
                }
            };

        // 3. 在写入接管配置之前先落盘接管标志：
        //    这样即使在接管过程中断电/kill，下次启动也能检测到并自动恢复。
        if let Err(e) = self.db.set_live_takeover_active(true).await {
            if let Err(clean_err) = self.db.delete_all_live_backups().await {
                log::warn!("清理 Live 备份失败: {clean_err}");
            }
            if started_proxy_before_takeover {
                let _ = self.stop().await;
            }
            return Err(format!("设置接管状态失败: {e}"));
        }

        // 4. 接管各应用的 Live 配置（写入代理地址，清空 Token）
        if let Err(e) = self.takeover_live_configs().await {
            // 接管失败（可能是部分写入），尝试恢复原始配置；若恢复失败则保留标志与备份，等待下次启动自动恢复。
            log::error!("接管 Live 配置失败，尝试恢复原始配置: {e}");
            match self.restore_live_configs().await {
                Ok(()) => {
                    let _ = self.db.set_live_takeover_active(false).await;
                    let _ = self.db.delete_all_live_backups().await;
                }
                Err(restore_err) => {
                    log::error!("恢复原始配置失败，将保留备份以便下次启动恢复: {restore_err}");
                }
            }
            if started_proxy_before_takeover {
                let _ = self.stop().await;
            }
            return Err(e);
        }

        // 5. 启动代理服务器
        match self.start().await {
            Ok(info) => Ok(info),
            Err(e) => {
                // 启动失败，恢复原始配置
                log::error!("代理启动失败，尝试恢复原始配置: {e}");
                match self.restore_live_configs().await {
                    Ok(()) => {
                        let _ = self.db.set_live_takeover_active(false).await;
                        let _ = self.db.delete_all_live_backups().await;
                    }
                    Err(restore_err) => {
                        log::error!("恢复原始配置失败，将保留备份以便下次启动恢复: {restore_err}");
                    }
                }
                if started_proxy_before_takeover {
                    let _ = self.stop().await;
                }
                Err(e)
            }
        }
    }

    /// 获取各应用的接管状态（是否改写该应用的 Live 配置指向本地代理）
    pub async fn get_takeover_status(&self) -> Result<ProxyTakeoverStatus, String> {
        // 从 proxy_config.enabled 读取（优先），兼容旧的 live_backup 备份检测
        let claude_enabled = self
            .db
            .get_proxy_config_for_app("claude")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        let codex_enabled = self
            .db
            .get_proxy_config_for_app("codex")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        let gemini_enabled = self
            .db
            .get_proxy_config_for_app("gemini")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        let grokbuild_enabled = self
            .db
            .get_proxy_config_for_app("grokbuild")
            .await
            .map(|c| c.enabled)
            .unwrap_or(false);
        // OpenCode and OpenClaw don't support proxy features, always return false
        let opencode_enabled = false;
        let openclaw_enabled = false;

        Ok(ProxyTakeoverStatus {
            claude: claude_enabled,
            codex: codex_enabled,
            gemini: gemini_enabled,
            grokbuild: grokbuild_enabled,
            opencode: opencode_enabled,
            openclaw: openclaw_enabled,
        })
    }

    /// 为指定应用开启/关闭 Live 接管
    ///
    /// - 开启：自动启动代理服务，仅接管当前 app 的 Live 配置
    /// - 关闭：仅恢复当前 app 的 Live 配置；若无其它接管，则自动停止代理服务
    pub async fn set_takeover_for_app(&self, app_type: &str, enabled: bool) -> Result<(), String> {
        let app = AppType::from_str(app_type).map_err(|e| format!("无效的应用类型: {e}"))?;
        if !app.supports_local_proxy() {
            return Err(format!("{} 不支持本地路由", app.as_str()));
        }
        let app_type_str = app.as_str();
        let _guard = self.switch_locks.lock_for_app(app_type_str).await;

        if enabled {
            // 1) 代理服务未运行则自动启动
            if !self.is_running().await {
                self.start().await?;
            }

            // 2) 已接管则直接返回（幂等）；但如果缺少备份或占位符残留，需要重建接管
            let current_config = self
                .db
                .get_proxy_config_for_app(app_type_str)
                .await
                .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;

            let mut restore_existing_backup_before_takeover = false;
            if current_config.enabled {
                let has_backup = match self.db.get_live_backup(app_type_str).await {
                    Ok(v) => v.is_some(),
                    Err(e) => {
                        log::warn!("读取 {app_type_str} 备份失败（将继续重建接管）: {e}");
                        false
                    }
                };
                let live_matches_current_proxy =
                    match self.live_takeover_matches_current_proxy(&app).await {
                        Ok(value) => value,
                        Err(e) => {
                            log::warn!("检测 {app_type_str} 接管配置失败（将继续重建接管）: {e}");
                            false
                        }
                    };

                // 必须 backup 存在，且 live 确实指向当前代理地址，才算真接管。
                // 只看占位符会把半接管/旧端口残留误判为可复用，导致开启接管后
                // live 文件仍停留在普通供应商配置。
                if has_backup && live_matches_current_proxy {
                    if matches!(app, AppType::Codex) {
                        if let Some(provider_id) =
                            crate::settings::get_effective_current_provider(&self.db, &app)
                                .map_err(|error| error.to_string())?
                        {
                            if let Some(account_id) = self
                                .db
                                .get_provider_by_id(&provider_id, app_type_str)
                                .map_err(|error| error.to_string())?
                                .filter(crate::proxy::providers::is_codex_official_provider)
                                .and_then(|provider| provider.meta)
                                .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
                                .filter(|id| !id.trim().is_empty())
                            {
                                self.codex_oauth_manager
                                    .ensure_account_exists(account_id.trim())
                                    .await
                                    .map_err(|error| error.to_string())?;
                            }
                        }
                    }
                    self.refresh_active_target_from_current_provider(&app).await;
                    return Ok(());
                }
                restore_existing_backup_before_takeover = has_backup;

                log::warn!(
                    "{app_type_str} 标记为已接管，但 backup={has_backup} live_matches_current_proxy={live_matches_current_proxy}，正在重新接管并补齐 Live"
                );
            }

            // 3) 备份 Live 配置（严格：目标 app 不存在则报错）
            if restore_existing_backup_before_takeover {
                self.restore_live_config_for_app_inner(&app).await?;
            } else {
                self.backup_live_config_strict(&app).await?;

                // 4) 同步 Live Token 到数据库（仅当前 app）
                if let Err(e) = self.sync_live_to_provider(&app).await {
                    let _ = self.db.delete_live_backup(app_type_str).await;
                    return Err(e);
                }
            }

            // The persistent Official backup intentionally excludes auth.json
            // because its refresh token keeps rotating during takeover. Keep an
            // exact in-memory pre-write snapshot for activation failures so a
            // partial managed write can still restore the user's prior login.
            let codex_live_before_takeover = if matches!(&app, AppType::Codex) {
                Some(
                    crate::codex_config::CodexLiveStateSnapshot::capture()
                        .map_err(|error| format!("捕获 Codex 接管前状态失败: {error}"))?,
                )
            } else {
                None
            };

            // 5) 写入接管配置（仅当前 app）
            if let Err(e) = self.takeover_live_config_strict(&app).await {
                log::error!("{app_type_str} 接管 Live 配置失败，尝试恢复: {e}");
                match self
                    .rollback_failed_takeover_activation(&app, codex_live_before_takeover.as_ref())
                    .await
                {
                    Ok(()) => {
                        // 恢复成功才清理备份，避免失败场景下丢失唯一可回滚来源
                        let _ = self.db.delete_live_backup(app_type_str).await;
                    }
                    Err(restore_err) => {
                        log::error!(
                            "{app_type_str} 恢复 Live 配置失败，将保留备份以便下次启动恢复: {restore_err}"
                        );
                    }
                }
                return Err(e);
            }

            // 6) 设置 proxy_config.enabled = true
            let enable_result = async {
                let mut updated_config = self
                    .db
                    .get_proxy_config_for_app(app_type_str)
                    .await
                    .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;
                updated_config.enabled = true;
                self.db
                    .update_proxy_config_for_app(updated_config)
                    .await
                    .map_err(|e| format!("设置 {app_type_str} enabled 状态失败: {e}"))
            }
            .await;
            if let Err(error) = enable_result {
                log::error!("{app_type_str} 提交接管状态失败，尝试恢复 Live: {error}");
                match self
                    .rollback_failed_takeover_activation(&app, codex_live_before_takeover.as_ref())
                    .await
                {
                    Ok(()) => {
                        let _ = self.db.delete_live_backup(app_type_str).await;
                    }
                    Err(restore_error) => {
                        log::error!(
                            "{app_type_str} 恢复 Live 配置失败，将保留备份以便下次恢复: {restore_error}"
                        );
                    }
                }
                return Err(error);
            }

            // 7) 兼容旧逻辑：写入 any-of 标志（失败不影响功能）
            let _ = self.db.set_live_takeover_active(true).await;

            self.refresh_active_target_from_current_provider(&app).await;

            // 8) Warn if the current provider is official (risk of account ban via proxy)
            if let Ok(Some(current_id)) =
                crate::settings::get_effective_current_provider(&self.db, &app)
            {
                if let Ok(Some(provider)) = self.db.get_provider_by_id(&current_id, app_type_str) {
                    if provider.category.as_deref() == Some("official")
                        && !crate::services::provider::official_provider_supports_proxy_takeover(
                            &app, &provider,
                        )
                    {
                        if let Some(handle) = self.app_handle.read().await.as_ref() {
                            let _ = handle.emit(
                                "proxy-official-warning",
                                serde_json::json!({
                                    "appType": app_type_str,
                                    "providerName": provider.name,
                                }),
                            );
                        }
                    }
                }
            }

            return Ok(());
        }

        // 关闭接管：检查 enabled 状态
        let current_config = self
            .db
            .get_proxy_config_for_app(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;

        if !current_config.enabled {
            return Ok(()); // 未接管，幂等返回
        }

        // 1) 恢复 Live 配置
        //
        // 必须走 with_fallback 版本：备份 → SSOT → 清理占位符 的三层兜底。
        // 简版 restore_live_config_for_app 在备份缺失时会静默 Ok(())，
        // 留下接管时写入的占位符（代理地址/PROXY_MANAGED token），客户端无法工作。
        self.restore_live_config_for_app_with_fallback_inner(&app)
            .await?;

        // 2) 删除该 app 的备份（避免长期存储敏感 Token）
        self.db
            .delete_live_backup(app_type_str)
            .await
            .map_err(|e| format!("删除 {app_type_str} Live 备份失败: {e}"))?;

        // 3) 设置 proxy_config.enabled = false
        let mut updated_config = self
            .db
            .get_proxy_config_for_app(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;
        updated_config.enabled = false;
        self.db
            .update_proxy_config_for_app(updated_config)
            .await
            .map_err(|e| format!("清除 {app_type_str} enabled 状态失败: {e}"))?;

        // 4) 清除该应用的健康状态（关闭代理时重置队列状态）
        self.db
            .clear_provider_health_for_app(app_type_str)
            .await
            .map_err(|e| format!("清除 {app_type_str} 健康状态失败: {e}"))?;

        // 5) 若无其它接管，更新旧标志，并停止代理服务
        // 检查是否还有其它 app 的 enabled = true
        let any_enabled = self
            .db
            .is_live_takeover_active()
            .await
            .map_err(|e| format!("检查接管状态失败: {e}"))?;

        if !any_enabled {
            let _ = self.db.set_live_takeover_active(false).await;

            if self.is_running().await {
                // 此时没有任何 app 处于接管状态，停止服务即可
                let _ = self.stop().await;
            }
        }

        Ok(())
    }

    /// 同步关闭指定应用的 Live 接管（恢复配置并清标志，不停止代理服务）。
    ///
    /// 用于 `ProfileService::apply` 等 sync 路径：调用者所在线程可能没有 Tokio
    /// runtime，无法执行 `set_takeover_for_app(false)` 里的停止服务/等待任务等
    /// Tokio IO。这里只恢复 Live 文件、删除备份、清除 DB 接管标志，让后续
    /// `ProviderService::switch` 能正常写入官方供应商配置。
    ///
    /// 代理服务本身保持运行；当最后一个应用也关闭接管后，下次用户手动关闭
    /// 代理或程序退出时会自然停止。
    pub fn disable_takeover_for_app_sync(&self, app_type: &AppType) -> Result<(), String> {
        let app_type_str = app_type.as_str();

        // 1) 恢复原始 Live 配置（备份 → SSOT → 清理占位符 三层兜底）
        futures::executor::block_on(self.restore_live_config_for_app_with_fallback_inner(app_type))
            .map_err(|e| format!("恢复 {app_type_str} Live 配置失败: {e}"))?;

        // 2) 删除该 app 的备份
        futures::executor::block_on(self.db.delete_live_backup(app_type_str))
            .map_err(|e| format!("删除 {app_type_str} Live 备份失败: {e}"))?;

        // 3) 设置 proxy_config.enabled = false
        let mut config =
            futures::executor::block_on(self.db.get_proxy_config_for_app(app_type_str))
                .map_err(|e| format!("获取 {app_type_str} 配置失败: {e}"))?;
        if config.enabled {
            config.enabled = false;
            futures::executor::block_on(self.db.update_proxy_config_for_app(config))
                .map_err(|e| format!("清除 {app_type_str} enabled 状态失败: {e}"))?;
        }

        // 4) 清除该应用的健康状态
        futures::executor::block_on(self.db.clear_provider_health_for_app(app_type_str))
            .map_err(|e| format!("清除 {app_type_str} 健康状态失败: {e}"))?;

        // 5) 清旧标志
        let _ = futures::executor::block_on(self.db.set_live_takeover_active(false));

        Ok(())
    }

    /// 同步 Live 配置中的 Token 到数据库
    ///
    /// 在清空 Live Token 之前调用，确保数据库中的 Provider 配置有最新的 Token。
    /// 这样代理才能从数据库读取到正确的认证信息。
    async fn sync_live_to_provider(&self, app_type: &AppType) -> Result<(), String> {
        let live_config = match app_type {
            AppType::Claude => self.read_claude_live()?,
            AppType::Codex => self.read_codex_live()?,
            AppType::Gemini => self.read_gemini_live()?,
            AppType::GrokBuild => self.read_grok_live()?,
            _ => return Err("该应用不支持代理功能".to_string()),
        };

        self.sync_live_config_to_provider(app_type, &live_config)
            .await
    }

    async fn sync_live_config_to_provider(
        &self,
        app_type: &AppType,
        live_config: &Value,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::Claude)
                        .map_err(|e| format!("获取 Claude 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "claude")
                    {
                        if let Some(env) = live_config.get("env").and_then(|v| v.as_object()) {
                            let token_pair = [
                                "ANTHROPIC_AUTH_TOKEN",
                                "ANTHROPIC_API_KEY",
                                "OPENROUTER_API_KEY",
                                "OPENAI_API_KEY",
                            ]
                            .into_iter()
                            .find_map(|key| {
                                env.get(key)
                                    .and_then(|v| v.as_str())
                                    .map(|s| (key, s.trim()))
                            })
                            .filter(|(_, token)| {
                                !token.is_empty() && *token != PROXY_TOKEN_PLACEHOLDER
                            });

                            if let Some((token_key, token)) = token_pair {
                                let env_obj = provider
                                    .settings_config
                                    .get_mut("env")
                                    .and_then(|v| v.as_object_mut());

                                match env_obj {
                                    Some(obj) => {
                                        if token_key == "ANTHROPIC_AUTH_TOKEN"
                                            || token_key == "ANTHROPIC_API_KEY"
                                        {
                                            let mut updated = false;
                                            if obj.contains_key("ANTHROPIC_AUTH_TOKEN") {
                                                obj.insert(
                                                    "ANTHROPIC_AUTH_TOKEN".to_string(),
                                                    json!(token),
                                                );
                                                updated = true;
                                            }
                                            if obj.contains_key("ANTHROPIC_API_KEY") {
                                                obj.insert(
                                                    "ANTHROPIC_API_KEY".to_string(),
                                                    json!(token),
                                                );
                                                updated = true;
                                            }
                                            if !updated {
                                                obj.insert(token_key.to_string(), json!(token));
                                            }
                                        } else {
                                            obj.insert(token_key.to_string(), json!(token));
                                        }
                                    }
                                    None => {
                                        // 至少写入一份可用的 Token
                                        if provider.settings_config.is_null() {
                                            provider.settings_config = json!({});
                                        }

                                        if let Some(root) = provider.settings_config.as_object_mut()
                                        {
                                            root.insert(
                                                "env".to_string(),
                                                json!({ token_key: token }),
                                            );
                                        } else {
                                            log::warn!(
                                                "Claude provider settings_config 格式异常（非对象），跳过写入 Token (provider: {provider_id})"
                                            );
                                        }
                                    }
                                }

                                if let Err(e) = self.db.update_provider_settings_config(
                                    "claude",
                                    &provider_id,
                                    &provider.settings_config,
                                ) {
                                    log::warn!("同步 Claude Token 到数据库失败: {e}");
                                } else {
                                    log::info!(
                                        "已同步 Claude Token 到数据库 (provider: {provider_id})"
                                    );
                                }
                            }
                        }
                    }
                }
            }
            AppType::Codex => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::Codex)
                        .map_err(|e| format!("获取 Codex 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "codex")
                    {
                        // Official rows are routing/account selectors, not
                        // credential stores. Their auth must remain empty even
                        // when the live Codex login uses OPENAI_API_KEY mode.
                        if crate::proxy::providers::is_codex_official_provider(&provider) {
                            return Ok(());
                        }
                        if let Some(token) = live_config
                            .get("auth")
                            .and_then(|v| v.get("OPENAI_API_KEY"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty() && *s != PROXY_TOKEN_PLACEHOLDER)
                        {
                            if let Some(auth_obj) = provider
                                .settings_config
                                .get_mut("auth")
                                .and_then(|v| v.as_object_mut())
                            {
                                auth_obj.insert("OPENAI_API_KEY".to_string(), json!(token));
                            } else {
                                if provider.settings_config.is_null() {
                                    provider.settings_config = json!({});
                                }

                                if let Some(root) = provider.settings_config.as_object_mut() {
                                    root.insert(
                                        "auth".to_string(),
                                        json!({ "OPENAI_API_KEY": token }),
                                    );
                                } else {
                                    log::warn!(
                                        "Codex provider settings_config 格式异常（非对象），跳过写入 Token (provider: {provider_id})"
                                    );
                                }
                            }

                            if let Err(e) = self.db.update_provider_settings_config(
                                "codex",
                                &provider_id,
                                &provider.settings_config,
                            ) {
                                log::warn!("同步 Codex Token 到数据库失败: {e}");
                            } else {
                                log::info!("已同步 Codex Token 到数据库 (provider: {provider_id})");
                            }
                        }
                    }
                }
            }
            AppType::Gemini => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::Gemini)
                        .map_err(|e| format!("获取 Gemini 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "gemini")
                    {
                        if let Some(token) = live_config
                            .get("env")
                            .and_then(|v| v.get("GEMINI_API_KEY"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty() && *s != PROXY_TOKEN_PLACEHOLDER)
                        {
                            if let Some(env_obj) = provider
                                .settings_config
                                .get_mut("env")
                                .and_then(|v| v.as_object_mut())
                            {
                                env_obj.insert("GEMINI_API_KEY".to_string(), json!(token));
                            } else {
                                if provider.settings_config.is_null() {
                                    provider.settings_config = json!({});
                                }

                                if let Some(root) = provider.settings_config.as_object_mut() {
                                    root.insert(
                                        "env".to_string(),
                                        json!({ "GEMINI_API_KEY": token }),
                                    );
                                } else {
                                    log::warn!(
                                        "Gemini provider settings_config 格式异常（非对象），跳过写入 Token (provider: {provider_id})"
                                    );
                                }
                            }

                            if let Err(e) = self.db.update_provider_settings_config(
                                "gemini",
                                &provider_id,
                                &provider.settings_config,
                            ) {
                                log::warn!("同步 Gemini Token 到数据库失败: {e}");
                            } else {
                                log::info!(
                                    "已同步 Gemini Token 到数据库 (provider: {provider_id})"
                                );
                            }
                        }
                    }
                }
            }
            AppType::GrokBuild => {
                let provider_id =
                    crate::settings::get_effective_current_provider(&self.db, &AppType::GrokBuild)
                        .map_err(|e| format!("获取 Grok Build 当前供应商失败: {e}"))?;

                if let Some(provider_id) = provider_id {
                    if let Ok(Some(mut provider)) =
                        self.db.get_provider_by_id(&provider_id, "grokbuild")
                    {
                        let live_config_toml = live_config
                            .get("config")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        if let Some(token) =
                            crate::grok_config::extract_inline_api_key(live_config_toml)
                        {
                            if !token.is_empty() && token != PROXY_TOKEN_PLACEHOLDER {
                                if let Some(provider_config) = provider
                                    .settings_config
                                    .get("config")
                                    .and_then(Value::as_str)
                                {
                                    let updated =
                                        crate::grok_config::update_api_key(provider_config, &token)
                                            .map_err(|e| {
                                                format!("更新 Grok Build API Key 失败: {e}")
                                            })?;
                                    provider.settings_config["config"] = json!(updated);
                                    self.db
                                        .update_provider_settings_config(
                                            "grokbuild",
                                            &provider_id,
                                            &provider.settings_config,
                                        )
                                        .map_err(|e| {
                                            format!("同步 Grok Build Token 到数据库失败: {e}")
                                        })?;
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    async fn sync_live_to_providers(&self) -> Result<(), String> {
        if let Ok(live_config) = self.read_claude_live() {
            self.sync_live_config_to_provider(&AppType::Claude, &live_config)
                .await?;
        }

        if let Ok(live_config) = self.read_codex_live() {
            self.sync_live_config_to_provider(&AppType::Codex, &live_config)
                .await?;
        }

        if let Ok(live_config) = self.read_gemini_live() {
            self.sync_live_config_to_provider(&AppType::Gemini, &live_config)
                .await?;
        }

        if let Ok(live_config) = self.read_grok_live() {
            self.sync_live_config_to_provider(&AppType::GrokBuild, &live_config)
                .await?;
        }

        log::info!("Live 配置 Token 同步完成");
        Ok(())
    }

    /// 停止代理服务器
    pub async fn stop(&self) -> Result<(), String> {
        if let Some(server) = self.server.write().await.take() {
            server
                .stop()
                .await
                .map_err(|e| format!("停止代理服务器失败: {e}"))?;

            // 停止时设置 proxy_enabled = false
            let mut global_config = self
                .db
                .get_global_proxy_config()
                .await
                .map_err(|e| format!("获取全局代理配置失败: {e}"))?;

            if global_config.proxy_enabled {
                global_config.proxy_enabled = false;
                if let Err(e) = self.db.update_global_proxy_config(global_config).await {
                    log::warn!("更新代理总开关失败: {e}");
                }
            }

            log::info!("代理服务器已停止");
            Ok(())
        } else {
            Err("代理服务器未运行".to_string())
        }
    }

    /// 停止代理服务器（恢复 Live 配置，用户手动关闭时使用）
    ///
    /// 会清除 settings 表中的代理状态，下次启动不会自动恢复。
    pub async fn stop_with_restore(&self) -> Result<(), String> {
        // 1. 停止代理服务器（即使未运行也继续执行恢复逻辑）
        if let Err(e) = self.stop().await {
            log::warn!("停止代理服务器失败（将继续恢复 Live 配置）: {e}");
        }

        // 2. 恢复原始 Live 配置
        self.restore_live_configs().await?;

        // 3. 清除 proxy_config 表中的接管状态（兼容旧版）
        self.db
            .set_live_takeover_active(false)
            .await
            .map_err(|e| format!("清除接管状态失败: {e}"))?;

        // 4. 清除所有应用的 enabled 状态（用户手动关闭，不需要下次自动恢复）
        for app_type in ["claude", "codex", "gemini", "grokbuild"] {
            if let Ok(mut config) = self.db.get_proxy_config_for_app(app_type).await {
                if config.enabled {
                    config.enabled = false;
                    if let Err(e) = self.db.update_proxy_config_for_app(config).await {
                        log::warn!("清除 {app_type} enabled 状态失败: {e}");
                    }
                }
            }
        }

        // 5. 删除备份
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        // 6. 重置健康状态（让健康徽章恢复为正常）
        self.db
            .clear_all_provider_health()
            .await
            .map_err(|e| format!("重置健康状态失败: {e}"))?;

        // 注意：不清除故障转移队列和开关状态，保留供下次开启代理时使用
        log::info!("代理已停止，Live 配置已恢复");
        Ok(())
    }

    /// 停止代理服务器（恢复 Live 配置，但保留 settings 表中的代理状态）
    ///
    /// 用于程序正常退出时，保留代理状态以便下次启动时自动恢复
    pub async fn stop_with_restore_keep_state(&self) -> Result<(), String> {
        // 1. 停止代理服务器（即使未运行也继续执行恢复逻辑）
        if let Err(e) = self.stop().await {
            log::warn!("停止代理服务器失败（将继续恢复 Live 配置）: {e}");
        }

        // 2. 恢复原始 Live 配置
        self.restore_live_configs().await?;

        // 保留各应用的 enabled 和故障转移配置，下次启动时自动恢复。
        // live_takeover_active 已废弃，无需通过旧接口回写配置。

        // 3. 删除备份（Live 配置已恢复，备份不再需要）
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        // 4. 重置健康状态
        self.db
            .clear_all_provider_health()
            .await
            .map_err(|e| format!("重置健康状态失败: {e}"))?;

        log::info!("代理已停止，Live 配置已恢复（保留代理状态，下次启动将自动恢复）");
        Ok(())
    }

    /// 备份各应用的 Live 配置
    async fn backup_live_configs(&self) -> Result<(), String> {
        // Claude
        if let Ok(config) = self.read_claude_live() {
            // 跳过已被代理接管的 Live：避免把代理占位符当作"原始 Live"存进备份槽。
            // 否则下次 start_with_takeover 在异常历史状态下（Live 已是占位符）再次
            // 调用本函数，会用代理配置覆盖一个原本正常的备份；之后 stop 恢复时
            // 即便走到备份路径也会把代理占位符再写回 Live，永久卡在 127.0.0.1:15721。
            if Self::live_has_proxy_placeholder_for_app(&AppType::Claude, &config) {
                log::warn!("claude Live 已被代理接管，不备份（避免把代理配置固化进备份槽）；下次 stop 会从 SSOT 重建 Live");
            } else {
                let json_str = serde_json::to_string(&config)
                    .map_err(|e| format!("序列化 Claude 配置失败: {e}"))?;
                self.db
                    .save_live_backup("claude", &json_str)
                    .await
                    .map_err(|e| format!("备份 Claude 配置失败: {e}"))?;
            }
        }

        // Codex
        if let Ok(mut config) = self.read_codex_live() {
            if Self::live_has_proxy_placeholder_for_app(&AppType::Codex, &config) {
                log::warn!("codex Live 已被代理接管，不备份（避免把代理配置固化进备份槽）；下次 stop 会从 SSOT 重建 Live");
            } else {
                self.strip_current_official_codex_auth_from_backup(&mut config)?;
                let json_str = serde_json::to_string(&config)
                    .map_err(|e| format!("序列化 Codex 配置失败: {e}"))?;
                self.db
                    .save_live_backup("codex", &json_str)
                    .await
                    .map_err(|e| format!("备份 Codex 配置失败: {e}"))?;
            }
        }

        // Gemini
        if let Ok(config) = self.read_gemini_live() {
            if Self::live_has_proxy_placeholder_for_app(&AppType::Gemini, &config) {
                log::warn!("gemini Live 已被代理接管，不备份（避免把代理配置固化进备份槽）；下次 stop 会从 SSOT 重建 Live");
            } else {
                let json_str = serde_json::to_string(&config)
                    .map_err(|e| format!("序列化 Gemini 配置失败: {e}"))?;
                self.db
                    .save_live_backup("gemini", &json_str)
                    .await
                    .map_err(|e| format!("备份 Gemini 配置失败: {e}"))?;
            }
        }

        // Grok Build
        if let Ok(config) = self.read_grok_live() {
            if Self::live_has_proxy_placeholder_for_app(&AppType::GrokBuild, &config) {
                log::warn!("grokbuild Live 已被代理接管，不备份；下次 stop 会从 SSOT 重建 Live");
            } else {
                let json_str = serde_json::to_string(&config)
                    .map_err(|e| format!("序列化 Grok Build 配置失败: {e}"))?;
                self.db
                    .save_live_backup("grokbuild", &json_str)
                    .await
                    .map_err(|e| format!("备份 Grok Build 配置失败: {e}"))?;
            }
        }

        log::info!("已备份所有应用的 Live 配置");
        Ok(())
    }

    /// 备份指定应用的 Live 配置（严格模式：目标配置不存在则返回错误）
    async fn backup_live_config_strict(&self, app_type: &AppType) -> Result<(), String> {
        let (app_type_str, mut config) = match app_type {
            AppType::Claude => ("claude", self.read_claude_live()?),
            AppType::Codex => ("codex", self.read_codex_live()?),
            AppType::Gemini => ("gemini", self.read_gemini_live()?),
            AppType::GrokBuild => ("grokbuild", self.read_grok_live()?),
            _ => return Err("该应用不支持代理功能".to_string()),
        };

        // 跳过已被代理接管的 Live：避免把代理占位符当作"原始 Live"存进备份槽
        // （见 backup_live_configs 中的注释）。
        if Self::live_has_proxy_placeholder_for_app(app_type, &config) {
            log::warn!(
                "{app_type_str} Live 已被代理接管，不备份（避免把代理配置固化进备份槽）；下次 stop 会从 SSOT 重建 Live"
            );
            return Ok(());
        }

        if matches!(app_type, AppType::Codex) {
            self.strip_current_official_codex_auth_from_backup(&mut config)?;
        }

        let json_str = serde_json::to_string(&config)
            .map_err(|e| format!("序列化 {app_type_str} 配置失败: {e}"))?;
        self.db
            .save_live_backup(app_type_str, &json_str)
            .await
            .map_err(|e| format!("备份 {app_type_str} 配置失败: {e}"))?;

        Ok(())
    }

    /// 构造写入 Live 的代理地址（处理 0.0.0.0 / IPv6 等特殊情况）
    async fn build_proxy_urls(&self) -> Result<(String, String), String> {
        let config = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // listen_address 可能是 0.0.0.0（用于监听所有网卡），但客户端无法用 0.0.0.0 连接；
        // 因此写回到各应用配置时，优先使用本机回环地址。
        let connect_host = match config.listen_address.as_str() {
            "0.0.0.0" => "127.0.0.1".to_string(),
            "::" => "::1".to_string(),
            _ => config.listen_address.clone(),
        };
        let connect_host_for_url = if connect_host.contains(':') && !connect_host.starts_with('[') {
            format!("[{connect_host}]")
        } else {
            connect_host
        };

        let mut listen_port = config.listen_port;
        if let Some(server) = self.server.read().await.as_ref() {
            let status = server.get_status().await;
            if status.running {
                listen_port = status.port;
            }
        }
        if listen_port == 0 {
            return Err("代理监听端口为 0，但代理服务器尚未运行，无法生成接管地址".to_string());
        }

        let proxy_origin = format!("http://{}:{}", connect_host_for_url, listen_port);
        let proxy_url = proxy_origin.clone();
        let proxy_codex_base_url = format!("{}/v1", proxy_origin.trim_end_matches('/'));

        Ok((proxy_url, proxy_codex_base_url))
    }

    /// Grok Build live 是否具备可接管的自定义模型表。
    ///
    /// 官方态 live（Grok CLI 自带 OAuth 登录、无 `[model.*]` 表）没有注入
    /// 占位符的落点：Grok CLI 以「config 是否为空」区分官方 OAuth / 自定义
    /// 供应商两种模式，表达不出「官方 OAuth + 自定义 base_url」。官方供应商
    /// 的接管能力门见 `official_provider_supports_proxy_takeover`（按应用逐个
    /// 开，目前仅 Codex），调用方应跳过接管或直接报错。官方态的用量统计由
    /// `session_usage_grokbuild` 从会话日志导入，不依赖代理。
    fn grok_live_config_supports_takeover(config: &Value) -> bool {
        config
            .get("config")
            .and_then(Value::as_str)
            .and_then(crate::grok_config::extract_model_config)
            .is_some()
    }

    fn apply_grok_takeover_fields(config: &mut Value, proxy_base_url: &str) -> Result<(), String> {
        let config_toml = config
            .get("config")
            .and_then(Value::as_str)
            .ok_or_else(|| "Grok Build 配置缺少 config 字段".to_string())?;
        let updated = crate::grok_config::apply_proxy_takeover(
            config_toml,
            proxy_base_url,
            PROXY_TOKEN_PLACEHOLDER,
        )
        .map_err(|e| format!("更新 Grok Build 接管配置失败: {e}"))?;
        config["config"] = json!(updated);
        Ok(())
    }

    /// 接管各应用的 Live 配置（写入代理地址）
    ///
    /// 代理服务器的路由已经根据 API 端点自动区分应用类型：
    /// - `/v1/messages` → Claude
    /// - `/v1/chat/completions`, `/v1/responses` → Codex
    /// - `/v1beta/*` → Gemini
    ///
    /// 因此不需要在 URL 中添加应用前缀。
    async fn takeover_live_configs(&self) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
        let proxy_grok_base_url = format!("{}/grokbuild/v1", proxy_url.trim_end_matches('/'));

        // Claude: 修改 ANTHROPIC_BASE_URL，使用占位符替代真实 Token（代理会注入真实 Token）
        if let Ok(mut live_config) = self.read_claude_live() {
            let claude_provider = self.require_current_provider_for_app(&AppType::Claude)?;
            let claude_provider = self.claude_provider_with_effective_settings(&claude_provider)?;
            Self::apply_claude_takeover_fields_for_provider(
                &mut live_config,
                &proxy_url,
                &claude_provider,
            );
            self.write_claude_live(&live_config)?;
            log::info!("Claude Live 配置已接管，代理地址: {proxy_url}");
        }

        // Codex: project the selected provider through the local Responses endpoint.
        if self.read_codex_live().is_ok() {
            let codex_provider = self.require_current_provider_for_app(&AppType::Codex)?;
            self.sync_codex_live_from_provider_while_proxy_active(&codex_provider)
                .await?;
            log::info!("Codex Live 配置已接管，代理地址: {proxy_codex_base_url}");
        }

        // Gemini: 修改 GOOGLE_GEMINI_BASE_URL，使用占位符替代真实 Token（代理会注入真实 Token）
        if let Ok(mut live_config) = self.read_gemini_live() {
            if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                // 使用占位符，避免显示缺少 key 的警告
                env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
            } else {
                live_config["env"] = json!({
                    "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                    "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                });
            }
            self.write_gemini_live(&live_config)?;
            log::info!("Gemini Live 配置已接管，代理地址: {proxy_url}");
        }

        // Grok Build: keep its own provider namespace while reusing Responses forwarding.
        if let Ok(mut live_config) = self.read_grok_live() {
            if Self::grok_live_config_supports_takeover(&live_config) {
                Self::apply_grok_takeover_fields(&mut live_config, &proxy_grok_base_url)?;
                self.write_grok_live(&live_config)?;
                log::info!("Grok Build Live 配置已接管，代理地址: {proxy_grok_base_url}");
            } else {
                log::info!("Grok Build Live 处于官方登录态（无自定义模型表），跳过代理接管");
            }
        }

        Ok(())
    }

    /// 接管指定应用的 Live 配置（严格模式：目标配置不存在则返回错误）
    async fn takeover_live_config_strict(&self, app_type: &AppType) -> Result<(), String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
        let proxy_grok_base_url = format!("{}/grokbuild/v1", proxy_url.trim_end_matches('/'));

        match app_type {
            AppType::Claude => {
                let mut live_config = self.read_claude_live()?;
                let claude_provider = self.require_current_provider_for_app(&AppType::Claude)?;
                let claude_provider =
                    self.claude_provider_with_effective_settings(&claude_provider)?;
                Self::apply_claude_takeover_fields_for_provider(
                    &mut live_config,
                    &proxy_url,
                    &claude_provider,
                );
                self.write_claude_live(&live_config)?;
                log::info!("Claude Live 配置已接管，代理地址: {proxy_url}");
            }
            AppType::Codex => {
                self.read_codex_live()?;
                let codex_provider = self.require_current_provider_for_app(&AppType::Codex)?;
                self.sync_codex_live_from_provider_while_proxy_active(&codex_provider)
                    .await?;
                log::info!("Codex Live 配置已接管，代理地址: {proxy_codex_base_url}");
            }
            AppType::Gemini => {
                let mut live_config = self.read_gemini_live()?;

                if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                    env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                    env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                } else {
                    live_config["env"] = json!({
                        "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                        "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                    });
                }

                self.write_gemini_live(&live_config)?;
                log::info!("Gemini Live 配置已接管，代理地址: {proxy_url}");
            }
            AppType::GrokBuild => {
                let mut live_config = self.read_grok_live()?;
                if !Self::grok_live_config_supports_takeover(&live_config) {
                    return Err(
                        "Grok Build 当前为官方登录态（无自定义模型表），官方供应商不支持代理接管 \
                         (Grok Build is using the official login without a custom model table; \
                         official providers cannot be taken over by the proxy)"
                            .to_string(),
                    );
                }
                Self::apply_grok_takeover_fields(&mut live_config, &proxy_grok_base_url)?;
                self.write_grok_live(&live_config)?;
                log::info!("Grok Build Live 配置已接管，代理地址: {proxy_grok_base_url}");
            }
            _ => return Err("该应用不支持代理功能".to_string()),
        }

        Ok(())
    }

    /// 接管指定应用的 Live 配置（尽力而为：配置不存在/读取失败则跳过）
    async fn takeover_live_config_best_effort(&self, app_type: &AppType) -> Result<(), String> {
        let (proxy_url, _) = self.build_proxy_urls().await?;
        let proxy_grok_base_url = format!("{}/grokbuild/v1", proxy_url.trim_end_matches('/'));

        match app_type {
            AppType::Claude => {
                if let Ok(mut live_config) = self.read_claude_live() {
                    let claude_provider = self
                        .get_current_provider_for_app(&AppType::Claude)
                        .ok()
                        .flatten();
                    if let Some(provider) = claude_provider.as_ref() {
                        let provider = self.claude_provider_with_effective_settings(provider)?;
                        Self::apply_claude_takeover_fields_for_provider(
                            &mut live_config,
                            &proxy_url,
                            &provider,
                        );
                    } else {
                        Self::apply_claude_takeover_fields_with_policy(
                            &mut live_config,
                            &proxy_url,
                            ClaudeTakeoverAuthPolicy::PreserveExistingOrAuthToken,
                        );
                    }
                    let _ = self.write_claude_live(&live_config);
                }
            }
            AppType::Codex if self.read_codex_live().is_ok() => {
                let codex_provider = self.require_current_provider_for_app(&AppType::Codex)?;
                self.sync_codex_live_from_provider_while_proxy_active(&codex_provider)
                    .await?;
            }
            AppType::Gemini => {
                if let Ok(mut live_config) = self.read_gemini_live() {
                    if let Some(env) = live_config.get_mut("env").and_then(|v| v.as_object_mut()) {
                        env.insert("GOOGLE_GEMINI_BASE_URL".to_string(), json!(&proxy_url));
                        env.insert("GEMINI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
                    } else {
                        live_config["env"] = json!({
                            "GOOGLE_GEMINI_BASE_URL": &proxy_url,
                            "GEMINI_API_KEY": PROXY_TOKEN_PLACEHOLDER
                        });
                    }

                    let _ = self.write_gemini_live(&live_config);
                }
            }
            AppType::GrokBuild => {
                if let Ok(mut live_config) = self.read_grok_live() {
                    if Self::grok_live_config_supports_takeover(&live_config) {
                        Self::apply_grok_takeover_fields(&mut live_config, &proxy_grok_base_url)?;
                        let _ = self.write_grok_live(&live_config);
                    } else {
                        log::info!(
                            "Grok Build Live 处于官方登录态（无自定义模型表），跳过代理接管"
                        );
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Re-project an app's takeover Live config after a proxy restart.
    ///
    /// The per-app switch lock is shared with provider switch/update and Auth
    /// Center credential removal. Re-read `enabled` only after acquiring it so
    /// a stale pre-lock takeover snapshot cannot recreate Live state after a
    /// concurrent disable/remove transaction has committed.
    async fn reproject_takeover_live_config_if_enabled(
        &self,
        app_type: &AppType,
    ) -> Result<bool, String> {
        let app_type_str = app_type.as_str();
        let _guard = self.switch_locks.lock_for_app(app_type_str).await;
        let current_config = match self.db.get_proxy_config_for_app(app_type_str).await {
            Ok(config) => config,
            Err(error) => {
                log::warn!(
                    "读取 {app_type_str} 接管状态失败，跳过代理重启后的 Live 重投影: {error}"
                );
                return Ok(false);
            }
        };
        if !current_config.enabled {
            return Ok(false);
        }

        self.takeover_live_config_best_effort(app_type).await?;
        Ok(true)
    }

    async fn restore_live_config_for_app_inner(&self, app_type: &AppType) -> Result<(), String> {
        match app_type {
            AppType::Claude => {
                if let Ok(Some(backup)) = self.db.get_live_backup("claude").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Claude 备份失败: {e}"))?;
                    self.write_claude_live(&config)?;
                    log::info!("Claude Live 配置已恢复");
                }
            }
            AppType::Codex => {
                if let Ok(Some(backup)) = self.db.get_live_backup("codex").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Codex 备份失败: {e}"))?;
                    self.write_codex_restore_backup(&config)?;
                    log::info!("Codex Live 配置已恢复");
                }
            }
            AppType::Gemini => {
                if let Ok(Some(backup)) = self.db.get_live_backup("gemini").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Gemini 备份失败: {e}"))?;
                    self.write_gemini_live(&config)?;
                    log::info!("Gemini Live 配置已恢复");
                }
            }
            AppType::GrokBuild => {
                if let Ok(Some(backup)) = self.db.get_live_backup("grokbuild").await {
                    let config: Value = serde_json::from_str(&backup.original_config)
                        .map_err(|e| format!("解析 Grok Build 备份失败: {e}"))?;
                    self.write_grok_live(&config)?;
                    log::info!("Grok Build Live 配置已恢复");
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// 恢复原始 Live 配置
    async fn restore_live_configs(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        for app_type in [
            AppType::Claude,
            AppType::Codex,
            AppType::Gemini,
            AppType::GrokBuild,
        ] {
            if let Err(e) = self
                .restore_live_config_for_app_with_fallback(&app_type)
                .await
            {
                errors.push(e);
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }

    async fn restore_live_config_for_app_with_fallback(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type.as_str()).await;
        self.restore_live_config_for_app_with_fallback_inner(app_type)
            .await
    }

    pub(crate) async fn restore_live_config_for_app_with_fallback_inner(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        let app_type_str = app_type.as_str();

        // 1) 优先从 Live 备份恢复（这是"原始 Live"的唯一可靠来源）
        let backup = self
            .db
            .get_live_backup(app_type_str)
            .await
            .map_err(|e| format!("获取 {app_type_str} Live 备份失败: {e}"))?;
        if let Some(backup) = backup {
            let config: Value = serde_json::from_str(&backup.original_config)
                .map_err(|e| format!("解析 {app_type_str} 备份失败: {e}"))?;

            // 备份若是代理占位符（异常历史：上次 stop 失败导致 Live 留在了代理状态，
            // 下次接管时又被错误地备份成"原始 Live"），不能直接用 — 否则 stop 后
            // Live 永远卡在 127.0.0.1:15721。落到下面的 SSOT 兜底重建。
            if Self::live_has_proxy_placeholder_for_app(app_type, &config) {
                log::warn!(
                    "{app_type_str} 备份本身已是代理占位符（异常历史状态），跳过备份，改走 SSOT 重建 Live"
                );
            } else {
                self.write_live_config_for_app(app_type, &config)?;
                log::info!("{app_type_str} Live 配置已从备份恢复");
                return Ok(());
            }
        }

        // 2) 兜底：备份缺失，但 Live 仍包含接管占位符（异常退出/历史 bug 场景）
        if !self.detect_takeover_in_live_config_for_app(app_type) {
            return Ok(());
        }

        // 2.1) 优先从 SSOT（当前供应商）重建 Live（比"清理字段"更可用）
        match self.restore_live_from_ssot_for_app(app_type) {
            Ok(true) => {
                log::info!("{app_type_str} Live 配置已从 SSOT 恢复（无备份兜底）");
                return Ok(());
            }
            Ok(false) => {
                log::warn!(
                    "{app_type_str} Live 备份缺失，且无法从 SSOT 恢复，将尝试清理接管占位符"
                );
            }
            Err(e) => {
                log::error!(
                    "{app_type_str} Live 备份缺失，SSOT 恢复失败，将尝试清理接管占位符: {e}"
                );
            }
        }

        // 2.2) 最后兜底：尽力清理占位符与本地代理地址，避免长期卡在代理占位符状态
        self.cleanup_takeover_placeholders_in_live_for_app(app_type)?;
        log::info!("{app_type_str} Live 接管占位符已清理（无备份兜底）");
        Ok(())
    }

    fn write_live_config_for_app(&self, app_type: &AppType, config: &Value) -> Result<(), String> {
        match app_type {
            AppType::Claude => self.write_claude_live(config),
            AppType::Codex => self.write_codex_restore_backup(config),
            AppType::Gemini => self.write_gemini_live(config),
            AppType::GrokBuild => self.write_grok_live(config),
            _ => Err("该应用不支持代理功能".to_string()),
        }
    }

    pub fn detect_takeover_in_live_config_for_app(&self, app_type: &AppType) -> bool {
        match app_type {
            AppType::Claude => match self.read_claude_live() {
                Ok(config) => Self::is_claude_live_taken_over(&config),
                Err(_) => false,
            },
            AppType::Codex => match self.read_codex_live() {
                Ok(config) => Self::is_codex_live_taken_over(&config),
                Err(_) => false,
            },
            AppType::Gemini => match self.read_gemini_live() {
                Ok(config) => Self::is_gemini_live_taken_over(&config),
                Err(_) => false,
            },
            AppType::GrokBuild => match self.read_grok_live() {
                Ok(config) => Self::is_grok_live_taken_over(&config),
                Err(_) => false,
            },
            _ => false,
        }
    }

    /// 当 Live 备份缺失时，尝试用 SSOT（当前供应商）写回 Live，以解除占位符接管。
    ///
    /// 返回值：
    /// - Ok(true)：已成功写回
    /// - Ok(false)：缺少当前供应商/供应商不存在/供应商本身含占位符，无法写回
    fn restore_live_from_ssot_for_app(&self, app_type: &AppType) -> Result<bool, String> {
        let current_id = crate::settings::get_effective_current_provider(&self.db, app_type)
            .map_err(|e| format!("获取 {app_type:?} 当前供应商失败: {e}"))?;

        let Some(current_id) = current_id else {
            return Ok(false);
        };

        let providers = self
            .db
            .get_all_providers(app_type.as_str())
            .map_err(|e| format!("读取 {app_type:?} 供应商列表失败: {e}"))?;

        let Some(provider) = providers.get(&current_id) else {
            return Ok(false);
        };

        // 供应商配置本身含接管占位符时不可写回（历史异常：接管期间 Live 被
        // 误导入成了供应商）。写回只会把占位符固化进 Live；返回 Ok(false)
        // 让调用方落到"清理占位符"兜底。
        if Self::live_has_proxy_placeholder_for_app(app_type, &provider.settings_config) {
            log::warn!(
                "{app_type:?} 当前供应商配置含代理接管占位符（疑似接管期间被导入的残留），跳过 SSOT 写回，改走占位符清理"
            );
            return Ok(false);
        }

        write_live_with_common_config_for_codex_oauth_manager(
            self.db.as_ref(),
            app_type,
            provider,
            &self.codex_oauth_manager,
        )
        .map_err(|e| format!("写入 {app_type:?} Live 配置失败: {e}"))?;

        Ok(true)
    }

    fn cleanup_takeover_placeholders_in_live_for_app(
        &self,
        app_type: &AppType,
    ) -> Result<(), String> {
        match app_type {
            AppType::Claude => self.cleanup_claude_takeover_placeholders_in_live(),
            AppType::Codex => self.cleanup_codex_takeover_placeholders_in_live(),
            AppType::Gemini => self.cleanup_gemini_takeover_placeholders_in_live(),
            AppType::GrokBuild => self.cleanup_grok_takeover_placeholders_in_live(),
            _ => Ok(()),
        }
    }

    fn is_local_proxy_url(url: &str) -> bool {
        let url = url.trim();
        if !url.starts_with("http://") {
            return false;
        }
        let rest = &url["http://".len()..];
        rest.starts_with("127.0.0.1")
            || rest.starts_with("localhost")
            || rest.starts_with("0.0.0.0")
            || rest.starts_with("[::1]")
            || rest.starts_with("[::]")
            || rest.starts_with("::1")
            || rest.starts_with("::")
    }

    fn proxy_urls_match(actual: &str, expected: &str) -> bool {
        actual.trim().trim_end_matches('/') == expected.trim().trim_end_matches('/')
    }

    fn codex_config_has_base_url_matching(
        config_text: &str,
        predicate: impl Fn(&str) -> bool,
    ) -> bool {
        let Ok(doc) = toml::from_str::<toml::Value>(config_text) else {
            return false;
        };

        let active_provider = doc
            .get("model_provider")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty());

        if let Some(provider_id) = active_provider {
            if doc
                .get("model_providers")
                .and_then(|value| value.get(provider_id))
                .and_then(|value| value.get("base_url"))
                .and_then(|value| value.as_str())
                .is_some_and(&predicate)
            {
                return true;
            }
        }

        doc.get("base_url")
            .and_then(|value| value.as_str())
            .is_some_and(predicate)
    }

    async fn live_takeover_matches_current_proxy(
        &self,
        app_type: &AppType,
    ) -> Result<bool, String> {
        let (proxy_url, proxy_codex_base_url) = self.build_proxy_urls().await?;
        let proxy_grok_base_url = format!("{}/grokbuild/v1", proxy_url.trim_end_matches('/'));

        match app_type {
            AppType::Claude => {
                let config = self.read_claude_live()?;
                let base_url_matches = config
                    .get("env")
                    .and_then(|value| value.get("ANTHROPIC_BASE_URL"))
                    .and_then(|value| value.as_str())
                    .is_some_and(|url| Self::proxy_urls_match(url, &proxy_url));
                Ok(Self::is_claude_live_taken_over(&config) && base_url_matches)
            }
            AppType::Codex => {
                let config = self.read_codex_live()?;
                let base_url_matches = config
                    .get("config")
                    .and_then(|value| value.as_str())
                    .is_some_and(|config_text| {
                        Self::codex_config_has_base_url_matching(config_text, |url| {
                            Self::proxy_urls_match(url, &proxy_codex_base_url)
                        })
                    });
                Ok(Self::is_codex_live_taken_over(&config) && base_url_matches)
            }
            AppType::Gemini => {
                let config = self.read_gemini_live()?;
                let base_url_matches = config
                    .get("env")
                    .and_then(|value| value.get("GOOGLE_GEMINI_BASE_URL"))
                    .and_then(|value| value.as_str())
                    .is_some_and(|url| Self::proxy_urls_match(url, &proxy_url));
                Ok(Self::is_gemini_live_taken_over(&config) && base_url_matches)
            }
            AppType::GrokBuild => {
                let config = self.read_grok_live()?;
                let base_url_matches =
                    config
                        .get("config")
                        .and_then(Value::as_str)
                        .is_some_and(|config_toml| {
                            crate::grok_config::base_url_matches(config_toml, |url| {
                                Self::proxy_urls_match(url, &proxy_grok_base_url)
                            })
                        });
                Ok(Self::is_grok_live_taken_over(&config) && base_url_matches)
            }
            _ => Ok(false),
        }
    }

    fn cleanup_claude_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_claude_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
        };

        for key in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ] {
            if env.get(key).and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
                env.remove(key);
            }
        }

        if env
            .get("ANTHROPIC_BASE_URL")
            .and_then(|v| v.as_str())
            .map(Self::is_local_proxy_url)
            .unwrap_or(false)
        {
            env.remove("ANTHROPIC_BASE_URL");
        }

        self.write_claude_live(&config)?;
        Ok(())
    }

    fn cleanup_codex_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_codex_live()?;

        if let Some(auth) = config.get_mut("auth").and_then(|v| v.as_object_mut()) {
            if auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
            {
                auth.remove("OPENAI_API_KEY");
            }
        }

        if let Some(cfg_str) = config.get("config").and_then(|v| v.as_str()) {
            let updated = Self::remove_local_toml_base_url(cfg_str);
            let updated =
                crate::codex_config::remove_codex_experimental_bearer_token_if(&updated, |token| {
                    token == PROXY_TOKEN_PLACEHOLDER
                })
                .map_err(|e| format!("清理 Codex 接管占位符失败: {e}"))?;
            let updated = crate::codex_config::remove_codex_official_proxy_route(&updated)
                .map_err(|e| format!("清理 Codex 官方接管路由失败: {e}"))?;
            config["config"] = json!(updated);
        }

        self.write_codex_live(&config)?;
        Ok(())
    }

    /// Remove local proxy base_url from TOML（委托给 codex_config 共享实现）
    fn remove_local_toml_base_url(toml_str: &str) -> String {
        crate::codex_config::remove_codex_toml_base_url_if(toml_str, Self::is_local_proxy_url)
    }

    fn cleanup_gemini_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let mut config = self.read_gemini_live()?;

        let Some(env) = config.get_mut("env").and_then(|v| v.as_object_mut()) else {
            return Ok(());
        };

        if env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
            env.remove("GEMINI_API_KEY");
        }

        if env
            .get("GOOGLE_GEMINI_BASE_URL")
            .and_then(|v| v.as_str())
            .map(Self::is_local_proxy_url)
            .unwrap_or(false)
        {
            env.remove("GOOGLE_GEMINI_BASE_URL");
        }

        self.write_gemini_live(&config)?;
        Ok(())
    }

    fn cleanup_grok_takeover_placeholders_in_live(&self) -> Result<(), String> {
        let config = self.read_grok_live()?;
        let Some(config_toml) = config.get("config").and_then(Value::as_str) else {
            return Ok(());
        };
        if !crate::grok_config::has_proxy_placeholder(config_toml, PROXY_TOKEN_PLACEHOLDER) {
            return Ok(());
        }

        // A valid provider snapshot should normally restore before this fallback.
        // Clearing the token prevents a stale local route from looking usable.
        let updated = crate::grok_config::update_api_key(config_toml, "")
            .map_err(|e| format!("清理 Grok Build 接管占位符失败: {e}"))?;
        crate::config::write_text_file(&crate::grok_config::get_grok_config_path(), &updated)
            .map_err(|e| format!("写入 Grok Build 配置失败: {e}"))
    }

    /// 检查是否处于 Live 接管模式
    pub async fn is_takeover_active(&self) -> Result<bool, String> {
        let status = self.get_takeover_status().await?;
        Ok(status.claude || status.codex || status.gemini || status.grokbuild)
    }

    /// 从异常退出中恢复（启动时调用）
    ///
    /// 检测到 Live 备份残留时调用此方法。
    /// 会恢复 Live 配置、清除接管标志、删除备份。
    pub async fn recover_from_crash(&self) -> Result<(), String> {
        // 1. 恢复 Live 配置
        self.restore_live_configs().await?;

        // 2. 清除接管标志
        self.db
            .set_live_takeover_active(false)
            .await
            .map_err(|e| format!("清除接管状态失败: {e}"))?;

        // 3. 删除备份
        self.db
            .delete_all_live_backups()
            .await
            .map_err(|e| format!("删除备份失败: {e}"))?;

        log::info!("已从异常退出中恢复 Live 配置");
        Ok(())
    }

    /// 检测 Live 配置是否处于"被接管"的残留状态
    ///
    /// 用于兜底处理：当数据库备份缺失但 Live 文件已经写成代理占位符时，
    /// 启动流程可以据此触发恢复逻辑。
    pub fn detect_takeover_in_live_configs(&self) -> bool {
        if let Ok(config) = self.read_claude_live() {
            if Self::is_claude_live_taken_over(&config) {
                return true;
            }
        }

        if let Ok(config) = self.read_codex_live() {
            if Self::is_codex_live_taken_over(&config) {
                return true;
            }
        }

        if let Ok(config) = self.read_gemini_live() {
            if Self::is_gemini_live_taken_over(&config) {
                return true;
            }
        }

        if let Ok(config) = self.read_grok_live() {
            if Self::is_grok_live_taken_over(&config) {
                return true;
            }
        }

        false
    }

    fn is_claude_live_taken_over(config: &Value) -> bool {
        let env = match config.get("env").and_then(|v| v.as_object()) {
            Some(env) => env,
            None => return false,
        };

        for key in [
            "ANTHROPIC_AUTH_TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENAI_API_KEY",
        ] {
            if env.get(key).and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER) {
                return true;
            }
        }

        false
    }

    fn codex_live_has_proxy_placeholder(config: &Value) -> bool {
        if config
            .get("auth")
            .and_then(|v| v.as_object())
            .and_then(|auth| auth.get("OPENAI_API_KEY"))
            .and_then(|v| v.as_str())
            == Some(PROXY_TOKEN_PLACEHOLDER)
        {
            return true;
        }

        config
            .get("config")
            .and_then(|v| v.as_str())
            .and_then(crate::codex_config::extract_codex_experimental_bearer_token)
            .as_deref()
            == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    fn is_codex_live_taken_over(config: &Value) -> bool {
        Self::codex_live_has_proxy_placeholder(config)
            || config
                .get("config")
                .and_then(|v| v.as_str())
                .is_some_and(crate::codex_config::codex_config_has_official_proxy_route)
    }

    fn is_gemini_live_taken_over(config: &Value) -> bool {
        let env = match config.get("env").and_then(|v| v.as_object()) {
            Some(env) => env,
            None => return false,
        };
        env.get("GEMINI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    fn is_grok_live_taken_over(config: &Value) -> bool {
        config
            .get("config")
            .and_then(Value::as_str)
            .is_some_and(|config_toml| {
                crate::grok_config::has_proxy_placeholder(config_toml, PROXY_TOKEN_PLACEHOLDER)
            })
    }

    /// 判断给定的 Live/备份配置是否已被代理接管（包含占位符）
    ///
    /// 用途：检测"备份里存的其实是代理配置"这种异常历史状态。
    /// 如果发现，备份不可信，备份路径不能写入（否则会把代理配置固化进备份槽），
    /// 恢复路径不能读取（否则会把代理占位符原样写回 Live，永久卡在代理地址）。
    /// 两种情况下都应该走 SSOT 兜底重建 Live。
    fn live_has_proxy_placeholder_for_app(app_type: &AppType, config: &Value) -> bool {
        match app_type {
            AppType::Claude => Self::is_claude_live_taken_over(config),
            AppType::Codex => Self::is_codex_live_taken_over(config),
            AppType::Gemini => Self::is_gemini_live_taken_over(config),
            AppType::GrokBuild => Self::is_grok_live_taken_over(config),
            _ => false,
        }
    }

    /// 从供应商配置更新 Live 备份（用于代理模式下的热切换）
    ///
    /// 与 backup_live_configs() 不同，此方法从供应商的 settings_config 生成备份，
    /// 而不是从 Live 文件读取（因为 Live 文件已被代理接管）。
    pub async fn update_live_backup_from_provider(
        &self,
        app_type: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        let _guard = self.switch_locks.lock_for_app(app_type).await;
        self.update_live_backup_from_provider_inner(app_type, provider, None)
            .await
    }

    /// 仅供已持有 per-app 切换锁的调用方使用。
    pub(crate) async fn update_live_backup_from_provider_inner(
        &self,
        app_type: &str,
        provider: &Provider,
        clear_codex_auth_for_account: Option<&str>,
    ) -> Result<(), String> {
        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("未知的应用类型: {app_type}"))?;
        let mut effective_settings = if matches!(app_type_enum, AppType::Codex) {
            build_effective_provider_for_live_with_codex_oauth_manager(
                self.db.as_ref(),
                &app_type_enum,
                provider,
                &self.codex_oauth_manager,
            )
            .map_err(|e| format!("构建 {app_type} 有效配置失败: {e}"))?
            .settings_config
        } else {
            build_effective_settings_with_common_config(self.db.as_ref(), &app_type_enum, provider)
                .map_err(|e| format!("构建 {app_type} 有效配置失败: {e}"))?
        };

        if matches!(app_type_enum, AppType::Codex) {
            let is_codex_official = crate::proxy::providers::is_codex_official_provider(provider);
            let existing_backup_value = self
                .db
                .get_live_backup(app_type)
                .await
                .map_err(|e| format!("读取 {app_type} 现有备份失败: {e}"))?
                .map(|backup| {
                    serde_json::from_str::<Value>(&backup.original_config)
                        .map_err(|e| format!("解析 {app_type} 现有备份失败: {e}"))
                })
                .transpose()?;
            // A stale takeover marker can survive without its DB backup (for
            // example after a partial cleanup). In that abnormal state the live
            // auth file is still the only copy of the user's login, so use it as
            // the preservation source instead of replacing it with the empty
            // official seed snapshot.
            let existing_backup_value =
                existing_backup_value.or_else(|| self.read_codex_live().ok());

            if let Some(existing_value) = existing_backup_value.as_ref() {
                Self::preserve_toml_mcp_servers_from_existing_config(
                    &mut effective_settings,
                    existing_value,
                )?;
                if let Some(account_id) = clear_codex_auth_for_account {
                    Self::clear_codex_auth_in_backup(
                        &mut effective_settings,
                        existing_value,
                        account_id,
                    )?;
                } else if provider
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
                    .is_none()
                {
                    Self::preserve_codex_auth_in_backup(
                        &mut effective_settings,
                        existing_value,
                        is_codex_official,
                    )?;
                }
            }

            // 统一会话开关：备份是接管释放时恢复 live 的来源，官方配置的
            // 共享 custom 路由注入必须落在备份里，否则恢复后开关失效。
            crate::codex_config::apply_codex_unified_session_bucket_to_settings(
                if is_codex_official {
                    Some("official")
                } else {
                    provider.category.as_deref()
                },
                &mut effective_settings,
            )
            .map_err(|e| format!("注入统一会话路由失败: {e}"))?;

            if is_codex_official {
                // auth.json keeps rotating independently while takeover is
                // active. Persisting it in the DB backup would later roll a
                // valid R1 generation back to the frozen R0 generation.
                if let Some(root) = effective_settings.as_object_mut() {
                    root.remove("auth");
                }
            }
        }

        if matches!(app_type_enum, AppType::GrokBuild) {
            let existing_value = self
                .db
                .get_live_backup(app_type)
                .await
                .map_err(|e| format!("读取 {app_type} 现有备份失败: {e}"))?
                .map(|backup| {
                    serde_json::from_str::<Value>(&backup.original_config)
                        .map_err(|e| format!("解析 {app_type} 现有备份失败: {e}"))
                })
                .transpose()?
                .or_else(|| self.read_grok_live().ok());
            if let Some(existing_value) = existing_value.as_ref() {
                Self::preserve_toml_mcp_servers_from_existing_config(
                    &mut effective_settings,
                    existing_value,
                )?;
            }
        }

        let backup_json = match app_type_enum {
            AppType::Claude => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Claude 配置失败: {e}"))?,
            AppType::Codex => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Codex 配置失败: {e}"))?,
            AppType::GrokBuild => serde_json::to_string(&effective_settings)
                .map_err(|e| format!("序列化 Grok Build 配置失败: {e}"))?,
            AppType::Gemini => {
                // Gemini takeover 仅修改 .env；settings.json（含 mcpServers）保持原样。
                let env_backup = if let Some(env) = effective_settings.get("env") {
                    json!({ "env": env })
                } else {
                    json!({ "env": {} })
                };
                serde_json::to_string(&env_backup)
                    .map_err(|e| format!("序列化 Gemini 配置失败: {e}"))?
            }
            _ => return Err(format!("未知的应用类型: {app_type}")),
        };

        self.db
            .save_live_backup(app_type, &backup_json)
            .await
            .map_err(|e| format!("更新 {app_type} 备份失败: {e}"))?;

        log::info!("已更新 {app_type} Live 备份（热切换）");
        Ok(())
    }

    pub async fn hot_switch_provider(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<HotSwitchOutcome, String> {
        crate::copilot_bridge::require_codex(app_type).map_err(|e| e.to_string())?;
        let _guard = self.switch_locks.lock_for_app(app_type).await;
        let changed =
            crate::copilot_bridge::current(&self.db).map_err(|e| e.to_string())? != provider_id;
        crate::copilot_bridge::select(&self.db, provider_id).map_err(|e| e.to_string())?;
        if let Some(server) = self.server.read().await.as_ref() {
            if let Some(provider) = self
                .db
                .get_provider_by_id(provider_id, app_type)
                .map_err(|e| e.to_string())?
            {
                server
                    .set_active_target(app_type, provider_id, &provider.name)
                    .await;
            }
        }
        Ok(HotSwitchOutcome {
            logical_target_changed: changed,
        })
    }

    pub(crate) async fn hot_switch_provider_inner(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<HotSwitchOutcome, String> {
        let app_type_enum =
            AppType::from_str(app_type).map_err(|_| format!("无效的应用类型: {app_type}"))?;
        let provider = self
            .db
            .get_provider_by_id(provider_id, app_type)
            .map_err(|e| format!("读取供应商失败: {e}"))?
            .ok_or_else(|| format!("供应商不存在: {provider_id}"))?;

        // Defense-in-depth: only Codex official providers support native OpenAI
        // auth passthrough during takeover.
        if provider.category.as_deref() == Some("official")
            && !crate::services::provider::official_provider_supports_proxy_takeover(
                &app_type_enum,
                &provider,
            )
        {
            return Err(
                "代理接管模式下不能切换到官方供应商 (Cannot switch to official provider during proxy takeover)"
                    .to_string(),
            );
        }

        let previous_provider_id =
            crate::settings::get_effective_current_provider(&self.db, &app_type_enum)
                .map_err(|e| format!("读取当前供应商失败: {e}"))?;
        let previous_provider = previous_provider_id
            .as_deref()
            .map(|id| {
                self.db
                    .get_provider_by_id(id, app_type_enum.as_str())
                    .map_err(|error| format!("读取原供应商失败: {error}"))
            })
            .transpose()?
            .flatten();
        let previous_managed_codex_account_id = previous_provider
            .as_ref()
            .and_then(|provider| provider.meta.as_ref())
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
            .map(|account_id| account_id.trim().to_string())
            .filter(|account_id| !account_id.is_empty());
        let target_managed_codex_account_id = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
            .map(|account_id| account_id.trim().to_string())
            .filter(|account_id| !account_id.is_empty());
        let outgoing_managed_codex_account_id = previous_managed_codex_account_id
            .as_ref()
            .filter(|account_id| target_managed_codex_account_id.as_ref() != Some(*account_id))
            .cloned();
        let previous_local_provider_id = crate::settings::get_current_provider(&app_type_enum);
        let logical_target_changed = previous_provider_id.as_deref() != Some(provider_id);

        let has_backup = self
            .db
            .get_live_backup(app_type_enum.as_str())
            .await
            .map_err(|e| format!("读取 {app_type} 备份失败: {e}"))?
            .is_some();
        let live_taken_over = self.detect_takeover_in_live_config_for_app(&app_type_enum);
        let should_sync_backup = has_backup || live_taken_over;
        let outgoing_live_auth_guard =
            if should_sync_backup && matches!(app_type_enum, AppType::Codex) {
                match outgoing_managed_codex_account_id.as_deref() {
                    Some(account_id) => self
                        .codex_oauth_manager
                        .prepare_live_auth_for_account_switch_away(account_id)
                        .await
                        .map(Some)
                        .map_err(|error| error.to_string())?,
                    None => None,
                }
            } else {
                None
            };

        // All fallible backup/live writes must finish before committing the logical
        // current provider. Otherwise a failed hot switch leaves the UI pointing at
        // the new provider while the proxy still serves the old one (and the next
        // query may fall back to the first provider).
        let previous_backup = if should_sync_backup {
            self.db
                .get_live_backup(app_type_enum.as_str())
                .await
                .map_err(|e| format!("读取 {app_type} 原备份失败: {e}"))?
        } else {
            None
        };
        let previous_codex_live_state =
            if should_sync_backup && matches!(app_type_enum, AppType::Codex) {
                Some(
                    crate::codex_config::CodexLiveStateSnapshot::capture()
                        .map_err(|error| format!("捕获 Codex 热切换前状态失败: {error}"))?,
                )
            } else {
                None
            };

        let prepare_result: Result<(), String> = async {
            if should_sync_backup {
                self.update_live_backup_from_provider_inner(
                    app_type,
                    &provider,
                    outgoing_managed_codex_account_id.as_deref(),
                )
                .await?;

                if matches!(app_type_enum, AppType::Claude) {
                    self.sync_claude_live_from_provider_while_proxy_active(&provider)
                        .await?;
                } else if live_taken_over && matches!(app_type_enum, AppType::Codex) {
                    self.sync_codex_live_from_provider_while_proxy_active_guarded(
                        &provider,
                        outgoing_managed_codex_account_id.as_deref(),
                        outgoing_live_auth_guard.as_ref(),
                    )
                    .await?;
                } else if live_taken_over && matches!(app_type_enum, AppType::GrokBuild) {
                    self.sync_grok_live_from_provider_while_proxy_active(&provider)
                        .await?;
                }
            }

            if has_backup && !live_taken_over && matches!(app_type_enum, AppType::Codex) {
                let effective_provider =
                    build_effective_provider_for_live_with_codex_oauth_manager(
                        self.db.as_ref(),
                        &AppType::Codex,
                        &provider,
                        &self.codex_oauth_manager,
                    )
                    .map_err(|e| format!("构建 Codex 有效配置失败: {e}"))?;
                let effective_settings = &effective_provider.settings_config;
                let auth = effective_settings
                    .get("auth")
                    .ok_or_else(|| "Codex 供应商缺少 auth 配置".to_string())?;
                let config_str = effective_settings.get("config").and_then(|v| v.as_str());
                let profile = crate::proxy::providers::resolve_codex_catalog_tool_profile(
                    &effective_provider,
                );

                if let (Some(account_id), Some(guard)) = (
                    outgoing_managed_codex_account_id.as_deref(),
                    outgoing_live_auth_guard.as_ref(),
                ) {
                    guard
                        .ensure_unchanged(account_id)
                        .map_err(|error| error.to_string())?;
                }

                crate::codex_config::write_codex_provider_live_with_catalog(
                    effective_settings,
                    effective_provider.category.as_deref(),
                    auth,
                    config_str,
                    profile,
                )
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                if let Some(account_id) = target_managed_codex_account_id.as_deref() {
                    crate::codex_config::record_codex_managed_oauth_live_auth(auth, account_id)
                        .map_err(|error| format!("记录 Codex 托管认证标记失败: {error}"))?;
                }
            }

            if should_sync_backup && matches!(app_type_enum, AppType::Codex) {
                if let (Some(account_id), Some(guard)) = (
                    outgoing_managed_codex_account_id.as_deref(),
                    outgoing_live_auth_guard.as_ref(),
                ) {
                    guard
                        .clear_outgoing(account_id)
                        .map_err(|error| error.to_string())?;
                }
            }

            Ok(())
        }
        .await;

        if let Err(error) = prepare_result {
            self.rollback_hot_switch_preparation(
                &app_type_enum,
                previous_backup.as_ref(),
                previous_provider_id.as_deref(),
                should_sync_backup,
                live_taken_over,
                previous_codex_live_state.as_ref(),
            )
            .await;
            return Err(error);
        }

        if let Err(error) = crate::settings::set_current_provider(&app_type_enum, Some(provider_id))
        {
            self.rollback_hot_switch_preparation(
                &app_type_enum,
                previous_backup.as_ref(),
                previous_provider_id.as_deref(),
                should_sync_backup,
                live_taken_over,
                previous_codex_live_state.as_ref(),
            )
            .await;
            return Err(format!("更新本地当前供应商失败: {error}"));
        }
        if let Err(error) = self
            .db
            .set_current_provider(app_type_enum.as_str(), provider_id)
        {
            if let Err(rollback_error) = crate::settings::set_current_provider(
                &app_type_enum,
                previous_local_provider_id.as_deref(),
            ) {
                log::error!("数据库切换失败后恢复本地当前供应商失败: {rollback_error}");
            }
            self.rollback_hot_switch_preparation(
                &app_type_enum,
                previous_backup.as_ref(),
                previous_provider_id.as_deref(),
                should_sync_backup,
                live_taken_over,
                previous_codex_live_state.as_ref(),
            )
            .await;
            return Err(format!("更新当前供应商失败: {error}"));
        }

        if let Some(server) = self.server.read().await.as_ref() {
            server
                .set_active_target(app_type_enum.as_str(), &provider.id, &provider.name)
                .await;
        }

        Ok(HotSwitchOutcome {
            logical_target_changed,
        })
    }

    #[cfg(test)]
    async fn lock_switch_for_test(&self, app_type: &str) -> tokio::sync::OwnedMutexGuard<()> {
        self.switch_locks.lock_for_app(app_type).await
    }

    fn preserve_toml_mcp_servers_from_existing_config(
        target_settings: &mut Value,
        existing_config: &Value,
    ) -> Result<(), String> {
        let target_obj = target_settings
            .as_object_mut()
            .ok_or_else(|| "TOML 应用备份必须是 JSON 对象".to_string())?;

        let target_config = target_obj
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mut target_doc = if target_config.trim().is_empty() {
            toml_edit::DocumentMut::new()
        } else {
            target_config
                .parse::<toml_edit::DocumentMut>()
                .map_err(|e| format!("解析新的 config.toml 失败: {e}"))?
        };

        let existing_config = existing_config
            .get("config")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if existing_config.trim().is_empty() {
            target_obj.insert("config".to_string(), json!(target_doc.to_string()));
            return Ok(());
        }

        let existing_doc = existing_config
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("解析现有 config.toml 备份失败: {e}"))?;

        if let Some(existing_mcp_servers) = existing_doc.get("mcp_servers") {
            match target_doc.get_mut("mcp_servers") {
                Some(target_mcp_servers) => {
                    if let (Some(target_table), Some(existing_table)) = (
                        target_mcp_servers.as_table_like_mut(),
                        existing_mcp_servers.as_table_like(),
                    ) {
                        for (server_id, server_item) in existing_table.iter() {
                            if target_table.get(server_id).is_none() {
                                target_table.insert(server_id, server_item.clone());
                            }
                        }
                    } else {
                        log::warn!(
                            "config.toml contains a non-table mcp_servers section; skipping MCP merge"
                        );
                    }
                }
                None => {
                    target_doc["mcp_servers"] = existing_mcp_servers.clone();
                }
            }
        }

        target_obj.insert("config".to_string(), json!(target_doc.to_string()));
        Ok(())
    }

    fn clear_codex_auth_in_backup(
        target_settings: &mut Value,
        existing_backup: &Value,
        account_id: &str,
    ) -> Result<(), String> {
        let Some(existing_auth) = existing_backup.get("auth") else {
            return Ok(());
        };
        let Some(target_obj) = target_settings.as_object_mut() else {
            return Err("Codex 备份必须是 JSON 对象".to_string());
        };

        // Access and refresh tokens rotate independently while Codex is running,
        // so backup stripping uses the stable local-account marker plus workspace
        // ID rather than a token fingerprint.
        if crate::codex_config::codex_live_auth_is_managed_chatgpt_login(existing_auth, account_id)
        {
            // Do not copy the outgoing managed bundle over the target. Keep the
            // target provider's own auth material intact (important for a
            // managed -> third-party hot switch); unbound official targets are
            // already empty and official backups are stripped below.
            return Ok(());
        }

        target_obj.insert("auth".to_string(), existing_auth.clone());
        Ok(())
    }

    fn preserve_codex_auth_in_backup(
        target_settings: &mut Value,
        existing_backup: &Value,
        preserve_api_key: bool,
    ) -> Result<(), String> {
        let Some(existing_auth) = existing_backup
            .get("auth")
            .filter(|auth| {
                !Self::codex_auth_has_proxy_placeholder(auth)
                    && (crate::codex_config::codex_auth_has_oauth_login_material(auth)
                        || (preserve_api_key
                            && crate::codex_config::codex_auth_has_login_material(auth)))
            })
            .cloned()
        else {
            return Ok(());
        };

        let Some(target_obj) = target_settings.as_object_mut() else {
            return Ok(());
        };

        let provider_auth = target_obj.get("auth").cloned().unwrap_or_else(|| json!({}));
        if let Some(config_text) = target_obj.get("config").and_then(|value| value.as_str()) {
            let live_config = crate::codex_config::prepare_codex_provider_live_config(
                &provider_auth,
                config_text,
            )
            .map_err(|e| format!("更新 Codex 备份配置失败: {e}"))?;
            target_obj.insert("config".to_string(), json!(live_config));
        }
        target_obj.insert("auth".to_string(), existing_auth);

        Ok(())
    }

    /// 恢复备份到 Codex live 时绝不覆盖官方 ChatGPT 登录。
    ///
    /// 备份是接管开启时的快照，而 live 的 `auth.json` 可能被 Codex
    /// （登录 / token 自刷新）或接管中的托管账号切换更新——所以只要 live
    /// 持有真实登录凭据（非接管占位符），它就比快照新，必须获胜：登录发生在接管期间时备份
    /// 里是第三方 API key（#6277 的循环覆盖链），把它降级进 config 的
    /// `experimental_bearer_token`；备份也是官方形态时它的 tokens 只是旧
    /// 副本，同样不写回。摘掉备份 auth 槽后 `write_codex_live_verbatim`
    /// 只写 config.toml，live 登录零接触。判定用
    /// `codex_auth_has_credential_login_material`——`last_refresh` /
    /// `tokens.account_id` 等元数据残留不算登录，既不让 sk-+元数据形态的
    /// 备份逃过降级，也不把元数据残留的 live 误当官方登录。与
    /// `auth.json` 独立读取，避免损坏的 config.toml 掩盖有效登录。官方
    /// provider 缺失 auth 文件时也必须保留，因为它表示接管期间主动登出；
    /// 当前 provider 无法归类时沿用备份侧的保守策略，不回放未知来源的旧凭据。
    /// 与 `preserve_codex_auth_in_backup` 语义对称（那边保护备份方向），同样
    /// 不受"非接管切换保留官方登录"设置门控（接管子系统的既有不变量是
    /// 无条件不清官方登录）。
    fn preserve_codex_oauth_login_on_restore(
        &self,
        target: &mut Value,
    ) -> Result<CodexAuthFileSnapshot, String> {
        let auth_snapshot = CodexAuthFileSnapshot::capture()?;
        let live_auth = auth_snapshot.value()?;
        let live_has_login = live_auth.as_ref().is_some_and(|auth| {
            !Self::codex_auth_has_proxy_placeholder(auth)
                && crate::codex_config::codex_auth_has_credential_login_material(auth)
        });
        let Some(target_obj) = target.as_object_mut() else {
            return Ok(auth_snapshot);
        };

        if live_has_login {
            if let Some(config_text) = target_obj
                .get("config")
                .and_then(|value| value.as_str())
                .map(str::to_string)
            {
                let provider_auth = target_obj.get("auth").cloned().unwrap_or_else(|| json!({}));
                match crate::codex_config::prepare_codex_provider_live_config(
                    &provider_auth,
                    &config_text,
                ) {
                    Ok(live_config) => {
                        target_obj.insert("config".to_string(), json!(live_config));
                    }
                    Err(e) => {
                        // 降级失败时仍优先保住登录：API key 可从 DB 供应商配置随时
                        // 重新落盘，OAuth 登录被覆盖则只能重新登录。
                        log::warn!(
                            "Codex 恢复：备份 API key 降级进 config 失败，仅跳过 auth 覆盖: {e}"
                        );
                    }
                }
            }
            target_obj.remove("auth");
            log::info!(
                "Codex 恢复：live 持有官方 ChatGPT 登录凭据（恒比备份快照新），保留登录，仅恢复 config"
            );
            return Ok(auth_snapshot);
        }

        let missing_preservable_auth =
            live_auth.is_none() && self.should_preserve_current_codex_auth()?;
        if missing_preservable_auth {
            target_obj.remove("auth");
            log::info!("Codex 恢复：保留当前缺失的 auth 状态，仅恢复 config");
        }
        Ok(auth_snapshot)
    }

    /// 代理模式下切换供应商（热切换，并按需刷新代理安全的 Live 显示字段）
    pub async fn switch_proxy_target(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), String> {
        self.hot_switch_provider(app_type, provider_id)
            .await
            .map(|_| ())
    }

    // ==================== Live 配置读写辅助方法 ====================

    /// 接管 Codex 时，本地客户端必须继续以 Responses wire API 访问代理。
    /// 真实上游是否走 Chat Completions 由 provider 配置决定，并在代理内部转换。
    fn apply_codex_proxy_toml_config_for_provider(
        toml_str: &str,
        proxy_url: &str,
        provider: Option<&Provider>,
    ) -> Result<String, String> {
        if provider.is_some_and(crate::proxy::providers::is_codex_official_provider) {
            return crate::codex_config::apply_codex_official_proxy_route(toml_str, proxy_url)
                .map_err(|e| format!("生成 Codex 官方接管配置失败: {e}"));
        }

        let updated = crate::codex_config::update_codex_toml_field(toml_str, "base_url", proxy_url)
            .map_err(|e| format!("更新 Codex 代理地址失败: {e}"))?;
        let mut updated =
            crate::codex_config::update_codex_toml_field(&updated, "wire_api", "responses")
                .map_err(|e| format!("更新 Codex wire_api 失败: {e}"))?;

        if let Some(upstream_model) =
            provider.and_then(crate::proxy::providers::codex_provider_upstream_model)
        {
            updated =
                crate::codex_config::update_codex_toml_field(&updated, "model", &upstream_model)
                    .map_err(|e| format!("更新 Codex 上游模型失败: {e}"))?;
        }

        Ok(updated)
    }

    fn apply_codex_takeover_auth_placeholder(settings: &mut Value, provider: Option<&Provider>) {
        if provider.is_some_and(crate::proxy::providers::is_codex_official_provider) {
            return;
        }

        if let Some(auth) = settings.get_mut("auth").and_then(|v| v.as_object_mut()) {
            auth.insert("OPENAI_API_KEY".to_string(), json!(PROXY_TOKEN_PLACEHOLDER));
        } else if let Some(root) = settings.as_object_mut() {
            root.insert(
                "auth".to_string(),
                json!({ "OPENAI_API_KEY": PROXY_TOKEN_PLACEHOLDER }),
            );
        }
    }

    fn apply_codex_takeover_fields_for_provider(
        settings: &mut Value,
        proxy_base_url: &str,
        provider: &Provider,
    ) -> Result<(), String> {
        Self::apply_codex_takeover_auth_placeholder(settings, Some(provider));
        let config_text = settings
            .get("config")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let projected = Self::apply_codex_proxy_toml_config_for_provider(
            &config_text,
            proxy_base_url,
            Some(provider),
        )?;
        settings["config"] = json!(projected);
        Self::attach_codex_model_catalog_from_provider(settings, Some(provider));
        Ok(())
    }

    fn attach_codex_model_catalog_from_provider(
        live_config: &mut Value,
        provider: Option<&Provider>,
    ) {
        let Some(provider) = provider else {
            return;
        };

        let model_catalog = provider
            .settings_config
            .get("modelCatalog")
            .cloned()
            .unwrap_or_else(|| json!({ "models": [] }));

        if let Some(root) = live_config.as_object_mut() {
            root.insert("modelCatalog".to_string(), model_catalog);
        }
    }

    fn read_claude_live(&self) -> Result<Value, String> {
        let path = get_claude_settings_path();
        if !path.exists() {
            return Err("Claude 配置文件不存在".to_string());
        }

        let mut value: Value =
            read_json_file(&path).map_err(|e| format!("读取 Claude 配置失败: {e}"))?;

        if value.is_null() {
            value = json!({});
        }

        if !value.is_object() {
            let kind = match &value {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            return Err(format!(
                "Claude 配置文件格式错误：根节点必须是 JSON 对象（当前为 {kind}），路径: {}",
                path.display()
            ));
        }

        Ok(value)
    }

    fn write_claude_live(&self, config: &Value) -> Result<(), String> {
        let path = get_claude_settings_path();
        let settings = crate::services::provider::sanitize_claude_settings_for_live(config);
        write_json_file(&path, &settings).map_err(|e| format!("写入 Claude 配置失败: {e}"))
    }

    fn read_codex_live(&self) -> Result<Value, String> {
        crate::codex_config::read_codex_live_settings()
            .map_err(|e| format!("读取 Codex Live 配置失败: {e}"))
    }

    fn write_codex_live(&self, config: &Value) -> Result<(), String> {
        self.write_codex_live_verbatim(config)
    }

    fn write_codex_restore_backup(&self, config: &Value) -> Result<(), String> {
        let mut config = config.clone();
        let auth_snapshot = self.preserve_codex_oauth_login_on_restore(&mut config)?;
        self.write_codex_live_verbatim_with_auth_guard(&config, Some(&auth_snapshot))
    }

    fn write_codex_live_for_provider(
        &self,
        config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), String> {
        let Some(provider) = provider else {
            if crate::settings::preserve_codex_official_auth_on_switch() {
                if let (Some(auth), Some(config_str)) = (
                    config.get("auth"),
                    config.get("config").and_then(|v| v.as_str()),
                ) {
                    if auth.get("OPENAI_API_KEY").and_then(|v| v.as_str())
                        == Some(PROXY_TOKEN_PLACEHOLDER)
                    {
                        let live_config = crate::codex_config::prepare_codex_provider_live_config(
                            auth, config_str,
                        )
                        .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                        crate::codex_config::write_codex_live_config_atomic(Some(&live_config))
                            .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                        return Ok(());
                    }
                }
            }

            return self.write_codex_live_verbatim(config);
        };

        let auth = config
            .get("auth")
            .ok_or_else(|| "Codex 配置缺少 auth 字段".to_string())?;
        let config_str = config.get("config").and_then(|v| v.as_str());
        let profile = crate::proxy::providers::resolve_codex_catalog_tool_profile(provider);

        crate::codex_config::write_codex_provider_live_with_catalog(
            config,
            provider.category.as_deref(),
            auth,
            config_str,
            profile,
        )
        .map_err(|e| format!("写入 Codex 配置失败: {e}"))
    }

    fn codex_auth_has_proxy_placeholder(auth: &Value) -> bool {
        auth.get("OPENAI_API_KEY").and_then(|v| v.as_str()) == Some(PROXY_TOKEN_PLACEHOLDER)
    }

    /// The login state Codex will observe for `config_text`, as far as
    /// cc-switch can tell without touching the keyring: `Some(true)` signed
    /// in, `Some(false)` signed out, `None` undecidable. Which store Codex
    /// reads is decided first (`cli_auth_credentials_store`), and
    /// `auth.json` is only opened for the one mode that reads it:
    /// - `file` (the default): the file decides — see
    ///   `codex_auth_file_has_login`;
    /// - `ephemeral`: every process starts signed out, whatever is on disk;
    /// - `keyring`: Codex never opens the file (and deletes it after saving
    ///   to the keyring), so the file says nothing — undecidable;
    /// - `auto` (`AutoAuthStorage::load`): the keyring wins whenever it holds
    ///   anything and the file is only a fallback, so even a login in the
    ///   file cannot be ranked without reading the keyring — undecidable;
    /// - anything Codex would reject: undecidable.
    fn codex_live_login_state(config_text: &str) -> Option<bool> {
        use crate::codex_config::CodexAuthStoreMode;

        match crate::codex_config::codex_config_auth_store_mode(config_text) {
            CodexAuthStoreMode::File => Some(Self::codex_auth_file_has_login()),
            CodexAuthStoreMode::Ephemeral => Some(false),
            CodexAuthStoreMode::Keyring
            | CodexAuthStoreMode::Auto
            | CodexAuthStoreMode::Unknown => None,
        }
    }

    /// Whether the live `auth.json` holds a login Codex's file store would
    /// load. The takeover placeholder is not a login, and neither are
    /// Bedrock credentials or leftover metadata
    /// (`codex_auth_has_openai_account_material`). A file that is missing,
    /// unreadable or unparsable is "no stored auth" to Codex as well
    /// (`FileAuthStorage::load` fails and `AuthManager::load_auth` swallows
    /// it with `.ok()`), so it means signed out here and must never fail the
    /// takeover write.
    fn codex_auth_file_has_login() -> bool {
        let auth = match CodexAuthFileSnapshot::capture().and_then(|snapshot| snapshot.value()) {
            Ok(Some(auth)) => auth,
            Ok(None) => return false,
            Err(error) => {
                log::warn!("Codex auth.json 不可读，按未登录处理: {error}");
                return false;
            }
        };
        !Self::codex_auth_has_proxy_placeholder(&auth)
            && crate::codex_config::codex_auth_has_openai_account_material(&auth)
    }

    fn write_codex_takeover_live_for_provider(
        &self,
        config: &Value,
        provider: Option<&Provider>,
    ) -> Result<(), String> {
        let official_passthrough =
            provider.is_some_and(crate::proxy::providers::is_codex_official_provider);
        let managed_account_id = provider
            .and_then(|provider| provider.meta.as_ref())
            .and_then(|meta| meta.managed_account_id_for("codex_oauth"))
            .filter(|account_id| !account_id.trim().is_empty());
        let managed_official = official_passthrough && managed_account_id.is_some();
        let placeholder_auth = config
            .get("auth")
            .is_some_and(Self::codex_auth_has_proxy_placeholder);

        // Takeover must never overwrite Codex's long-lived ChatGPT login. For
        // third-party providers the placeholder is moved into config.toml; for
        // codex-official no placeholder is needed because requires_openai_auth
        // makes Codex supply its native authorization.
        if official_passthrough || placeholder_auth {
            let config_str = config.get("config").and_then(|v| v.as_str()).unwrap_or("");
            let profile = provider
                .map(crate::proxy::providers::resolve_codex_catalog_tool_profile)
                .unwrap_or(crate::codex_config::CodexCatalogToolProfile::ProxyChat);
            let prepared_config =
                crate::codex_config::prepare_codex_live_config_text_with_optional_catalog(
                    config, config_str, profile,
                )
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
            if managed_official {
                let auth = config
                    .get("auth")
                    .ok_or_else(|| "Codex 托管官方配置缺少 auth 字段".to_string())?;
                // An explicitly managed official account is different from the
                // unbound native-login passthrough: the selected account owns
                // auth.json and must replace any previously active account.
                crate::codex_config::write_codex_live_for_provider(
                    Some("official"),
                    auth,
                    Some(&prepared_config),
                )
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                crate::codex_config::record_codex_managed_oauth_live_auth(
                    auth,
                    managed_account_id
                        .as_deref()
                        .expect("managed official account checked"),
                )
                .map_err(|e| format!("记录 Codex 托管认证标记失败: {e}"))?;
                return Ok(());
            }
            let live_config = if official_passthrough {
                prepared_config
            } else {
                let injected = crate::codex_config::prepare_codex_provider_live_config(
                    config.get("auth").unwrap_or(&Value::Null),
                    &prepared_config,
                )
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
                // Takeover never touches auth.json, but it no longer owns the
                // file's presence: a preservation-off direct switch deletes
                // the login before takeover is enabled, and `codex logout`
                // can remove it mid-takeover. The stored card's
                // `requires_openai_auth` (presets carried `true` from the
                // pre-0.149 era) would then trap the TUI in the login screen
                // — Codex decides that screen from the flag and its account
                // probe alone, never from the bearer token — so stamp the
                // flag to the observed login state, exactly as the direct
                // switch does. When the state is undecidable from disk
                // (keyring-backed or auto stores) the card's flag is left
                // alone. Proxy-injected OAuth cards (xai_oauth, copilot) are
                // excluded outright: the effective snapshot already carries
                // the neutralized `false` (`neutralize_codex_proxy_oauth_fallback`)
                // because the official login is never their credential, and
                // a login on disk must not raise it back to `true`.
                let proxy_injected_oauth =
                    provider.is_some_and(Provider::uses_proxy_injected_oauth);
                let live_login_state = if proxy_injected_oauth {
                    None
                } else {
                    Self::codex_live_login_state(&injected)
                };
                match live_login_state {
                    Some(live_has_login) => {
                        crate::codex_config::align_codex_requires_openai_auth_with_login_preservation(
                            &injected,
                            live_has_login,
                        )
                        .map_err(|e| format!("写入 Codex 配置失败: {e}"))?
                    }
                    None => injected,
                }
            };
            crate::codex_config::write_codex_live_config_atomic(Some(&live_config))
                .map_err(|e| format!("写入 Codex 配置失败: {e}"))?;
            return Ok(());
        }

        self.write_codex_live_for_provider(config, provider)
    }

    fn write_codex_live_verbatim(&self, config: &Value) -> Result<(), String> {
        self.write_codex_live_verbatim_with_auth_guard(config, None)
    }

    fn write_codex_live_verbatim_with_auth_guard(
        &self,
        config: &Value,
        expected_auth: Option<&CodexAuthFileSnapshot>,
    ) -> Result<(), String> {
        use crate::codex_config::{get_codex_auth_path, get_codex_config_path};

        let auth = config.get("auth");
        let config_str = config.get("config").and_then(|v| v.as_str());

        let catalog_snapshot = if expected_auth.is_some()
            && config_str.is_some()
            && config.get("modelCatalog").is_some()
        {
            Some(
                crate::codex_config::CodexModelCatalogFileSnapshot::capture()
                    .map_err(|e| format!("捕获 Codex 模型目录失败: {e}"))?,
            )
        } else {
            None
        };

        // Decide the config.toml text ONCE, before splitting on auth. A stored
        // Codex backup comes in two shapes needing opposite handling:
        //  - snapshot backup (`read_codex_live_settings`): no inline `modelCatalog`;
        //    the config text already carries the live `model_catalog_json` pointer
        //    → keep raw, or projection would strip it.
        //  - provider-rebuilt backup (`update_live_backup_from_provider`): inline
        //    `modelCatalog` (DB SSOT) with a pointer-less config text → project,
        //    or the mapping is lost on restore.
        // The projection decision is orthogonal to auth: a provider-rebuilt backup
        // can pair an inline `modelCatalog` with empty/absent `auth.json` (the key
        // living in the config's `experimental_bearer_token`). Computing it up here
        // keeps every config-writing branch — write-auth, delete-auth, no-auth —
        // consistent instead of letting the empty-auth path skip projection.
        // Verbatim restore has no Provider in hand (we only have the stored
        // backup config), so the catalog tool profile can't be recovered here.
        // Default to ProxyChat: a restored native-direct backup keeps its inline
        // modelCatalog but would not get apply_patch re-stripped until the next
        // provider switch rewrites it via write_live_snapshot. Acceptable known
        // limitation (restore-of-deleted-provider-backup only).
        let prepared_cfg_result = config_str
            .map(|cfg| {
                crate::codex_config::prepare_codex_live_config_text_with_optional_catalog(
                    config,
                    cfg,
                    crate::codex_config::CodexCatalogToolProfile::ProxyChat,
                )
            })
            .transpose()
            .map_err(|e| format!("写入 Codex 配置失败: {e}"));
        let prepared_cfg = match prepared_cfg_result {
            Ok(prepared_cfg) => prepared_cfg,
            Err(error) => {
                if let Some(snapshot) = catalog_snapshot.as_ref() {
                    snapshot.restore().map_err(|rollback_error| {
                        format!("{error}; 回滚 Codex 模型目录失败: {rollback_error}")
                    })?;
                }
                return Err(error);
            }
        };

        let write_result = if let (Some(expected_auth), Some(auth)) = (expected_auth, auth) {
            (|| -> Result<(), String> {
                let replacement = if auth.as_object().is_some_and(Map::is_empty) {
                    None
                } else {
                    Some(
                        serde_json::to_vec_pretty(auth)
                            .map_err(|error| format!("序列化 Codex auth 失败: {error}"))?,
                    )
                };
                let mut transaction = CodexAuthFileTransaction::begin(expected_auth)?;
                if let Err(error) = transaction.install(replacement) {
                    return match transaction.rollback() {
                        Ok(()) => Err(error),
                        Err(rollback_error) => {
                            Err(format!("{error}; 回滚 Codex auth 失败: {rollback_error}"))
                        }
                    };
                }

                let config_result = prepared_cfg.as_deref().map_or(Ok(()), |cfg| {
                    crate::config::write_text_file(&get_codex_config_path(), cfg)
                        .map_err(|error| format!("写入 Codex config 失败: {error}"))
                });
                match config_result {
                    Ok(()) => transaction.commit(),
                    Err(error) => match transaction.rollback() {
                        Ok(()) => Err(error),
                        Err(rollback_error) => {
                            Err(format!("{error}; 回滚 Codex auth 失败: {rollback_error}"))
                        }
                    },
                }
            })()
        } else {
            match (auth, prepared_cfg.as_deref()) {
                (Some(auth), Some(cfg)) => {
                    if auth.as_object().is_some_and(Map::is_empty) {
                        // Unguarded provider writes preserve an existing login;
                        // only restore transactions interpret empty auth as an
                        // exact-generation deletion.
                        crate::config::write_text_file(&get_codex_config_path(), cfg)
                            .map_err(|e| format!("写入 Codex config 失败: {e}"))
                    } else {
                        crate::codex_config::write_codex_live_atomic(auth, Some(cfg))
                            .map_err(|e| format!("写入 Codex 配置失败: {e}"))
                    }
                }
                (Some(auth), None) => {
                    if auth.as_object().is_some_and(Map::is_empty) {
                        Ok(())
                    } else {
                        write_json_file(&get_codex_auth_path(), auth)
                            .map_err(|e| format!("写入 Codex auth 失败: {e}"))
                    }
                }
                (None, Some(cfg)) => crate::config::write_text_file(&get_codex_config_path(), cfg)
                    .map_err(|e| format!("写入 Codex config 失败: {e}")),
                (None, None) => Ok(()),
            }
        };

        if let Err(error) = write_result {
            if let Some(snapshot) = catalog_snapshot.as_ref() {
                snapshot.restore().map_err(|rollback_error| {
                    format!("{error}; 回滚 Codex 模型目录失败: {rollback_error}")
                })?;
            }
            return Err(error);
        }

        Ok(())
    }

    fn read_gemini_live(&self) -> Result<Value, String> {
        use crate::gemini_config::{env_to_json, get_gemini_env_path, read_gemini_env};

        let env_path = get_gemini_env_path();
        if !env_path.exists() {
            return Err("Gemini .env 文件不存在".to_string());
        }

        let env_map = read_gemini_env().map_err(|e| format!("读取 Gemini env 失败: {e}"))?;
        Ok(env_to_json(&env_map))
    }

    fn write_gemini_live(&self, config: &Value) -> Result<(), String> {
        use crate::gemini_config::{json_to_env, write_gemini_env_atomic};

        let env_map = json_to_env(config).map_err(|e| format!("转换 Gemini 配置失败: {e}"))?;
        write_gemini_env_atomic(&env_map).map_err(|e| format!("写入 Gemini env 失败: {e}"))?;
        Ok(())
    }

    fn read_grok_live(&self) -> Result<Value, String> {
        crate::grok_config::read_grok_live_settings()
            .map_err(|e| format!("读取 Grok Build 配置失败: {e}"))
    }

    fn write_grok_live(&self, config: &Value) -> Result<(), String> {
        crate::grok_config::write_grok_live_settings(config)
            .map_err(|e| format!("写入 Grok Build 配置失败: {e}"))
    }

    // ==================== 原有方法 ====================

    /// 获取服务器状态
    pub async fn get_status(&self) -> Result<ProxyStatus, String> {
        if let Some(server) = self.server.read().await.as_ref() {
            Ok(server.get_status().await)
        } else {
            // 服务器未运行时返回默认状态
            Ok(ProxyStatus {
                running: false,
                ..Default::default()
            })
        }
    }

    /// 获取代理配置
    pub async fn get_config(&self) -> Result<ProxyConfig, String> {
        self.db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))
    }

    /// 更新代理配置
    pub async fn update_config(&self, config: &ProxyConfig) -> Result<(), String> {
        // 记录旧配置用于判定是否需要重启
        let previous = self
            .db
            .get_proxy_config()
            .await
            .map_err(|e| format!("获取代理配置失败: {e}"))?;

        // 保存到数据库（保持 live_takeover_active 状态不变）
        let mut new_config = config.clone();
        new_config.live_takeover_active = previous.live_takeover_active;

        self.db
            .update_proxy_config(new_config.clone())
            .await
            .map_err(|e| format!("保存代理配置失败: {e}"))?;

        // 检查服务器当前状态
        let mut server_guard = self.server.write().await;
        if server_guard.is_none() {
            return Ok(());
        }

        // 判断是否需要重启（地址或端口变更）
        let require_restart = new_config.listen_address != previous.listen_address
            || new_config.listen_port != previous.listen_port;

        if require_restart {
            if let Some(server) = server_guard.take() {
                server
                    .stop()
                    .await
                    .map_err(|e| format!("重启前停止代理服务器失败: {e}"))?;
            }

            let app_handle = self.app_handle.read().await.clone();
            let new_server = ProxyServer::new(new_config.clone(), self.db.clone(), app_handle);
            let info = new_server
                .start()
                .await
                .map_err(|e| format!("重启代理服务器失败: {e}"))?;
            if let Err(e) = self
                .persist_ephemeral_listen_port_if_needed(&new_config, info.port)
                .await
            {
                let _ = new_server.stop().await;
                return Err(e);
            }

            *server_guard = Some(new_server);
            log::info!("代理配置已更新，服务器已自动重启应用最新配置");

            // Connection changes are exposed as suggestions; client files stay read-only.
            return Ok(());
        } else if let Some(server) = server_guard.as_ref() {
            server.apply_runtime_config(&new_config).await;
            log::info!("代理配置已实时应用，无需重启代理服务器");
        }

        Ok(())
    }

    /// 检查服务器是否正在运行
    pub async fn is_running(&self) -> bool {
        self.server.read().await.is_some()
    }

    /// 热更新熔断器配置
    ///
    /// 如果代理服务器正在运行，将新配置应用到所有已创建的熔断器实例
    pub async fn update_circuit_breaker_configs(
        &self,
        config: crate::proxy::CircuitBreakerConfig,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server.update_circuit_breaker_configs(config).await;
            log::info!("已热更新运行中的熔断器配置");
        } else {
            log::debug!("代理服务器未运行，熔断器配置将在下次启动时生效");
        }
        Ok(())
    }

    /// 热更新指定应用的熔断器配置
    pub async fn update_circuit_breaker_config_for_app(
        &self,
        app_type: &str,
        config: crate::proxy::CircuitBreakerConfig,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .update_circuit_breaker_config_for_app(app_type, config)
                .await;
            log::info!("已热更新 {app_type} 运行中的熔断器配置");
        } else {
            log::debug!("{app_type} 熔断器配置将在下次代理启动时生效");
        }
        Ok(())
    }

    /// 重置指定 Provider 的熔断器
    ///
    /// 如果代理服务器正在运行，立即重置内存中的熔断器状态
    pub async fn reset_provider_circuit_breaker(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Result<(), String> {
        if let Some(server) = self.server.read().await.as_ref() {
            server
                .reset_provider_circuit_breaker(provider_id, app_type)
                .await;
            log::info!("已重置 Provider {provider_id} (app: {app_type}) 的熔断器");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }

            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }

            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    async fn seed_distinct_app_proxy_configs(db: &Database) -> Vec<Value> {
        let mut configs = Vec::new();
        for (app, retries) in [("claude", 6), ("codex", 0), ("gemini", 2), ("grokbuild", 3)] {
            let mut config = db.get_proxy_config_for_app(app).await.unwrap();
            config.enabled = retries % 2 == 0;
            config.auto_failover_enabled = retries % 2 != 0;
            config.max_retries = retries;
            config.streaming_first_byte_timeout = 30 + retries;
            config.streaming_idle_timeout = 90 + retries;
            config.non_streaming_timeout = 300 + retries;
            config.circuit_failure_threshold = 5 + retries;
            configs.push(serde_json::to_value(&config).unwrap());
            db.update_proxy_config_for_app(config).await.unwrap();
        }
        configs
    }

    async fn assert_app_proxy_configs_unchanged(db: &Database, configs: &[Value]) {
        for expected in configs {
            let app = expected["appType"].as_str().unwrap();
            let actual = db.get_proxy_config_for_app(app).await.unwrap();
            assert_eq!(serde_json::to_value(actual).unwrap(), *expected, "{app}");
        }
    }

    #[tokio::test]
    #[serial]
    async fn shutdown_preserves_app_proxy_configs() {
        let _home = TempHome::new();
        crate::settings::reload_settings().unwrap();
        let db = Arc::new(Database::memory().unwrap());
        let configs = seed_distinct_app_proxy_configs(&db).await;
        let service = ProxyService::new(db.clone());

        let backup = json!({"env": {"ANTHROPIC_BASE_URL": "https://example.com"}});
        db.save_live_backup("claude", &backup.to_string())
            .await
            .unwrap();

        let mut config = db.get_global_proxy_config().await.unwrap();
        config.listen_port = 0;
        db.update_global_proxy_config(config).await.unwrap();
        service.start().await.unwrap();
        service.stop().await.unwrap();

        assert_app_proxy_configs_unchanged(&db, &configs).await;
        assert!(!get_claude_settings_path().exists());
        assert_eq!(
            db.get_live_backup("claude")
                .await
                .unwrap()
                .unwrap()
                .original_config,
            backup.to_string()
        );
    }

    #[tokio::test]
    async fn ephemeral_port_preserves_app_proxy_configs() {
        let db = Arc::new(Database::memory().unwrap());
        let configs = seed_distinct_app_proxy_configs(&db).await;
        let service = ProxyService::new(db.clone());
        let mut config = db.get_proxy_config().await.unwrap();
        config.listen_port = 0;
        let mut expected_global = db.get_global_proxy_config().await.unwrap();
        expected_global.listen_port = 23456;

        service
            .persist_ephemeral_listen_port_if_needed(&config, 23456)
            .await
            .unwrap();

        assert_app_proxy_configs_unchanged(&db, &configs).await;
        assert_eq!(
            serde_json::to_value(db.get_global_proxy_config().await.unwrap()).unwrap(),
            serde_json::to_value(expected_global).unwrap()
        );

        config.listen_port = 23456;
        service
            .persist_ephemeral_listen_port_if_needed(&config, 34567)
            .await
            .unwrap();
        assert_eq!(
            db.get_global_proxy_config().await.unwrap().listen_port,
            23456
        );
        assert_app_proxy_configs_unchanged(&db, &configs).await;
    }

    #[tokio::test]
    async fn unsupported_apps_are_rejected_before_proxy_side_effects() {
        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db);

        assert!(service.set_takeover_for_app("pi", true).await.is_err());
        assert!(!service.is_running().await);
        assert!(service.switch_proxy_target("pi", "missing").await.is_err());
    }

    #[tokio::test]
    #[serial]
    async fn hot_switch_provider_serializes_same_app_switches() {
        use tokio::time::{sleep, Duration};

        let _home = TempHome::new();
        crate::settings::reload_settings().expect("reload settings");

        let db = Arc::new(Database::memory().expect("init db"));
        let service = ProxyService::new(db.clone());

        let mut provider_a = Provider::with_id(
            "a".to_string(),
            "A".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "a-key" } }),
            None,
        );
        let mut provider_b = Provider::with_id(
            "b".to_string(),
            "B".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "b-key" } }),
            None,
        );
        let mut provider_c = Provider::with_id(
            "c".to_string(),
            "C".to_string(),
            json!({ "env": { "ANTHROPIC_API_KEY": "c-key" } }),
            None,
        );

        for provider in [&mut provider_a, &mut provider_b, &mut provider_c] {
            provider.meta = Some(crate::provider::ProviderMeta {
                provider_type: Some("github_copilot".into()),
                ..Default::default()
            });
        }

        db.save_provider("codex", &provider_a)
            .expect("save provider a");
        db.save_provider("codex", &provider_b)
            .expect("save provider b");
        db.save_provider("codex", &provider_c)
            .expect("save provider c");
        db.set_current_provider("codex", "a")
            .expect("set current provider");
        crate::settings::set_current_provider(&AppType::Codex, Some("a"))
            .expect("set local current provider");
        db.save_live_backup("codex", "{\"env\":{}}")
            .await
            .expect("seed live backup");

        let guard = service.lock_switch_for_test("codex").await;
        let service_for_b = service.clone();
        let service_for_c = service.clone();

        let switch_b = tokio::spawn(async move {
            service_for_b
                .hot_switch_provider("codex", "b")
                .await
                .expect("switch to b")
        });
        sleep(Duration::from_millis(20)).await;
        let switch_c = tokio::spawn(async move {
            service_for_c
                .hot_switch_provider("codex", "c")
                .await
                .expect("switch to c")
        });

        sleep(Duration::from_millis(20)).await;
        drop(guard);

        let outcome_b = switch_b.await.expect("join switch b");
        let outcome_c = switch_c.await.expect("join switch c");
        assert!(outcome_b.logical_target_changed);
        assert!(outcome_c.logical_target_changed);

        assert_eq!(
            crate::settings::get_effective_current_provider(&db, &AppType::Codex)
                .expect("effective current"),
            Some("c".to_string())
        );
        assert_eq!(
            crate::settings::get_current_provider(&AppType::Codex).as_deref(),
            Some("c")
        );
        assert_eq!(
            db.get_current_provider("codex").expect("db current"),
            Some("c".to_string())
        );

        let backup = db
            .get_live_backup("codex")
            .await
            .expect("get live backup")
            .expect("backup exists");
        assert_eq!(backup.original_config, "{\"env\":{}}");
    }
}
