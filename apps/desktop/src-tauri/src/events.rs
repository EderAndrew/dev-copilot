//! Bridge from the application [`EventPublisher`] to per-session IPC channels
//! (contracts/ipc-events.md).
//!
//! `seq` is assigned here, monotonic per session and kept across resubscriptions so the
//! frontend can drop stale events.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use devcopilot_application::events::{
    AnalysisStatus, FinalSegment, ProviderStage, SessionEvent, SourceStatus,
};
use devcopilot_application::ports::EventPublisher;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::provider::Locality;
use devcopilot_domain::session::{FailureReason, SessionState};
use devcopilot_domain::transcript::GapReason;
use serde::{Deserialize, Serialize};

use crate::commands::questions::QuestionDto;
use crate::commands::records::RecordDto;
use crate::dto::Code;

/// Final segment as sent to the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SegmentDto {
    /// Segment id.
    pub id: String,
    /// Position within the session.
    pub seq: u64,
    /// Source, when the session has two sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Code<AudioSourceKind>>,
    /// Start offset in ms.
    pub start_ms: u64,
    /// End offset in ms.
    pub end_ms: u64,
    /// Text.
    pub text: String,
}

/// `source_status.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum SourceStatusDto {
    Ok,
    Lost,
    PermissionRevoked,
}

/// `analysis_status.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum AnalysisStatusDto {
    Running,
    Ok,
    InvalidOutput,
    ProviderError,
    RateLimited,
}

/// `provider_in_use.role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(missing_docs)]
pub enum ProviderStageDto {
    Transcription,
    Analysis,
    Questions,
}

impl From<ProviderStage> for ProviderStageDto {
    fn from(stage: ProviderStage) -> Self {
        match stage {
            ProviderStage::Transcription => Self::Transcription,
            ProviderStage::Analysis => Self::Analysis,
            ProviderStage::Questions => Self::Questions,
        }
    }
}

/// Event payload; the `type` tag and field names follow ipc-events.md.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[allow(missing_docs)]
pub enum SessionEventBody {
    StateChanged {
        state: Code<SessionState>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        failure: Option<Code<FailureReason>>,
    },
    CaptureIndicator {
        active: bool,
        paused: bool,
        sources: Vec<Code<AudioSourceKind>>,
    },
    PartialSegment {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<Code<AudioSourceKind>>,
        start_ms: u64,
        text: String,
    },
    FinalSegment {
        segment: SegmentDto,
    },
    Gap {
        reason: Code<GapReason>,
        start_ms: u64,
        duration_ms: u64,
    },
    SourceStatus {
        source: Code<AudioSourceKind>,
        status: SourceStatusDto,
    },
    AnalysisStatus {
        run_id: String,
        status: AnalysisStatusDto,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created: Option<u32>,
    },
    RecordProposed {
        record: Box<RecordDto>,
    },
    QuestionAnswered {
        question: Box<QuestionDto>,
    },
    ProviderInUse {
        role: ProviderStageDto,
        locality: Code<Locality>,
        provider_id: String,
    },
    ProviderFallback {
        role: ProviderStageDto,
        from_provider_id: String,
        to_provider_id: String,
    },
    TranscriptionLag {
        lag_ms: u64,
    },
    StorageError {
        message_key: String,
    },
}

/// One event as delivered over the channel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcSessionEvent {
    /// Monotonic per session.
    pub seq: u64,
    /// Payload.
    #[serde(flatten)]
    pub body: SessionEventBody,
}

impl From<SessionEvent> for SessionEventBody {
    fn from(event: SessionEvent) -> Self {
        match event {
            SessionEvent::StateChanged { state, failure } => Self::StateChanged {
                state: Code(state),
                failure: failure.map(Code),
            },
            SessionEvent::CaptureIndicator {
                active,
                paused,
                sources,
            } => Self::CaptureIndicator {
                active,
                paused,
                sources: sources.into_iter().map(Code).collect(),
            },
            SessionEvent::PartialSegment {
                source,
                start,
                text,
            } => Self::PartialSegment {
                source: source.map(Code),
                start_ms: start.as_millis(),
                text: text.into_inner(),
            },
            SessionEvent::FinalSegment(FinalSegment {
                id,
                seq,
                source,
                start,
                end,
                text,
            }) => Self::FinalSegment {
                segment: SegmentDto {
                    id: id.to_string(),
                    seq,
                    source: source.map(Code),
                    start_ms: start.as_millis(),
                    end_ms: end.as_millis(),
                    text: text.into_inner().into_inner(),
                },
            },
            SessionEvent::Gap {
                reason,
                start,
                duration,
            } => Self::Gap {
                reason: Code(reason),
                start_ms: start.as_millis(),
                duration_ms: duration.as_millis(),
            },
            SessionEvent::SourceStatus { source, status } => Self::SourceStatus {
                source: Code(source),
                status: match status {
                    SourceStatus::Ok => SourceStatusDto::Ok,
                    SourceStatus::Lost => SourceStatusDto::Lost,
                    SourceStatus::PermissionRevoked => SourceStatusDto::PermissionRevoked,
                },
            },
            SessionEvent::AnalysisStatus { run_id, status } => {
                let (status, created) = match status {
                    AnalysisStatus::Running => (AnalysisStatusDto::Running, None),
                    AnalysisStatus::Ok { created } => (AnalysisStatusDto::Ok, Some(created)),
                    AnalysisStatus::InvalidOutput => (AnalysisStatusDto::InvalidOutput, None),
                    AnalysisStatus::ProviderError => (AnalysisStatusDto::ProviderError, None),
                    AnalysisStatus::RateLimited => (AnalysisStatusDto::RateLimited, None),
                };
                Self::AnalysisStatus {
                    run_id: run_id.to_string(),
                    status,
                    created,
                }
            }
            SessionEvent::RecordProposed(record) => Self::RecordProposed {
                record: Box::new(RecordDto::from(record.as_ref())),
            },
            SessionEvent::QuestionAnswered(question) => Self::QuestionAnswered {
                question: Box::new(QuestionDto::from(question.as_ref())),
            },
            SessionEvent::ProviderInUse {
                stage,
                locality,
                provider_id,
            } => Self::ProviderInUse {
                role: stage.into(),
                locality: Code(locality),
                provider_id: provider_id.to_string(),
            },
            SessionEvent::ProviderFallback {
                stage,
                from_provider_id,
                to_provider_id,
            } => Self::ProviderFallback {
                role: stage.into(),
                from_provider_id: from_provider_id.to_string(),
                to_provider_id: to_provider_id.to_string(),
            },
            SessionEvent::TranscriptionLag { lag } => Self::TranscriptionLag {
                lag_ms: lag.as_millis(),
            },
            SessionEvent::StorageError { message_key } => Self::StorageError {
                message_key: message_key.to_owned(),
            },
        }
    }
}

/// Destination of a session's events (a Tauri channel in the app, a recorder in tests).
pub trait EventSink: Send + Sync {
    /// Delivers one event; `false` when the receiver is gone.
    fn deliver(&self, event: IpcSessionEvent) -> bool;
}

impl EventSink for tauri::ipc::Channel<IpcSessionEvent> {
    fn deliver(&self, event: IpcSessionEvent) -> bool {
        self.send(event).is_ok()
    }
}

#[derive(Default)]
struct SessionStream {
    last_seq: u64,
    sink: Option<Box<dyn EventSink>>,
}

/// [`EventPublisher`] delivering to the latest subscriber of each session.
#[derive(Default)]
pub struct ChannelEventPublisher {
    streams: Mutex<HashMap<SessionId, SessionStream>>,
}

impl ChannelEventPublisher {
    fn streams(&self) -> MutexGuard<'_, HashMap<SessionId, SessionStream>> {
        self.streams.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Routes future events of `session` to `sink`, replacing any previous subscriber.
    pub fn subscribe(&self, session: SessionId, sink: Box<dyn EventSink>) {
        self.streams().entry(session).or_default().sink = Some(sink);
    }

    /// Stops delivering events of `session` (sequence numbering is kept).
    pub fn unsubscribe(&self, session: SessionId) {
        if let Some(stream) = self.streams().get_mut(&session) {
            stream.sink = None;
        }
    }

    /// Forgets a deleted session entirely.
    pub fn forget(&self, session: SessionId) {
        self.streams().remove(&session);
    }
}

impl EventPublisher for ChannelEventPublisher {
    fn publish(&self, session: SessionId, event: SessionEvent) {
        let mut streams = self.streams();
        let stream = streams.entry(session).or_default();
        stream.last_seq += 1;
        let event = IpcSessionEvent {
            seq: stream.last_seq,
            body: event.into(),
        };
        if let Some(sink) = &stream.sink
            && !sink.deliver(event)
        {
            tracing::debug!(%session, "event subscriber gone");
            stream.sink = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use devcopilot_domain::Redacted;
    use devcopilot_domain::time::Duration;

    use super::*;

    #[derive(Default)]
    struct Recorder {
        events: Mutex<Vec<IpcSessionEvent>>,
        closed: std::sync::atomic::AtomicBool,
    }

    impl EventSink for Arc<Recorder> {
        fn deliver(&self, event: IpcSessionEvent) -> bool {
            if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
                return false;
            }
            self.events.lock().unwrap().push(event);
            true
        }
    }

    fn indicator(active: bool) -> SessionEvent {
        SessionEvent::CaptureIndicator {
            active,
            paused: false,
            sources: vec![AudioSourceKind::Microphone],
        }
    }

    #[test]
    fn seq_is_monotonic_per_session_and_survives_resubscribe() {
        let publisher = ChannelEventPublisher::default();
        let session = SessionId::new();
        let other = SessionId::new();
        let first = Arc::new(Recorder::default());
        publisher.subscribe(session, Box::new(first.clone()));

        publisher.publish(session, indicator(true));
        publisher.publish(other, indicator(true));
        publisher.publish(session, indicator(false));

        let second = Arc::new(Recorder::default());
        publisher.subscribe(session, Box::new(second.clone()));
        publisher.publish(session, indicator(true));

        let seqs = |r: &Recorder| {
            r.events
                .lock()
                .unwrap()
                .iter()
                .map(|e| e.seq)
                .collect::<Vec<_>>()
        };
        assert_eq!(seqs(&first), [1, 2]);
        assert_eq!(seqs(&second), [3]);
    }

    #[test]
    fn closed_subscriber_is_dropped() {
        let publisher = ChannelEventPublisher::default();
        let session = SessionId::new();
        let recorder = Arc::new(Recorder::default());
        publisher.subscribe(session, Box::new(recorder.clone()));
        recorder
            .closed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        publisher.publish(session, indicator(true));
        recorder
            .closed
            .store(false, std::sync::atomic::Ordering::SeqCst);
        publisher.publish(session, indicator(false));
        assert!(recorder.events.lock().unwrap().is_empty());
    }

    #[test]
    fn json_shape_matches_contract() {
        let event = IpcSessionEvent {
            seq: 7,
            body: SessionEvent::Gap {
                reason: GapReason::Overload,
                start: Duration::from_millis(1_000),
                duration: Duration::from_millis(250),
            }
            .into(),
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({
                "type": "gap", "seq": 7, "reason": "overload", "startMs": 1000, "durationMs": 250
            })
        );
        let partial: SessionEventBody = SessionEvent::PartialSegment {
            source: None,
            start: Duration::ZERO,
            text: Redacted::new("oi".into()),
        }
        .into();
        assert_eq!(
            serde_json::to_value(&partial).unwrap(),
            serde_json::json!({ "type": "partial_segment", "startMs": 0, "text": "oi" })
        );
    }
}
