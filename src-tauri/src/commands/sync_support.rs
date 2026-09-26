use crate::error::AppError;
use crate::services::model_pricing;
use crate::settings;
use crate::store::AppState;

pub(crate) fn run_post_restore_sync(app_state: &AppState) -> Result<(), AppError> {
    let mut failures = Vec::new();

    if let Err(error) = crate::copilot_bridge::initialize(app_state) {
        failures.push(format!("Copilot bridge: {error}"));
    }
    if let Err(error) = model_pricing::sync_local_model_pricing(&app_state.db) {
        failures.push(format!("model pricing: {error}"));
    }
    if let Err(error) = settings::reload_settings() {
        failures.push(format!("settings cache: {error}"));
    }

    match app_state.db.get_log_config() {
        Ok(log_config) => log::set_max_level(log_config.to_level_filter()),
        Err(error) => {
            log::set_max_level(log::LevelFilter::Info);
            failures.push(format!("runtime log level: {error}"));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "Post-restore synchronization failed: {}",
            failures.join("; ")
        )))
    }
}

fn post_sync_warning<E: std::fmt::Display>(err: E) -> String {
    format!("Post-operation synchronization failed: {err}")
}

pub(crate) fn post_sync_warning_from_result(
    result: Result<Result<(), AppError>, String>,
) -> Option<String> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(err)) => Some(post_sync_warning(err)),
        Err(err) => Some(post_sync_warning(err)),
    }
}

#[cfg(test)]
mod tests {
    use super::post_sync_warning_from_result;

    #[test]
    fn post_sync_warning_from_result_returns_none_on_success() {
        let warning = post_sync_warning_from_result(Ok(Ok(())));
        assert!(warning.is_none());
    }

    #[test]
    fn post_sync_warning_from_result_returns_some_on_sync_error() {
        let warning =
            post_sync_warning_from_result(Ok(Err(crate::error::AppError::Config("boom".into()))));
        assert!(warning.is_some());
    }

    #[tokio::test]
    async fn post_sync_warning_from_result_returns_some_on_join_error() {
        let handle = tokio::spawn(async move {
            panic!("forced join error");
        });
        let join_err = handle.await.expect_err("task should panic");
        let warning = post_sync_warning_from_result(Err(join_err.to_string()));
        assert!(warning.is_some());
    }
}
