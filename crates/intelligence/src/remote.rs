//! Shared plumbing of the remote adapters (research R5, R6, R9).
//!
//! Every remote request goes through [`RemoteTarget::authorize`] first: the [`RemoteGate`] is
//! asked for exactly the categories the request carries, and a refusal returns
//! [`ProviderError::AuthorizationRequired`] before any byte is sent (SC-013). The HTTP client
//! accepts `https` only, verifies TLS (`rustls`) and follows no redirects, so a request can
//! never end up at a destination the user did not authorize. Errors never carry the URL, the
//! API key or response bodies.

use std::sync::Arc;
use std::time::Duration;

use devcopilot_application::ports::{
    AnalysisOutput, AnalysisProvider, AnalysisRequest, AnswerOutput, BoxFuture, ProviderDescriptor,
    ProviderError, QuestionRequest, RemoteGate, SecretStore,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::provider::{DataCategory, Locality, Provider};
use reqwest::header::{HeaderName, HeaderValue};
use tokio_util::sync::CancellationToken;

use crate::output_validation::{parse_analysis_output, parse_answer_output};
use crate::prompt::{AnalysisPrompt, analysis_prompt, answer_prompt};

/// Deadline of one remote analysis or question call.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Destination, credentials and gate of one configured remote provider.
#[derive(Clone)]
pub struct RemoteTarget {
    descriptor: ProviderDescriptor,
    base_url: String,
    api_key: Option<Redacted<String>>,
    gate: Arc<dyn RemoteGate>,
    client: reqwest::Client,
}

impl std::fmt::Debug for RemoteTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteTarget")
            .field("provider", &self.descriptor.id)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .finish_non_exhaustive()
    }
}

impl RemoteTarget {
    /// Target for a configured remote provider; its API key is read from `secrets`.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Unavailable`] for local providers, when the credential store cannot be
    /// read or holds a key that is not UTF-8, or when the HTTP client cannot be built.
    pub fn new(
        provider: &Provider,
        secrets: &dyn SecretStore,
        gate: Arc<dyn RemoteGate>,
        timeout: Duration,
    ) -> Result<Self, ProviderError> {
        let endpoint = provider
            .endpoint()
            .filter(|_| provider.locality() == Locality::Remote)
            .ok_or(ProviderError::Unavailable)?;
        let api_key = match provider.credential_ref() {
            Some(reference) => secrets
                .get(reference)
                .map_err(|_| ProviderError::Unavailable)?
                .map(|secret| {
                    String::from_utf8(secret.expose().clone())
                        .map(|key| Redacted::new(key.trim().to_owned()))
                        .map_err(|_| ProviderError::Unavailable)
                })
                .transpose()?
                .filter(|key| !key.expose().is_empty()),
            None => None,
        };
        Self::build(provider, endpoint.to_string(), api_key, gate, timeout, true)
    }

    /// Target pointing at `base_url` (plain `http` allowed), for tests against a local fake
    /// server.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Unavailable`] when the HTTP client cannot be built.
    #[cfg(feature = "test-support")]
    pub fn for_test(
        provider: &Provider,
        base_url: &str,
        api_key: Option<&str>,
        gate: Arc<dyn RemoteGate>,
        timeout: Duration,
    ) -> Result<Self, ProviderError> {
        Self::build(
            provider,
            base_url.trim_end_matches('/').to_owned(),
            api_key.map(|key| Redacted::new(key.to_owned())),
            gate,
            timeout,
            false,
        )
    }

    fn build(
        provider: &Provider,
        base_url: String,
        api_key: Option<Redacted<String>>,
        gate: Arc<dyn RemoteGate>,
        timeout: Duration,
        https_only: bool,
    ) -> Result<Self, ProviderError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .https_only(https_only)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| ProviderError::Unavailable)?;
        Ok(Self {
            descriptor: ProviderDescriptor {
                id: provider.id(),
                locality: Locality::Remote,
                data_categories: provider.data_categories().iter().copied().collect(),
            },
            base_url,
            api_key,
            gate,
            client,
        })
    }

    /// Non-sensitive description.
    #[must_use]
    pub fn descriptor(&self) -> &ProviderDescriptor {
        &self.descriptor
    }

    /// Whether the provider is configured to receive `category`.
    #[must_use]
    pub fn declares(&self, category: DataCategory) -> bool {
        self.descriptor.data_categories.contains(&category)
    }

    /// `base_url` followed by `path`.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    /// The API key, if one is stored.
    #[must_use]
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_ref().map(|key| key.expose().as_str())
    }

    /// The HTTP client.
    #[must_use]
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Asks the gate for `categories`; must be called right before each request.
    ///
    /// # Errors
    ///
    /// [`ProviderError::AuthorizationRequired`] when no granted authorization covers them.
    pub fn authorize(&self, categories: &[DataCategory]) -> Result<(), ProviderError> {
        self.gate
            .check(&self.descriptor.id, categories)
            .map_err(|_| ProviderError::AuthorizationRequired)
    }
}

/// Header carrying a secret; marked sensitive so it is never printed by the HTTP stack.
///
/// # Errors
///
/// [`ProviderError::Unavailable`] when the value is not a valid header value.
pub fn secret_header(
    name: &'static str,
    value: &str,
) -> Result<(HeaderName, HeaderValue), ProviderError> {
    let mut header = HeaderValue::from_str(value).map_err(|_| ProviderError::Unavailable)?;
    header.set_sensitive(true);
    Ok((HeaderName::from_static(name), header))
}

/// Drops from `request` what the provider was not configured to receive, and returns the
/// categories the rest carries (FR-046: only declared categories are sent).
pub fn strip_analysis(target: &RemoteTarget, request: &mut AnalysisRequest) -> Vec<DataCategory> {
    let mut categories = vec![DataCategory::Transcript];
    if target.declares(DataCategory::Records) {
        if !request.records.is_empty() {
            categories.push(DataCategory::Records);
        }
    } else {
        request.records.clear();
    }
    categories
}

/// As [`strip_analysis`] for a question.
pub fn strip_question(target: &RemoteTarget, request: &mut QuestionRequest) -> Vec<DataCategory> {
    let mut categories = vec![DataCategory::Questions];
    if target.declares(DataCategory::Transcript) {
        if !request.segments.is_empty() {
            categories.push(DataCategory::Transcript);
        }
    } else {
        request.segments.clear();
    }
    if target.declares(DataCategory::Records) {
        if !request.records.is_empty() {
            categories.push(DataCategory::Records);
        }
    } else {
        request.records.clear();
    }
    categories
}

/// Maps a transport failure without the URL.
#[must_use]
pub fn http_error(err: &reqwest::Error) -> ProviderError {
    if err.is_timeout() {
        ProviderError::Timeout
    } else if err.is_connect() {
        ProviderError::Unavailable
    } else {
        ProviderError::Other("request failed".into())
    }
}

/// Maps a non-success HTTP status.
#[must_use]
pub fn status_error(status: reqwest::StatusCode) -> ProviderError {
    match status.as_u16() {
        408 | 504 => ProviderError::Timeout,
        code => ProviderError::Other(format!("http status {code}")),
    }
}

/// Reads a JSON body, mapping a malformed one to [`ProviderError::InvalidOutput`].
///
/// # Errors
///
/// [`ProviderError::Timeout`], [`ProviderError::InvalidOutput`].
pub async fn read_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ProviderError> {
    response.json().await.map_err(|err| {
        if err.is_timeout() {
            ProviderError::Timeout
        } else {
            ProviderError::InvalidOutput
        }
    })
}

/// Saturating sum of token counts.
#[must_use]
pub fn tokens(prompt: Option<u64>, completion: Option<u64>) -> u32 {
    u32::try_from(prompt.unwrap_or(0).saturating_add(completion.unwrap_or(0))).unwrap_or(u32::MAX)
}

/// One chat call of a remote LLM API: system instructions and delimited data in, reply text
/// and tokens used out. Implementations offer the model no tools (Art. IV.14).
pub trait ChatBackend: Send + Sync {
    /// Destination, credentials and gate.
    fn target(&self) -> &RemoteTarget;

    /// Sends `prompt`; the gate has already been checked.
    fn complete<'a>(
        &'a self,
        prompt: &'a AnalysisPrompt,
    ) -> BoxFuture<'a, Result<(String, u32), ProviderError>>;
}

/// [`AnalysisProvider`] over a remote [`ChatBackend`]: strips undeclared categories, checks
/// the gate before every call and accepts only schema-valid output.
#[derive(Debug)]
pub struct RemoteAnalysisProvider<B> {
    backend: B,
}

impl<B: ChatBackend> RemoteAnalysisProvider<B> {
    /// Wraps `backend`.
    #[must_use]
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }

    async fn call(&self, mut request: AnalysisRequest) -> Result<AnalysisOutput, ProviderError> {
        let categories = strip_analysis(self.backend.target(), &mut request);
        self.backend.target().authorize(&categories)?;
        let (content, tokens_used) = self.backend.complete(&analysis_prompt(&request)).await?;
        Ok(AnalysisOutput {
            proposals: parse_analysis_output(&content)?,
            tokens_used,
        })
    }

    async fn ask(&self, mut request: QuestionRequest) -> Result<AnswerOutput, ProviderError> {
        let categories = strip_question(self.backend.target(), &mut request);
        self.backend.target().authorize(&categories)?;
        let (content, tokens_used) = self.backend.complete(&answer_prompt(&request)).await?;
        Ok(AnswerOutput {
            answer: parse_answer_output(&content)?,
            tokens_used,
        })
    }
}

impl<B: ChatBackend> AnalysisProvider for RemoteAnalysisProvider<B> {
    fn descriptor(&self) -> ProviderDescriptor {
        self.backend.target().descriptor().clone()
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

    fn answer(
        &self,
        request: QuestionRequest,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<AnswerOutput, ProviderError>> {
        Box::pin(async move {
            tokio::select! {
                () = cancel.cancelled() => Err(ProviderError::Cancelled),
                result = self.ask(request) => result,
            }
        })
    }
}
