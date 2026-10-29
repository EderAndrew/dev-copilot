//! Microphone and system audio together (T148, US8): each source's audio reaches the
//! transcriber tagged with its source, finals are tagged per source (FR-016), and a failing
//! system source is reported while the microphone keeps capturing (US8.3).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use devcopilot_application::events::{SessionEvent, SourceStatus};
use devcopilot_application::ports::{
    AudioChunk, AudioError, SegmentRepository, SourceFault, TranscriptUpdate,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::{AudioSourceKind, PermissionState};
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::session::SessionState;
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::Duration;
use devcopilot_domain::transcript::{SegmentKind, TranscriptSegment};
use support::{Fixture, fixture, indicators, wait_until};

const MIC: AudioSourceKind = AudioSourceKind::Microphone;
const SYSTEM: AudioSourceKind = AudioSourceKind::System;

fn one_second(source: AudioSourceKind, start_s: u64) -> AudioChunk {
    AudioChunk {
        source,
        start: Duration::from_secs(start_s),
        samples: vec![0.0; 16_000],
    }
}

fn final_update(source: AudioSourceKind, start_s: u64, text: &str) -> TranscriptUpdate {
    TranscriptUpdate::Final {
        source,
        start: Duration::from_secs(start_s),
        end: Duration::from_secs(start_s + 1),
        text: Redacted::new(SegmentText::parse(text).unwrap()),
    }
}

fn finals(f: &Fixture, id: SessionId) -> Vec<TranscriptSegment> {
    f.segments
        .list_by_session(id)
        .unwrap()
        .into_iter()
        .filter(|s| matches!(s.kind(), SegmentKind::Final { .. }))
        .collect()
}

async fn both(f: &Fixture) -> SessionId {
    let session = f.create(&[MIC, SYSTEM]);
    f.start_capturing(session.id()).await;
    session.id()
}

#[tokio::test]
async fn audio_of_both_sources_reaches_the_transcriber_tagged() {
    let f = fixture();
    let id = both(&f).await;
    assert_eq!(
        indicators(&f.events(id)).last(),
        Some(&(true, vec![MIC, SYSTEM]))
    );
    f.mic.push_chunk(one_second(MIC, 0)).unwrap();
    f.system.push_chunk(one_second(SYSTEM, 0)).unwrap();
    wait_until(|| f.transcriber.received().len() == 2).await;
    let mut sources: Vec<_> = f.transcriber.received().iter().map(|c| c.source).collect();
    sources.sort();
    assert_eq!(sources, [MIC, SYSTEM]);
}

#[tokio::test]
async fn finals_are_tagged_with_their_source() {
    let f = fixture();
    let id = both(&f).await;
    f.transcriber.emit(final_update(MIC, 0, "eu falei"));
    f.transcriber.emit(final_update(SYSTEM, 1, "eles falaram"));
    wait_until(|| finals(&f, id).len() == 2).await;
    let tagged: Vec<_> = finals(&f, id)
        .iter()
        .map(TranscriptSegment::source)
        .collect();
    assert_eq!(tagged, [Some(MIC), Some(SYSTEM)]);
    // Published finals carry the tag too.
    let published: Vec<_> = f
        .events(id)
        .into_iter()
        .filter_map(|e| match e {
            SessionEvent::FinalSegment(segment) => Some(segment.source),
            _ => None,
        })
        .collect();
    assert_eq!(published, [Some(MIC), Some(SYSTEM)]);
}

#[tokio::test]
async fn single_source_finals_are_not_tagged() {
    let f = fixture();
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    f.transcriber.emit(final_update(MIC, 0, "só microfone"));
    wait_until(|| finals(&f, session.id()).len() == 1).await;
    assert_eq!(finals(&f, session.id())[0].source(), None);
}

#[tokio::test]
async fn system_source_lost_keeps_the_microphone() {
    let f = fixture();
    let id = both(&f).await;
    f.system.trigger_fault(SourceFault::Lost);
    wait_until(|| !f.system.is_capturing()).await;
    wait_until(|| indicators(&f.events(id)).last() == Some(&(true, vec![MIC]))).await;
    assert!(f.events(id).contains(&SessionEvent::SourceStatus {
        source: SYSTEM,
        status: SourceStatus::Lost,
    }));
    assert_eq!(f.state(id), SessionState::Capturing);
    // The microphone is still transcribed.
    f.mic.push_chunk(one_second(MIC, 3)).unwrap();
    wait_until(|| f.transcriber.received().iter().any(|c| c.source == MIC)).await;
    f.transcriber.emit(final_update(MIC, 3, "continua"));
    wait_until(|| finals(&f, id).len() == 1).await;
    assert_eq!(finals(&f, id)[0].source(), Some(MIC));
}

#[tokio::test]
async fn system_permission_revoked_keeps_the_microphone() {
    let f = fixture();
    let id = both(&f).await;
    f.system.trigger_fault(SourceFault::PermissionRevoked);
    wait_until(|| indicators(&f.events(id)).last() == Some(&(true, vec![MIC]))).await;
    assert!(f.events(id).contains(&SessionEvent::SourceStatus {
        source: SYSTEM,
        status: SourceStatus::PermissionRevoked,
    }));
    assert!(f.mic.is_capturing());
}

#[tokio::test]
async fn system_failing_to_open_keeps_the_microphone() {
    let f = fixture();
    let session = f.create(&[MIC, SYSTEM]);
    f.system
        .fail_start_with(Some(AudioError::PermissionNotGranted(
            PermissionState::Denied,
        )));
    f.start_capturing(session.id()).await;
    assert!(f.mic.is_capturing());
    assert!(
        f.events(session.id())
            .contains(&SessionEvent::SourceStatus {
                source: SYSTEM,
                status: SourceStatus::PermissionRevoked,
            })
    );
}

#[tokio::test]
async fn losing_both_sources_fails_the_session() {
    let f = fixture();
    let id = both(&f).await;
    f.system.trigger_fault(SourceFault::Lost);
    wait_until(|| indicators(&f.events(id)).last() == Some(&(true, vec![MIC]))).await;
    f.mic.trigger_fault(SourceFault::Lost);
    f.wait_for_state(id, SessionState::Failed).await;
}
