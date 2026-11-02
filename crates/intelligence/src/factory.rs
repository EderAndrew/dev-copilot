//! Adapter factories: map a configured provider to its adapter. Remote adapters get the
//! [`RemoteGate`] and read their API key from the OS credential store (FR-049); building one
//! sends nothing.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use devcopilot_application::ports::{
    AnalysisProvider, AnalysisProviderFactory, ProviderError, RemoteGate, SecretStore, Transcriber,
    TranscriberFactory,
};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{Provider, ProviderAdapterKind};

use crate::anthropic::{AnthropicAnalysisProvider, AnthropicChat};
use crate::ollama::OllamaAnalysisProvider;
use crate::openai_chat::{OpenAiChat, OpenAiChatAnalysisProvider};
use crate::openai_stt::OpenAiSttTranscriber;
use crate::remote::{REQUEST_TIMEOUT, RemoteTarget};
use crate::whisper_local::WhisperTranscriberFactory;

/// Deadline of one remote transcription upload (≤ 25 s of audio).
pub const STT_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Builds transcribers: `whisper_local` and `openai_compatible_stt`.
pub struct TranscriberAdapterFactory {
    whisper: WhisperTranscriberFactory,
    gate: Arc<dyn RemoteGate>,
    secrets: Arc<dyn SecretStore>,
}

impl TranscriberAdapterFactory {
    /// Factory reading local models from `models_dir`.
    #[must_use]
    pub fn new(
        models_dir: impl Into<PathBuf>,
        gate: Arc<dyn RemoteGate>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            whisper: WhisperTranscriberFactory::new(models_dir),
            gate,
            secrets,
        }
    }
}

impl TranscriberFactory for TranscriberAdapterFactory {
    fn create(
        &self,
        provider: &Provider,
        language: SessionLanguage,
    ) -> Result<Arc<dyn Transcriber>, ProviderError> {
        match provider.adapter() {
            ProviderAdapterKind::WhisperLocal => self.whisper.create(provider, language),
            ProviderAdapterKind::OpenAiCompatibleStt => {
                let target = RemoteTarget::new(
                    provider,
                    self.secrets.as_ref(),
                    Arc::clone(&self.gate),
                    STT_REQUEST_TIMEOUT,
                )?;
                Ok(Arc::new(OpenAiSttTranscriber::new(
                    target,
                    provider.model(),
                    language,
                )))
            }
            _ => Err(ProviderError::Unavailable),
        }
    }
}

/// Builds analysis adapters: `ollama`, `anthropic` and `openai_compatible_chat`.
pub struct AnalysisAdapterFactory {
    gate: Arc<dyn RemoteGate>,
    secrets: Arc<dyn SecretStore>,
}

impl AnalysisAdapterFactory {
    /// Factory whose remote adapters check `gate` and read keys from `secrets`.
    #[must_use]
    pub fn new(gate: Arc<dyn RemoteGate>, secrets: Arc<dyn SecretStore>) -> Self {
        Self { gate, secrets }
    }

    fn target(&self, provider: &Provider) -> Result<RemoteTarget, ProviderError> {
        RemoteTarget::new(
            provider,
            self.secrets.as_ref(),
            Arc::clone(&self.gate),
            REQUEST_TIMEOUT,
        )
    }
}

impl AnalysisProviderFactory for AnalysisAdapterFactory {
    fn create(&self, provider: &Provider) -> Result<Arc<dyn AnalysisProvider>, ProviderError> {
        match provider.adapter() {
            ProviderAdapterKind::Ollama => Ok(Arc::new(OllamaAnalysisProvider::new(provider)?)),
            ProviderAdapterKind::Anthropic => Ok(Arc::new(AnthropicAnalysisProvider::new(
                AnthropicChat::new(self.target(provider)?, provider.model())?,
            ))),
            ProviderAdapterKind::OpenAiCompatibleChat => {
                Ok(Arc::new(OpenAiChatAnalysisProvider::new(OpenAiChat::new(
                    self.target(provider)?,
                    provider.model(),
                ))))
            }
            ProviderAdapterKind::WhisperLocal | ProviderAdapterKind::OpenAiCompatibleStt => {
                Err(ProviderError::Unavailable)
            }
        }
    }
}
