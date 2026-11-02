//! Transcription and LLM provider adapters implementing the application ports.
//!
//! Vendor payloads stay inside this crate and never leak into domain types
//! (Constitution Art. VI.2). Transcript content and model output are untrusted data. Remote
//! adapters reach the network only after the `RemoteGate` allows each request (research R9).

pub mod anthropic;
pub mod factory;
pub mod model_download;
pub mod ollama;
pub mod openai_chat;
pub mod openai_stt;
pub mod output_validation;
pub mod prompt;
pub mod remote;
pub mod vad;
pub mod whisper_local;

pub use anthropic::{AnthropicAnalysisProvider, AnthropicChat};
pub use factory::{AnalysisAdapterFactory, TranscriberAdapterFactory};
pub use model_download::{DownloadError, ModelDownloader, ModelSpec};
pub use ollama::OllamaAnalysisProvider;
pub use openai_chat::{OpenAiChat, OpenAiChatAnalysisProvider};
pub use openai_stt::OpenAiSttTranscriber;
pub use remote::RemoteTarget;
pub use whisper_local::{WhisperTranscriber, WhisperTranscriberFactory};
