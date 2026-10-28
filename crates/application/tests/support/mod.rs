//! Shared fixture for session use-case tests.

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use devcopilot_application::analysis_service::{AnalysisDeps, AnalysisService};
use devcopilot_application::events::SessionEvent;
use devcopilot_application::ports::{AudioSource, SessionRepository};
use devcopilot_application::provider_service::ProviderService;
use devcopilot_application::question_service::{QuestionDeps, QuestionService};
use devcopilot_application::record_service::RecordService;
use devcopilot_application::session_service::{NewSession, SessionDeps, SessionService};
use devcopilot_application::settings_service::{AnalysisLimits, SettingsService, SettingsUpdate};
use devcopilot_application::testing::{
    FakeClock, InMemoryAnalysisRunRepository, InMemoryProviderRepository,
    InMemoryQuestionRepository, InMemoryRecordRepository, InMemorySegmentRepository,
    InMemorySessionRepository, InMemorySettingsRepository, RecordingEventPublisher,
    ScriptedAnalysisProvider, ScriptedAnalysisProviderFactory, ScriptedAudioSource,
    ScriptedTranscriber, ScriptedTranscriberFactory, ToggleableSecretStore,
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
    pub segments: Arc<InMemorySegmentRepository>,
    pub transcriber: Arc<ScriptedTranscriber>,
    pub transcribers: Arc<ScriptedTranscriberFactory>,
    pub providers: Arc<ProviderService>,
    pub settings: Arc<SettingsService>,
    pub secrets: Arc<ToggleableSecretStore>,
    pub mic: Arc<ScriptedAudioSource>,
    pub system: Arc<ScriptedAudioSource>,
    pub clock: Arc<FakeClock>,
    pub events: Arc<RecordingEventPublisher>,
    pub analyzer: Arc<ScriptedAnalysisProvider>,
    pub analyzers: Arc<ScriptedAnalysisProviderFactory>,
    pub records: Arc<InMemoryRecordRepository>,
    pub runs: Arc<InMemoryAnalysisRunRepository>,
    pub analysis: AnalysisService,
    pub record_service: RecordService,
    pub questions: Arc<InMemoryQuestionRepository>,
    pub question_service: QuestionService,
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
    let segments = Arc::new(InMemorySegmentRepository::default());
    let transcriber = Arc::new(ScriptedTranscriber::local(Vec::new()));
    let transcribers = Arc::new(ScriptedTranscriberFactory::new(transcriber.clone()));
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
    let analyzer = Arc::new(ScriptedAnalysisProvider::local());
    let analyzers = Arc::new(ScriptedAnalysisProviderFactory::new(analyzer.clone()));
    let records = Arc::new(InMemoryRecordRepository::default());
    let runs = Arc::new(InMemoryAnalysisRunRepository::default());
    let analysis = AnalysisService::new(AnalysisDeps {
        providers: providers.clone(),
        analyzers: analyzers.clone(),
        records: records.clone(),
        runs: runs.clone(),
        segments: segments.clone(),
        settings: settings.clone(),
        clock: clock.clone(),
        events: events.clone(),
        tick: StdDuration::from_millis(5),
    });
    let service = SessionService::new(SessionDeps {
        sessions: sessions.clone(),
        segments: segments.clone(),
        transcribers: transcribers.clone(),
        providers: providers.clone(),
        settings: settings.clone(),
        secrets: secrets.clone(),
        sources: vec![
            mic.clone() as Arc<dyn AudioSource>,
            system.clone() as Arc<dyn AudioSource>,
        ],
        clock: clock.clone(),
        events: events.clone(),
        analysis: Some(analysis.clone()),
    });
    let record_service = RecordService::new(records.clone(), sessions.clone(), clock.clone());
    let questions = Arc::new(InMemoryQuestionRepository::default());
    let question_service = QuestionService::new(QuestionDeps {
        providers: providers.clone(),
        analyzers: analyzers.clone(),
        questions: questions.clone(),
        records: records.clone(),
        segments: segments.clone(),
        sessions: sessions.clone(),
        clock: clock.clone(),
        events: events.clone(),
    });
    Fixture {
        service,
        sessions,
        segments,
        transcriber,
        transcribers,
        providers,
        settings,
        secrets,
        mic,
        system,
        clock,
        events,
        analyzer,
        analyzers,
        records,
        runs,
        analysis,
        record_service,
        questions,
        question_service,
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

    /// Configures a local (Ollama) analysis provider; returns its id.
    pub fn add_analyzer(&self) -> ProviderId {
        let id = ProviderId::new();
        self.providers
            .upsert_local(ProviderSpec {
                id,
                role: ProviderRole::Analysis,
                adapter: ProviderAdapterKind::Ollama,
                endpoint: Some("http://127.0.0.1:11434".into()),
                model: "llama3.1".into(),
                credential_ref: None,
                data_categories: BTreeSet::from([DataCategory::Transcript, DataCategory::Records]),
            })
            .unwrap();
        id
    }

    pub fn set_analysis_limits(&self, limits: AnalysisLimits) {
        self.settings
            .update(SettingsUpdate {
                analysis_limits: Some(limits),
                ..SettingsUpdate::default()
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
