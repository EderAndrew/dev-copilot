//! `session_create`, `session_start`, `session_cancel_start`, `session_stop`,
//! `session_subscribe`, `session_list`, `session_get` and `session_rename`
//! (contracts/ipc-commands.md "Sessões").
//!
//! Each Tauri command is a thin wrapper over a plain function taking the [`Core`], so the
//! validation path is testable without a running app.

use devcopilot_application::session_service::NewSession;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::session::{FailureReason, Session, SessionState};
use devcopilot_domain::text::Title;
use devcopilot_domain::transcript::{GapReason, SegmentKind, TranscriptSegment};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;

use super::IpcError;
use super::questions::QuestionDto;
use super::records::RecordDto;
use crate::composition::{AppState, Core};
use crate::dto::Code;
use crate::events::{ChannelEventPublisher, EventSink, IpcSessionEvent, SegmentDto};

/// Page size of `session_list` when `limit` is absent.
pub const DEFAULT_PAGE_SIZE: usize = 50;

/// Session as returned by every session command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SessionDto {
    pub id: String,
    pub title: String,
    pub language: Code<SessionLanguage>,
    pub state: Code<SessionState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<Code<FailureReason>>,
    pub sources: Vec<Code<AudioSourceKind>>,
    /// Unix epoch milliseconds.
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<i64>,
    pub active_duration_ms: u64,
}

impl From<&Session> for SessionDto {
    fn from(session: &Session) -> Self {
        Self {
            id: session.id().to_string(),
            title: session.title().as_str().to_owned(),
            language: Code(session.language()),
            state: Code(session.state()),
            failure: session.failure().map(Code),
            sources: session.sources().iter().copied().map(Code).collect(),
            created_at: session.created_at().unix_millis(),
            started_at: session.started_at().map(|t| t.unix_millis()),
            ended_at: session.ended_at().map(|t| t.unix_millis()),
            active_duration_ms: session.active_duration().as_millis(),
        }
    }
}

impl From<Session> for SessionDto {
    fn from(session: Session) -> Self {
        Self::from(&session)
    }
}

/// `session_create` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SessionCreateDto {
    pub title: String,
    pub language: Code<SessionLanguage>,
    pub sources: Vec<Code<AudioSourceKind>>,
}

/// `session_start` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SessionStartDto {
    pub session_id: String,
    pub consent_acknowledged: bool,
}

/// Input of commands that only name a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SessionIdDto {
    pub session_id: String,
}

impl SessionIdDto {
    /// The parsed session id.
    ///
    /// # Errors
    ///
    /// `invalid_input` when not a UUID.
    pub fn id(&self) -> Result<SessionId, IpcError> {
        Ok(SessionId::parse(&self.session_id)?)
    }
}

/// `session_list` input.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SessionListDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// `session_list` output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SessionPageDto {
    pub items: Vec<SessionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// `session_rename` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SessionRenameDto {
    pub session_id: String,
    pub title: String,
}

/// A persisted transcript item: a final segment or a gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[allow(missing_docs)]
pub enum TranscriptItemDto {
    Final {
        segment: SegmentDto,
    },
    Gap {
        reason: Code<GapReason>,
        start_ms: u64,
        duration_ms: u64,
    },
}

impl From<&TranscriptSegment> for TranscriptItemDto {
    fn from(segment: &TranscriptSegment) -> Self {
        match segment.kind() {
            SegmentKind::Final { text } => Self::Final {
                segment: SegmentDto {
                    id: segment.id().to_string(),
                    seq: segment.seq(),
                    source: segment.source().map(Code),
                    start_ms: segment.start().as_millis(),
                    end_ms: segment.end().as_millis(),
                    text: text.expose().as_str().to_owned(),
                },
            },
            SegmentKind::Gap { reason } => Self::Gap {
                reason: Code(*reason),
                start_ms: segment.start().as_millis(),
                duration_ms: segment.duration().as_millis(),
            },
        }
    }
}

/// `session_get` output: everything persisted for a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct SessionDetailDto {
    pub session: SessionDto,
    /// Finals and gaps in `seq` order.
    pub transcript: Vec<TranscriptItemDto>,
    /// Records in creation order.
    pub records: Vec<RecordDto>,
    /// Questions in the order they were asked (FR-038).
    pub questions: Vec<QuestionDto>,
}

/// Validates and creates an idle session.
///
/// # Errors
///
/// `invalid_input` (title bounds, sources), `source_unavailable`, storage errors.
pub fn create(core: &Core, request: SessionCreateDto) -> Result<SessionDto, IpcError> {
    let new = NewSession {
        title: Title::parse(request.title)?,
        language: request.language.0,
        sources: request.sources.into_iter().map(|c| c.0).collect(),
    };
    Ok(core.sessions.create(new)?.into())
}

/// Starts capture; returns the session in `starting`.
///
/// # Errors
///
/// `invalid_input` (id, consent), `not_found`, `invalid_state`, `permission_denied`,
/// `no_transcriber`, `secret_store_unavailable`, `source_unavailable`.
pub fn start(core: &Core, request: SessionStartDto) -> Result<SessionDto, IpcError> {
    let id = SessionId::parse(&request.session_id)?;
    Ok(core
        .sessions
        .start(id, request.consent_acknowledged)?
        .into())
}

/// Cancels a start in progress; returns the session in `idle`.
///
/// # Errors
///
/// `invalid_input`, `not_found`, `invalid_state` outside `starting`.
pub fn cancel_start(core: &Core, request: &SessionIdDto) -> Result<SessionDto, IpcError> {
    Ok(core.sessions.cancel_start(request.id()?)?.into())
}

/// Stops capture; returns the session in `stopping` without waiting for the drain.
///
/// # Errors
///
/// `invalid_input`, `not_found`, `invalid_state`.
pub fn stop(core: &Core, request: &SessionIdDto) -> Result<SessionDto, IpcError> {
    Ok(core.sessions.stop(request.id()?)?.into())
}

/// Routes the session's events to `sink`, replacing any previous subscriber.
///
/// # Errors
///
/// `invalid_input`, `not_found`.
pub fn subscribe(
    core: &Core,
    events: &ChannelEventPublisher,
    request: &SessionIdDto,
    sink: Box<dyn EventSink>,
) -> Result<SessionDto, IpcError> {
    let session = core.sessions.get(request.id()?)?;
    events.subscribe(session.id(), sink);
    Ok(session.into())
}

/// One page of the session history, newest first.
///
/// # Errors
///
/// `invalid_input` (cursor, limit outside 1..=100), storage errors.
pub fn list(core: &Core, request: &SessionListDto) -> Result<SessionPageDto, IpcError> {
    let cursor = request
        .cursor
        .as_deref()
        .map(SessionId::parse)
        .transpose()?;
    let page = core
        .sessions
        .list_page(cursor, request.limit.unwrap_or(DEFAULT_PAGE_SIZE))?;
    Ok(SessionPageDto {
        items: page.items.iter().map(SessionDto::from).collect(),
        next_cursor: page.next_cursor.map(|id| id.to_string()),
    })
}

/// A session with its transcript and records.
///
/// # Errors
///
/// `invalid_input`, `not_found`, storage errors.
pub fn detail(core: &Core, request: &SessionIdDto) -> Result<SessionDetailDto, IpcError> {
    let id = request.id()?;
    let session = core.sessions.get(id)?;
    Ok(SessionDetailDto {
        session: SessionDto::from(&session),
        transcript: core
            .sessions
            .transcript(id)?
            .iter()
            .map(TranscriptItemDto::from)
            .collect(),
        records: core.records.list(id)?.iter().map(RecordDto::from).collect(),
        questions: core
            .questions
            .list(id)?
            .iter()
            .map(QuestionDto::from)
            .collect(),
    })
}

/// Renames a session.
///
/// # Errors
///
/// `invalid_input` (id, title bounds), `not_found`, storage errors.
pub fn rename(core: &Core, request: SessionRenameDto) -> Result<SessionDto, IpcError> {
    let id = SessionId::parse(&request.session_id)?;
    Ok(core
        .sessions
        .rename(id, Title::parse(request.title)?)?
        .into())
}

/// Lists saved sessions (US4.3).
///
/// # Errors
///
/// See [`list`].
#[tauri::command]
pub async fn session_list(
    state: State<'_, AppState>,
    request: SessionListDto,
) -> Result<SessionPageDto, IpcError> {
    list(state.core()?, &request)
}

/// Opens a saved session (US4.3, US4.4).
///
/// # Errors
///
/// See [`detail`].
#[tauri::command]
pub async fn session_get(
    state: State<'_, AppState>,
    request: SessionIdDto,
) -> Result<SessionDetailDto, IpcError> {
    detail(state.core()?, &request)
}

/// Renames a session.
///
/// # Errors
///
/// See [`rename`].
#[tauri::command]
pub async fn session_rename(
    state: State<'_, AppState>,
    request: SessionRenameDto,
) -> Result<SessionDto, IpcError> {
    rename(state.core()?, request)
}

/// Creates an idle session (FR-001).
///
/// # Errors
///
/// See [`create`].
#[tauri::command]
pub async fn session_create(
    state: State<'_, AppState>,
    request: SessionCreateDto,
) -> Result<SessionDto, IpcError> {
    create(state.core()?, request)
}

/// Starts capture by explicit user action (FR-002).
///
/// # Errors
///
/// See [`start`].
#[tauri::command]
pub async fn session_start(
    state: State<'_, AppState>,
    request: SessionStartDto,
) -> Result<SessionDto, IpcError> {
    start(state.core()?, request)
}

/// Cancels a start in progress (FR-003).
///
/// # Errors
///
/// See [`cancel_start`].
#[tauri::command]
pub async fn session_cancel_start(
    state: State<'_, AppState>,
    request: SessionIdDto,
) -> Result<SessionDto, IpcError> {
    cancel_start(state.core()?, &request)
}

/// Stops capture immediately (FR-005).
///
/// # Errors
///
/// See [`stop`].
#[tauri::command]
pub async fn session_stop(
    state: State<'_, AppState>,
    request: SessionIdDto,
) -> Result<SessionDto, IpcError> {
    stop(state.core()?, &request)
}

/// Subscribes `channel` to the session's events and returns the current session, so the UI
/// can resynchronize after reconnecting (contracts/ipc-events.md).
///
/// # Errors
///
/// See [`subscribe`].
#[tauri::command]
pub async fn session_subscribe(
    state: State<'_, AppState>,
    request: SessionIdDto,
    channel: Channel<IpcSessionEvent>,
) -> Result<SessionDto, IpcError> {
    subscribe(state.core()?, &state.events, &request, Box::new(channel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_payload_rejects_unknown_fields_and_codes() {
        let ok: SessionCreateDto = serde_json::from_str(
            r#"{"title":"Daily","language":"pt-BR","sources":["microphone"]}"#,
        )
        .unwrap();
        assert_eq!(ok.sources, [Code(AudioSourceKind::Microphone)]);
        for bad in [
            r#"{"title":"Daily","language":"pt-BR","sources":["speaker"]}"#,
            r#"{"title":"Daily","language":"fr","sources":["microphone"]}"#,
            r#"{"title":"Daily","language":"pt-BR","sources":["microphone"],"autoStart":true}"#,
            r#"{"language":"pt-BR","sources":["microphone"]}"#,
        ] {
            assert!(
                serde_json::from_str::<SessionCreateDto>(bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn start_requires_explicit_consent_flag() {
        assert!(serde_json::from_str::<SessionStartDto>(r#"{"sessionId":"x"}"#).is_err());
    }
}
