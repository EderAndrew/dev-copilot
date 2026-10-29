//! Diagnostic log redaction (T157, SC-014, FR-051): a scripted session runs through the real
//! composition root — capture, transcript, analysis, records, questions, export and a remote
//! provider key — while every log event is written to a file at the most verbose level. The
//! file must contain the operations but none of the session content or the secret.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use devcopilot_application::export_service::ExportRequest;
use devcopilot_application::ports::{
    AnalysisOutput, AnswerOutput, AudioSource, EventPublisher, ProviderError, TranscriptUpdate,
};
use devcopilot_application::session_service::NewSession;
use devcopilot_application::testing::{
    FakeClock, RecordingEventPublisher, ScriptedAnalysisProvider, ScriptedAnalysisProviderFactory,
    ScriptedAudioSource, ScriptedTranscriber, ScriptedTranscriberFactory, ToggleableSecretStore,
};
use devcopilot_desktop_lib::composition::{Core, Platform, compose_core};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::export::{ExportContent, ExportFormat};
use devcopilot_domain::ids::{ProviderId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{DataCategory, ProviderAdapterKind, ProviderRole, ProviderSpec};
use devcopilot_domain::question::QuestionStatus;
use devcopilot_domain::question::{Answer, Certainty};
use devcopilot_domain::record::{Nature, Proposal, RecordKind, RecordStatus, UserRecord};
use devcopilot_domain::session::SessionState;
use devcopilot_domain::text::{
    AnswerText, QuestionText, Rationale, RecordText, SegmentText, Title,
};
use devcopilot_domain::time::Duration;

/// Content that must never reach the log (synthetic, Art. IV.16).
const TITLE: &str = "Planejamento-ZETA-titulo";
const SPOKEN: &str = "frase-falada-ZETA-7731 sobre o cliente";
const SPOKEN_AFTER_FAILURE: &str = "segunda-frase-ZETA-8842";
const QUESTION: &str = "pergunta-ZETA-5519 sobre prazos?";
const ANSWER: &str = "resposta-ZETA-6620 com detalhes";
const PROPOSED: &str = "registro-proposto-ZETA-4408";
const USER_RECORD: &str = "registro-usuario-ZETA-9917";
const SECRET: &str = "sk-ant-ZETA-SECRET-0d1e2f3a";

const FORBIDDEN: &[&str] = &[
    TITLE,
    "ZETA-7731",
    "ZETA-8842",
    "ZETA-5519",
    "ZETA-6620",
    "ZETA-4408",
    "ZETA-9917",
    "ZETA-SECRET",
];

/// Log file shared by every thread of the test runtime.
#[derive(Clone)]
struct LogFile(Arc<Mutex<File>>);

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

fn install_log(path: &PathBuf) {
    let file = LogFile(Arc::new(Mutex::new(File::create(path).unwrap())));
    // Global, so tasks on every runtime thread are captured; most verbose level on purpose.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || file.clone())
        .try_init()
        .unwrap();
}

async fn until(what: &str, condition: impl Fn() -> bool) {
    tokio::time::timeout(StdDuration::from_secs(3), async {
        while !condition() {
            tokio::time::sleep(StdDuration::from_millis(2)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{what} not reached"));
}

fn final_update(start_s: u64, text: &str) -> TranscriptUpdate {
    TranscriptUpdate::Final {
        source: AudioSourceKind::Microphone,
        start: Duration::from_secs(start_s),
        end: Duration::from_secs(start_s + 1),
        text: Redacted::new(SegmentText::parse(text).unwrap()),
    }
}

fn configure_providers(core: &Core) {
    core.providers
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
    core.providers
        .upsert_local(ProviderSpec {
            id: ProviderId::new(),
            role: ProviderRole::Analysis,
            adapter: ProviderAdapterKind::Ollama,
            endpoint: Some("http://127.0.0.1:11434".into()),
            model: "llama3.1".into(),
            credential_ref: None,
            data_categories: BTreeSet::from([
                DataCategory::Transcript,
                DataCategory::Records,
                DataCategory::Questions,
            ]),
        })
        .unwrap();
    // A remote provider with a stored key (never authorized: nothing is sent).
    let remote = ProviderId::new();
    core.providers
        .upsert(ProviderSpec {
            id: remote,
            role: ProviderRole::Transcription,
            adapter: ProviderAdapterKind::OpenAiCompatibleStt,
            endpoint: Some("https://stt.example.com/v1".into()),
            model: "whisper-1".into(),
            credential_ref: None,
            data_categories: BTreeSet::from([DataCategory::Audio]),
        })
        .unwrap();
    core.providers
        .set_secret(remote, Redacted::new(SECRET.as_bytes().to_vec()))
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scripted_session_leaves_no_content_or_secret_in_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let log_path = dir.path().join("devcopilot.log");
    install_log(&log_path);

    let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
    let transcriber = Arc::new(ScriptedTranscriber::local(Vec::new()));
    let analyzer = Arc::new(ScriptedAnalysisProvider::local());
    let core = compose_core(
        &dir.path().join("data"),
        Platform {
            secrets: Arc::new(ToggleableSecretStore::available()),
            sources: vec![mic.clone() as Arc<dyn AudioSource>],
            clock: Arc::new(FakeClock::default()),
            events: Arc::new(RecordingEventPublisher::default()) as Arc<dyn EventPublisher>,
            transcribers: Arc::new(ScriptedTranscriberFactory::new(transcriber.clone())),
            gate: Arc::default(),
            analyzers: Arc::new(ScriptedAnalysisProviderFactory::new(analyzer.clone())),
        },
    )
    .unwrap();
    configure_providers(&core);

    // Capture with a transcriber failure in the middle (errors are logged too).
    let session = core
        .sessions
        .create(NewSession {
            title: Title::parse(TITLE).unwrap(),
            language: SessionLanguage::PtBr,
            sources: vec![AudioSourceKind::Microphone],
        })
        .unwrap();
    let id: SessionId = session.id();
    core.sessions.start(id, true).unwrap();
    until("capturing", || {
        core.sessions.get(id).unwrap().state() == SessionState::Capturing
    })
    .await;
    transcriber.emit(final_update(0, SPOKEN));
    until("first final", || {
        core.sessions.transcript(id).unwrap().len() == 1
    })
    .await;
    transcriber.fail_now(ProviderError::Other("crash".into()));
    transcriber.emit(final_update(2, SPOKEN_AFTER_FAILURE));
    until("second final", || {
        core.sessions.transcript(id).unwrap().len() >= 2
    })
    .await;

    // A question and its answer.
    analyzer.push_answer(Ok(AnswerOutput {
        answer: Answer {
            text: Redacted::new(AnswerText::parse(ANSWER).unwrap()),
            certainty: Certainty::NoEvidence,
            evidence: Vec::new(),
        },
        tokens_used: 10,
    }));
    core.questions
        .ask(id, Redacted::new(QuestionText::parse(QUESTION).unwrap()))
        .unwrap();
    until("answer", || {
        core.questions
            .list(id)
            .unwrap()
            .iter()
            .any(|q| q.status() == QuestionStatus::Answered)
    })
    .await;

    core.sessions.stop(id).unwrap();
    until("ended", || {
        core.sessions.get(id).unwrap().state() == SessionState::Ended
    })
    .await;

    // Final analysis proposing a record, plus a record written by the user.
    analyzer.push(Ok(AnalysisOutput {
        proposals: vec![Proposal {
            kind: RecordKind::Requirement,
            text: Redacted::new(RecordText::parse(PROPOSED).unwrap()),
            nature: Nature::Suggested,
            rationale: Redacted::new(Rationale::default()),
            evidence: Vec::new(),
            acceptance_criteria: Vec::new(),
            alternatives: Vec::new(),
            relates_to: None,
        }],
        tokens_used: 30,
    }));
    core.analysis
        .run_final(&core.sessions.get(id).unwrap())
        .unwrap();
    until("proposal", || {
        core.records
            .list(id)
            .unwrap()
            .iter()
            .any(|r| r.text().expose().as_str() == PROPOSED)
    })
    .await;
    core.records
        .create(
            id,
            UserRecord {
                kind: RecordKind::Decision,
                text: Redacted::new(RecordText::parse(USER_RECORD).unwrap()),
                rationale: Redacted::new(Rationale::default()),
                status: RecordStatus::Confirmed,
            },
        )
        .unwrap();
    let documents = core
        .export
        .render(&ExportRequest {
            session_id: id,
            formats: BTreeSet::from([ExportFormat::Markdown, ExportFormat::Json]),
            content: BTreeSet::from([ExportContent::Plan, ExportContent::Transcript]),
        })
        .unwrap();
    assert!(documents[0].contents.expose().contains(PROPOSED));
    tracing::info!(text = ?Redacted::new(SPOKEN.to_owned()), "redaction wrapper check");

    let log = std::fs::read_to_string(&log_path).unwrap();
    // The log did record the session's life, by id and code only.
    for expected in [
        "session capturing",
        "session ended",
        "transcriber stopped unexpectedly",
        "provider credential stored",
        "[redacted]",
    ] {
        assert!(log.contains(expected), "log lacks `{expected}`");
    }
    assert!(log.contains(&id.to_string()));
    for phrase in FORBIDDEN {
        assert!(!log.contains(phrase), "log leaked `{phrase}`");
    }
}
