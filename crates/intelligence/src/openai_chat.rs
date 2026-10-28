//! [`AnalysisProvider`] over an API compatible with OpenAI Chat Completions (research R6), as
//! an authorized remote provider.
//!
//! `POST {endpoint}/chat/completions` with a system message and the delimited session data as
//! the user message, `response_format = json_object` and no tools (Art. IV.14). The API key,
//! when stored, goes in `Authorization: Bearer`; self-hosted servers may need none. Replies are
//! accepted only through the local schema validation; a reply cut by the length limit is
//! invalid output.

use devcopilot_application::ports::{BoxFuture, ProviderError};
use serde::{Deserialize, Serialize};

use crate::prompt::AnalysisPrompt;
use crate::remote::{
    ChatBackend, RemoteAnalysisProvider, RemoteTarget, http_error, read_json, secret_header,
    status_error, tokens,
};

#[derive(Serialize)]
struct Message<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [Message<'a>; 2],
    response_format: ResponseFormat,
}

#[derive(Deserialize)]
struct ChatReply {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ReplyMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ReplyMessage {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
}

/// OpenAI-compatible Chat Completions backend.
#[derive(Debug, Clone)]
pub struct OpenAiChat {
    target: RemoteTarget,
    model: String,
}

/// Analysis and questions through an OpenAI-compatible Chat Completions API.
pub type OpenAiChatAnalysisProvider = RemoteAnalysisProvider<OpenAiChat>;

impl OpenAiChat {
    /// Backend for `target` using `model`.
    #[must_use]
    pub fn new(target: RemoteTarget, model: &str) -> Self {
        Self {
            target,
            model: model.to_owned(),
        }
    }

    async fn send(&self, prompt: &AnalysisPrompt) -> Result<(String, u32), ProviderError> {
        let body = ChatRequest {
            model: &self.model,
            messages: [
                Message {
                    role: "system",
                    content: &prompt.system,
                },
                Message {
                    role: "user",
                    content: &prompt.data,
                },
            ],
            response_format: ResponseFormat {
                kind: "json_object",
            },
        };
        let mut request = self
            .target
            .client()
            .post(self.target.url("/chat/completions"))
            .json(&body);
        if let Some(key) = self.target.api_key() {
            let (name, value) = secret_header("authorization", &format!("Bearer {key}"))?;
            request = request.header(name, value);
        }
        let response = request.send().await.map_err(|err| http_error(&err))?;
        let status = response.status();
        if !status.is_success() {
            return Err(status_error(status));
        }
        let reply: ChatReply = read_json(response).await?;
        let choice = reply
            .choices
            .into_iter()
            .next()
            .ok_or(ProviderError::InvalidOutput)?;
        if choice.finish_reason.as_deref() == Some("length") {
            return Err(ProviderError::InvalidOutput);
        }
        let content = choice.message.content.ok_or(ProviderError::InvalidOutput)?;
        let usage = reply.usage.as_ref();
        Ok((
            content,
            tokens(
                usage.and_then(|u| u.prompt_tokens),
                usage.and_then(|u| u.completion_tokens),
            ),
        ))
    }
}

impl ChatBackend for OpenAiChat {
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
