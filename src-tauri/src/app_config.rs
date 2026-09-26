use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::error::AppError;

/// The bridge exposes one client protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppType {
    Codex,
}

impl AppType {
    pub fn as_str(&self) -> &'static str {
        "codex"
    }
}

impl FromStr for AppType {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.trim().eq_ignore_ascii_case("codex") {
            Ok(Self::Codex)
        } else {
            Err(AppError::InvalidInput(format!(
                "Unsupported application {value:?}. Allowed value: codex."
            )))
        }
    }
}
