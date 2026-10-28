//! `IpcError` (contracts/ipc-commands.md): stable code, i18n key and non-sensitive details.

use std::collections::BTreeMap;

use devcopilot_application::{ApplicationError, ErrorCode};
use devcopilot_domain::DomainError;
use serde::{Deserialize, Serialize};

/// Error returned by every command. Never carries session content, paths or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IpcError {
    /// Stable code.
    pub code: String,
    /// i18n key of the user-facing message.
    pub message_key: String,
    /// Non-sensitive details (e.g. `{ field: "title" }`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

impl IpcError {
    fn new(code: ErrorCode) -> Self {
        Self {
            code: code.as_str().to_owned(),
            message_key: code.message_key().to_owned(),
            details: BTreeMap::new(),
        }
    }

    /// `invalid_input` for a request field that failed DTO validation.
    #[must_use]
    pub fn invalid_input(field: &str, problem: &str) -> Self {
        let mut err = Self::new(ErrorCode::InvalidInput);
        err.details.insert("field".into(), field.to_owned());
        err.details.insert("problem".into(), problem.to_owned());
        err
    }
}

impl From<ApplicationError> for IpcError {
    fn from(err: ApplicationError) -> Self {
        match &err {
            ApplicationError::InvalidInput { field, problem } => {
                Self::invalid_input(field, problem)
            }
            ApplicationError::AuthorizationRequired {
                provider: Some(provider),
            } => {
                let mut ipc = Self::new(err.code());
                ipc.details
                    .insert("providerId".into(), provider.to_string());
                ipc
            }
            ApplicationError::Storage(_) | ApplicationError::Internal(_) => {
                // Details stay in the local log; the UI only gets the code.
                tracing::warn!(code = err.code().as_str(), error = %err, "command failed");
                Self::new(err.code())
            }
            _ => Self::new(err.code()),
        }
    }
}

impl From<DomainError> for IpcError {
    fn from(err: DomainError) -> Self {
        ApplicationError::from(err).into()
    }
}

#[cfg(test)]
mod tests {
    use devcopilot_application::ports::RepositoryError;

    use super::*;

    #[test]
    fn invalid_input_lists_field_and_problem() {
        let err = IpcError::from(ApplicationError::InvalidInput {
            field: "title",
            problem: "too_long",
        });
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "code": "invalid_input",
                "messageKey": "errors.invalidInput",
                "details": { "field": "title", "problem": "too_long" }
            })
        );
    }

    #[test]
    fn storage_details_never_reach_the_frontend() {
        let err = IpcError::from(ApplicationError::Storage(RepositoryError::Storage(
            "disk I/O error at /Users/me/secret.db".into(),
        )));
        assert_eq!(err.code, "storage_error");
        assert!(err.details.is_empty());
        assert!(!serde_json::to_string(&err).unwrap().contains("/Users"));
    }

    #[test]
    fn authorization_required_names_the_provider() {
        let provider = devcopilot_domain::ids::ProviderId::new();
        let err = IpcError::from(ApplicationError::AuthorizationRequired {
            provider: Some(provider),
        });
        assert_eq!(err.code, "authorization_required");
        assert_eq!(
            err.details.get("providerId").map(String::as_str),
            Some(provider.to_string().as_str())
        );
    }

    #[test]
    fn codes_follow_application_errors() {
        assert_eq!(
            IpcError::from(ApplicationError::SecretStoreUnavailable).message_key,
            "errors.secretStoreUnavailable"
        );
        assert_eq!(IpcError::from(ApplicationError::NotFound).code, "not_found");
    }
}
