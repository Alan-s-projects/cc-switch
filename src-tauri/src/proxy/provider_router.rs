//! Select the configured Copilot entry for Codex.
use crate::{AppError, Database, Provider};
use std::sync::Arc;

pub struct ProviderRouter {
    db: Arc<Database>,
}
impl ProviderRouter {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
    pub async fn select_provider(&self) -> Result<Provider, AppError> {
        let id = crate::copilot_bridge::current(&self.db)?;
        let provider = self
            .db
            .get_provider_by_id(&id, "codex")?
            .ok_or(AppError::NoProvidersConfigured)?;
        crate::copilot_bridge::require_copilot(&provider)?;
        Ok(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn selects_only_the_current_copilot() {
        let db = Arc::new(Database::memory().unwrap());
        for id in ["atlas-primary", "atlas-legacy-secondary"] {
            let mut provider = Provider::with_id(id.into(), id.into(), serde_json::json!({}));
            provider.meta = Some(crate::ProviderMeta {
                provider_type: Some("github_copilot".into()),
                ..Default::default()
            });
            db.save_provider("codex", &provider).unwrap();
        }
        db.set_current_provider("codex", "atlas-primary").unwrap();
        let router = ProviderRouter::new(db);
        let selected = router.select_provider().await.unwrap();
        assert_eq!(selected.id, "atlas-primary");
    }
}
