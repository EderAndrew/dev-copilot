//! Dev Copilot domain: entities, value objects, state machines and invariants.
//!
//! This crate is pure business logic and must not depend on Tauri, platform audio APIs,
//! databases or AI SDKs (Constitution Art. II.2).

/// Implements `code()`/`parse()` for a fieldless enum with stable snake_case codes.
macro_rules! code_enum {
    ($ty:ident, $field:literal, { $($variant:ident => $code:literal),+ $(,)? }) => {
        impl $ty {
            /// Stable snake_case code used over IPC and in storage.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self { $(Self::$variant => $code),+ }
            }

            /// Parses a code produced by [`Self::code`].
            ///
            /// # Errors
            ///
            /// Returns [`crate::DomainError::InvalidInput`] for unknown codes.
            pub fn parse(code: &str) -> Result<Self, $crate::error::DomainError> {
                match code {
                    $($code => Ok(Self::$variant),)+
                    _ => Err($crate::error::DomainError::invalid(
                        $field,
                        $crate::error::InputProblem::Malformed,
                    )),
                }
            }
        }
    };
}

pub mod analysis_run;
pub mod audio_source;
pub mod error;
pub mod export;
pub mod ids;
pub mod language;
pub mod provider;
pub mod question;
pub mod record;
pub mod record_change;
pub mod redacted;
pub mod session;
pub mod text;
pub mod time;
pub mod transcript;

pub use error::{DomainError, InputProblem};
pub use redacted::Redacted;
