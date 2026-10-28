//! Shared fixture for session use-case tests.

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use devcopilot_application::events::SessionEvent;
use devcopilot_application::ports::{AudioSource, SessionRepository};
use devcopilot_application::provider_service::ProviderService;
use devcopilot_application::session_service::{NewSession, SessionDeps, SessionService};
use devcopilot_application::settings_service::{SettingsService, SettingsUpdate};
use devcopilot_application::testing::{
    FakeClock, InMemoryProviderRepository, InMemorySessionRepository, InMemorySettingsRepository,
    RecordingEventPublisher, ScriptedAudioSource, ToggleableSecretStore,
};
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{ProviderId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{DataCategory, ProviderAdapterKind, ProviderRole, ProviderSpec};
use devcopilot_domain::session::{Session, SessionState};
use devcopilot_domain::text::Title;

pub struct Fixture {
    pub service: SessionService,
    pub sessions: Arc<InMemorySessionRepository>,
    pub providers: Arc<ProviderService>,
    pub settings: Arc<SettingsService>,
    pub secrets: Arc<ToggleableSecretStore>,
    pub mic: Arc<ScriptedAudioSource>,
    pub system: Arc<ScriptedAudioSource>,
    pub clock: Arc<FakeClock>,
    pub events: Arc<RecordingEventPublisher>,
}

/// Microphone and system sources granted, a local transcriber configured, consent reminder
/// active.
pub fn fixture() -> Fixture {
    let fixture = fixture_without_transcriber();
    fixture.add_transcriber();
    fixture
}

/// [`fixture`] with no transcription provider configured.
pub fn fixture_without_transcriber() -> Fixture {
    let sessions = Arc::new(InMemorySessionRepository::default());
    let settings_repo = Arc::new(InMemorySettingsRepository::default());
    let providers = Arc::new(ProviderService::new(
        Arc::new(InMemoryProviderRepository::default()),
        settings_repo.clone(),
    ));
    let settings = Arc::new(SettingsService::new(settings_repo));
    let secrets = Arc::new(ToggleableSecretStore::available());
    let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
    let system = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::System));
    let clock = Arc::new(FakeClock::default());
    let events = Arc::new(RecordingEventPublisher::default());
    let service = SessionService::new(SessionDeps {
        sessions: sessions.clone(),
        providers: providers.clone(),
        settings: settings.clone(),
        secrets: secrets.clone(),
        sources: vec![
            mic.clone() as Arc<dyn AudioSource>,
            system.clone() as Arc<dyn AudioSource>,
        ],
        clock: clock.clone(),
        events: events.clone(),
    });
    Fixture {
        service,
        sessions,
        providers,
        settings,
        secrets,
        mic,
        system,
        clock,
        events,
    }
}

impl Fixture {
    pub fn add_transcriber(&self) {
        self.providers
            .upsert_local(ProviderSpec {
                id: ProviderId::new(),
                role: ProviderRole::Transcription,
                adapter: ProviderAdapterKind::WhisperLocal,
                endpoint: None,
                model: "ggml-base".into(),
                credential_ref: None,
                data_categories: BTreeSet::from([DataCategory::Audio]),
            })
            .unwrap();
    }

    pub fn dismiss_consent_reminder(&self) {
        self.settings
            .update(SettingsUpdate {
                consent_reminder_dismissed: Some(true),
                ..SettingsUpdate::default()
            })
            .unwrap();
    }

    pub fn create(&self, sources: &[AudioSourceKind]) -> Session {
        self.service
            .create(NewSession {
                title: Title::parse("Refinamento técnico").unwrap(),
                language: SessionLanguage::PtBr,
                sources: sources.to_vec(),
            })
            .unwrap()
    }

    pub fn create_mic(&self) -> Session {
        self.create(&[AudioSourceKind::Microphone])
    }

    pub fn state(&self, id: SessionId) -> SessionState {
        self.service.get(id).unwrap().state()
    }

    pub fn stored(&self, id: SessionId) -> Session {
        self.sessions.get(id).unwrap()
    }

    pub fn events(&self, id: SessionId) -> Vec<SessionEvent> {
        self.events.events_for(id)
    }

    /// Waits (real time, bounded) until the session reaches `state`.
    pub async fn wait_for_state(&self, id: SessionId, state: SessionState) {
        wait_until(|| self.state(id) == state).await;
    }

    /// Starts `id` with consent and waits until it captures.
    pub async fn start_capturing(&self, id: SessionId) {
        self.service.start(id, true).unwrap();
        self.wait_for_state(id, SessionState::Capturing).await;
    }
}

/// Polls `condition` for up to 2 s.
pub async fn wait_until(condition: impl Fn() -> bool) {
    tokio::time::timeout(StdDuration::from_secs(2), async {
        while !condition() {
            tokio::time::sleep(StdDuration::from_millis(1)).await;
        }
    })
    .await
    .expect("condition not reached in time");
}

/// `state_changed` states in order.
pub fn states(events: &[SessionEvent]) -> Vec<SessionState> {
    events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::StateChanged { state, .. } => Some(*state),
            _ => None,
        })
        .collect()
}

/// `capture_indicator` payloads in order.
pub fn indicators(events: &[SessionEvent]) -> Vec<(bool, Vec<AudioSourceKind>)> {
    events
        .iter()
        .filter_map(|event| match event {
            SessionEvent::CaptureIndicator {
                active, sources, ..
            } => Some((*active, sources.clone())),
            _ => None,
        })
        .collect()
}
