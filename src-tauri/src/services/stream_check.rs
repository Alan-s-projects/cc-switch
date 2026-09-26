//! Copilot endpoint reachability. Any HTTP response is reachable; no inference
//! or authentication probe is sent, and only transient timeouts are retried.
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Operational,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamCheckConfig {
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub degraded_threshold_ms: u64,
}

impl Default for StreamCheckConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 8,
            max_retries: 1,
            degraded_threshold_ms: 6000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamCheckResult {
    pub status: HealthStatus,
    pub success: bool,
    pub message: String,
    pub response_time_ms: Option<u64>,
    pub http_status: Option<u16>,
    // Retained in the local health-log format; reachability does not use a model.
    pub model_used: String,
    pub tested_at: i64,
    pub retry_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<String>,
}

pub struct StreamCheckService;

impl StreamCheckService {
    pub async fn check_with_retry(
        endpoint: &str,
        config: &StreamCheckConfig,
    ) -> Result<StreamCheckResult, AppError> {
        let endpoint = endpoint.trim();
        if endpoint.is_empty() {
            return Err(AppError::Message("Copilot endpoint is empty".into()));
        }
        let client = crate::proxy::http_client::get();
        for attempt in 0..=config.max_retries {
            let started = Instant::now();
            let response = client
                .get(endpoint)
                .timeout(Duration::from_secs(config.timeout_secs))
                .header("accept", "*/*")
                .header("accept-encoding", "identity")
                .send()
                .await;
            let result = response
                .map(|response| response.status().as_u16())
                .map_err(|error| {
                    if error.is_timeout() {
                        AppError::Message("Request timeout".into())
                    } else if error.is_connect() {
                        AppError::Message(format!("Connection failed: {}", error.without_url()))
                    } else {
                        AppError::Message(error.without_url().to_string())
                    }
                });
            let mut result = Self::build_result(
                result,
                started.elapsed().as_millis() as u64,
                config.degraded_threshold_ms,
            );
            result.retry_count = attempt;
            if result.success
                || !Self::should_retry(&result.message)
                || attempt == config.max_retries
            {
                return Ok(result);
            }
        }
        unreachable!("the inclusive attempt range always returns a result")
    }

    fn build_result(
        result: Result<u16, AppError>,
        latency_ms: u64,
        threshold_ms: u64,
    ) -> StreamCheckResult {
        let (status, success, message, http_status) = match result {
            Ok(code) => (
                Self::determine_status(latency_ms, threshold_ms),
                true,
                "Reachable".to_string(),
                Some(code),
            ),
            Err(error) => (HealthStatus::Failed, false, error.to_string(), None),
        };
        StreamCheckResult {
            status,
            success,
            message,
            http_status,
            response_time_ms: Some(latency_ms),
            model_used: String::new(),
            tested_at: chrono::Utc::now().timestamp(),
            retry_count: 0,
            error_category: None,
        }
    }

    fn determine_status(latency_ms: u64, threshold_ms: u64) -> HealthStatus {
        if latency_ms <= threshold_ms {
            HealthStatus::Operational
        } else {
            HealthStatus::Degraded
        }
    }

    fn should_retry(message: &str) -> bool {
        let message = message.to_lowercase();
        message.contains("timeout") || message.contains("abort") || message.contains("timed out")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_for_reachability_not_inference() {
        let config = StreamCheckConfig::default();
        assert_eq!(
            (
                config.timeout_secs,
                config.max_retries,
                config.degraded_threshold_ms
            ),
            (8, 1, 6000)
        );
    }

    #[test]
    fn any_http_status_is_reachable_without_claiming_authentication() {
        for code in [200, 401, 403, 404, 429, 500, 503] {
            let result = StreamCheckService::build_result(Ok(code), 80, 6000);
            assert!(result.success);
            assert_eq!(result.status, HealthStatus::Operational);
            assert_eq!(result.http_status, Some(code));
            assert!(result.model_used.is_empty());
        }
    }

    #[test]
    fn network_failure_is_not_reachable() {
        let result = StreamCheckService::build_result(
            Err(AppError::Message("Connection refused".into())),
            100,
            6000,
        );
        assert!(!result.success);
        assert_eq!(result.status, HealthStatus::Failed);
        assert_eq!(result.http_status, None);
    }

    #[test]
    fn latency_threshold_is_inclusive() {
        assert_eq!(
            StreamCheckService::determine_status(6000, 6000),
            HealthStatus::Operational
        );
        assert_eq!(
            StreamCheckService::determine_status(6001, 6000),
            HealthStatus::Degraded
        );
    }

    #[test]
    fn only_transient_timeout_messages_are_retried() {
        for message in ["Request timeout", "timed out", "Request aborted"] {
            assert!(StreamCheckService::should_retry(message));
        }
        for message in ["Connection refused", "DNS failure", "Certificate invalid"] {
            assert!(!StreamCheckService::should_retry(message));
        }
    }
}
