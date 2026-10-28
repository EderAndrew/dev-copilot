//! `export_session` writes only where the save dialog says, and nothing when it is cancelled
//! (T121; FR-041, plan.md threat "Escrita fora do destino de exportação").

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use devcopilot_application::ports::{AudioSource, BoxFuture, EventPublisher};
use devcopilot_application::session_service::NewSession;
use devcopilot_application::testing::{
    FakeClock, RecordingEventPublisher, ScriptedAnalysisProvider, ScriptedAnalysisProviderFactory,
    ScriptedAudioSource, ScriptedTranscriber, ScriptedTranscriberFactory, ToggleableSecretStore,
};
use devcopilot_desktop_lib::commands::export::{self, ExportSessionDto, SaveDialog};
use devcopilot_desktop_lib::composition::{Core, Platform, compose_core};
use devcopilot_desktop_lib::dto::Code;
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::export::{ExportContent, ExportFormat};
use devcopilot_domain::ids::SessionId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::record::{RecordKind, RecordStatus, UserRecord};
use devcopilot_domain::text::{Rationale, RecordText, Title};

/// Save dialog answering from a script and remembering what it offered.
#[derive(Default)]
struct ScriptedDialog {
    answers: Mutex<VecDeque<Option<PathBuf>>>,
    offered: Mutex<Vec<(String, ExportFormat)>>,
}

impl ScriptedDialog {
    fn answering(answers: Vec<Option<PathBuf>>) -> Self {
        Self {
            answers: Mutex::new(answers.into()),
            offered: Mutex::default(),
        }
    }

    fn offered(&self) -> Vec<(String, ExportFormat)> {
        self.offered.lock().unwrap().clone()
    }
}

impl SaveDialog for ScriptedDialog {
    fn choose(&self, file_name: &str, format: ExportFormat) -> BoxFuture<'_, Option<PathBuf>> {
        self.offered
            .lock()
            .unwrap()
            .push((file_name.to_owned(), format));
        let answer = self.answers.lock().unwrap().pop_front().flatten();
        Box::pin(async move { answer })
    }
}

struct Harness {
    core: Core,
    session: SessionId,
    data: tempfile::TempDir,
    out: tempfile::TempDir,
}

fn harness() -> Harness {
    let data = tempfile::tempdir().unwrap();
    let core = compose_core(
        data.path(),
        Platform {
            secrets: Arc::new(ToggleableSecretStore::available()),
            sources: vec![
                Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone))
                    as Arc<dyn AudioSource>,
            ],
            clock: Arc::new(FakeClock::default()),
            events: Arc::new(RecordingEventPublisher::default()) as Arc<dyn EventPublisher>,
            transcribers: Arc::new(ScriptedTranscriberFactory::new(Arc::new(
                ScriptedTranscriber::local(Vec::new()),
            ))),
            analyzers: Arc::new(ScriptedAnalysisProviderFactory::new(Arc::new(
                ScriptedAnalysisProvider::local(),
            ))),
        },
    )
    .unwrap();
    let session = core
        .sessions
        .create(NewSession {
            title: Title::parse("Revisão da API").unwrap(),
            language: SessionLanguage::PtBr,
            sources: vec![AudioSourceKind::Microphone],
        })
        .unwrap()
        .id();
    core.records
        .create(
            session,
            UserRecord {
                kind: RecordKind::Requirement,
                text: Redacted::new(RecordText::parse("Exportar em CSV").unwrap()),
                rationale: Redacted::new(Rationale::parse("").unwrap()),
                status: RecordStatus::Confirmed,
            },
        )
        .unwrap();
    Harness {
        core,
        session,
        data,
        out: tempfile::tempdir().unwrap(),
    }
}

fn dto(session: SessionId, formats: &[ExportFormat]) -> ExportSessionDto {
    ExportSessionDto {
        session_id: session.to_string(),
        formats: formats.iter().copied().map(Code).collect(),
        content: vec![Code(ExportContent::Plan), Code(ExportContent::Transcript)],
    }
}

fn both(session: SessionId) -> ExportSessionDto {
    dto(session, &[ExportFormat::Markdown, ExportFormat::Json])
}

/// Every file under `dir`, relative, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_owned()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                found.push(
                    path.strip_prefix(dir)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    found.sort();
    found
}

#[tokio::test]
async fn writes_exactly_the_chosen_paths() {
    let h = harness();
    let data_before = files(h.data.path());
    let dialog = ScriptedDialog::answering(vec![
        Some(h.out.path().join("plano.md")),
        Some(h.out.path().join("nested-name.json")),
    ]);

    let result = export::export(&h.core, &dialog, both(h.session))
        .await
        .unwrap();

    assert_eq!(result.written, ["plano.md", "nested-name.json"]);
    assert_eq!(files(h.out.path()), ["nested-name.json", "plano.md"]);
    // Nothing new next to the database (no temporary or stray copies).
    let data_after: Vec<_> = files(h.data.path())
        .into_iter()
        .filter(|f| !data_before.contains(f))
        .collect();
    assert!(
        data_after.iter().all(|f| f.starts_with("devcopilot.db")),
        "{data_after:?}"
    );
    let offered = dialog.offered();
    assert_eq!(
        offered.iter().map(|(_, f)| *f).collect::<Vec<_>>(),
        [ExportFormat::Markdown, ExportFormat::Json]
    );
    assert!(offered[0].0.starts_with("revisao-da-api-") && offered[0].0.ends_with(".md"));
    let json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(h.out.path().join("nested-name.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(json["format"], "devcopilot.session-export");
    assert_eq!(json["records"][0]["text"], "Exportar em CSV");
    let markdown = std::fs::read_to_string(h.out.path().join("plano.md")).unwrap();
    assert!(markdown.starts_with("# Revisão da API"));
    assert_eq!(h.core.export.history(h.session).unwrap().len(), 1);
}

#[tokio::test]
async fn cancelling_any_dialog_writes_nothing() {
    for answers in [vec![None], vec![Some(PathBuf::from("ignored.md")), None]] {
        let h = harness();
        let answers: Vec<_> = answers
            .into_iter()
            .map(|a| a.map(|name| h.out.path().join(name)))
            .collect();
        let dialog = ScriptedDialog::answering(answers.clone());

        let result = export::export(&h.core, &dialog, both(h.session))
            .await
            .unwrap();

        assert!(result.written.is_empty());
        assert!(files(h.out.path()).is_empty());
        assert_eq!(
            dialog.offered().len(),
            answers.len(),
            "stops at the cancelled dialog"
        );
        assert!(h.core.export.history(h.session).unwrap().is_empty());
    }
}

#[tokio::test]
async fn invalid_requests_open_no_dialog() {
    let h = harness();
    let dialog = ScriptedDialog::default();

    let err = export::export(&h.core, &dialog, dto(h.session, &[]))
        .await
        .unwrap_err();
    assert_eq!(err.code, "invalid_input");
    let err = export::export(&h.core, &dialog, both(SessionId::new()))
        .await
        .unwrap_err();
    assert_eq!(err.code, "not_found");
    let mut bad_id = both(h.session);
    bad_id.session_id = "../../etc/passwd".into();
    let err = export::export(&h.core, &dialog, bad_id).await.unwrap_err();
    assert_eq!(err.code, "invalid_input");

    assert!(dialog.offered().is_empty());
    assert!(files(h.out.path()).is_empty());
}

#[tokio::test]
async fn the_same_path_for_two_formats_is_rejected() {
    let h = harness();
    let path = h.out.path().join("export");
    let dialog = ScriptedDialog::answering(vec![Some(path.clone()), Some(path)]);

    let err = export::export(&h.core, &dialog, both(h.session))
        .await
        .unwrap_err();

    assert_eq!(err.code, "invalid_input");
    assert!(files(h.out.path()).is_empty());
}

#[tokio::test]
async fn unwritable_destination_is_a_storage_error_without_the_path() {
    let h = harness();
    let path = h.out.path().join("missing-dir").join("plano.json");
    let dialog = ScriptedDialog::answering(vec![Some(path)]);

    let err = export::export(&h.core, &dialog, dto(h.session, &[ExportFormat::Json]))
        .await
        .unwrap_err();

    assert_eq!(err.code, "storage_error");
    assert!(err.details.values().all(|v| !v.contains("missing-dir")));
    assert!(h.core.export.history(h.session).unwrap().is_empty());
}
