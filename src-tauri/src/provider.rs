use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Atlas owns this provider record; it never writes it to Codex configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(rename = "settingsConfig")]
    pub settings_config: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ProviderMeta>,
}

impl Provider {
    pub fn with_id(id: String, name: String, settings_config: Value) -> Self {
        Self {
            id,
            name,
            settings_config,
            meta: None,
        }
    }

    pub fn is_github_copilot(&self) -> bool {
        self.meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref())
            == Some("github_copilot")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthBindingSource {
    #[default]
    ProviderConfig,
    ManagedAccount,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthBinding {
    #[serde(default)]
    pub source: AuthBindingSource,
    #[serde(rename = "authProvider", skip_serializing_if = "Option::is_none")]
    pub auth_provider: Option<String>,
    #[serde(rename = "accountId", skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodexCopilotApiFormat {
    #[default]
    Auto,
    OpenaiResponses,
    OpenaiChat,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderMeta {
    #[serde(rename = "providerType", skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    #[serde(
        rename = "codexCopilotApiFormat",
        skip_serializing_if = "Option::is_none"
    )]
    pub codex_copilot_api_format: Option<CodexCopilotApiFormat>,
    #[serde(rename = "authBinding", skip_serializing_if = "Option::is_none")]
    pub auth_binding: Option<AuthBinding>,
    /// Read compatibility for a saved Copilot account binding.
    #[serde(rename = "githubAccountId", skip_serializing_if = "Option::is_none")]
    pub github_account_id: Option<String>,
}

impl ProviderMeta {
    pub fn managed_account_id_for(&self, auth_provider: &str) -> Option<String> {
        if auth_provider != "github_copilot" {
            return None;
        }
        if let Some(binding) = &self.auth_binding {
            if binding.source == AuthBindingSource::ManagedAccount
                && binding.auth_provider.as_deref() == Some(auth_provider)
            {
                return binding.account_id.clone();
            }
        }
        self.github_account_id.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn explicit_default_account_does_not_restore_a_legacy_account_binding() {
        let meta: ProviderMeta = serde_json::from_value(json!({
            "githubAccountId": "old-account",
            "authBinding": {
                "source": "managed_account",
                "authProvider": "github_copilot"
            }
        }))
        .unwrap();
        assert_eq!(meta.managed_account_id_for("github_copilot"), None);
    }

    #[test]
    fn existing_copilot_metadata_keeps_its_protocol_and_account() {
        let meta: ProviderMeta = serde_json::from_value(json!({
            "providerType": "github_copilot",
            "githubAccountId": "account",
            "codexCopilotApiFormat": "openai_responses",
            "retiredFeature": true
        }))
        .unwrap();
        assert_eq!(
            meta.managed_account_id_for("github_copilot").as_deref(),
            Some("account")
        );
        assert_eq!(
            meta.codex_copilot_api_format,
            Some(CodexCopilotApiFormat::OpenaiResponses)
        );
        assert!(serde_json::to_value(&meta)
            .unwrap()
            .get("retiredFeature")
            .is_none());
    }
}
