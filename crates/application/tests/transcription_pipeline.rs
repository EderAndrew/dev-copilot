//! Transcription stage of the capture pipeline (T062, US2): partial/final handling,
//! persistence on arrival, overload, provider failure, stop drain, lag and storage failure.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::{Duration as StdDuration, Instant};

use devcopilot_application::events::{ProviderStage, SessionEvent};
use devcopilot_application::ports::{
    AudioChunk, ProviderError, RepositoryError, SegmentRepository, SourceFault, Transcriber,
    TranscriptUpdate,
};
use devcopilot_application::session_service::DRAIN_DEADLINE;
use devcopilot_application::testing::TranscriberEnd;
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::session::{FailureReason, SessionState};
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::Duration;
use devcopilot_domain::transcript::{GapReason, TranscriptSegment};
use support::{Fixture, fixture, indicators, wait_until};

const MIC: AudioSourceKind = AudioSourceKind::Microphone;
const SYSTEM: AudioSourceKind = AudioSourceKind::System;

fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

fn final_update(source: AudioSourceKind, start_s: u64, end_s: u64, text: &str) -> TranscriptUpdate {
    TranscriptUpdate::Final {
        source,
        start: secs(start_s),
        end: secs(end_s),
        text: Redacted::new(SegmentText::parse(text).unwrap()),
    }
}

fn partial(start_s: u64, text: &str) -> TranscriptUpdate {
    TranscriptUpdate::Partial {
        source: MIC,
        start: secs(start_s),
        text: Redacted::new(text.into()),
    }
}

fn one_second(source: AudioSourceKind, start_s: u64) -> AudioChunk {
    AudioChunk {
        source,
        start: secs(start_s),
        samples: vec![0.0; 16_000],
    }
}

/// Pushes `count` one-second chunks starting at `from_s`.
fn push_seconds(f: &Fixture, from_s: u64, count: u64) {
    for second in from_s..from_s + count {
        f.mic.push_chunk(one_second(MIC, second)).unwrap();
    }
}

fn stored(f: &Fixture, id: SessionId) -> Vec<TranscriptSegment> {
    f.segments.list_by_session(id).unwrap()
}

fn gaps(f: &Fixture, id: SessionId) -> Vec<(GapReason, Duration, Duration)> {
    stored(f, id)
        .iter()
        .filter_map(|s| s.gap_reason().map(|r| (r, s.start(), s.end())))
        .collect()
}

fn texts(f: &Fixture, id: SessionId) -> Vec<String> {
    stored(f, id)
        .iter()
        .filter_map(|s| s.text().map(|t| t.expose().as_str().to_owned()))
        .collect()
}

fn lag_events(events: &[SessionEvent]) -> Vec<u64> {
    events
        .iter()
        .filter_map(|e| match e {
            SessionEvent::TranscriptionLag { lag } => Some(lag.as_millis()),
            _ => None,
        })
        .collect()
}

async fn capturing(f: &Fixture, sources: &[AudioSourceKind]) -> SessionId {
    let id = f.create(sources).id();
    f.start_capturing(id).await;
    id
}

#[tokio::test]
async fn partials_are_published_never_persisted_and_finals_are_persisted_on_arrival() {
    let f = fixture();
    let id = capturing(&f, &[MIC]).await;
    f.transcriber.emit(partial(0, "olá mun"));
    wait_until(|| {
        f.events(id)
            .iter()
            .any(|e| matches!(e, SessionEvent::PartialSegment { .. }))
    })
    .await;
    assert!(stored(&f, id).is_empty(), "partials are never persisted");

    f.transcriber.emit(final_update(MIC, 0, 2, "olá mundo"));
    wait_until(|| texts(&f, id) == ["olá mundo"]).await;
    let segment = &stored(&f, id)[0];
    assert_eq!(
        (segment.seq(), segment.start(), segment.end()),
        (0, secs(0), secs(2))
    );
    assert_eq!(segment.source(), None, "one source: no tag");
    // Persisted before being published, while still capturing.
    assert_eq!(f.state(id), SessionState::Capturing);
    let event = f
        .events(id)
        .into_iter()
        .find_map(|e| match e {
            SessionEvent::FinalSegment(segment) => Some(segment),
            _ => None,
        })
        .unwrap();
    assert_eq!(event.id, segment.id());
    assert_eq!(event.text.expose().as_str(), "olá mundo");
}

#[tokio::test]
async fn capture_start_reports_the_transcription_provider_and_session_language() {
    let f = fixture();
    let id = capturing(&f, &[MIC]).await;
    let descriptor = f.transcriber.descriptor();
    assert!(f.events(id).contains(&SessionEvent::ProviderInUse {
        stage: ProviderStage::Transcription,
        locality: descriptor.locality,
        provider_id: descriptor.id,
    }));
    assert_eq!(f.transcribers.languages(), [SessionLanguage::PtBr]);
}

#[tokio::test]
async fn start_is_refused_when_the_transcriber_cannot_be_built() {
    let f = fixture();
    f.transcribers.set_unavailable(true);
    let id = f.create_mic().id();
    let err = f.service.start(id, true).unwrap_err();
    assert_eq!(err, devcopilot_application::ApplicationError::NoTranscriber);
    assert_eq!(f.state(id), SessionState::Idle);
    assert_eq!(f.mic.start_count(), 0);
}

#[tokio::test]
async fn audio_reaches_the_transcriber_in_session_offsets() {
    let f = fixture();
    let _id = capturing(&f, &[MIC]).await;
    push_seconds(&f, 0, 3);
    wait_until(|| f.transcriber.received().len() == 3).await;
    let starts: Vec<_> = f.transcriber.received().iter().map(|c| c.start).collect();
    assert_eq!(starts, [secs(0), secs(1), secs(2)]);
}

#[tokio::test]
async fn finals_of_a_two_source_session_carry_their_source() {
    let f = fixture();
    let id = capturing(&f, &[MIC, SYSTEM]).await;
    f.transcriber.emit(final_update(SYSTEM, 0, 1, "do sistema"));
    f.transcriber.emit(final_update(MIC, 1, 2, "do microfone"));
    wait_until(|| stored(&f, id).len() == 2).await;
    let sources: Vec<_> = stored(&f, id)
        .iter()
        .map(TranscriptSegment::source)
        .collect();
    assert_eq!(sources, [Some(SYSTEM), Some(MIC)]);
}

#[tokio::test]
async fn lag_above_15_s_is_reported_and_cleared_on_recovery() {
    let f = fixture();
    let id = capturing(&f, &[MIC]).await;
    f.transcriber.hold_input();
    push_seconds(&f, 0, 22);
    wait_until(|| !lag_events(&f.events(id)).is_empty()).await;
    assert!(lag_events(&f.events(id))[0] > 15_000);
    assert!(gaps(&f, id).is_empty(), "lag alone drops nothing");

    f.transcriber.release_input();
    wait_until(|| lag_events(&f.events(id)).len() == 2).await;
    assert_eq!(lag_events(&f.events(id))[1], 0);
    wait_until(|| f.transcriber.received().len() == 22).await;
}

#[tokio::test]
async fn overload_drops_the_oldest_audio_and_records_a_gap() {
    let f = fixture();
    let id = capturing(&f, &[MIC]).await;
    f.transcriber.hold_input();
    push_seconds(&f, 0, 40);
    wait_until(|| !gaps(&f, id).is_empty()).await;
    // Controls keep answering while overloaded.
    assert_eq!(f.state(id), SessionState::Capturing);
    f.transcriber.release_input();
    let (reason, start, end) = gaps(&f, id)[0];
    assert_eq!(reason, GapReason::Overload);
    assert!(
        end.saturating_sub(start) >= secs(15),
        "oldest 15 s dropped at once"
    );
    assert!(f.events(id).iter().any(|e| matches!(
        e,
        SessionEvent::Gap {
            reason: GapReason::Overload,
            ..
        }
    )));
    // The dropped audio never reaches the transcriber; the newest audio does.
    wait_until(|| f.transcriber.received().last().map(|c| c.start) == Some(secs(39))).await;
    let received = f.transcriber.received();
    assert!(received.iter().all(|c| c.start < start || c.start >= end));
    assert!(received.len() < 40);
}

#[tokio::test]
async fn provider_failure_records_a_gap_and_restarts_the_transcriber() {
    let f = fixture();
    let id = capturing(&f, &[MIC]).await;
    f.transcriber
        .emit(final_update(MIC, 0, 1, "antes da falha"));
    push_seconds(&f, 0, 4);
    wait_until(|| f.transcriber.received().len() == 4 && texts(&f, id).len() == 1).await;
    f.transcriber
        .fail_now(ProviderError::Other("crashed".into()));
    wait_until(|| f.transcriber.run_count() == 2).await;
    wait_until(|| !gaps(&f, id).is_empty()).await;
    assert_eq!(
        gaps(&f, id),
        [(GapReason::ProviderFailure, secs(1), secs(4))]
    );
    assert_eq!(
        texts(&f, id),
        ["antes da falha"],
        "finals survive the failure"
    );
    assert_eq!(f.state(id), SessionState::Capturing);
    push_seconds(&f, 4, 1);
    wait_until(|| f.transcriber.received().len() == 5).await;
}

#[tokio::test]
async fn repeated_provider_failures_fail_the_session_as_no_transcriber() {
    let f = fixture();
    f.transcriber
        .set_end(TranscriberEnd::Fail(ProviderError::Unavailable));
    // Fails before a test could observe `capturing`.
    let id = f.create_mic().id();
    f.service.start(id, true).unwrap();
    f.wait_for_state(id, SessionState::Failed).await;
    assert_eq!(f.stored(id).failure(), Some(FailureReason::NoTranscriber));
    assert_eq!(f.transcriber.run_count(), 3);
    assert!(!f.mic.is_capturing());
    assert_eq!(
        indicators(&f.events(id)).last().map(|(active, _)| *active),
        Some(false)
    );
}

#[tokio::test]
async fn stop_lets_the_transcriber_flush_its_last_final() {
    let f = fixture();
    let id = capturing(&f, &[MIC]).await;
    f.transcriber
        .emit_on_close(vec![final_update(MIC, 0, 3, "última frase")]);
    push_seconds(&f, 0, 3);
    wait_until(|| f.transcriber.received().len() == 3).await;
    f.service.stop(id).unwrap();
    f.wait_for_state(id, SessionState::Ended).await;
    assert_eq!(texts(&f, id), ["última frase"]);
    assert!(gaps(&f, id).is_empty());
}

#[tokio::test(start_paused = true)]
async fn stop_with_a_hung_transcriber_drains_for_5_s_then_records_cancelled_on_stop() {
    let f = fixture();
    f.transcriber.set_end(TranscriberEnd::Hang);
    let id = capturing(&f, &[MIC]).await;
    push_seconds(&f, 0, 6);
    tokio::task::yield_now().await;

    let session = f.service.stop(id).unwrap();
    assert_eq!(session.state(), SessionState::Stopping);
    assert!(!f.mic.is_capturing());

    let stopped_at = tokio::time::Instant::now();
    tokio::time::timeout(StdDuration::from_secs(30), async {
        while f.state(id) != SessionState::Ended {
            tokio::time::sleep(StdDuration::from_millis(50)).await;
        }
    })
    .await
    .expect("session never ended");
    assert!(stopped_at.elapsed() >= DRAIN_DEADLINE);
    assert!(stopped_at.elapsed() < DRAIN_DEADLINE + StdDuration::from_secs(1));
    assert_eq!(
        gaps(&f, id),
        [(GapReason::CancelledOnStop, secs(0), secs(6))]
    );
}

#[tokio::test]
async fn controls_respond_within_500_ms_while_the_transcriber_hangs() {
    let f = fixture();
    f.transcriber.set_end(TranscriberEnd::Hang);
    let id = f.create_mic().id();
    let started = Instant::now();
    f.service.start(id, true).unwrap();
    assert!(started.elapsed() < StdDuration::from_millis(500));
    f.wait_for_state(id, SessionState::Capturing).await;
    push_seconds(&f, 0, 2);
    let stopping = Instant::now();
    f.service.stop(id).unwrap();
    assert!(stopping.elapsed() < StdDuration::from_millis(500));
    assert_eq!(
        indicators(&f.events(id)).last().map(|(active, _)| *active),
        Some(false)
    );
}

#[tokio::test]
async fn storage_failure_reports_and_fails_the_session_keeping_prior_finals() {
    let f = fixture();
    let id = capturing(&f, &[MIC]).await;
    f.transcriber.emit(final_update(MIC, 0, 1, "gravado"));
    wait_until(|| texts(&f, id).len() == 1).await;

    f.segments
        .fail_appends_with(Some(RepositoryError::Storage("disk full".into())));
    f.transcriber.emit(final_update(MIC, 1, 2, "perdido"));
    f.wait_for_state(id, SessionState::Failed).await;
    assert_eq!(f.stored(id).failure(), Some(FailureReason::StorageError));
    assert!(
        f.events(id)
            .iter()
            .any(|e| matches!(e, SessionEvent::StorageError { .. }))
    );
    assert!(!f.mic.is_capturing());
    f.segments.fail_appends_with(None);
    assert_eq!(texts(&f, id), ["gravado"]);
}

#[tokio::test]
async fn losing_the_only_source_records_the_unfinalized_audio_as_source_lost() {
    let f = fixture();
    let id = capturing(&f, &[MIC]).await;
    f.transcriber.emit(final_update(MIC, 0, 1, "ok"));
    push_seconds(&f, 0, 3);
    wait_until(|| f.transcriber.received().len() == 3 && texts(&f, id).len() == 1).await;
    f.mic.trigger_fault(SourceFault::Lost);
    f.wait_for_state(id, SessionState::Failed).await;
    assert_eq!(gaps(&f, id), [(GapReason::SourceLost, secs(1), secs(3))]);
}

#[tokio::test]
async fn seq_continues_after_segments_already_stored() {
    let f = fixture();
    let id = capturing(&f, &[MIC]).await;
    f.transcriber.emit(final_update(MIC, 0, 1, "um"));
    f.transcriber.emit(final_update(MIC, 1, 2, "dois"));
    wait_until(|| texts(&f, id).len() == 2).await;
    let seqs: Vec<_> = stored(&f, id).iter().map(TranscriptSegment::seq).collect();
    assert_eq!(seqs, [0, 1]);
}
