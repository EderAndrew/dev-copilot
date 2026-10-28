//! Application errors with stable codes (contracts/ipc-commands.md `IpcError.code`).

use devcopilot_domain::DomainError;
use thiserror::Error;

use crate::ports::{AuthorizationError, ProviderError, RepositoryError, SecretStoreError};

/// Stable error code shared with the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// Input failed validation.
    InvalidInput,
    /// Referenced entity does not exist.
    NotFound,
    /// Operation not allowed in the current state.
    InvalidState,
    /// OS permission not granted.
    PermissionDenied,
    /// Audio source unavailable.
    SourceUnavailable,
    /// No usable transcription provider.
    NoTranscriber,
    /// OS credential store unavailable.
    SecretStoreUnavailable,
    /// Remote provider lacks authorization.
    AuthorizationRequired,
    /// Provider failure.
    ProviderError,
    /// Storage failure.
    StorageError,
    /// Unexpected failure.
    Internal,
}

impl ErrorCode {
    /// snake_case code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "invalid_input",
            Self::NotFound => "not_found",
            Self::InvalidState => "invalid_state",
            Self::PermissionDenied => "permission_denied",
            Self::SourceUnavailable => "source_unavailable",
            Self::NoTranscriber => "no_transcriber",
            Self::SecretStoreUnavailable => "secret_store_unavailable",
            Self::AuthorizationRequired => "authorization_required",
            Self::ProviderError => "provider_error",
            Self::StorageError => "storage_error",
            Self::Internal => "internal",
        }
    }

    /// i18n key of the user-facing message (`errors.<camelCase>`).
    #[must_use]
    pub const fn message_key(self) -> &'static str {
        match self {
            Self::InvalidInput => "errors.invalidInput",
            Self::NotFound => "errors.notFound",
            Self::InvalidState => "errors.invalidState",
            Self::PermissionDenied => "errors.permissionDenied",
            Self::SourceUnavailable => "errors.sourceUnavailable",
            Self::NoTranscriber => "errors.noTranscriber",
            Self::SecretStoreUnavailable => "errors.secretStoreUnavailable",
            Self::AuthorizationRequired => "errors.authorizationRequired",
            Self::ProviderError => "errors.providerError",
            Self::StorageError => "errors.storageError",
            Self::Internal => "errors.internal",
        }
    }
}

/// Error returned by use cases. Messages never contain session content or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApplicationError {
    /// Input failed validation on `field`.
    #[error("invalid input: {field} ({problem})")]
    InvalidInput {
        /// Offending field.
        field: &'static str,
        /// Problem code.
        problem: &'static str,
    },
    /// Referenced entity does not exist.
    #[error("not found")]
    NotFound,
    /// Operation not allowed in the current state.
    #[error("invalid state: {0}")]
    InvalidState(String),
    /// OS permission not granted.
    #[error("permission denied")]
    PermissionDenied,
    /// Audio source unavailable.
    #[error("source unavailable")]
    SourceUnavailable,
    /// No usable transcription provider (FR-045a).
    #[error("no usable transcriber")]
    NoTranscriber,
    /// OS credential store unavailable (FR-039a).
    #[error("secret store unavailable")]
    SecretStoreUnavailable,
    /// Remote provider lacks authorization (FR-046).
    #[error("authorization required")]
    AuthorizationRequired,
    /// Provider failure.
    #[error("provider error")]
    Provider(#[source] ProviderError),
    /// Storage failure.
    #[error("storage error")]
    Storage(#[source] RepositoryError),
    /// Unexpected failure.
    #[error("internal error: {0}")]
    Internal(String),
}

impl ApplicationError {
    /// Stable code for the IPC boundary.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidInput { .. } => ErrorCode::InvalidInput,
            Self::NotFound => ErrorCode::NotFound,
            Self::InvalidState(_) => ErrorCode::InvalidState,
            Self::PermissionDenied => ErrorCode::PermissionDenied,
            Self::SourceUnavailable => ErrorCode::SourceUnavailable,
            Self::NoTranscriber => ErrorCode::NoTranscriber,
            Self::SecretStoreUnavailable => ErrorCode::SecretStoreUnavailable,
            Self::AuthorizationRequired => ErrorCode::AuthorizationRequired,
            Self::Provider(_) => ErrorCode::ProviderError,
            Self::Storage(_) => ErrorCode::StorageError,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }
}

impl From<DomainError> for ApplicationError {
    fn from(err: DomainError) -> Self {
        match err {
            DomainError::InvalidInput { field, problem } => Self::InvalidInput {
                field,
                problem: problem.code(),
            },
            DomainError::InvalidTransition { from, action } => {
                Self::InvalidState(format!("{action} not allowed from {from}"))
            }
            DomainError::InvariantViolation(rule) => Self::InvalidState(rule.to_owned()),
        }
    }
}

impl From<RepositoryError> for ApplicationError {
    fn from(err: RepositoryError) -> Self {
        match err {
            RepositoryError::NotFound => Self::NotFound,
            RepositoryError::SecretStoreUnavailable => Self::SecretStoreUnavailable,
            other => Self::Storage(other),
        }
    }
}

impl From<SecretStoreError> for ApplicationError {
    fn from(err: SecretStoreError) -> Self {
        match err {
            SecretStoreError::Unavailable => Self::SecretStoreUnavailable,
            SecretStoreError::Backend(detail) => Self::Internal(detail),
        }
    }
}

impl From<ProviderError> for ApplicationError {
    fn from(err: ProviderError) -> Self {
        match err {
            ProviderError::AuthorizationRequired => Self::AuthorizationRequired,
            other => Self::Provider(other),
        }
    }
}

impl From<AuthorizationError> for ApplicationError {
    fn from(_: AuthorizationError) -> Self {
        Self::AuthorizationRequired
    }
}

#[cfg(test)]
mod tests {
    use devcopilot_domain::InputProblem;

    use super::*;

    #[test]
    fn maps_sources_to_stable_codes() {
        let cases: [(ApplicationError, &str); 8] = [
            (
                DomainError::invalid("title", InputProblem::TooLong).into(),
                "invalid_input",
            ),
            (
                DomainError::InvalidTransition {
                    from: "ended",
                    action: "start",
                }
                .into(),
                "invalid_state",
            ),
            (RepositoryError::NotFound.into(), "not_found"),
            (
                RepositoryError::SecretStoreUnavailable.into(),
                "secret_store_unavailable",
            ),
            (
                RepositoryError::Storage("disk full".into()).into(),
                "storage_error",
            ),
            (
                SecretStoreError::Unavailable.into(),
                "secret_store_unavailable",
            ),
            (
                ProviderError::AuthorizationRequired.into(),
                "authorization_required",
            ),
            (ProviderError::Timeout.into(), "provider_error"),
        ];
        for (err, code) in cases {
            assert_eq!(err.code().as_str(), code, "{err:?}");
        }
    }

    #[test]
    fn invalid_input_keeps_field_and_problem() {
        let err: ApplicationError = DomainError::invalid("title", InputProblem::Empty).into();
        assert_eq!(
            err,
            ApplicationError::InvalidInput {
                field: "title",
                problem: "empty"
            }
        );
    }

    #[test]
    fn message_keys_follow_convention() {
        assert_eq!(
            ErrorCode::SecretStoreUnavailable.message_key(),
            "errors.secretStoreUnavailable"
        );
    }
}
