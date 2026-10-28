//! Session use cases with application fakes (T041): start preconditions, single active
//! session, stop timing, cancel during start, and source faults (FR-002–FR-012, FR-039a,
//! FR-045a, SC-002, SC-004).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use devcopilot_application::ApplicationError;
use devcopilot_application::events::{SessionEvent, SourceStatus};
use devcopilot_application::ports::{AudioError, RepositoryError, SourceFault};
use devcopilot_application::session_service::NewSession;
use devcopilot_domain::audio_source::{
    AudioSourceKind, Availability, PermissionState, UnavailableReason,
};
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::session::{FailureReason, SessionState};
use devcopilot_domain::text::Title;
use devcopilot_domain::time::Duration;
use support::{fixture, indicators, states, wait_until};

const MIC: AudioSourceKind = AudioSourceKind::Microphone;
const SYSTEM: AudioSourceKind = AudioSourceKind::System;

// --- creation ----------------------------------------------------------------------------------

#[tokio::test]
async fn created_session_is_idle_and_captures_nothing() {
    let f = fixture();
    let session = f.create_mic();
    assert_eq!(session.state(), SessionState::Idle);
    assert_eq!(f.stored(session.id()).state(), SessionState::Idle);
    assert_eq!(f.mic.start_count(), 0);
    assert_eq!(f.mic.permission_request_count(), 0);
    assert!(f.events(session.id()).is_empty());
}

#[tokio::test]
async fn create_rejects_unavailable_or_invalid_sources() {
    let f = fixture();
    f.system.set_availability(Availability::Unavailable(
        UnavailableReason::NotValidatedOnPlatform,
    ));
    let new = |sources: Vec<AudioSourceKind>| NewSession {
        title: Title::parse("t").unwrap(),
        language: SessionLanguage::En,
        sources,
    };
    assert_eq!(
        f.service.create(new(vec![MIC, SYSTEM])).unwrap_err(),
        ApplicationError::SourceUnavailable
    );
    assert!(matches!(
        f.service.create(new(vec![])).unwrap_err(),
        ApplicationError::InvalidInput {
            field: "sources",
            ..
        }
    ));
    assert!(f.service.list().unwrap().is_empty());
}

#[tokio::test]
async fn sources_report_availability_and_permission() {
    let f = fixture();
    f.mic
        .set_permission(PermissionState::NotRequested, PermissionState::Granted);
    f.system.set_availability(Availability::Unavailable(
        UnavailableReason::NotValidatedOnPlatform,
    ));
    let sources = f.service.sources();
    assert_eq!(sources.len(), 2);
    assert_eq!(sources[0].kind, MIC);
    assert_eq!(sources[0].permission, PermissionState::NotRequested);
    assert!(!sources[1].availability.is_available());
    assert_eq!(
        f.service.request_permission(MIC).await,
        Ok(PermissionState::Granted)
    );
    assert_eq!(
        f.service.request_permission(SYSTEM).await,
        Err(ApplicationError::SourceUnavailable)
    );
}

// --- start preconditions -----------------------------------------------------------------------

#[tokio::test]
async fn start_blocked_without_permission() {
    let f = fixture();
    let session = f.create_mic();
    for denied in [PermissionState::Denied, PermissionState::Revoked] {
        f.mic.set_permission(denied, denied);
        assert_eq!(
            f.service.start(session.id(), true).unwrap_err(),
            ApplicationError::PermissionDenied
        );
    }
    assert_eq!(f.state(session.id()), SessionState::Idle);
    assert_eq!(f.mic.start_count(), 0);
    assert!(f.events(session.id()).is_empty());
}

#[tokio::test]
async fn start_blocked_without_transcriber() {
    let f = support::fixture_without_transcriber();
    let session = f.create_mic();
    assert_eq!(
        f.service.start(session.id(), true).unwrap_err(),
        ApplicationError::NoTranscriber
    );
    assert_eq!(f.state(session.id()), SessionState::Idle);
    assert_eq!(f.mic.start_count(), 0);
    f.add_transcriber();
    f.start_capturing(session.id()).await;
}

#[tokio::test]
async fn start_blocked_when_secret_store_unavailable() {
    let f = fixture();
    let session = f.create_mic();
    f.secrets.set_available(false);
    assert_eq!(
        f.service.start(session.id(), true).unwrap_err(),
        ApplicationError::SecretStoreUnavailable
    );
    assert_eq!(f.state(session.id()), SessionState::Idle);
    assert_eq!(f.mic.start_count(), 0);
}

#[tokio::test]
async fn start_requires_consent_until_reminder_dismissed() {
    let f = fixture();
    let session = f.create_mic();
    assert_eq!(
        f.service.start(session.id(), false).unwrap_err(),
        ApplicationError::InvalidInput {
            field: "consent_acknowledged",
            problem: "required"
        }
    );
    assert_eq!(f.state(session.id()), SessionState::Idle);
    f.dismiss_consent_reminder();
    f.service.start(session.id(), false).unwrap();
    f.wait_for_state(session.id(), SessionState::Capturing)
        .await;
}

#[tokio::test]
async fn only_one_session_can_be_active() {
    let f = fixture();
    let first = f.create_mic();
    let second = f.create_mic();
    f.start_capturing(first.id()).await;
    assert!(matches!(
        f.service.start(second.id(), true).unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
    assert!(matches!(
        f.service.start(first.id(), true).unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
    assert_eq!(f.state(second.id()), SessionState::Idle);
    assert_eq!(f.mic.start_count(), 1);

    f.service.stop(first.id()).unwrap();
    f.wait_for_state(first.id(), SessionState::Ended).await;
    f.start_capturing(second.id()).await;
}

#[tokio::test]
async fn start_unknown_or_terminal_session_is_rejected() {
    let f = fixture();
    assert_eq!(
        f.service.start(SessionId::new(), true).unwrap_err(),
        ApplicationError::NotFound
    );
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    f.service.stop(session.id()).unwrap();
    f.wait_for_state(session.id(), SessionState::Ended).await;
    assert!(matches!(
        f.service.start(session.id(), true).unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
}

// --- start flow --------------------------------------------------------------------------------

#[tokio::test]
async fn start_with_granted_permission_reaches_capturing() {
    let f = fixture();
    let session = f.create_mic();
    let returned = f.service.start(session.id(), true).unwrap();
    assert_eq!(returned.state(), SessionState::Starting);
    f.wait_for_state(session.id(), SessionState::Capturing)
        .await;
    assert!(f.mic.is_capturing());
    assert_eq!(f.mic.permission_request_count(), 0);
    let events = f.events(session.id());
    assert_eq!(
        states(&events),
        [SessionState::Starting, SessionState::Capturing]
    );
    assert_eq!(indicators(&events), [(true, vec![MIC])]);
    let stored = f.stored(session.id());
    assert_eq!(stored.state(), SessionState::Capturing);
    assert!(stored.started_at().is_some());
}

#[tokio::test]
async fn start_asks_permission_and_captures_only_after_grant() {
    let f = fixture();
    let session = f.create_mic();
    f.mic
        .set_permission(PermissionState::NotRequested, PermissionState::Granted);
    f.mic.hold_permission_prompt();
    f.service.start(session.id(), true).unwrap();
    wait_until(|| f.mic.permission_request_count() == 1).await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(f.state(session.id()), SessionState::Starting);
    assert_eq!(f.mic.start_count(), 0, "captured before the grant");
    f.mic.answer_permission_prompt();
    f.wait_for_state(session.id(), SessionState::Capturing)
        .await;
    assert_eq!(f.mic.start_count(), 1);
}

#[tokio::test]
async fn permission_denied_at_prompt_fails_without_capture() {
    let f = fixture();
    let session = f.create_mic();
    f.mic
        .set_permission(PermissionState::NotRequested, PermissionState::Denied);
    f.service.start(session.id(), true).unwrap();
    f.wait_for_state(session.id(), SessionState::Failed).await;
    let stored = f.stored(session.id());
    assert_eq!(stored.failure(), Some(FailureReason::PermissionDenied));
    assert_eq!(f.mic.start_count(), 0);
    assert!(
        indicators(&f.events(session.id())).is_empty(),
        "indicator never turned on"
    );
}

#[tokio::test]
async fn second_source_failing_to_open_closes_the_first() {
    let f = fixture();
    let session = f.create(&[MIC, SYSTEM]);
    f.system.fail_start_with(Some(AudioError::DeviceLost));
    f.service.start(session.id(), true).unwrap();
    f.wait_for_state(session.id(), SessionState::Failed).await;
    assert_eq!(
        f.stored(session.id()).failure(),
        Some(FailureReason::SourceUnavailable)
    );
    assert_eq!(f.mic.start_count(), 1);
    assert!(!f.mic.is_capturing());
}

#[tokio::test]
async fn cancel_during_starting_returns_to_idle_and_opens_nothing() {
    let f = fixture();
    let session = f.create_mic();
    f.mic
        .set_permission(PermissionState::NotRequested, PermissionState::Granted);
    f.mic.hold_permission_prompt();
    f.service.start(session.id(), true).unwrap();
    wait_until(|| f.mic.permission_request_count() == 1).await;

    let cancelled = f.service.cancel_start(session.id()).unwrap();
    assert_eq!(cancelled.state(), SessionState::Idle);
    f.mic.answer_permission_prompt();
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    assert_eq!(f.state(session.id()), SessionState::Idle);
    assert_eq!(f.stored(session.id()).state(), SessionState::Idle);
    assert_eq!(f.mic.start_count(), 0);
    assert!(!f.mic.is_capturing());
    assert_eq!(
        states(&f.events(session.id())),
        [SessionState::Starting, SessionState::Idle]
    );
    // The session can be started again afterwards.
    f.start_capturing(session.id()).await;
}

#[tokio::test]
async fn cancel_start_outside_starting_is_invalid_state() {
    let f = fixture();
    let session = f.create_mic();
    assert!(matches!(
        f.service.cancel_start(session.id()).unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
    f.start_capturing(session.id()).await;
    assert!(matches!(
        f.service.cancel_start(session.id()).unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
    assert_eq!(
        f.service.cancel_start(SessionId::new()).unwrap_err(),
        ApplicationError::NotFound
    );
}

// --- stop ----------------------------------------------------------------------------------------

#[tokio::test]
async fn stop_turns_indicator_off_and_closes_sources_before_returning() {
    let f = fixture();
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    f.clock.advance(Duration::from_secs(90));

    let before = f.events(session.id()).len();
    let stopping = f.service.stop(session.id()).unwrap();
    // Synchronously on return: no wait on drain or providers (SC-002, SC-006).
    assert_eq!(stopping.state(), SessionState::Stopping);
    assert!(!f.mic.is_capturing(), "source still open after stop");
    let after_stop = f.events(session.id())[before..].to_vec();
    assert_eq!(
        after_stop.first(),
        Some(&SessionEvent::CaptureIndicator {
            active: false,
            paused: false,
            sources: vec![]
        })
    );
    assert_eq!(states(&after_stop), [SessionState::Stopping]);

    f.wait_for_state(session.id(), SessionState::Ended).await;
    let stored = f.stored(session.id());
    assert_eq!(stored.state(), SessionState::Ended);
    assert_eq!(stored.active_duration(), Duration::from_secs(90));
    assert!(stored.ended_at().is_some());
    assert_eq!(
        states(&f.events(session.id())),
        [
            SessionState::Starting,
            SessionState::Capturing,
            SessionState::Stopping,
            SessionState::Ended
        ]
    );
}

#[tokio::test]
async fn stop_outside_capture_is_invalid_state() {
    let f = fixture();
    let session = f.create_mic();
    assert!(matches!(
        f.service.stop(session.id()).unwrap_err(),
        ApplicationError::InvalidState(_)
    ));
    assert_eq!(
        f.service.stop(SessionId::new()).unwrap_err(),
        ApplicationError::NotFound
    );
}

#[tokio::test]
async fn stop_persist_failure_is_reported_and_capture_still_stops() {
    let f = fixture();
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    f.sessions
        .fail_updates_with(Some(RepositoryError::Storage("disk full".into())));
    f.service.stop(session.id()).unwrap();
    assert!(!f.mic.is_capturing());
    assert!(
        f.events(session.id())
            .iter()
            .any(|e| matches!(e, SessionEvent::StorageError { .. }))
    );
}

// --- source faults -------------------------------------------------------------------------------

#[tokio::test]
async fn sole_source_loss_fails_session_and_keeps_content() {
    let f = fixture();
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    f.mic.trigger_fault(SourceFault::Lost);
    f.wait_for_state(session.id(), SessionState::Failed).await;

    let stored = f.stored(session.id());
    assert_eq!(stored.failure(), Some(FailureReason::SourceUnavailable));
    assert!(stored.started_at().is_some());
    assert!(!f.mic.is_capturing());
    let events = f.events(session.id());
    assert!(events.contains(&SessionEvent::SourceStatus {
        source: MIC,
        status: SourceStatus::Lost
    }));
    assert_eq!(indicators(&events).last(), Some(&(false, vec![])));
    // Content is kept: the session is still listed.
    assert_eq!(f.service.list().unwrap().len(), 1);
}

#[tokio::test]
async fn permission_revoked_mid_capture_stops_that_source() {
    let f = fixture();
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    f.mic.trigger_fault(SourceFault::PermissionRevoked);
    f.wait_for_state(session.id(), SessionState::Failed).await;

    assert!(!f.mic.is_capturing());
    assert_eq!(
        f.stored(session.id()).failure(),
        Some(FailureReason::PermissionDenied)
    );
    assert!(
        f.events(session.id())
            .contains(&SessionEvent::SourceStatus {
                source: MIC,
                status: SourceStatus::PermissionRevoked
            })
    );
}

#[tokio::test]
async fn losing_one_of_two_sources_keeps_capturing_with_the_other() {
    let f = fixture();
    let session = f.create(&[MIC, SYSTEM]);
    f.start_capturing(session.id()).await;
    f.system.trigger_fault(SourceFault::Lost);
    wait_until(|| indicators(&f.events(session.id())).len() == 2).await;

    assert_eq!(f.state(session.id()), SessionState::Capturing);
    assert!(f.mic.is_capturing());
    assert!(!f.system.is_capturing());
    assert_eq!(
        indicators(&f.events(session.id())),
        [(true, vec![MIC, SYSTEM]), (true, vec![MIC])]
    );
}

#[tokio::test]
async fn default_device_change_never_switches_device() {
    // A default-device change on the platform is reported by the adapter as `Lost` for the
    // bound device (FR-011); the pipeline must not reopen the source on another device.
    let f = fixture();
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    f.mic.trigger_fault(SourceFault::Lost);
    f.wait_for_state(session.id(), SessionState::Failed).await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(f.mic.start_count(), 1, "source was reopened");
    assert!(!f.mic.is_capturing());
}

#[tokio::test]
async fn os_suspend_is_source_loss_without_auto_resume() {
    let f = fixture();
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    // Suspend/lock: the OS stops the stream; the adapter reports it as lost.
    f.mic.trigger_fault(SourceFault::Lost);
    f.wait_for_state(session.id(), SessionState::Failed).await;
    // "Wake up": nothing restarts on its own, and the failed session cannot be restarted.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert_eq!(f.mic.start_count(), 1);
    assert!(f.service.start(session.id(), true).is_err());
    assert_eq!(f.mic.start_count(), 1);
}

#[tokio::test]
async fn fault_after_stop_is_ignored() {
    let f = fixture();
    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    f.service.stop(session.id()).unwrap();
    f.mic.trigger_fault(SourceFault::Lost);
    f.wait_for_state(session.id(), SessionState::Ended).await;
    assert_eq!(f.stored(session.id()).failure(), None);
}
