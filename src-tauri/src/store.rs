use crate::{services::ProxyService, Database};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
}
impl AppState {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            proxy_service: ProxyService::new(db.clone()),
            db,
        }
    }
}
