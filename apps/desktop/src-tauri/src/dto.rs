//! Shared IPC DTO helpers (contracts/ipc-commands.md conventions).
//!
//! Domain enums cross the boundary as their stable snake_case codes through [`Code`], so
//! deserialization validates values with the same rules as the domain.

use std::fmt;

use devcopilot_domain::DomainError;
use devcopilot_domain::audio_source::{AudioSourceKind, PermissionState, UnavailableReason};
use devcopilot_domain::export::{ExportContent, ExportFormat};
use devcopilot_domain::language::{SessionLanguage, UiLanguage};
use devcopilot_domain::provider::{
    AuthorizationState, AuthorizationStatus, DataCategory, Locality, ProviderAdapterKind,
    ProviderRole,
};
use devcopilot_domain::question::{Certainty, QuestionStatus};
use devcopilot_domain::record::{Nature, RecordKind, RecordStatus};
use devcopilot_domain::session::{FailureReason, SessionState};
use devcopilot_domain::transcript::GapReason;
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};

/// Domain enum with a stable textual code.
pub trait CodeEnum: Sized + Copy {
    /// Stable code.
    fn code(self) -> &'static str;
    /// Parses a code.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] for unknown codes.
    fn parse(code: &str) -> Result<Self, DomainError>;
}

macro_rules! code_enum {
    ($($ty:ty),+ $(,)?) => {$(
        impl CodeEnum for $ty {
            fn code(self) -> &'static str {
                <$ty>::code(self)
            }

            fn parse(code: &str) -> Result<Self, DomainError> {
                <$ty>::parse(code)
            }
        }
    )+};
}

code_enum!(
    AudioSourceKind,
    AuthorizationState,
    AuthorizationStatus,
    Certainty,
    DataCategory,
    ExportContent,
    ExportFormat,
    FailureReason,
    GapReason,
    Locality,
    Nature,
    PermissionState,
    ProviderAdapterKind,
    ProviderRole,
    QuestionStatus,
    RecordKind,
    RecordStatus,
    SessionLanguage,
    SessionState,
    UiLanguage,
    UnavailableReason,
);

/// A domain enum (de)serialized as its code.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Code<T>(pub T);

impl<T: CodeEnum> fmt::Debug for Code<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.code())
    }
}

impl<T: CodeEnum> Serialize for Code<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.code())
    }
}

impl<'de, T: CodeEnum> Deserialize<'de> for Code<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        T::parse(&raw)
            .map(Code)
            .map_err(|_| de::Error::custom("unknown code"))
    }
}

impl<T> From<T> for Code<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_through_json() {
        let json = serde_json::to_string(&Code(SessionLanguage::PtBr)).unwrap();
        assert_eq!(json, "\"pt-BR\"");
        let back: Code<SessionLanguage> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.0, SessionLanguage::PtBr);
        assert!(serde_json::from_str::<Code<AudioSourceKind>>("\"speaker\"").is_err());
    }
}
