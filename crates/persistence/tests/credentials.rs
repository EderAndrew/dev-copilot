//! Provider credentials live only in the OS credential store (T132, FR-049, SC-014): never in
//! the database, the diagnostic log or an export.

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::io::Write;
use std::sync::{Arc, Mutex};

use devcopilot_application::export_service::{ExportDeps, ExportRequest, ExportService};
use devcopilot_application::ports::{SecretStore, SessionRepository};
use devcopilot_application::provider_service::ProviderService;
use devcopilot_application::settings_service::SettingsService;
use devcopilot_application::testing::{FakeClock, ToggleableSecretStore};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::export::{ExportContent, ExportFormat};
use devcopilot_domain::ids::{ProviderId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{
    CredentialRef, DataCategory, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
use devcopilot_domain::session::Session;
use devcopilot_domain::text::Title;
use devcopilot_domain::time::Timestamp;
use devcopilot_persistence::{
    Database, SqliteExportRepository, SqliteProviderRepository, SqliteQuestionRepository,
    SqliteRecordRepository, SqliteSegmentRepository, SqliteSessionRepository,
    SqliteSettingsRepository,
};

const SECRET: &str = "sk-ant-TEST-6f1c0d9e-very-secret";

/// Log sink shared with the subscriber.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Every text value stored in any table of the database.
fn all_stored_text(db: &Database) -> String {
    db.with_conn(|conn| {
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let mut text = String::new();
        for table in tables {
            let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\"")).unwrap();
            let columns = stmt.column_count();
            let mut rows = stmt.query([]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                for i in 0..columns {
                    if let Ok(value) = row.get::<_, String>(i) {
                        text.push_str(&value);
                        text.push('\n');
                    }
                }
            }
        }
        Ok(text)
    })
    .unwrap()
}

#[test]
fn api_keys_stay_in_the_credential_store() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(ToggleableSecretStore::available());
    let db = Arc::new(Database::open_in(dir.path(), secrets.as_ref()).unwrap());
    let settings_repo = Arc::new(SqliteSettingsRepository::new(Arc::clone(&db)));
    let clock = Arc::new(FakeClock::at(1_000));
    let providers = ProviderService::new(
        Arc::new(SqliteProviderRepository::new(Arc::clone(&db))),
        settings_repo.clone(),
        secrets.clone(),
        clock.clone(),
    );
    let sessions = Arc::new(SqliteSessionRepository::new(Arc::clone(&db)));
    let export = ExportService::new(ExportDeps {
        sessions: sessions.clone(),
        segments: Arc::new(SqliteSegmentRepository::new(Arc::clone(&db))),
        records: Arc::new(SqliteRecordRepository::new(Arc::clone(&db))),
        questions: Arc::new(SqliteQuestionRepository::new(Arc::clone(&db))),
        exports: Arc::new(SqliteExportRepository::new(Arc::clone(&db))),
        settings: Arc::new(SettingsService::new(settings_repo)),
        clock: clock.clone(),
    });

    let log = Captured::default();
    let writer = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || writer.clone())
        .finish();
    let id = ProviderId::new();
    let session = Session::new(
        SessionId::new(),
        Title::parse("Planejamento").unwrap(),
        SessionLanguage::PtBr,
        &[AudioSourceKind::Microphone],
        Timestamp::from_unix_millis(1_000).unwrap(),
    )
    .unwrap();
    let documents = tracing::subscriber::with_default(subscriber, || {
        providers
            .upsert(ProviderSpec {
                id,
                role: ProviderRole::Analysis,
                adapter: ProviderAdapterKind::Anthropic,
                endpoint: Some("https://api.example.com".into()),
                model: "claude-opus-5-5".into(),
                credential_ref: None,
                data_categories: BTreeSet::from([DataCategory::Transcript]),
            })
            .unwrap();
        providers
            .set_secret(id, Redacted::new(SECRET.as_bytes().to_vec()))
            .unwrap();
        providers.authorize(id).unwrap();
        tracing::info!(secret = ?Redacted::new(SECRET.to_owned()), "redaction check");
        sessions.create(&session).unwrap();
        export
            .render(&ExportRequest {
                session_id: session.id(),
                formats: BTreeSet::from([ExportFormat::Markdown, ExportFormat::Json]),
                content: BTreeSet::from([ExportContent::Plan, ExportContent::Transcript]),
            })
            .unwrap()
    });

    // In the credential store, under the provider's reference only.
    let reference = CredentialRef::for_provider(id);
    assert_eq!(
        secrets
            .get(&reference)
            .unwrap()
            .unwrap()
            .expose()
            .as_slice(),
        SECRET.as_bytes()
    );
    // The database holds the reference, never the key.
    let stored = all_stored_text(&db);
    assert!(stored.contains(reference.as_str()));
    assert!(!stored.contains(SECRET));
    // Logs saw the operations but not the key.
    let log = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert!(log.contains("provider credential stored"));
    assert!(log.contains("[redacted]"));
    assert!(!log.contains(SECRET));
    // Exports carry neither the key nor its reference.
    assert_eq!(documents.len(), 2);
    for document in documents {
        assert!(!document.contents.expose().contains(SECRET));
        assert!(!document.contents.expose().contains(reference.as_str()));
    }
}

#[test]
fn unavailable_store_keeps_the_key_out_of_everything() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(ToggleableSecretStore::available());
    let db = Arc::new(Database::open_in(dir.path(), secrets.as_ref()).unwrap());
    let providers = ProviderService::new(
        Arc::new(SqliteProviderRepository::new(Arc::clone(&db))),
        Arc::new(SqliteSettingsRepository::new(Arc::clone(&db))),
        secrets.clone(),
        Arc::new(FakeClock::at(1_000)),
    );
    let id = ProviderId::new();
    providers
        .upsert(ProviderSpec {
            id,
            role: ProviderRole::Transcription,
            adapter: ProviderAdapterKind::OpenAiCompatibleStt,
            endpoint: Some("https://stt.example.com/v1".into()),
            model: "whisper-1".into(),
            credential_ref: None,
            data_categories: BTreeSet::from([DataCategory::Audio]),
        })
        .unwrap();
    secrets.set_available(false);
    assert!(
        providers
            .set_secret(id, Redacted::new(SECRET.as_bytes().to_vec()))
            .is_err()
    );
    // No fallback to plain-text storage.
    assert!(!all_stored_text(&db).contains(SECRET));
    secrets.set_available(true);
    assert_eq!(secrets.get(&CredentialRef::for_provider(id)).unwrap(), None);
}
