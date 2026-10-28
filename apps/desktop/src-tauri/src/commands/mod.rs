//! Thin IPC commands: deserialize, validate, delegate to a use case, map the result
//! (Constitution Art. VII.4, contracts/ipc-commands.md).
//!
//! Commands with input take a single `request` argument whose shape is the contract payload.

pub mod audio;
pub mod error;
pub mod providers;
pub mod questions;
pub mod records;
pub mod sessions;
pub mod settings;

pub use error::IpcError;
