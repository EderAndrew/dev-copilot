//! Provider commands: `provider_list`, `provider_upsert`, `provider_set_default`, the remote
//! provider commands `provider_set_secret`, `provider_authorization_preview`,
//! `provider_authorize` and `provider_revoke` (US7), and the local model commands
//! `local_model_list` and `local_model_download`.
//!
//! Authorization is enforced in the core, never by the UI (Art. IV.11): these commands only
//! record what the user saw and accepted. Secrets go straight to the OS credential store and
//! are never returned, logged or echoed in errors (FR-049).

use std::collections::BTreeSet;

use devcopilot_application::ApplicationError;
use devcopilot_application::provider_service::{AuthorizationPreview, ProviderEntry};
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::provider::{
    AuthorizationState, AuthorizationStatus, DataCategory, Locality, ProviderAdapterKind,
    ProviderAuthorization, ProviderRole, ProviderSpec,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;

use super::IpcError;
use crate::composition::AppState;
use crate::dto::Code;
use crate::models::{LocalModelDto, LocalModels, ModelDownloadEvent};

/// Provider as listed to the UI; never contains secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProviderDto {
    pub id: String,
    pub role: Code<ProviderRole>,
    pub adapter: Code<ProviderAdapterKind>,
    pub locality: Code<Locality>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub model: String,
    pub data_categories: Vec<Code<DataCategory>>,
    pub is_default: bool,
    pub authorization: Code<AuthorizationStatus>,
    /// Whether an API key is stored in the OS credential store (the key itself never is
    /// returned).
    pub has_credential: bool,
}

/// `provider_upsert` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProviderUpsertDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub role: Code<ProviderRole>,
    pub adapter: Code<ProviderAdapterKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub model: String,
    pub data_categories: Vec<Code<DataCategory>>,
}

/// `provider_set_default` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProviderSetDefaultDto {
    pub role: Code<ProviderRole>,
    pub provider_id: String,
}

impl From<ProviderEntry> for ProviderDto {
    fn from(entry: ProviderEntry) -> Self {
        let provider = entry.provider;
        Self {
            id: provider.id().to_string(),
            role: Code(provider.role()),
            adapter: Code(provider.adapter()),
            locality: Code(provider.locality()),
            endpoint: provider.endpoint().map(ToString::to_string),
            model: provider.model().to_owned(),
            data_categories: provider
                .data_categories()
                .iter()
                .copied()
                .map(Code)
                .collect(),
            is_default: entry.is_default,
            authorization: Code(entry.authorization),
            has_credential: provider.credential_ref().is_some(),
        }
    }
}

/// `provider_set_secret` input. `Debug` never prints the secret.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProviderSetSecretDto {
    pub provider_id: String,
    pub secret: String,
}

impl std::fmt::Debug for ProviderSetSecretDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSetSecretDto")
            .field("provider_id", &self.provider_id)
            .field("secret", &"[redacted]")
            .finish()
    }
}

/// Input naming one provider (`provider_authorization_preview`, `provider_revoke`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProviderIdDto {
    pub provider_id: String,
}

/// `provider_authorize` input; `accepted` must be `true`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProviderAuthorizeDto {
    pub provider_id: String,
    pub accepted: bool,
}

/// What the authorization dialog shows before the user accepts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct AuthorizationPreviewDto {
    pub provider_id: String,
    pub destination: String,
    pub data_categories: Vec<Code<DataCategory>>,
    pub retention_notice: String,
}

impl From<AuthorizationPreview> for AuthorizationPreviewDto {
    fn from(preview: AuthorizationPreview) -> Self {
        Self {
            provider_id: preview.provider_id.to_string(),
            destination: preview.destination,
            data_categories: preview.data_categories.into_iter().map(Code).collect(),
            retention_notice: preview.retention_notice,
        }
    }
}

/// A granted or revoked authorization, as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ProviderAuthorizationDto {
    pub provider_id: String,
    pub state: Code<AuthorizationState>,
    pub destination: String,
    pub data_categories: Vec<Code<DataCategory>>,
    pub retention_notice: String,
    pub granted_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<i64>,
}

impl From<ProviderAuthorization> for ProviderAuthorizationDto {
    fn from(auth: ProviderAuthorization) -> Self {
        Self {
            provider_id: auth.provider_id().to_string(),
            state: Code(auth.state()),
            destination: auth.destination().to_owned(),
            data_categories: auth.data_categories().iter().copied().map(Code).collect(),
            retention_notice: auth.retention_notice().to_owned(),
            granted_at: auth.granted_at().unix_millis(),
            revoked_at: auth.revoked_at().map(|t| t.unix_millis()),
        }
    }
}

impl ProviderUpsertDto {
    /// Validates ids and builds the domain spec (field rules are checked by the domain).
    ///
    /// # Errors
    ///
    /// `invalid_input` for a malformed id.
    pub fn into_spec(self) -> Result<ProviderSpec, IpcError> {
        let id = match self.id.as_deref() {
            Some(raw) => ProviderId::parse(raw)?,
            None => ProviderId::new(),
        };
        Ok(ProviderSpec {
            id,
            role: self.role.0,
            adapter: self.adapter.0,
            endpoint: self.endpoint,
            model: self.model,
            credential_ref: None,
            data_categories: self
                .data_categories
                .into_iter()
                .map(|c| c.0)
                .collect::<BTreeSet<_>>(),
        })
    }
}

/// Every configured provider.
///
/// # Errors
///
/// `secret_store_unavailable` when storage could not be opened; `storage_error`.
#[tauri::command]
pub async fn provider_list(state: State<'_, AppState>) -> Result<Vec<ProviderDto>, IpcError> {
    let entries = state.core()?.providers.list()?;
    Ok(entries.into_iter().map(Into::into).collect())
}

/// Creates or replaces a provider. Remote endpoints must be `https`; changing a remote
/// provider's adapter, endpoint or categories revokes its authorization.
///
/// # Errors
///
/// `invalid_input` for invalid fields; see [`provider_list`].
#[tauri::command]
pub async fn provider_upsert(
    state: State<'_, AppState>,
    request: ProviderUpsertDto,
) -> Result<ProviderDto, IpcError> {
    let core = state.core()?;
    let id = core.providers.upsert(request.into_spec()?)?.id();
    Ok(core.providers.entry(id)?.into())
}

/// Longest secret accepted over IPC, in characters.
const SECRET_MAX_CHARS: usize = 8 * 1024;

/// Stores a remote provider's API key in the OS credential store.
///
/// # Errors
///
/// `invalid_input` for malformed ids, empty or oversized secrets and local providers;
/// `not_found`; `secret_store_unavailable`.
#[tauri::command]
pub async fn provider_set_secret(
    state: State<'_, AppState>,
    request: ProviderSetSecretDto,
) -> Result<(), IpcError> {
    let id = ProviderId::parse(&request.provider_id)?;
    if request.secret.chars().count() > SECRET_MAX_CHARS {
        return Err(IpcError::invalid_input("secret", "too_long"));
    }
    if request.secret.chars().any(char::is_control) {
        return Err(IpcError::invalid_input("secret", "control_character"));
    }
    let secret = Redacted::new(request.secret.trim().as_bytes().to_vec());
    state.core()?.providers.set_secret(id, secret)?;
    Ok(())
}

/// Destination, data categories and retention notice to show before authorizing.
///
/// # Errors
///
/// `invalid_input` for malformed ids and local providers; `not_found`.
#[tauri::command]
pub async fn provider_authorization_preview(
    state: State<'_, AppState>,
    request: ProviderIdDto,
) -> Result<AuthorizationPreviewDto, IpcError> {
    let id = ProviderId::parse(&request.provider_id)?;
    Ok(state.core()?.providers.authorization_preview(id)?.into())
}

/// Records the user's explicit acceptance of what the preview showed (FR-046).
///
/// # Errors
///
/// `invalid_input` unless `accepted` is `true`; see [`provider_authorization_preview`].
#[tauri::command]
pub async fn provider_authorize(
    state: State<'_, AppState>,
    request: ProviderAuthorizeDto,
) -> Result<ProviderAuthorizationDto, IpcError> {
    let id = ProviderId::parse(&request.provider_id)?;
    if !request.accepted {
        return Err(IpcError::invalid_input("accepted", "required"));
    }
    Ok(state.core()?.providers.authorize(id)?.into())
}

/// Revokes a provider's authorization; no new request is sent from now on (FR-047).
///
/// # Errors
///
/// `invalid_state` when it is not authorized; `not_found`.
#[tauri::command]
pub async fn provider_revoke(
    state: State<'_, AppState>,
    request: ProviderIdDto,
) -> Result<ProviderAuthorizationDto, IpcError> {
    let id = ProviderId::parse(&request.provider_id)?;
    Ok(state.core()?.providers.revoke(id)?.into())
}

/// Makes a provider the default for its role.
///
/// # Errors
///
/// `invalid_input` for malformed ids or role mismatch; `not_found`.
#[tauri::command]
pub async fn provider_set_default(
    state: State<'_, AppState>,
    request: ProviderSetDefaultDto,
) -> Result<(), IpcError> {
    let id = ProviderId::parse(&request.provider_id)?;
    state.core()?.providers.set_default(request.role.0, id)?;
    Ok(())
}

/// `local_model_download` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct LocalModelDownloadDto {
    pub model_id: String,
}

/// `local_model_download` output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct JobIdDto {
    pub job_id: String,
}

fn models(state: &AppState) -> Result<&std::sync::Arc<LocalModels>, IpcError> {
    state.models.as_ref().ok_or_else(|| {
        IpcError::from(ApplicationError::Internal(
            "app data directory unavailable".into(),
        ))
    })
}

/// Downloadable transcription models with origin, size and local state.
///
/// # Errors
///
/// `internal` without an app data directory.
#[tauri::command]
pub async fn local_model_list(state: State<'_, AppState>) -> Result<Vec<LocalModelDto>, IpcError> {
    Ok(models(&state)?.list())
}

/// Downloads a catalog model by explicit user action; progress arrives on `channel`.
///
/// # Errors
///
/// `invalid_input` for ids outside the catalog; `invalid_state` while it is downloading.
#[tauri::command]
pub async fn local_model_download(
    state: State<'_, AppState>,
    request: LocalModelDownloadDto,
    channel: Channel<ModelDownloadEvent>,
) -> Result<JobIdDto, IpcError> {
    let job_id = models(&state)?.start_download(&request.model_id, Box::new(channel))?;
    Ok(JobIdDto { job_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_parses_contract_payload() {
        let dto: ProviderUpsertDto = serde_json::from_str(
            r#"{"role":"analysis","adapter":"ollama","endpoint":"http://127.0.0.1:11434","model":"llama3","dataCategories":["transcript","records","transcript"]}"#,
        )
        .unwrap();
        let spec = dto.into_spec().unwrap();
        assert_eq!(spec.adapter, ProviderAdapterKind::Ollama);
        assert_eq!(spec.data_categories.len(), 2);
        assert!(spec.credential_ref.is_none());
    }

    #[test]
    fn secret_never_appears_in_debug_output() {
        let dto: ProviderSetSecretDto = serde_json::from_str(
            r#"{"providerId":"019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a7b","secret":"sk-very-secret"}"#,
        )
        .unwrap();
        assert!(!format!("{dto:?}").contains("sk-very-secret"));
    }

    #[test]
    fn authorize_payload_requires_accepted_flag() {
        assert!(
            serde_json::from_str::<ProviderAuthorizeDto>(
                r#"{"providerId":"019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a7b"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn upsert_rejects_bad_ids_and_unknown_values() {
        let bad_id = ProviderUpsertDto {
            id: Some("nope".into()),
            role: Code(ProviderRole::Transcription),
            adapter: Code(ProviderAdapterKind::WhisperLocal),
            endpoint: None,
            model: "m".into(),
            data_categories: vec![Code(DataCategory::Audio)],
        };
        let err = bad_id.into_spec().unwrap_err();
        assert_eq!(
            err.details.get("field").map(String::as_str),
            Some("provider_id")
        );
        assert!(
            serde_json::from_str::<ProviderUpsertDto>(
                r#"{"role":"analysis","adapter":"shell","model":"m","dataCategories":[]}"#
            )
            .is_err()
        );
        assert!(serde_json::from_str::<ProviderUpsertDto>(
            r#"{"role":"analysis","adapter":"ollama","model":"m","dataCategories":[],"secret":"x"}"#
        )
        .is_err());
    }
}
