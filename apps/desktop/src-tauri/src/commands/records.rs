//! `record_set_status`, `record_edit`, `record_create`, `plan_get` and `analysis_run_final`
//! (contracts/ipc-commands.md "Registros e plano"), and the `RecordDto` shared with the
//! `record_proposed` event.
//!
//! The actor of every change made here is the user (FR-024); the AI never reaches this path.
//! A final analysis only creates proposals, like incremental analysis (FR-033).

use devcopilot_application::plan::DevelopmentPlan;
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{RecordId, SessionId};
use devcopilot_domain::record::{
    Nature, Origin, RecordEdit, RecordKind, RecordStatus, TechnicalRecord, UserRecord,
};
use devcopilot_domain::text::{ListItem, Rationale, RecordText};
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

/// `record_edit` input; absent fields keep their value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct RecordEditDto {
    pub record_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance_criteria: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternatives: Option<Vec<String>>,
}

/// `record_create` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct RecordCreateDto {
    pub session_id: String,
    pub kind: Code<RecordKind>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    pub status: Code<RecordStatus>,
}

/// One plan section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct PlanSectionDto {
    pub kind: Code<RecordKind>,
    pub items: Vec<RecordDto>,
}

/// `plan_get` output: the derived development plan (FR-031).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct PlanDto {
    pub session_id: String,
    pub sections: Vec<PlanSectionDto>,
}

impl From<&DevelopmentPlan> for PlanDto {
    fn from(plan: &DevelopmentPlan) -> Self {
        Self {
            session_id: plan.session_id.to_string(),
            sections: plan
                .sections
                .iter()
                .map(|section| PlanSectionDto {
                    kind: Code(section.kind),
                    items: section.items.iter().map(RecordDto::from).collect(),
                })
                .collect(),
        }
    }
}

/// `analysis_run_final` output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct RunIdDto {
    pub run_id: String,
}

fn list_items(values: Vec<String>) -> Result<Vec<Redacted<ListItem>>, IpcError> {
    values
        .into_iter()
        .map(|value| Ok(Redacted::new(ListItem::parse(value)?)))
        .collect()
}

fn rationale(value: Option<String>) -> Result<Redacted<Rationale>, IpcError> {
    Ok(Redacted::new(Rationale::parse(value.unwrap_or_default())?))
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

/// Validates and applies a user edit.
///
/// # Errors
///
/// `invalid_input` (id, bounds, empty edit, lists on the wrong kind), `not_found`, storage
/// errors.
pub fn edit(core: &Core, request: RecordEditDto) -> Result<RecordDto, IpcError> {
    let id = RecordId::parse(&request.record_id)?;
    let edit = RecordEdit {
        text: request
            .text
            .map(|text| RecordText::parse(text).map(Redacted::new))
            .transpose()?,
        rationale: request.rationale.map(|r| rationale(Some(r))).transpose()?,
        acceptance_criteria: request.acceptance_criteria.map(list_items).transpose()?,
        alternatives: request.alternatives.map(list_items).transpose()?,
    };
    Ok(RecordDto::from(&core.records.edit(id, edit)?))
}

/// Validates and creates a user record.
///
/// # Errors
///
/// `invalid_input` (id, bounds), `not_found`, storage errors.
pub fn create(core: &Core, request: RecordCreateDto) -> Result<RecordDto, IpcError> {
    let session = SessionId::parse(&request.session_id)?;
    let draft = UserRecord {
        kind: request.kind.0,
        text: Redacted::new(RecordText::parse(request.text)?),
        rationale: rationale(request.rationale)?,
        status: request.status.0,
    };
    Ok(RecordDto::from(&core.records.create(session, draft)?))
}

/// The development plan of a session.
///
/// # Errors
///
/// `invalid_input`, `not_found`, storage errors.
pub fn plan(core: &Core, request: &super::sessions::SessionIdDto) -> Result<PlanDto, IpcError> {
    let session = SessionId::parse(&request.session_id)?;
    Ok(PlanDto::from(&core.records.plan(session)?))
}

/// Starts a final analysis of an ended or failed session.
///
/// # Errors
///
/// `invalid_input`, `not_found`, `invalid_state` (session not ended/failed, or a final
/// analysis already running), `provider_error` (no usable analysis provider), storage errors.
pub fn run_final(
    core: &Core,
    request: &super::sessions::SessionIdDto,
) -> Result<RunIdDto, IpcError> {
    let session = core.sessions.get(SessionId::parse(&request.session_id)?)?;
    Ok(RunIdDto {
        run_id: core.analysis.run_final(&session)?.to_string(),
    })
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

/// Edits a record by explicit user action (FR-032).
///
/// # Errors
///
/// See [`edit`].
#[tauri::command]
pub async fn record_edit(
    state: State<'_, AppState>,
    request: RecordEditDto,
) -> Result<RecordDto, IpcError> {
    edit(state.core()?, request)
}

/// Adds a user record to a session's plan (FR-032).
///
/// # Errors
///
/// See [`create`].
#[tauri::command]
pub async fn record_create(
    state: State<'_, AppState>,
    request: RecordCreateDto,
) -> Result<RecordDto, IpcError> {
    create(state.core()?, request)
}

/// Development plan of a session (FR-031).
///
/// # Errors
///
/// See [`plan`].
#[tauri::command]
pub async fn plan_get(
    state: State<'_, AppState>,
    request: super::sessions::SessionIdDto,
) -> Result<PlanDto, IpcError> {
    plan(state.core()?, &request)
}

/// Final analysis by explicit request (FR-033); results arrive as session events.
///
/// # Errors
///
/// See [`run_final`].
#[tauri::command]
pub async fn analysis_run_final(
    state: State<'_, AppState>,
    request: super::sessions::SessionIdDto,
) -> Result<RunIdDto, IpcError> {
    run_final(state.core()?, &request)
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
