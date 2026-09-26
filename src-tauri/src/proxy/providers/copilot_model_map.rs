//! Exact GPT model and endpoint selection from the authenticated Copilot catalog.
use super::copilot_auth::CopilotModel;
use crate::provider::CodexCopilotApiFormat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopilotProtocol {
    Responses,
    Chat,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotTransport {
    pub protocol: CopilotProtocol,
    pub endpoint: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCopilotModel {
    pub id: String,
    pub transport: Option<CopilotTransport>,
}

pub fn is_gpt_model(model: &str) -> bool {
    let model = model.trim();
    model.len() > 4
        && model
            .get(..4)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("gpt-"))
}

pub fn resolve_model_with_format(
    client_id: &str,
    models: &[CopilotModel],
    api_format: CodexCopilotApiFormat,
) -> Option<ResolvedCopilotModel> {
    if !is_gpt_model(client_id) {
        return None;
    }
    let model = models
        .iter()
        .find(|model| model.id.eq_ignore_ascii_case(client_id.trim()))?;
    Some(ResolvedCopilotModel {
        id: model.id.clone(),
        transport: transport_for(model, api_format),
    })
}

fn transport_for(
    model: &CopilotModel,
    api_format: CodexCopilotApiFormat,
) -> Option<CopilotTransport> {
    let supported = |expected: &[&str]| {
        model.supported_endpoints.iter().find_map(|endpoint| {
            let path = endpoint
                .split_once('?')
                .map_or(endpoint.as_str(), |(path, _)| path)
                .trim_end_matches('/');
            expected
                .iter()
                .any(|candidate| path.eq_ignore_ascii_case(candidate))
                .then(|| path.to_string())
        })
    };

    let protocols: &[CopilotProtocol] = match api_format {
        CodexCopilotApiFormat::Auto => &[CopilotProtocol::Responses, CopilotProtocol::Chat],
        CodexCopilotApiFormat::OpenaiResponses => &[CopilotProtocol::Responses],
        CodexCopilotApiFormat::OpenaiChat => &[CopilotProtocol::Chat],
    };
    protocols.iter().find_map(|&protocol| {
        let paths: &[&str] = match protocol {
            CopilotProtocol::Responses => &["/responses", "/v1/responses"],
            CopilotProtocol::Chat => &["/chat/completions", "/v1/chat/completions"],
        };
        supported(paths).map(|endpoint| CopilotTransport { protocol, endpoint })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn model(id: &str, endpoints: &[&str]) -> CopilotModel {
        CopilotModel {
            id: id.into(),
            name: id.into(),
            vendor: "OpenAI".into(),
            supported_endpoints: endpoints.iter().map(|value| value.to_string()).collect(),
            ..Default::default()
        }
    }
    #[test]
    fn selects_only_the_requested_gpt_and_an_advertised_protocol() {
        let models = vec![
            model("gpt-test", &["/responses", "/chat/completions"]),
            model("other-model", &["/responses"]),
        ];
        let resolved =
            resolve_model_with_format("GPT-TEST", &models, CodexCopilotApiFormat::Auto).unwrap();
        assert_eq!(resolved.id, "gpt-test");
        assert_eq!(
            resolved.transport.unwrap().protocol,
            CopilotProtocol::Responses
        );
        assert!(
            resolve_model_with_format("gpt-missing", &models, CodexCopilotApiFormat::Auto)
                .is_none()
        );
        assert!(
            resolve_model_with_format("other-model", &models, CodexCopilotApiFormat::Auto)
                .is_none()
        );
        let chat = vec![model("gpt-chat", &["/chat/completions"])];
        assert!(resolve_model_with_format(
            "gpt-chat",
            &chat,
            CodexCopilotApiFormat::OpenaiResponses
        )
        .unwrap()
        .transport
        .is_none());
        assert_eq!(
            resolve_model_with_format("gpt-chat", &chat, CodexCopilotApiFormat::Auto)
                .unwrap()
                .transport
                .unwrap()
                .protocol,
            CopilotProtocol::Chat
        );
    }
}
