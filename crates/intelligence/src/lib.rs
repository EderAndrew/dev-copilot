//! Transcription and LLM provider adapters implementing the application ports.
//!
//! Vendor payloads stay inside this crate and never leak into domain types
//! (Constitution Art. VI.2). Transcript content and model output are untrusted data.

pub mod model_download;
pub mod ollama;
pub mod output_validation;
pub mod prompt;
pub mod vad;
pub mod whisper_local;

pub use model_download::{DownloadError, ModelDownloader, ModelSpec};
pub use ollama::{AnalysisAdapterFactory, OllamaAnalysisProvider};
pub use whisper_local::{WhisperTranscriber, WhisperTranscriberFactory};
