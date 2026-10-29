//! Content at rest (T158): session content written through the repositories is unreadable in
//! the database and WAL files (SC-018), and no audio — WAV or raw PCM — is written to the app
//! data, cache or temp directories, including while the remote STT adapter uploads chunks
//! (FR-020).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use devcopilot_application::ports::{
    AudioChunk, QuestionRepository, RecordRepository, RemoteGate, SegmentRepository,
    SessionRepository, Transcriber,
};
use devcopilot_application::testing::{FakeRemoteGate, ToggleableSecretStore};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{ProviderId, QuestionId, RecordId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{
    DataCategory, Provider, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
use devcopilot_domain::question::ContextQuestion;
use devcopilot_domain::record::{RecordKind, RecordStatus, TechnicalRecord, UserRecord};
use devcopilot_domain::session::Session;
use devcopilot_domain::text::{QuestionText, Rationale, RecordText, SegmentText, Title};
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::{SegmentKind, SegmentSpec, TranscriptSegment};
use devcopilot_intelligence::vad::ChunkerConfig;
use devcopilot_intelligence::{OpenAiSttTranscriber, RemoteTarget};
use devcopilot_persistence::{
    Database, SqliteQuestionRepository, SqliteRecordRepository, SqliteSegmentRepository,
    SqliteSessionRepository,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const TITLE: &str = "Planejamento OMEGA-titulo";
const SPOKEN: &str = "frase falada OMEGA-3141 sobre o contrato";
const RECORD: &str = "registro OMEGA-2718 confirmado";
const QUESTION: &str = "pergunta OMEGA-1618 sobre o prazo?";
const PHRASES: &[&str] = &["OMEGA-titulo", "OMEGA-3141", "OMEGA-2718", "OMEGA-1618"];

fn at(millis: i64) -> Timestamp {
    Timestamp::from_unix_millis(millis).unwrap()
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn session_content_is_unreadable_in_the_database_and_wal() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = ToggleableSecretStore::available();
    let db = Arc::new(Database::open_in(dir.path(), &secrets).unwrap());
    let sessions = SqliteSessionRepository::new(Arc::clone(&db));
    let segments = SqliteSegmentRepository::new(Arc::clone(&db));
    let records = SqliteRecordRepository::new(Arc::clone(&db));
    let questions = SqliteQuestionRepository::new(Arc::clone(&db));

    let mut session = Session::new(
        SessionId::new(),
        Title::parse(TITLE).unwrap(),
        SessionLanguage::PtBr,
        &[AudioSourceKind::Microphone],
        at(1_000),
    )
    .unwrap();
    sessions.create(&session).unwrap();
    session.start().unwrap();
    session.ready(at(2_000)).unwrap();
    sessions.update(&session).unwrap();
    let id = session.id();
    segments
        .append(&TranscriptSegment::restore(SegmentSpec {
            id: SegmentId::new(),
            session_id: id,
            seq: 0,
            kind: SegmentKind::Final {
                text: Redacted::new(SegmentText::parse(SPOKEN).unwrap()),
            },
            source: None,
            start: Duration::ZERO,
            end: Duration::from_secs(3),
        }))
        .unwrap();
    let record = TechnicalRecord::create_by_user(
        RecordId::new(),
        id,
        UserRecord {
            kind: RecordKind::Decision,
            text: Redacted::new(RecordText::parse(RECORD).unwrap()),
            rationale: Redacted::new(Rationale::default()),
            status: RecordStatus::Confirmed,
        },
        at(3_000),
    )
    .unwrap();
    records.insert(&record, &record.creation()).unwrap();
    let question = ContextQuestion::ask(
        QuestionId::new(),
        id,
        Redacted::new(QuestionText::parse(QUESTION).unwrap()),
        None,
        at(4_000),
    );
    questions.insert(&question).unwrap();

    // Sanity: the content is stored and readable through the key.
    assert_eq!(segments.list_by_session(id).unwrap().len(), 1);
    // Scan while the WAL still holds the last writes, then again after a checkpoint.
    let scan = || {
        let files = files_under(dir.path());
        assert!(
            files.iter().any(|f| f.to_string_lossy().ends_with("-wal")),
            "WAL file expected while the database is open"
        );
        for file in files {
            let bytes = std::fs::read(&file).unwrap();
            for phrase in PHRASES {
                assert!(
                    !contains(&bytes, phrase.as_bytes()),
                    "`{phrase}` readable in {}",
                    file.display()
                );
            }
        }
    };
    scan();
    db.with_conn(|conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        Ok(())
    })
    .unwrap();
    scan();
}

/// New files under `dirs` since `before`, by path.
fn new_files(dirs: &[PathBuf], before: &HashSet<PathBuf>) -> Vec<PathBuf> {
    dirs.iter()
        .flat_map(|dir| files_under(dir))
        .filter(|path| !before.contains(path))
        .collect()
}

/// Whether `bytes` hold this test's audio: the uploaded 16-bit samples, WAV-wrapped or raw.
/// Matching only our own samples keeps other applications' media caches from tripping it.
fn looks_like_audio(bytes: &[u8], pcm_probe: &[u8]) -> bool {
    contains(bytes, pcm_probe)
}

#[tokio::test]
async fn remote_stt_upload_writes_no_audio_to_disk() {
    let data = tempfile::tempdir().unwrap();
    let watched = vec![
        data.path().to_owned(),
        std::env::temp_dir(),
        dirs_cache().unwrap_or_else(std::env::temp_dir),
    ];
    let before: HashSet<PathBuf> = watched.iter().flat_map(|d| files_under(d)).collect();

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "text": "ok" })))
        .mount(&server)
        .await;
    let provider = Provider::new(ProviderSpec {
        id: ProviderId::new(),
        role: ProviderRole::Transcription,
        adapter: ProviderAdapterKind::OpenAiCompatibleStt,
        endpoint: Some("https://stt.example.com/v1".into()),
        model: "whisper-1".into(),
        credential_ref: None,
        data_categories: BTreeSet::from([DataCategory::Audio]),
    })
    .unwrap();
    let gate = Arc::new(FakeRemoteGate::default());
    gate.grant(provider.id(), &[DataCategory::Audio]);
    let target = RemoteTarget::for_test(
        &provider,
        &server.uri(),
        Some("sk-test"),
        gate as Arc<dyn RemoteGate>,
        StdDuration::from_secs(5),
    )
    .unwrap();
    let transcriber = OpenAiSttTranscriber::new(target, "whisper-1", SessionLanguage::PtBr)
        .with_chunker(ChunkerConfig {
            silence_to_finalize: Duration::from_millis(300),
            ..ChunkerConfig::default()
        });
    // A distinctive tone, so its PCM can be searched for byte by byte.
    let samples: Vec<f32> = (0..16_000)
        .map(|i| 0.3 * (i as f32 * 0.0731).sin())
        .chain(std::iter::repeat_n(0.0, 16_000))
        .collect();
    let probe: Vec<u8> = samples[1_000..1_016]
        .iter()
        .flat_map(|s| {
            #[allow(clippy::cast_possible_truncation)]
            let value = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
            value.to_le_bytes()
        })
        .collect();
    let (tx, rx) = mpsc::channel(4);
    for start in [0_u64, 2] {
        tx.send(AudioChunk {
            source: AudioSourceKind::Microphone,
            start: Duration::from_secs(start),
            samples: samples.clone(),
        })
        .await
        .unwrap();
    }
    drop(tx);
    let (out, _updates) = mpsc::channel(8);
    transcriber
        .run(rx, out, CancellationToken::new())
        .await
        .unwrap();
    // The audio did leave through the adapter (in memory), proving the probe is meaningful.
    let uploads = server.received_requests().await.unwrap();
    assert_eq!(uploads.len(), 2);
    assert!(looks_like_audio(&uploads[0].body, &probe));
    assert!(contains(&uploads[0].body, b"RIFF"));

    for file in new_files(&watched, &before) {
        let Ok(bytes) = std::fs::read(&file) else {
            continue; // removed meanwhile by another process
        };
        assert!(
            !looks_like_audio(&bytes, &probe),
            "audio written to {}",
            file.display()
        );
    }
}

/// The user cache directory, where the platform has one.
fn dirs_cache() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library/Caches"))
    } else if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".cache")))
    }
}
