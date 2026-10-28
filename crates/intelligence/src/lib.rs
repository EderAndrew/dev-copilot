//! Transcription and LLM provider adapters implementing the application ports.
//!
//! Vendor payloads stay inside this crate and never leak into domain types
//! (Constitution Art. VI.2). Transcript content and model output are untrusted data.
