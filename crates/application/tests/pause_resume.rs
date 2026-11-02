//! Pause and resume (T143, US9, FR-006a): pausing closes every source and stops capture,
//! resuming re-checks transcriber, sources and permissions, the paused interval is stored as
//! `gap(paused)`, stop works from `paused`, and the active duration excludes pauses.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use devcopilot_application::ApplicationError;
use devcopilot_application::events::{SessionEvent, SourceStatus};
use devcopilot_application::ports::TranscriptUpdate;
use devcopilot_application::ports::{AudioChunk, SegmentRepository, SinkError};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::{
    AudioSourceKind, Availability, PermissionState, UnavailableReason,
};
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::session::SessionState;
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::Duration;
use devcopilot_domain::transcript::{GapReason, SegmentKind, TranscriptSegment};
use support::{Fixture, fixture, indicators, states, wait_until};

const MIC: AudioSourceKind = AudioSourceKind::Microphone;
const SYSTEM: AudioSourceKind = AudioSourceKind::System;

fn one_second(source: AudioSourceKind, start_s: u64) -> AudioChunk {
    AudioChunk {
        source,
        start: Duration::from_secs(start_s),
        samples: vec![0.0; 16_000],
    }
}

fn final_update(start_s: u64, end_s: u64, text: &str) -> TranscriptUpdate {
    TranscriptUpdate::Final {
        source: MIC,
        start: Duration::from_secs(start_s),
        end: Duration::from_secs(end_s),
        text: Redacted::new(SegmentText::parse(text).unwrap()),
    }
}

fn transcript(f: &Fixture, id: SessionId) -> Vec<TranscriptSegment> {
    f.segments.list_by_session(id).unwrap()
}

fn pause_gaps(f: &Fixture, id: SessionId) -> Vec<(Duration, Duration)> {
    transcript(f, id)
        .into_iter()
        .filter(|s| {
            matches!(
                s.kind(),
                SegmentKind::Gap {
                    reason: GapReason::Paused
                }
            )
        })
        .map(|s| (s.start(), s.end()))
        .collect()
}

async fn capturing(f: &Fixture) -> SessionId {
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    session.id()
}

/// Last `capture_indicator` as `(active, paused, sources)`.
fn last_indicator(f: &Fixture, id: SessionId) -> Option<(bool, bool, Vec<AudioSourceKind>)> {
    f.events(id).into_iter().rev().find_map(|e| match e {
        SessionEvent::CaptureIndicator {
            active,
            paused,
            sources,
        } => Some((active, paused, sources)),
        _ => None,
    })
}

/// Waits until the resumed capture has its sources open and the indicator is back on.
async fn wait_resumed(f: &Fixture, id: SessionId) {
    wait_until(|| {
        f.mic.is_capturing()
            && last_indicator(f, id).is_some_and(|(active, paused, _)| active && !paused)
    })
    .await;
}

#[tokio::test]
async fn pause_closes_every_source_and_shows_paused() {
    let f = fixture();
    let session = f.create(&[MIC, SYSTEM]);
    let id = session.id();
    f.start_capturing(id).await;
    f.clock.advance(Duration::from_secs(10));
    let paused = f.service.pause(id).unwrap();
    assert_eq!(paused.state(), SessionState::Paused);
    // Closed before `pause` returned: nothing more can be captured.
    assert!(!f.mic.is_capturing() && !f.system.is_capturing());
    assert_eq!(
        f.mic.push_chunk(one_second(MIC, 10)),
        Err(SinkError::Closed)
    );
    assert_eq!(
        last_indicator(&f, id),
        Some((true, true, vec![MIC, SYSTEM]))
    );
    assert_eq!(f.stored(id).state(), SessionState::Paused);
    assert_eq!(states(&f.events(id)).last(), Some(&SessionState::Paused));
}

#[tokio::test]
async fn audio_before_the_pause_is_still_transcribed_and_nothing_during_it() {
    let f = fixture();
    let id = capturing(&f).await;
    f.mic.push_chunk(one_second(MIC, 0)).unwrap();
    wait_until(|| f.transcriber.received().len() == 1).await;
    f.transcriber
        .emit_on_close(vec![final_update(0, 1, "antes da pausa")]);
    f.service.pause(id).unwrap();
    // The pre-pause window is flushed when its input closes.
    wait_until(|| transcript(&f, id).len() == 1).await;
    let received = f.transcriber.received().len();
    f.clock.advance(Duration::from_secs(30));
    assert!(f.mic.push_chunk(one_second(MIC, 1)).is_err());
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(f.transcriber.received().len(), received);
}

#[tokio::test]
async fn resume_reopens_sources_and_records_the_paused_interval() {
    let f = fixture();
    let id = capturing(&f).await;
    f.clock.advance(Duration::from_secs(10));
    f.service.pause(id).unwrap();
    f.clock.advance(Duration::from_secs(30));
    let resumed = f.service.resume(id).unwrap();
    assert_eq!(resumed.state(), SessionState::Capturing);
    wait_resumed(&f, id).await;
    assert_eq!(f.mic.start_count(), 2);
    assert_eq!(
        pause_gaps(&f, id),
        [(Duration::from_secs(10), Duration::from_secs(40))]
    );
    assert!(f.events(id).iter().any(|e| matches!(
        e,
        SessionEvent::Gap {
            reason: GapReason::Paused,
            duration,
            ..
        } if *duration == Duration::from_secs(30)
    )));
    // Audio after the resume is placed after the gap.
    f.mic.push_chunk(one_second(MIC, 0)).unwrap();
    wait_until(|| {
        f.transcriber
            .received()
            .last()
            .is_some_and(|c| c.start == Duration::from_secs(40))
    })
    .await;
}

#[tokio::test]
async fn active_duration_excludes_pauses() {
    let f = fixture();
    let id = capturing(&f).await;
    f.clock.advance(Duration::from_secs(10));
    f.service.pause(id).unwrap();
    f.clock.advance(Duration::from_secs(60));
    f.service.resume(id).unwrap();
    wait_resumed(&f, id).await;
    f.clock.advance(Duration::from_secs(5));
    f.service.stop(id).unwrap();
    f.wait_for_state(id, SessionState::Ended).await;
    assert_eq!(f.stored(id).active_duration(), Duration::from_secs(15));
}

#[tokio::test]
async fn stop_from_paused_ends_without_capturing_again() {
    let f = fixture();
    let id = capturing(&f).await;
    f.service.pause(id).unwrap();
    let stopping = f.service.stop(id).unwrap();
    assert_eq!(stopping.state(), SessionState::Stopping);
    f.wait_for_state(id, SessionState::Ended).await;
    assert_eq!(f.mic.start_count(), 1);
    assert!(!f.mic.is_capturing());
    assert!(pause_gaps(&f, id).is_empty());
}

#[tokio::test]
async fn resume_with_revoked_permission_stays_paused() {
    let f = fixture();
    let id = capturing(&f).await;
    f.service.pause(id).unwrap();
    f.mic
        .set_permission(PermissionState::Revoked, PermissionState::Revoked);
    assert_eq!(
        f.service.resume(id).map(|_| ()),
        Err(ApplicationError::PermissionDenied)
    );
    assert_eq!(f.state(id), SessionState::Paused);
    assert_eq!(f.mic.start_count(), 1);
    assert!(f.events(id).iter().any(|e| matches!(
        e,
        SessionEvent::SourceStatus {
            source: MIC,
            status: SourceStatus::PermissionRevoked
        }
    )));
    // Fixed in the OS: resuming works.
    f.mic
        .set_permission(PermissionState::Granted, PermissionState::Granted);
    f.service.resume(id).unwrap();
    wait_resumed(&f, id).await;
}

#[tokio::test]
async fn resume_skips_a_source_lost_during_the_pause() {
    let f = fixture();
    let session = f.create(&[MIC, SYSTEM]);
    let id = session.id();
    f.start_capturing(id).await;
    f.service.pause(id).unwrap();
    f.system
        .set_availability(Availability::Unavailable(UnavailableReason::NoDevice));
    f.service.resume(id).unwrap();
    wait_resumed(&f, id).await;
    assert!(!f.system.is_capturing());
    assert_eq!(f.system.start_count(), 1);
    assert!(f.events(id).iter().any(|e| matches!(
        e,
        SessionEvent::SourceStatus {
            source: SYSTEM,
            status: SourceStatus::Lost
        }
    )));
    assert_eq!(indicators(&f.events(id)).last(), Some(&(true, vec![MIC])));
}

#[tokio::test]
async fn resume_rechecks_the_transcriber() {
    let f = fixture();
    let id = capturing(&f).await;
    f.service.pause(id).unwrap();
    f.transcribers.set_unavailable(true);
    assert_eq!(
        f.service.resume(id).map(|_| ()),
        Err(ApplicationError::NoTranscriber)
    );
    assert_eq!(f.state(id), SessionState::Paused);
    f.secrets.set_available(false);
    f.transcribers.set_unavailable(false);
    assert_eq!(
        f.service.resume(id).map(|_| ()),
        Err(ApplicationError::SecretStoreUnavailable)
    );
}

#[tokio::test]
async fn pause_and_resume_are_refused_in_other_states() {
    let f = fixture();
    let session = f.create_mic();
    assert!(matches!(
        f.service.pause(session.id()),
        Err(ApplicationError::InvalidState(_))
    ));
    f.start_capturing(session.id()).await;
    assert!(matches!(
        f.service.resume(session.id()),
        Err(ApplicationError::InvalidState(_))
    ));
    f.service.pause(session.id()).unwrap();
    assert!(matches!(
        f.service.pause(session.id()),
        Err(ApplicationError::InvalidState(_))
    ));
    assert_eq!(
        f.service.pause(SessionId::new()).map(|_| ()),
        Err(ApplicationError::NotFound)
    );
}

#[tokio::test]
async fn repeated_pauses_keep_segments_in_order() {
    let f = fixture();
    let id = capturing(&f).await;
    for _ in 0..3 {
        f.clock.advance(Duration::from_secs(5));
        f.service.pause(id).unwrap();
        f.clock.advance(Duration::from_secs(5));
        f.service.resume(id).unwrap();
        wait_resumed(&f, id).await;
    }
    let gaps = pause_gaps(&f, id);
    assert_eq!(gaps.len(), 3);
    assert_eq!(gaps[2], (Duration::from_secs(25), Duration::from_secs(30)));
    let seqs: Vec<u64> = transcript(&f, id)
        .iter()
        .map(TranscriptSegment::seq)
        .collect();
    assert_eq!(seqs, (0..seqs.len() as u64).collect::<Vec<_>>());
}
