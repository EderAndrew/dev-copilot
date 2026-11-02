//! [`AnalysisProvider`] over the Anthropic Messages API (research R6), as an authorized remote
//! provider.
//!
//! `POST {endpoint}/v1/messages` with the system instructions in `system` and the delimited
//! session data as the only user message; no tools are declared (Art. IV.14). The model is
//! configurable. The reply is accepted only through the local schema validation
//! ([`crate::output_validation`]): the API's own JSON-schema output is not used because the
//! contract schema relies on length limits it does not support. A `refusal` stop is a provider
//! error and a truncated reply (`max_tokens`) is invalid output.

use devcopilot_application::ports::{BoxFuture, ProviderError};
use serde::{Deserialize, Serialize};

use crate::prompt::AnalysisPrompt;
use crate::remote::{
    ChatBackend, RemoteAnalysisProvider, RemoteTarget, http_error, read_json, secret_header,
    status_error, tokens,
};

/// API version sent in `anthropic-version`.
pub const API_VERSION: &str = "2026-10-21";
/// Output budget of one call; analysis replies are a few kilobytes of JSON.
pub const MAX_TOKENS: u32 = 16_000;

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: [Message<'a>; 1],
}

#[derive(Deserialize)]
struct MessagesReply {
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
}

/// Anthropic Messages API backend.
#[derive(Debug, Clone)]
pub struct AnthropicChat {
    target: RemoteTarget,
    model: String,
}

/// Analysis and questions through the Anthropic Messages API.
pub type AnthropicAnalysisProvider = RemoteAnalysisProvider<AnthropicChat>;

impl AnthropicChat {
    /// Backend for `target` using `model`.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Unavailable`] without an API key: the API always requires one.
    pub fn new(target: RemoteTarget, model: &str) -> Result<Self, ProviderError> {
        if target.api_key().is_none() {
            return Err(ProviderError::Unavailable);
        }
        Ok(Self {
            target,
            model: model.to_owned(),
        })
    }

    async fn send(&self, prompt: &AnalysisPrompt) -> Result<(String, u32), ProviderError> {
        let key = self.target.api_key().ok_or(ProviderError::Unavailable)?;
        let (name, value) = secret_header("x-api-key", key)?;
        let body = MessagesRequest {
            model: &self.model,
            max_tokens: MAX_TOKENS,
            system: &prompt.system,
            messages: [Message {
                role: "user",
                content: &prompt.data,
            }],
        };
        let response = self
            .target
            .client()
            .post(self.target.url("/v1/messages"))
            .header(name, value)
            .header("anthropic-version", API_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|err| http_error(&err))?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status));
        }
        let reply: MessagesReply = read_json(response).await?;
        match reply.stop_reason.as_deref() {
            Some("refusal") => return Err(ProviderError::Other("request refused".into())),
            Some("max_tokens") => return Err(ProviderError::InvalidOutput),
            _ => {}
        }
        let text: String = reply
            .content
            .into_iter()
            .filter(|block| block.kind == "text")
            .filter_map(|block| block.text)
            .collect();
        let usage = reply.usage.as_ref();
        Ok((
            text,
            tokens(
                usage.and_then(|u| u.input_tokens),
                usage.and_then(|u| u.output_tokens),
            ),
        ))
    }
}

impl ChatBackend for AnthropicChat {
    fn target(&self) -> &RemoteTarget {
        &self.target
    }

    fn complete<'a>(
        &'a self,
        prompt: &'a AnalysisPrompt,
    ) -> BoxFuture<'a, Result<(String, u32), ProviderError>> {
        Box::pin(self.send(prompt))
    }
}
