//! `provider_list`, `provider_upsert` (local providers), `provider_set_default`, and the local
//! model commands `local_model_list` and `local_model_download`.
//!
//! Remote providers, secrets and authorization commands arrive with US7 (T140).

use std::collections::BTreeSet;

use devcopilot_application::provider_service::ProviderEntry;
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::provider::{
    DataCategory, Locality, ProviderAdapterKind, ProviderRole, ProviderSpec,
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

/// Creates or replaces a local provider.
///
/// # Errors
///
/// `invalid_input` for invalid fields or remote adapters; see [`provider_list`].
#[tauri::command]
pub async fn provider_upsert(
    state: State<'_, AppState>,
    request: ProviderUpsertDto,
) -> Result<ProviderDto, IpcError> {
    let core = state.core()?;
    let id = core.providers.upsert_local(request.into_spec()?)?.id();
    core.providers
        .list()?
        .into_iter()
        .find(|entry| entry.provider.id() == id)
        .map(Into::into)
        .ok_or_else(|| IpcError::from(devcopilot_application::ApplicationError::NotFound))
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
        IpcError::from(devcopilot_application::ApplicationError::Internal(
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
