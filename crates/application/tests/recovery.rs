//! Startup recovery (T042): sessions left active by an unexpected exit become
//! `failed(interrupted)` and get a trailing `gap(interrupted)` (FR-007, US2.3, research R15).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::session::{FailureReason, Session, SessionState};
use devcopilot_domain::text::Title;
use devcopilot_domain::time::{Duration, Timestamp};
use support::fixture;

fn ts(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).unwrap()
}

/// A session driven to `state` as it would have been stored before a crash.
fn stored_in(state: SessionState) -> Session {
    let mut session = Session::new(
        SessionId::new(),
        Title::parse("Sessão anterior").unwrap(),
        SessionLanguage::PtBr,
        &[AudioSourceKind::Microphone],
        ts(1_000),
    )
    .unwrap();
    let steps: &[fn(&mut Session)] = match state {
        SessionState::Idle => &[],
        SessionState::Starting => &[|s| s.start().unwrap()],
        SessionState::Capturing => &[|s| s.start().unwrap(), |s| s.ready(ts(2_000)).unwrap()],
        SessionState::Paused => &[
            |s| s.start().unwrap(),
            |s| s.ready(ts(2_000)).unwrap(),
            |s| s.pause(ts(5_000)).unwrap(),
        ],
        SessionState::Stopping => &[
            |s| s.start().unwrap(),
            |s| s.ready(ts(2_000)).unwrap(),
            |s| s.stop(ts(6_000)).unwrap(),
        ],
        SessionState::Ended => &[
            |s| s.start().unwrap(),
            |s| s.ready(ts(2_000)).unwrap(),
            |s| s.stop(ts(6_000)).unwrap(),
            |s| s.drained(ts(7_000)).unwrap(),
        ],
        SessionState::Failed => &[
            |s| s.start().unwrap(),
            |s| s.fail(FailureReason::NoTranscriber, ts(2_000)).unwrap(),
        ],
    };
    for step in steps {
        step(&mut session);
    }
    // Round-trip through the stored form, as the repository would.
    Session::restore(session.snapshot()).unwrap()
}

#[tokio::test]
async fn active_sessions_become_failed_interrupted() {
    let f = fixture();
    let by_state: Vec<_> = SessionState::ALL
        .into_iter()
        .map(|state| {
            let session = stored_in(state);
            f.sessions.seed(session.clone());
            (state, session)
        })
        .collect();

    let mut recovered = f.service.recover_on_startup().unwrap();
    recovered.sort();
    let mut expected: Vec<_> = by_state
        .iter()
        .filter(|(state, _)| state.is_active())
        .map(|(_, s)| s.id())
        .collect();
    expected.sort();
    assert_eq!(recovered, expected);

    for (state, session) in &by_state {
        let after = f.stored(session.id());
        if state.is_active() {
            assert_eq!(after.state(), SessionState::Failed, "{state:?}");
            assert_eq!(
                after.failure(),
                Some(FailureReason::Interrupted),
                "{state:?}"
            );
        } else {
            assert_eq!(&after, session, "{state:?} must be untouched");
        }
    }
}

#[tokio::test]
async fn trailing_gap_only_for_sessions_that_captured() {
    let f = fixture();
    let starting = stored_in(SessionState::Starting);
    let capturing = stored_in(SessionState::Capturing);
    let paused = stored_in(SessionState::Paused);
    let stopping = stored_in(SessionState::Stopping);
    for session in [&starting, &capturing, &paused, &stopping] {
        f.sessions.seed(session.clone());
    }
    f.service.recover_on_startup().unwrap();

    let mut gaps = f.sessions.interrupted_gaps();
    gaps.sort();
    let mut expected = vec![capturing.id(), paused.id(), stopping.id()];
    expected.sort();
    assert_eq!(gaps, expected);
    assert_eq!(f.stored(starting.id()).ended_at(), None);
}

#[tokio::test]
async fn recovery_keeps_accumulated_active_time_and_is_idempotent() {
    let f = fixture();
    let paused = stored_in(SessionState::Paused);
    f.sessions.seed(paused.clone());
    f.clock.set(ts(60_000));
    f.service.recover_on_startup().unwrap();
    let after = f.stored(paused.id());
    assert_eq!(after.active_duration(), Duration::from_millis(3_000));
    assert_eq!(after.ended_at(), Some(ts(60_000)));
    assert!(f.service.recover_on_startup().unwrap().is_empty());
    assert_eq!(f.sessions.interrupted_gaps().len(), 1);
}

#[tokio::test]
async fn recovered_session_can_no_longer_capture() {
    let f = fixture();
    let capturing = stored_in(SessionState::Capturing);
    f.sessions.seed(capturing.clone());
    f.service.recover_on_startup().unwrap();
    assert!(f.service.start(capturing.id(), true).is_err());
    assert!(f.service.stop(capturing.id()).is_err());
    assert_eq!(f.mic.start_count(), 0);
}
