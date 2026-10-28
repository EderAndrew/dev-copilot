//! IPC validation of the session and audio commands (T044): malformed ids, title bounds,
//! unknown source kinds, start without consent, and `session_cancel_start` outside
//! `starting`. Every rejection leaves capture closed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::sync::Arc;

use devcopilot_application::ports::{AudioSource, EventPublisher};
use devcopilot_application::testing::{
    FakeClock, RecordingEventPublisher, ScriptedAnalysisProvider, ScriptedAnalysisProviderFactory,
    ScriptedAudioSource, ScriptedTranscriber, ScriptedTranscriberFactory, ToggleableSecretStore,
};
use devcopilot_desktop_lib::commands::IpcError;
use devcopilot_desktop_lib::commands::audio::{self, AvailabilityDto};
use devcopilot_desktop_lib::commands::sessions::{
    self, SessionCreateDto, SessionIdDto, SessionStartDto,
};
use devcopilot_desktop_lib::composition::{Core, Platform, compose_core};
use devcopilot_desktop_lib::dto::Code;
use devcopilot_domain::audio_source::{AudioSourceKind, Availability, UnavailableReason};
use devcopilot_domain::ids::{ProviderId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{DataCategory, ProviderAdapterKind, ProviderRole, ProviderSpec};
use devcopilot_domain::session::SessionState;

struct Harness {
    core: Core,
    mic: Arc<ScriptedAudioSource>,
    _dir: tempfile::TempDir,
}

fn harness() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
    let system = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::System));
    system.set_availability(Availability::Unavailable(
        UnavailableReason::NotValidatedOnPlatform,
    ));
    let core = compose_core(
        dir.path(),
        Platform {
            secrets: Arc::new(ToggleableSecretStore::available()),
            sources: vec![
                mic.clone() as Arc<dyn AudioSource>,
                system as Arc<dyn AudioSource>,
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
    Harness {
        core,
        mic,
        _dir: dir,
    }
}

fn create_dto(title: &str, sources: &[AudioSourceKind]) -> SessionCreateDto {
    SessionCreateDto {
        title: title.into(),
        language: Code(SessionLanguage::PtBr),
        sources: sources.iter().copied().map(Code).collect(),
    }
}

fn field(err: &IpcError) -> Option<&str> {
    err.details.get("field").map(String::as_str)
}

fn id_dto(id: &str) -> SessionIdDto {
    SessionIdDto {
        session_id: id.into(),
    }
}

#[tokio::test]
async fn create_validates_title_bounds() {
    let h = harness();
    for (title, problem) in [
        ("", "empty"),
        ("   ", "empty"),
        ("linha\nquebrada", "control_character"),
    ] {
        let err = sessions::create(&h.core, create_dto(title, &[AudioSourceKind::Microphone]))
            .unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(field(&err), Some("title"));
        assert_eq!(
            err.details.get("problem").map(String::as_str),
            Some(problem)
        );
    }
    let long = "x".repeat(201);
    let err =
        sessions::create(&h.core, create_dto(&long, &[AudioSourceKind::Microphone])).unwrap_err();
    assert_eq!(
        err.details.get("problem").map(String::as_str),
        Some("too_long")
    );
    let max = "x".repeat(200);
    let ok = sessions::create(&h.core, create_dto(&max, &[AudioSourceKind::Microphone])).unwrap();
    assert_eq!(ok.state, Code(SessionState::Idle));
}

#[tokio::test]
async fn create_validates_sources() {
    let h = harness();
    let err = sessions::create(&h.core, create_dto("Daily", &[])).unwrap_err();
    assert_eq!(field(&err), Some("sources"));
    let err =
        sessions::create(&h.core, create_dto("Daily", &[AudioSourceKind::System])).unwrap_err();
    assert_eq!(err.code, "source_unavailable");
    // Unknown kinds never reach the core: deserialization rejects them.
    let raw = serde_json::json!({ "title": "Daily", "language": "pt-BR", "sources": ["loopback"] });
    assert!(serde_json::from_value::<SessionCreateDto>(raw).is_err());
}

#[tokio::test]
async fn bad_ids_are_invalid_input_and_unknown_ids_not_found() {
    let h = harness();
    for bad in [
        "",
        "123",
        "not-a-uuid",
        "00000000-0000-0000-0000-00000000000g",
    ] {
        let err = sessions::stop(&h.core, &id_dto(bad)).unwrap_err();
        assert_eq!(err.code, "invalid_input", "{bad}");
        assert_eq!(field(&err), Some("session_id"));
        let err = sessions::start(
            &h.core,
            SessionStartDto {
                session_id: bad.into(),
                consent_acknowledged: true,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(
            sessions::cancel_start(&h.core, &id_dto(bad))
                .unwrap_err()
                .code,
            "invalid_input"
        );
    }
    let unknown = SessionId::new().to_string();
    assert_eq!(
        sessions::stop(&h.core, &id_dto(&unknown)).unwrap_err().code,
        "not_found"
    );
    assert_eq!(
        sessions::start(
            &h.core,
            SessionStartDto {
                session_id: unknown,
                consent_acknowledged: true
            }
        )
        .unwrap_err()
        .code,
        "not_found"
    );
}

#[tokio::test]
async fn start_without_consent_is_rejected_and_captures_nothing() {
    let h = harness();
    let session =
        sessions::create(&h.core, create_dto("Daily", &[AudioSourceKind::Microphone])).unwrap();
    let err = sessions::start(
        &h.core,
        SessionStartDto {
            session_id: session.id.clone(),
            consent_acknowledged: false,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, "invalid_input");
    assert_eq!(field(&err), Some("consent_acknowledged"));
    assert_eq!(h.mic.start_count(), 0);

    let started = sessions::start(
        &h.core,
        SessionStartDto {
            session_id: session.id.clone(),
            consent_acknowledged: true,
        },
    )
    .unwrap();
    assert_eq!(started.state, Code(SessionState::Starting));
}

#[tokio::test]
async fn cancel_start_outside_starting_is_invalid_state() {
    let h = harness();
    let session =
        sessions::create(&h.core, create_dto("Daily", &[AudioSourceKind::Microphone])).unwrap();
    let err = sessions::cancel_start(&h.core, &id_dto(&session.id)).unwrap_err();
    assert_eq!(err.code, "invalid_state");
    assert_eq!(err.message_key, "errors.invalidState");
}

#[tokio::test]
async fn stop_outside_capture_is_invalid_state() {
    let h = harness();
    let session =
        sessions::create(&h.core, create_dto("Daily", &[AudioSourceKind::Microphone])).unwrap();
    assert_eq!(
        sessions::stop(&h.core, &id_dto(&session.id))
            .unwrap_err()
            .code,
        "invalid_state"
    );
}

#[tokio::test]
async fn unknown_fields_are_rejected() {
    for raw in [
        serde_json::json!({ "sessionId": SessionId::new().to_string(), "consentAcknowledged": true, "force": true }),
        serde_json::json!({ "sessionId": SessionId::new().to_string() }),
    ] {
        assert!(serde_json::from_value::<SessionStartDto>(raw).is_err());
    }
    let raw = serde_json::json!({ "sessionId": SessionId::new().to_string(), "deviceId": "x" });
    assert!(serde_json::from_value::<SessionIdDto>(raw).is_err());
}

#[tokio::test]
async fn sources_list_reports_availability_and_reason() {
    let h = harness();
    let list = audio::list(&h.core);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].kind, Code(AudioSourceKind::Microphone));
    assert_eq!(list[0].availability, AvailabilityDto::Available);
    assert_eq!(list[1].availability, AvailabilityDto::Unavailable);
    assert_eq!(
        list[1].unavailable_reason,
        Some(Code(UnavailableReason::NotValidatedOnPlatform))
    );
}

#[tokio::test]
async fn interrupted_sessions_are_failed_on_startup() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(ToggleableSecretStore::available());
    let platform = || Platform {
        secrets: secrets.clone(),
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
    };
    let id = {
        let core = compose_core(dir.path(), platform()).unwrap();
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
        let session =
            sessions::create(&core, create_dto("Daily", &[AudioSourceKind::Microphone])).unwrap();
        sessions::start(
            &core,
            SessionStartDto {
                session_id: session.id.clone(),
                consent_acknowledged: true,
            },
        )
        .unwrap();
        // The app "crashes" here: the core is dropped while the session is active.
        session.id
    };
    let core = compose_core(dir.path(), platform()).unwrap();
    let session = core.sessions.get(SessionId::parse(&id).unwrap()).unwrap();
    assert_eq!(session.state(), SessionState::Failed);
    assert_eq!(
        session.failure(),
        Some(devcopilot_domain::session::FailureReason::Interrupted)
    );
}

#[test]
fn record_set_status_validates_ids() {
    use devcopilot_desktop_lib::commands::records::{self, RecordSetStatusDto};
    use devcopilot_domain::ids::RecordId;
    use devcopilot_domain::record::RecordStatus;

    let h = harness();
    let request = |record_id: String| RecordSetStatusDto {
        record_id,
        status: Code(RecordStatus::Confirmed),
    };
    let err = records::set_status(&h.core, &request("not-a-uuid".into())).unwrap_err();
    assert_eq!(err.code, "invalid_input");
    let err = records::set_status(&h.core, &request(RecordId::new().to_string())).unwrap_err();
    assert_eq!(err.code, "not_found");
}

#[test]
fn plan_and_history_commands_round_trip_through_storage() {
    use devcopilot_desktop_lib::commands::records::{self, RecordCreateDto, RecordEditDto};
    use devcopilot_desktop_lib::commands::sessions::{SessionListDto, SessionRenameDto};
    use devcopilot_domain::record::{RecordKind, RecordStatus};

    let h = harness();
    let session = sessions::create(
        &h.core,
        SessionCreateDto {
            title: "Planejamento".into(),
            language: Code(SessionLanguage::PtBr),
            sources: vec![Code(AudioSourceKind::Microphone)],
        },
    )
    .unwrap();
    let id = SessionIdDto {
        session_id: session.id.clone(),
    };
    let created = records::create(
        &h.core,
        RecordCreateDto {
            session_id: session.id.clone(),
            kind: Code(RecordKind::Task),
            text: "Criar endpoint".into(),
            rationale: None,
            status: Code(RecordStatus::Confirmed),
        },
    )
    .unwrap();
    assert_eq!(created.origin, records::OriginDto::User);
    let edited = records::edit(
        &h.core,
        RecordEditDto {
            record_id: created.id.clone(),
            text: Some("Criar endpoint de export".into()),
            rationale: None,
            acceptance_criteria: Some(vec!["Pagina de 100 em 100".into()]),
            alternatives: None,
        },
    )
    .unwrap();
    assert_eq!(edited.status, Code(RecordStatus::Confirmed));
    assert_eq!(edited.acceptance_criteria, ["Pagina de 100 em 100"]);

    let plan = records::plan(&h.core, &id).unwrap();
    assert_eq!(plan.sections[0].kind, Code(RecordKind::Task));
    assert_eq!(plan.sections[0].items, vec![edited.clone()]);

    let detail = sessions::detail(&h.core, &id).unwrap();
    assert_eq!(detail.records, vec![edited]);
    assert!(detail.transcript.is_empty());

    let renamed = sessions::rename(
        &h.core,
        SessionRenameDto {
            session_id: session.id.clone(),
            title: "Export".into(),
        },
    )
    .unwrap();
    assert_eq!(renamed.title, "Export");
    let page = sessions::list(&h.core, &SessionListDto::default()).unwrap();
    assert_eq!(page.items, vec![renamed]);
    assert_eq!(page.next_cursor, None);

    // Final analysis only on ended or failed sessions (FR-033).
    assert_eq!(
        records::run_final(&h.core, &id).unwrap_err().code,
        "invalid_state"
    );
}

#[test]
fn plan_and_history_commands_validate_input() {
    use devcopilot_desktop_lib::commands::records::{self, RecordEditDto};
    use devcopilot_desktop_lib::commands::sessions::{SessionListDto, SessionRenameDto};

    let h = harness();
    for limit in [0, 101] {
        let err = sessions::list(
            &h.core,
            &SessionListDto {
                cursor: None,
                limit: Some(limit),
            },
        )
        .unwrap_err();
        assert_eq!(err.code, "invalid_input");
    }
    let err = sessions::list(
        &h.core,
        &SessionListDto {
            cursor: Some("nope".into()),
            limit: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, "invalid_input");
    let missing = SessionIdDto {
        session_id: SessionId::new().to_string(),
    };
    assert_eq!(
        sessions::detail(&h.core, &missing).unwrap_err().code,
        "not_found"
    );
    assert_eq!(
        records::plan(&h.core, &missing).unwrap_err().code,
        "not_found"
    );
    let err = sessions::rename(
        &h.core,
        SessionRenameDto {
            session_id: missing.session_id.clone(),
            title: "x".repeat(201),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, "invalid_input");
    let err = records::edit(
        &h.core,
        RecordEditDto {
            record_id: devcopilot_domain::ids::RecordId::new().to_string(),
            text: None,
            rationale: None,
            acceptance_criteria: None,
            alternatives: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, "invalid_input");
    assert!(
        serde_json::from_str::<RecordEditDto>(r#"{"recordId":"x","status":"confirmed"}"#).is_err()
    );
}

#[tokio::test]
async fn question_commands_validate_and_round_trip_through_storage() {
    use devcopilot_desktop_lib::commands::questions::{self, QuestionAskDto, QuestionToRecordDto};
    use devcopilot_domain::ids::QuestionId;
    use devcopilot_domain::question::{Certainty, QuestionStatus};
    use devcopilot_domain::record::{Nature, RecordKind, RecordStatus};

    let h = harness();
    h.core
        .providers
        .upsert_local(ProviderSpec {
            id: ProviderId::new(),
            role: ProviderRole::Analysis,
            adapter: ProviderAdapterKind::Ollama,
            endpoint: Some("http://127.0.0.1:11434".into()),
            model: "llama3.1".into(),
            credential_ref: None,
            data_categories: BTreeSet::from([DataCategory::Transcript, DataCategory::Questions]),
        })
        .unwrap();
    let session =
        sessions::create(&h.core, create_dto("Daily", &[AudioSourceKind::Microphone])).unwrap();
    let ask = |session_id: &str, question: &str| QuestionAskDto {
        session_id: session_id.into(),
        question: question.into(),
    };

    // Input validation and state rules.
    let err = questions::ask(&h.core, ask("not-a-uuid", "Algo?")).unwrap_err();
    assert_eq!(err.code, "invalid_input");
    let err = questions::ask(&h.core, ask(&session.id, &"x".repeat(2_001))).unwrap_err();
    assert_eq!(
        (err.code.as_str(), field(&err)),
        ("invalid_input", Some("question"))
    );
    let err = questions::ask(&h.core, ask(&SessionId::new().to_string(), "Algo?")).unwrap_err();
    assert_eq!(err.code, "not_found");
    let err = questions::ask(&h.core, ask(&session.id, "Antes de começar?")).unwrap_err();
    assert_eq!(err.code, "invalid_state");

    sessions::start(
        &h.core,
        SessionStartDto {
            session_id: session.id.clone(),
            consent_acknowledged: true,
        },
    )
    .unwrap();
    let asked = questions::ask(&h.core, ask(&session.id, "O que foi decidido?")).unwrap();
    assert_eq!(asked.status, Code(QuestionStatus::Pending));
    assert_eq!(asked.question, "O que foi decidido?");

    // The answer is stored with the session and listed by `session_get`.
    let id = SessionIdDto {
        session_id: session.id.clone(),
    };
    let answered = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let detail = sessions::detail(&h.core, &id).unwrap();
            if detail.questions[0].status != Code(QuestionStatus::Pending) {
                return detail.questions[0].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(answered.id, asked.id);
    assert_eq!(answered.status, Code(QuestionStatus::Answered));
    assert_eq!(answered.certainty, Some(Code(Certainty::NoEvidence)));
    assert!(answered.evidence.is_empty());

    let record = questions::to_record(
        &h.core,
        &QuestionToRecordDto {
            question_id: asked.id.clone(),
            kind: Code(RecordKind::Risk),
        },
    )
    .unwrap();
    assert_eq!(record.status, Code(RecordStatus::Proposed));
    assert_eq!(record.nature, Code(Nature::Suggested));
    assert_eq!(record.rationale, "O que foi decidido?");
    assert_eq!(
        sessions::detail(&h.core, &id).unwrap().records,
        vec![record]
    );

    let err = questions::to_record(
        &h.core,
        &QuestionToRecordDto {
            question_id: QuestionId::new().to_string(),
            kind: Code(RecordKind::Risk),
        },
    )
    .unwrap_err();
    assert_eq!(err.code, "not_found");
}
