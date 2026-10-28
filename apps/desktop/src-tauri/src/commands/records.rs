//! `record_set_status` (contracts/ipc-commands.md "Registros e plano") and the `RecordDto`
//! shared with the `record_proposed` event.
//!
//! The actor of every change made here is the user (FR-024); the AI never reaches this path.

use devcopilot_domain::ids::RecordId;
use devcopilot_domain::record::{Nature, Origin, RecordKind, RecordStatus, TechnicalRecord};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::IpcError;
use crate::composition::{AppState, Core};
use crate::dto::Code;

/// `RecordDto.origin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum OriginDto {
    Ai,
    User,
}

/// A technical record as sent to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct RecordDto {
    pub id: String,
    pub session_id: String,
    pub kind: Code<RecordKind>,
    pub text: String,
    pub nature: Code<Nature>,
    pub status: Code<RecordStatus>,
    pub rationale: String,
    pub origin: OriginDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis_run_id: Option<String>,
    /// Ids of the segments cited as evidence.
    pub evidence: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub alternatives: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflicts_with: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<String>,
    /// Unix epoch milliseconds.
    pub created_at: i64,
    /// Unix epoch milliseconds.
    pub updated_at: i64,
}

impl From<&TechnicalRecord> for RecordDto {
    fn from(record: &TechnicalRecord) -> Self {
        let (origin, provider_id, analysis_run_id) = match record.origin() {
            Origin::Ai {
                provider_id,
                analysis_run_id,
            } => (
                OriginDto::Ai,
                Some(provider_id.to_string()),
                Some(analysis_run_id.to_string()),
            ),
            Origin::User => (OriginDto::User, None, None),
        };
        let list = |items: &[devcopilot_domain::Redacted<devcopilot_domain::text::ListItem>]| {
            items
                .iter()
                .map(|item| item.expose().as_str().to_owned())
                .collect()
        };
        Self {
            id: record.id().to_string(),
            session_id: record.session_id().to_string(),
            kind: Code(record.kind()),
            text: record.text().expose().as_str().to_owned(),
            nature: Code(record.nature()),
            status: Code(record.status()),
            rationale: record.rationale().expose().as_str().to_owned(),
            origin,
            provider_id,
            analysis_run_id,
            evidence: record.evidence().iter().map(ToString::to_string).collect(),
            acceptance_criteria: list(record.acceptance_criteria()),
            alternatives: list(record.alternatives()),
            conflicts_with: record.flags().conflicts_with.map(|id| id.to_string()),
            duplicate_of: record.flags().duplicate_of.map(|id| id.to_string()),
            created_at: record.created_at().unix_millis(),
            updated_at: record.updated_at().unix_millis(),
        }
    }
}

/// `record_set_status` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct RecordSetStatusDto {
    pub record_id: String,
    pub status: Code<RecordStatus>,
}

/// Validates and applies a user status change.
///
/// # Errors
///
/// `invalid_input` (id, status code), `not_found`, `invalid_state` (transition not allowed),
/// storage errors.
pub fn set_status(core: &Core, request: &RecordSetStatusDto) -> Result<RecordDto, IpcError> {
    let id = RecordId::parse(&request.record_id)?;
    Ok(RecordDto::from(
        &core.records.set_status(id, request.status.0)?,
    ))
}

/// Changes a record's status by explicit user action (FR-024–FR-026).
///
/// # Errors
///
/// See [`set_status`].
#[tauri::command]
pub async fn record_set_status(
    state: State<'_, AppState>,
    request: RecordSetStatusDto,
) -> Result<RecordDto, IpcError> {
    set_status(state.core()?, &request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_status_payload_is_strict() {
        let ok: RecordSetStatusDto = serde_json::from_str(
            r#"{"recordId":"019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6a90","status":"confirmed"}"#,
        )
        .unwrap();
        assert_eq!(ok.status, Code(RecordStatus::Confirmed));
        for bad in [
            r#"{"recordId":"x","status":"deleted"}"#,
            r#"{"recordId":"x","status":"confirmed","actor":"ai"}"#,
            r#"{"status":"confirmed"}"#,
        ] {
            assert!(
                serde_json::from_str::<RecordSetStatusDto>(bad).is_err(),
                "{bad}"
            );
        }
    }
}
