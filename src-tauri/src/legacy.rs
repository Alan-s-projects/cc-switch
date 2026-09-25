//! Data shapes retained only for importing historical CC Switch databases.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRepo {
    pub owner: String,
    pub name: String,
    pub branch: String,
    pub enabled: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillState {
    pub installed: bool,
    #[serde(rename = "installedAt")]
    pub installed_at: chrono::DateTime<chrono::Utc>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillStore {
    pub skills: HashMap<String, SkillState>,
    pub repos: Vec<SkillRepo>,
}
