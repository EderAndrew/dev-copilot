//! Transcription timings of the opt-in metrics mode (002 T013, research R7): one timing per
//! partial and final, in order, on the session timeline, and none once the stage stops.
//! Cancellation (the stop drain deadline aborting the stage) is covered by the unit test
//! `no_timing_once_the_stage_is_closed` in `src/transcription.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::sync::Arc;
use std::time::Duration as StdDuration;

use devcopilot_application::events::SessionEvent;
use devcopilot_application::ports::{AudioChunk, TimingKind, Transcriber, TranscriptUpdate};
use devcopilot_application::testing::RecordingMetricsSink;
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::session::SessionState;
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::Duration;
use support::{Fixture, fixture, fixture_with_metrics, wait_until};

const MIC: AudioSourceKind = AudioSourceKind::Microphone;

fn partial(start_ms: u64) -> TranscriptUpdate {
    TranscriptUpdate::Partial {
        source: MIC,
        start: Duration::from_millis(start_ms),
        text: Redacted::new("parcial".into()),
    }
}

fn final_update(start_ms: u64, end_ms: u64) -> TranscriptUpdate {
    TranscriptUpdate::Final {
        source: MIC,
        start: Duration::from_millis(start_ms),
        end: Duration::from_millis(end_ms),
        text: Redacted::new(SegmentText::parse("texto final").unwrap()),
    }
}

fn with_sink() -> (Fixture, Arc<RecordingMetricsSink>) {
    let sink = Arc::new(RecordingMetricsSink::default());
    (fixture_with_metrics(sink.clone()), sink)
}

async fn capturing(f: &Fixture) -> SessionId {
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    id
}

fn finals(f: &Fixture, id: SessionId) -> usize {
    f.events(id)
        .iter()
        .filter(|e| matches!(e, SessionEvent::FinalSegment(_)))
        .count()
}

#[tokio::test]
async fn one_timing_per_partial_and_final_in_order() {
    let (f, sink) = with_sink();
    capturing(&f).await;
    f.mic
        .push_chunk(AudioChunk {
            source: MIC,
            start: Duration::ZERO,
            samples: vec![0.0; 16_000],
        })
        .unwrap();
    wait_until(|| !f.transcriber.received().is_empty()).await;
    f.transcriber.emit(partial(0));
    f.transcriber.emit(partial(0));
    f.transcriber.emit(final_update(0, 2_000));
    f.transcriber.emit(partial(2_500));
    f.transcriber.emit(final_update(2_500, 4_000));
    wait_until(|| sink.timings().len() == 5).await;

    let timings = sink.timings();
    let kinds: Vec<TimingKind> = timings.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        [
            TimingKind::Partial,
            TimingKind::Partial,
            TimingKind::Final,
            TimingKind::Partial,
            TimingKind::Final,
        ]
    );
    let provider = f.transcriber.descriptor().id;
    assert!(timings.iter().all(|t| t.provider == provider));
    assert_eq!(
        (timings[2].speech_start_ms, timings[2].speech_end_ms),
        (0, 2_000)
    );
    assert_eq!(
        (timings[4].speech_start_ms, timings[4].speech_end_ms),
        (2_500, 4_000)
    );
    // A partial ends where the audio handed to the transcriber ends (1 s pushed).
    assert_eq!(
        (timings[0].speech_start_ms, timings[0].speech_end_ms),
        (0, 1_000)
    );
    assert!(
        timings
            .windows(2)
            .all(|w| w[0].emitted_ms <= w[1].emitted_ms),
        "emission instants are monotonic"
    );
}

#[tokio::test]
async fn speech_end_is_never_before_speech_start() {
    let (f, sink) = with_sink();
    let id = capturing(&f).await;
    // A partial ahead of all audio handed over: its end is clamped to its start.
    f.transcriber.emit(partial(5_000));
    f.transcriber.emit(final_update(5_000, 6_000));
    f.transcriber.emit(partial(0));
    wait_until(|| sink.timings().len() == 3).await;
    assert_eq!(finals(&f, id), 1);
    for timing in sink.timings() {
        assert!(timing.speech_end_ms >= timing.speech_start_ms, "{timing:?}");
    }
    assert_eq!(sink.timings()[0].speech_end_ms, 5_000);
}

#[tokio::test]
async fn without_a_sink_transcription_is_unchanged() {
    let f = fixture();
    let id = capturing(&f).await;
    f.transcriber.emit(partial(0));
    f.transcriber.emit(final_update(0, 1_000));
    wait_until(|| finals(&f, id) == 1).await;
    assert!(
        f.events(id)
            .iter()
            .any(|e| matches!(e, SessionEvent::PartialSegment { .. }))
    );
}

#[tokio::test]
async fn timings_stop_after_stop() {
    let (f, sink) = with_sink();
    let id = capturing(&f).await;
    f.transcriber.emit(final_update(0, 1_000));
    wait_until(|| sink.timings().len() == 1).await;

    f.service.stop(id).unwrap();
    f.wait_for_state(id, SessionState::Ended).await;
    f.transcriber.emit(partial(1_000));
    f.transcriber.emit(final_update(1_000, 2_000));
    tokio::time::sleep(StdDuration::from_millis(50)).await;
    assert_eq!(sink.timings().len(), 1, "no timing after the session ended");
}

#[tokio::test]
async fn timings_stop_while_paused() {
    let (f, sink) = with_sink();
    let id = capturing(&f).await;
    f.transcriber.emit(partial(0));
    wait_until(|| sink.timings().len() == 1).await;

    // Pausing cancels this capture's transcription once it drains.
    f.service.pause(id).unwrap();
    f.wait_for_state(id, SessionState::Paused).await;
    tokio::time::sleep(StdDuration::from_millis(20)).await;
    let before = sink.timings().len();
    f.transcriber.emit(partial(500));
    tokio::time::sleep(StdDuration::from_millis(50)).await;
    assert_eq!(sink.timings().len(), before, "no timing while paused");
}
