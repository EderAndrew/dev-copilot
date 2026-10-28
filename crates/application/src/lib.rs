//! Dev Copilot use cases and the ports they depend on.
//!
//! Infrastructure crates implement these ports; they are wired at the composition root in
//! `apps/desktop/src-tauri` (Constitution Art. II.3).

pub mod analysis_service;
pub mod capture_pipeline;
pub mod error;
pub mod events;
pub mod ports;
pub mod provider_service;
pub mod record_service;
pub mod session_service;
pub mod settings_service;
pub mod transcription;

#[cfg(any(test, feature = "test-support"))]
pub mod testing;

pub use error::{ApplicationError, ErrorCode};
