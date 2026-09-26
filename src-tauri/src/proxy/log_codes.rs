//! Diagnostic identifiers used by the listener and response processor.
pub mod srv {
    pub const ACCEPT_ERR: &str = "SRV-005";
    pub const CONN_ERR: &str = "SRV-006";
    pub const STARTED: &str = "SRV-001";
    pub const STOPPED: &str = "SRV-002";
    pub const STOP_TIMEOUT: &str = "SRV-003";
    pub const TASK_ERROR: &str = "SRV-004";
}
