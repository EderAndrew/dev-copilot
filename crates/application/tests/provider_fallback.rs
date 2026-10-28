//! Provider fallback rules (T130, FR-045b, FR-048, US7.3): a failing remote provider never
//! hands over to another remote provider; a local provider may take over, with a notice.

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

mod support;

use std::sync::Arc;

use devcopilot_application::ApplicationError;
use devcopilot_application::events::{ProviderStage, SessionEvent};
use devcopilot_application::ports::ProviderError;
use devcopilot_application::testing::{ScriptedTranscriber, TranscriberEnd};
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::provider::{
    DataCategory, Locality, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
use devcopilot_domain::session::{FailureReason, SessionState};
use support::{Fixture, fixture_without_transcriber, wait_until};

fn remote_stt(f: &Fixture, host: &str) -> ProviderId {
    let id = ProviderId::new();
    f.providers
        .upsert(ProviderSpec {
            id,
            role: ProviderRole::Transcription,
            adapter: ProviderAdapterKind::OpenAiCompatibleStt,
            endpoint: Some(format!("https://{host}/v1")),
            model: "whisper-1".into(),
            credential_ref: None,
            data_categories: [DataCategory::Audio].into(),
        })
        .unwrap();
    id
}

fn local_stt(f: &Fixture) -> ProviderId {
    let id = ProviderId::new();
    f.providers
        .upsert_local(ProviderSpec {
            id,
            role: ProviderRole::Transcription,
            adapter: ProviderAdapterKind::WhisperLocal,
            endpoint: None,
            model: "ggml-base".into(),
            credential_ref: None,
            data_categories: [DataCategory::Audio].into(),
        })
        .unwrap();
    id
}

fn route(f: &Fixture, id: ProviderId, locality: Locality) -> Arc<ScriptedTranscriber> {
    let transcriber = Arc::new(ScriptedTranscriber::for_provider(id, locality));
    f.transcribers.route(id, transcriber.clone());
    transcriber
}

fn in_use(events: &[SessionEvent]) -> Vec<(Locality, ProviderId)> {
    events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::ProviderInUse {
                stage: ProviderStage::Transcription,
                locality,
                provider_id,
            } => Some((*locality, *provider_id)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn failing_remote_never_switches_to_another_remote() {
    let f = fixture_without_transcriber();
    let primary = remote_stt(&f, "stt-a.example.com");
    let other = remote_stt(&f, "stt-b.example.com");
    f.providers
        .set_default(ProviderRole::Transcription, primary)
        .unwrap();
    f.providers.authorize(primary).unwrap();
    f.providers.authorize(other).unwrap();
    let failing = route(&f, primary, Locality::Remote);
    failing.set_end(TranscriberEnd::Fail(ProviderError::Unavailable));
    let other_transcriber = route(&f, other, Locality::Remote);

    let session = f.create_mic();
    f.service.start(session.id(), true).unwrap();
    // After the failure limit the session fails instead of moving on (the user is informed).
    f.wait_for_state(session.id(), SessionState::Failed).await;
    assert_eq!(
        f.stored(session.id()).failure(),
        Some(FailureReason::NoTranscriber)
    );
    assert_eq!(f.transcribers.created(), [primary]);
    assert_eq!(other_transcriber.run_count(), 0);
    assert!(
        !f.events(session.id())
            .iter()
            .any(|e| matches!(e, SessionEvent::ProviderFallback { .. }))
    );
}

#[tokio::test]
async fn failing_remote_falls_back_to_local_with_notice() {
    let f = fixture_without_transcriber();
    let remote = remote_stt(&f, "stt.example.com");
    let local = local_stt(&f);
    f.providers
        .set_default(ProviderRole::Transcription, remote)
        .unwrap();
    f.providers.authorize(remote).unwrap();
    let failing = route(&f, remote, Locality::Remote);
    failing.end_next_run(TranscriberEnd::Fail(ProviderError::Timeout));
    let local_transcriber = route(&f, local, Locality::Local);

    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    wait_until(|| local_transcriber.run_count() == 1).await;
    assert_eq!(failing.run_count(), 1, "the remote is not retried");
    let events = f.events(session.id());
    assert!(events.iter().any(|e| matches!(
        e,
        SessionEvent::ProviderFallback {
            stage: ProviderStage::Transcription,
            from_provider_id,
            to_provider_id,
        } if *from_provider_id == remote && *to_provider_id == local
    )));
    assert_eq!(
        in_use(&events),
        [(Locality::Remote, remote), (Locality::Local, local)]
    );
    assert_eq!(f.state(session.id()), SessionState::Capturing);
}

#[tokio::test]
async fn revoked_authorization_mid_session_moves_to_local() {
    let f = fixture_without_transcriber();
    let remote = remote_stt(&f, "stt.example.com");
    let local = local_stt(&f);
    f.providers
        .set_default(ProviderRole::Transcription, remote)
        .unwrap();
    f.providers.authorize(remote).unwrap();
    let remote_transcriber = route(&f, remote, Locality::Remote);
    let local_transcriber = route(&f, local, Locality::Local);

    let session = f.create_mic();
    f.start_capturing(session.id()).await;
    f.providers.revoke(remote).unwrap();
    // The remote adapter's gate check fails on its next request.
    remote_transcriber.fail_now(ProviderError::AuthorizationRequired);
    wait_until(|| local_transcriber.run_count() == 1).await;
    assert_eq!(remote_transcriber.run_count(), 1);
}

#[tokio::test]
async fn local_default_is_never_replaced_by_a_remote() {
    let f = fixture_without_transcriber();
    let local = local_stt(&f);
    let remote = remote_stt(&f, "stt.example.com");
    f.providers.authorize(remote).unwrap();
    f.providers
        .set_default(ProviderRole::Transcription, local)
        .unwrap();
    f.transcribers.set_unavailable(false);
    assert_eq!(f.providers.resolve_transcriber().unwrap().id(), local);
}

#[tokio::test]
async fn unusable_remote_default_resolves_to_local_only() {
    let f = fixture_without_transcriber();
    let primary = remote_stt(&f, "stt-a.example.com");
    let other = remote_stt(&f, "stt-b.example.com");
    f.providers.authorize(other).unwrap();
    f.providers
        .set_default(ProviderRole::Transcription, primary)
        .unwrap();
    // The default is not authorized; the other authorized remote is not picked instead.
    assert_eq!(
        f.providers.resolve_transcriber(),
        Err(ApplicationError::AuthorizationRequired {
            provider: Some(primary)
        })
    );
    let local = local_stt(&f);
    assert_eq!(f.providers.resolve_transcriber().unwrap().id(), local);
}

#[tokio::test]
async fn start_without_authorization_asks_for_it_and_captures_nothing() {
    let f = fixture_without_transcriber();
    let remote = remote_stt(&f, "stt.example.com");
    let session = f.create_mic();
    assert_eq!(
        f.service.start(session.id(), true).map(|_| ()),
        Err(ApplicationError::AuthorizationRequired {
            provider: Some(remote)
        })
    );
    assert_eq!(f.mic.start_count(), 0);
    assert_eq!(f.state(session.id()), SessionState::Idle);
}

#[tokio::test]
async fn remote_without_local_fails_and_informs() {
    let f = fixture_without_transcriber();
    let remote = remote_stt(&f, "stt.example.com");
    f.providers.authorize(remote).unwrap();
    let failing = route(&f, remote, Locality::Remote);
    failing.set_end(TranscriberEnd::Fail(ProviderError::Other(
        "http status 500".into(),
    )));
    let session = f.create_mic();
    f.service.start(session.id(), true).unwrap();
    f.wait_for_state(session.id(), SessionState::Failed).await;
    assert_eq!(
        f.stored(session.id()).failure(),
        Some(FailureReason::NoTranscriber)
    );
}
