//! Parse token usage from OpenAI Responses and Chat Completions.

use serde::{Deserialize, Serialize};
use serde_json::Value;

fn openai_cache_read_tokens(usage: &Value) -> u32 {
    usage
        .get("cache_read_input_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cached_tokens"))
        .or_else(|| usage.pointer("/prompt_tokens_details/cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32
}

fn openai_cache_write_tokens(usage: &Value) -> u32 {
    usage
        .get("cache_creation_input_tokens")
        .or_else(|| usage.pointer("/input_tokens_details/cache_write_tokens"))
        .or_else(|| usage.pointer("/prompt_tokens_details/cache_write_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32
}

fn response_id(body: &Value, field: &str) -> Option<String> {
    body.get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Token 使用量统计
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_creation_tokens: u32,
    /// 从响应中提取的实际模型名称（如果可用）
    pub model: Option<String>,
    /// 从响应中提取的消息 ID（用于跨源去重）
    ///
    #[serde(skip)]
    pub message_id: Option<String>,
}

impl TokenUsage {
    /// Scope upstream response identities to the client and provider.
    pub fn dedup_request_id(&self, app_type: &str, provider_id: &str) -> String {
        self.message_id
            .as_ref()
            .map(|message_id| format!("session:{app_type}:{provider_id}:{message_id}"))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }

    /// 是否产生了任一计费维度的 token。
    ///
    /// 用于在写入前过滤全 0 的空 usage：当 OpenAI 兼容上游在流式下省略 usage 时，
    /// 转换器会合成一个全 0 的终止事件，若无 message_id 则 `dedup_request_id`
    /// 退化为随机 UUID，导致每笔请求插入一条无意义的空行、虚增请求数。
    pub fn has_billable_tokens(&self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_read_tokens > 0
            || self.cache_creation_tokens > 0
    }
}

impl TokenUsage {
    /// 从 Codex API 非流式响应解析
    pub fn from_codex_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage");
        if usage.is_none() {
            log::debug!(
                "[Codex] 响应中没有 usage 字段，body keys: {:?}",
                body.as_object().map(|o| o.keys().collect::<Vec<_>>())
            );
            return None;
        }
        let usage = usage?;

        let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64());
        let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64());

        if input_tokens.is_none() || output_tokens.is_none() {
            log::debug!("[Codex] usage 字段缺少 input_tokens 或 output_tokens，usage: {usage:?}");
            return None;
        }

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let cached_tokens = openai_cache_read_tokens(usage);
        let cache_write_tokens = openai_cache_write_tokens(usage);

        Some(Self {
            input_tokens: input_tokens? as u32,
            output_tokens: output_tokens? as u32,
            cache_read_tokens: cached_tokens,
            cache_creation_tokens: cache_write_tokens,
            model,
            message_id: response_id(body, "id"),
        })
    }

    /// 智能 Codex 响应解析 - 自动检测 OpenAI 或 Codex 格式
    ///
    /// Codex 支持两种 API 格式：
    /// - `/v1/responses`: 使用 input_tokens/output_tokens
    /// - `/v1/chat/completions`: 使用 prompt_tokens/completion_tokens (OpenAI 格式)
    ///
    /// 注意：记录原始 input_tokens，费用计算时再减去 cached_tokens
    pub fn from_codex_response_auto(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;

        // 检测格式：OpenAI 使用 prompt_tokens，Codex 使用 input_tokens
        if usage.get("prompt_tokens").is_some() {
            log::debug!("[Codex] 检测到 OpenAI 格式 (prompt_tokens)");
            Self::from_openai_response(body)
        } else if usage.get("input_tokens").is_some() {
            log::debug!("[Codex] 检测到 Codex 格式 (input_tokens)");
            // 使用非调整版本，记录原始 input_tokens
            Self::from_codex_response(body)
        } else {
            log::debug!("[Codex] 无法识别响应格式，usage: {usage:?}");
            None
        }
    }

    /// 智能 Codex 流式响应解析 - 自动检测 Codex Responses / Images / OpenAI 格式
    pub fn from_codex_stream_events_auto(events: &[Value]) -> Option<Self> {
        log::debug!("[Codex] 智能解析流式事件，共 {} 个事件", events.len());

        // Incomplete/failed native Responses can still report billable tokens.
        // Prefer the final reported usage without inventing any missing counts.
        for event in events.iter().rev() {
            if matches!(
                event.get("type").and_then(Value::as_str),
                Some("response.completed" | "response.incomplete" | "response.failed")
            ) {
                if let Some(usage) = event
                    .get("response")
                    .and_then(Self::from_codex_response_auto)
                {
                    return Some(usage);
                }
            }
        }

        // Images API 流式格式 (image_generation.completed 事件)：usage 直接挂在
        // 事件顶层，字段形态与 Codex 非流式响应一致；倒序取最后一个能按该形态
        // 解析的事件，跳过前面不含 usage 的 partial_image 事件。解析不成立时
        // 继续走下面的 OpenAI 回退，不改变既有路径
        if let Some(usage) = events
            .iter()
            .rev()
            .filter(|event| event.pointer("/usage/input_tokens").is_some())
            .find_map(Self::from_codex_response)
        {
            log::debug!("[Codex] 找到顶层 usage.input_tokens 事件");
            return Some(usage);
        }

        // 回退到 OpenAI Chat Completions 格式 (最后一个 chunk 包含 usage)
        log::debug!("[Codex] 尝试 OpenAI 流式格式");
        Self::from_openai_stream_events(events)
    }

    /// 从 OpenAI Chat Completions API 响应解析 (prompt_tokens, completion_tokens)
    pub fn from_openai_response(body: &Value) -> Option<Self> {
        let usage = body.get("usage")?;

        // OpenAI 使用 prompt_tokens 和 completion_tokens
        let prompt_tokens = usage.get("prompt_tokens").and_then(|v| v.as_u64())?;
        let completion_tokens = usage.get("completion_tokens").and_then(|v| v.as_u64())?;

        // 获取 cached_tokens (可能在 prompt_tokens_details 中)
        let cached_tokens = openai_cache_read_tokens(usage);
        let cache_write_tokens = openai_cache_write_tokens(usage);

        // 提取响应中的模型名称
        let model = body
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Some(Self {
            input_tokens: prompt_tokens as u32,
            output_tokens: completion_tokens as u32,
            cache_read_tokens: cached_tokens,
            cache_creation_tokens: cache_write_tokens,
            model,
            message_id: response_id(body, "id"),
        })
    }

    /// 从 OpenAI Chat Completions API 流式响应解析
    pub fn from_openai_stream_events(events: &[Value]) -> Option<Self> {
        log::debug!("[Codex] 解析 OpenAI 流式事件，共 {} 个事件", events.len());
        // OpenAI 流式响应在最后一个 chunk 中包含 usage
        for event in events.iter().rev() {
            if let Some(usage) = event.get("usage") {
                if !usage.is_null() {
                    log::debug!("[Codex] 找到 usage: {usage:?}");
                    let mut parsed = Self::from_openai_response(event)?;
                    if parsed.message_id.is_none() {
                        parsed.message_id =
                            events.iter().find_map(|chunk| response_id(chunk, "id"));
                    }
                    return Some(parsed);
                }
            }
        }
        log::debug!("[Codex] 未找到 usage 信息");
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn response_ids_produce_scoped_dedup_keys_and_empty_ids_fall_back() {
        let response = json!({
            "id": "resp_123",
            "model": "gpt-5.6",
            "usage": { "input_tokens": 10, "output_tokens": 2 }
        });
        let usage = TokenUsage::from_codex_response(&response).unwrap();
        assert_eq!(usage.message_id.as_deref(), Some("resp_123"));
        assert_eq!(
            usage.dedup_request_id("codex", "provider-a"),
            "session:codex:provider-a:resp_123"
        );

        let empty = json!({
            "id": "",
            "usage": { "input_tokens": 10, "output_tokens": 2 }
        });
        let empty_usage = TokenUsage::from_codex_response(&empty).unwrap();
        assert!(empty_usage.message_id.is_none());
        assert!(!empty_usage
            .dedup_request_id("codex", "provider-a")
            .starts_with("session:"));
    }

    #[test]
    fn test_has_billable_tokens_gates_empty_usage() {
        // 全 0 usage（如上游省略 usage 时合成的全 0 终止事件）不应计费——
        // 这是 Codex 流式空行多记修复（D）的闸门依据。
        assert!(!TokenUsage::default().has_billable_tokens());
        // 仅有 cache_read 也属于真实计费 token，必须计入。
        let only_cache = TokenUsage {
            cache_read_tokens: 100,
            ..Default::default()
        };
        assert!(only_cache.has_billable_tokens());
        let normal = TokenUsage {
            input_tokens: 10,
            output_tokens: 5,
            ..Default::default()
        };
        assert!(normal.has_billable_tokens());
    }

    #[test]
    fn test_codex_response_auto_returns_some_for_synthetic_all_zero() {
        // P3 回归：上游非流式 Chat 省略 usage 时转换器合成的全 0 usage，from_codex_response_auto
        // 仍返回 Some（字段存在、无 positivity check）——证明 handlers 必须用 has_billable_tokens
        // 闸门才能挡住空行，单靠 `if let Some` 不够。
        let synthetic = json!({
            "usage": { "input_tokens": 0, "output_tokens": 0, "total_tokens": 0 }
        });
        let usage = TokenUsage::from_codex_response_auto(&synthetic)
            .expect("全 0 usage 字段存在时 from_codex_response_auto 返回 Some");
        assert!(
            !usage.has_billable_tokens(),
            "全 0 usage 必须被 has_billable_tokens 判为非计费，由 handlers 闸门跳过"
        );
    }

    #[test]
    fn test_codex_response_parsing_cached_tokens_in_details() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response(&response).unwrap();
        // 非调整模式：input_tokens 保持原值，但应记录缓存命中
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
    }

    #[test]
    fn test_codex_response_parsing_cache_write_tokens_in_details() {
        let response = json!({
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300,
                    "cache_write_tokens": 200
                }
            }
        });

        let usage = TokenUsage::from_codex_response(&response).unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.cache_read_tokens, 300);
        assert_eq!(usage.cache_creation_tokens, 200);
    }

    // ============================================================================
    // 智能 Codex 解析测试
    // ============================================================================

    #[test]
    fn test_codex_response_auto_openai_format() {
        // OpenAI 格式 (prompt_tokens/completion_tokens)
        let response = json!({
            "model": "gpt-4o",
            "usage": {
                "prompt_tokens": 1000,
                "completion_tokens": 500,
                "prompt_tokens_details": {
                    "cached_tokens": 200
                }
            }
        });

        let usage = TokenUsage::from_codex_response_auto(&response).unwrap();
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.model, Some("gpt-4o".to_string()));
    }

    #[test]
    fn test_codex_response_auto_codex_format() {
        // Codex 格式 (input_tokens/output_tokens)
        let response = json!({
            "model": "gpt-5.4",
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 500,
                "input_tokens_details": {
                    "cached_tokens": 300
                }
            }
        });

        let usage = TokenUsage::from_codex_response_auto(&response).unwrap();
        // 记录原始 input_tokens，不调整
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 300);
        assert_eq!(usage.model, Some("gpt-5.4".to_string()));
    }

    #[test]
    fn test_codex_stream_events_auto_codex_format() {
        // Codex Responses API 流式格式 (response.completed 事件)
        let events = vec![
            json!({
                "type": "response.created",
                "response": {
                    "id": "resp_123"
                }
            }),
            json!({
                "type": "response.completed",
                "response": {
                    "model": "gpt-5.4",
                    "usage": {
                        "input_tokens": 1000,
                        "output_tokens": 500,
                        "input_tokens_details": {
                            "cached_tokens": 200
                        }
                    }
                }
            }),
        ];

        let usage = TokenUsage::from_codex_stream_events_auto(&events).unwrap();
        // 记录原始 input_tokens，不调整
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 500);
        assert_eq!(usage.cache_read_tokens, 200);
        assert_eq!(usage.model, Some("gpt-5.4".to_string()));
    }

    #[test]
    fn test_codex_stream_events_auto_openai_format() {
        // OpenAI Chat Completions 流式格式 (最后一个 chunk 包含 usage)
        let events = vec![
            json!({
                "id": "chatcmpl-123",
                "model": "gpt-4o",
                "choices": [{"delta": {"content": "Hello"}}]
            }),
            json!({
                "id": "chatcmpl-123",
                "model": "gpt-4o",
                "choices": [{"delta": {}}],
                "usage": {
                    "prompt_tokens": 100,
                    "completion_tokens": 50
                }
            }),
        ];

        let usage = TokenUsage::from_codex_stream_events_auto(&events).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.model, Some("gpt-4o".to_string()));
    }

    #[test]
    fn test_codex_stream_events_auto_image_generation_completed() {
        // Images API 流式格式：usage 挂在 image_generation.completed 事件顶层，
        // 字段形态与 Codex 非流式响应一致 (input_tokens / output_tokens)
        let events = vec![
            json!({
                "type": "image_generation.partial_image",
                "b64_json": "cGFydGlhbA==",
                "partial_image_index": 0
            }),
            json!({
                "type": "image_generation.completed",
                "b64_json": "aW1hZ2U=",
                "created_at": 1778832973,
                "usage": {
                    "input_tokens": 1474,
                    "input_tokens_details": {
                        "image_tokens": 1457,
                        "text_tokens": 17
                    },
                    "output_tokens": 1372,
                    "output_tokens_details": {
                        "image_tokens": 1372,
                        "text_tokens": 0
                    },
                    "total_tokens": 2846
                }
            }),
        ];

        let usage = TokenUsage::from_codex_stream_events_auto(&events)
            .expect("image_generation.completed usage should be parsed");
        assert_eq!(usage.input_tokens, 1474);
        assert_eq!(usage.output_tokens, 1372);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_creation_tokens, 0);
        assert_eq!(usage.model, None);
    }
}
