//! Select the single configured Copilot entry. Legacy failover queues are ignored.
use crate::{AppError, Database, Provider};
use std::sync::Arc;

pub struct ProviderRouter {
    db: Arc<Database>,
}
impl ProviderRouter {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
    pub async fn select_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        crate::copilot_bridge::require_codex(app_type)?;
        let id = crate::copilot_bridge::current(&self.db)?;
        let provider = self
            .db
            .get_provider_by_id(&id, "codex")?
            .ok_or(AppError::NoProvidersConfigured)?;
        crate::copilot_bridge::require_copilot(&provider)?;
        Ok(vec![provider])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ignores_legacy_failover_and_rejects_other_clients() {
        let db = Arc::new(Database::memory().unwrap());
        for id in ["atlas-primary", "atlas-legacy-secondary"] {
            let mut provider = Provider::with_id(id.into(), id.into(), serde_json::json!({}), None);
            provider.meta = Some(crate::ProviderMeta {
                provider_type: Some("github_copilot".into()),
                ..Default::default()
            });
            db.save_provider("codex", &provider).unwrap();
        }
        db.set_current_provider("codex", "atlas-primary").unwrap();
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "UPDATE proxy_config SET auto_failover_enabled = 1 WHERE app_type = 'codex'",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE providers SET in_failover_queue = 1 WHERE app_type = 'codex'",
                [],
            )
            .unwrap();
        }
        let router = ProviderRouter::new(db);
        let selected = router.select_providers("codex").await.unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id, "atlas-primary");
        assert!(router.select_providers("claude").await.is_err());
    }
}
