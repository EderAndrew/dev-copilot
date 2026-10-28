//! [`AnalysisProvider`] over an Ollama server the user started on this machine (research R6).
//!
//! The endpoint must be the loopback interface: Ollama is a *local* provider, so session content
//! never leaves the machine. The HTTP client ignores proxy settings and follows no redirects for
//! the same reason. Requests use `/api/chat` with the system instructions and the delimited data
//! as separate messages, `format` set to the output schema, temperature 0 and **no tools**
//! (Art. IV.14). The reply is accepted only through [`parse_analysis_output`].

use std::sync::Arc;
use std::time::Duration;

use devcopilot_application::ports::{
    AnalysisOutput, AnalysisProvider, AnalysisProviderFactory, AnalysisRequest, BoxFuture,
    ProviderDescriptor, ProviderError,
};
use devcopilot_domain::provider::{Locality, Provider, ProviderAdapterKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::output_validation::{parse_analysis_output, schema};
use crate::prompt::analysis_prompt;

/// Deadline of one analysis call; local models can be slow on modest hardware.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'static str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatOptions {
    temperature: f32,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    stream: bool,
    format: &'a Value,
    options: ChatOptions,
}

#[derive(Deserialize)]
struct ChatReply {
    message: ChatReplyMessage,
    #[serde(default)]
    prompt_eval_count: Option<u64>,
    #[serde(default)]
    eval_count: Option<u64>,
}

#[derive(Deserialize)]
struct ChatReplyMessage {
    content: String,
}

/// Analysis through a local Ollama server.
#[derive(Debug, Clone)]
pub struct OllamaAnalysisProvider {
    descriptor: ProviderDescriptor,
    chat_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaAnalysisProvider {
    /// Adapter for a configured Ollama provider.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Unavailable`] when `provider` is not an Ollama provider with a loopback
    /// endpoint, or the HTTP client cannot be built.
    pub fn new(provider: &Provider) -> Result<Self, ProviderError> {
        Self::with_timeout(provider, REQUEST_TIMEOUT)
    }

    /// As [`Self::new`] with a custom request deadline.
    ///
    /// # Errors
    ///
    /// See [`Self::new`].
    pub fn with_timeout(provider: &Provider, timeout: Duration) -> Result<Self, ProviderError> {
        let endpoint = provider
            .endpoint()
            .filter(|endpoint| endpoint.is_loopback())
            .ok_or(ProviderError::Unavailable)?;
        if provider.adapter() != ProviderAdapterKind::Ollama {
            return Err(ProviderError::Unavailable);
        }
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ProviderError::Unavailable)?;
        Ok(Self {
            descriptor: ProviderDescriptor {
                id: provider.id(),
                locality: Locality::Local,
                data_categories: provider.data_categories().iter().copied().collect(),
            },
            chat_url: format!("{endpoint}/api/chat"),
            model: provider.model().to_owned(),
            client,
        })
    }

    async fn call(&self, request: AnalysisRequest) -> Result<AnalysisOutput, ProviderError> {
        let prompt = analysis_prompt(&request);
        let body = ChatRequest {
            model: &self.model,
            messages: [
                ChatMessage {
                    role: "system",
                    content: &prompt.system,
                },
                ChatMessage {
                    role: "user",
                    content: &prompt.data,
                },
            ],
            stream: false,
            format: schema()?,
            options: ChatOptions { temperature: 0.0 },
        };
        let response = self
            .client
            .post(&self.chat_url)
            .json(&body)
            .send()
            .await
            .map_err(|err| http_error(&err))?;
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            // Model not pulled, or not an Ollama server.
            return Err(ProviderError::Unavailable);
        }
        if !status.is_success() {
            return Err(ProviderError::Other(format!(
                "http status {}",
                status.as_u16()
            )));
        }
        let reply: ChatReply = response.json().await.map_err(|err| {
            if err.is_timeout() {
                ProviderError::Timeout
            } else {
                ProviderError::InvalidOutput
            }
        })?;
        let proposals = parse_analysis_output(&reply.message.content)?;
        let tokens = reply
            .prompt_eval_count
            .unwrap_or(0)
            .saturating_add(reply.eval_count.unwrap_or(0));
        Ok(AnalysisOutput {
            proposals,
            tokens_used: u32::try_from(tokens).unwrap_or(u32::MAX),
        })
    }
}

fn http_error(err: &reqwest::Error) -> ProviderError {
    if err.is_timeout() {
        ProviderError::Timeout
    } else if err.is_connect() {
        ProviderError::Unavailable
    } else {
        // Without the URL: errors are logged and must not carry endpoint details.
        ProviderError::Other("request failed".into())
    }
}

impl AnalysisProvider for OllamaAnalysisProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    fn analyze(
        &self,
        request: AnalysisRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<AnalysisOutput, ProviderError>> {
        Box::pin(async move {
            tokio::select! {
                () = cancel.cancelled() => Err(ProviderError::Cancelled),
                result = self.call(request) => result,
            }
        })
    }
}

/// Builds analysis adapters for configured providers. Remote adapters arrive with US7.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnalysisAdapterFactory;

impl AnalysisProviderFactory for AnalysisAdapterFactory {
    fn create(&self, provider: &Provider) -> Result<Arc<dyn AnalysisProvider>, ProviderError> {
        match provider.adapter() {
            ProviderAdapterKind::Ollama => Ok(Arc::new(OllamaAnalysisProvider::new(provider)?)),
            _ => Err(ProviderError::Unavailable),
        }
    }
}
