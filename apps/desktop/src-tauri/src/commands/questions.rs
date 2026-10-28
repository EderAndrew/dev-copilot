//! `question_ask` and `question_to_record` (contracts/ipc-commands.md "Perguntas"), and the
//! `QuestionDto` shared with `session_get` and the `question_answered` event.
//!
//! Answers arrive by event and never change records; turning one into a record is an explicit
//! user action that only creates a new `proposed` record (FR-037).

use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{QuestionId, SessionId};
use devcopilot_domain::question::{Certainty, ContextQuestion, EvidenceRef, QuestionStatus};
use devcopilot_domain::record::RecordKind;
use devcopilot_domain::text::QuestionText;
use serde::{Deserialize, Serialize};
use tauri::State;

use super::IpcError;
use super::records::RecordDto;
use crate::composition::{AppState, Core};
use crate::dto::Code;

/// Session content cited by an answer; the UI quotes it from the stored segment or record
/// (FR-053b).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
#[allow(missing_docs)]
pub enum EvidenceRefDto {
    Segment { id: String },
    Record { id: String },
}

impl From<&EvidenceRef> for EvidenceRefDto {
    fn from(evidence: &EvidenceRef) -> Self {
        match evidence {
            EvidenceRef::Segment(id) => Self::Segment { id: id.to_string() },
            EvidenceRef::Record(id) => Self::Record { id: id.to_string() },
        }
    }
}

/// A contextual question as sent to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct QuestionDto {
    pub id: String,
    pub session_id: String,
    pub question: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certainty: Option<Code<Certainty>>,
    pub evidence: Vec<EvidenceRefDto>,
    pub status: Code<QuestionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    /// Unix epoch milliseconds.
    pub asked_at: i64,
    /// Unix epoch milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_at: Option<i64>,
}

impl From<&ContextQuestion> for QuestionDto {
    fn from(question: &ContextQuestion) -> Self {
        Self {
            id: question.id().to_string(),
            session_id: question.session_id().to_string(),
            question: question.question().expose().as_str().to_owned(),
            answer: question
                .answer_text()
                .map(|answer| answer.expose().as_str().to_owned()),
            certainty: question.certainty().map(Code),
            evidence: question
                .evidence()
                .iter()
                .map(EvidenceRefDto::from)
                .collect(),
            status: Code(question.status()),
            provider_id: question.provider_id().map(|id| id.to_string()),
            asked_at: question.asked_at().unix_millis(),
            answered_at: question.answered_at().map(|at| at.unix_millis()),
        }
    }
}

/// `question_ask` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct QuestionAskDto {
    pub session_id: String,
    pub question: String,
}

/// `question_to_record` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct QuestionToRecordDto {
    pub question_id: String,
    pub kind: Code<RecordKind>,
}

/// Validates and asks a question; returns it `pending`.
///
/// # Errors
///
/// `invalid_input` (id, question bounds), `not_found`, `invalid_state` (session never started,
/// or a question about it is pending), `provider_error` (no usable analysis provider), storage
/// errors.
pub fn ask(core: &Core, request: QuestionAskDto) -> Result<QuestionDto, IpcError> {
    let session = SessionId::parse(&request.session_id)?;
    let question = Redacted::new(QuestionText::parse(request.question)?);
    Ok(QuestionDto::from(&core.questions.ask(session, question)?))
}

/// Turns an answer into a new proposed record.
///
/// # Errors
///
/// `invalid_input` (id, kind), `not_found`, `invalid_state` (not answered), storage errors.
pub fn to_record(core: &Core, request: &QuestionToRecordDto) -> Result<RecordDto, IpcError> {
    let id = QuestionId::parse(&request.question_id)?;
    Ok(RecordDto::from(
        &core.questions.to_record(id, request.kind.0)?,
    ))
}

/// Asks a question about a session during or after capture (FR-034); the answer arrives as a
/// `question_answered` event.
///
/// # Errors
///
/// See [`ask`].
#[tauri::command]
pub async fn question_ask(
    state: State<'_, AppState>,
    request: QuestionAskDto,
) -> Result<QuestionDto, IpcError> {
    ask(state.core()?, request)
}

/// Turns an answer into a `proposed` record by explicit user action (FR-037).
///
/// # Errors
///
/// See [`to_record`].
#[tauri::command]
pub async fn question_to_record(
    state: State<'_, AppState>,
    request: QuestionToRecordDto,
) -> Result<RecordDto, IpcError> {
    to_record(state.core()?, &request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payloads_are_strict() {
        let ok: QuestionToRecordDto = serde_json::from_str(
            r#"{"questionId":"019a2b3c-4d5e-7f60-8a1b-2c3d4e5f6b10","kind":"decision"}"#,
        )
        .unwrap();
        assert_eq!(ok.kind, Code(RecordKind::Decision));
        for bad in [
            r#"{"questionId":"x","kind":"wish"}"#,
            r#"{"questionId":"x","kind":"task","status":"confirmed"}"#,
        ] {
            assert!(
                serde_json::from_str::<QuestionToRecordDto>(bad).is_err(),
                "{bad}"
            );
        }
        assert!(
            serde_json::from_str::<QuestionAskDto>(
                r#"{"sessionId":"x","question":"q","tools":[]}"#
            )
            .is_err()
        );
        assert!(serde_json::from_str::<EvidenceRefDto>(r#"{"type":"file","id":"x"}"#).is_err());
    }
}
