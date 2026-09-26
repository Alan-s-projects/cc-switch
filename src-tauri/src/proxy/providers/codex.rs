use serde_json::Value;

pub fn is_codex_responses_endpoint(endpoint: &str) -> bool {
    matches!(
        endpoint
            .split('?')
            .next()
            .unwrap_or(endpoint)
            .trim_end_matches('/'),
        "/responses" | "/v1/responses" | "/responses/compact" | "/v1/responses/compact"
    )
}

/// Preserve an explicit cache key; only a stable client session may supply a fallback.
pub fn inject_codex_chat_prompt_cache_key(
    body: &mut Value,
    explicit_key: Option<&str>,
    session_id: Option<&str>,
) {
    if let Some(key) = explicit_key
        .filter(|key| !key.trim().is_empty())
        .or_else(|| session_id.filter(|key| !key.trim().is_empty()))
    {
        body["prompt_cache_key"] = Value::String(key.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preserves_cache_affinity_without_inventing_session_keys() {
        let mut body = json!({});
        inject_codex_chat_prompt_cache_key(&mut body, Some("explicit"), Some("session"));
        assert_eq!(body["prompt_cache_key"], "explicit");
        let mut no_session = json!({});
        inject_codex_chat_prompt_cache_key(&mut no_session, None, None);
        assert!(no_session.get("prompt_cache_key").is_none());
        for blank in ["", " \t "] {
            let mut fallback = json!({});
            inject_codex_chat_prompt_cache_key(&mut fallback, Some(blank), Some("session"));
            assert_eq!(fallback["prompt_cache_key"], "session");
        }
    }
}
