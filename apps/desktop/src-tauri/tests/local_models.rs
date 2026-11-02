//! `local_model_download` plumbing (T074): progress then completion over the channel, one
//! download per model, and the downloaded model makes the whisper provider usable.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use devcopilot_application::ports::TranscriberFactory;
use devcopilot_desktop_lib::models::{DownloadSink, LocalModels, ModelDownloadEvent};
use devcopilot_domain::ids::ProviderId;
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{
    DataCategory, Provider, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
use devcopilot_intelligence::WhisperTranscriberFactory;
use devcopilot_intelligence::model_download::ModelSpec;
use sha2::{Digest, Sha256};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default, Clone)]
struct Recorder(Arc<Mutex<Vec<ModelDownloadEvent>>>);

impl DownloadSink for Recorder {
    fn deliver(&self, event: ModelDownloadEvent) -> bool {
        self.0.lock().unwrap().push(event);
        true
    }
}

impl Recorder {
    fn events(&self) -> Vec<ModelDownloadEvent> {
        self.0.lock().unwrap().clone()
    }
}

async fn wait_for(condition: impl Fn() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("condition not reached");
}

fn spec(server: &MockServer, body: &[u8], sha: &[u8]) -> ModelSpec {
    let sha256 = Sha256::digest(sha)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    ModelSpec::for_test(
        "base",
        format!("{}/ggml-base.bin", server.uri()),
        body.len() as u64,
        sha256,
    )
}

#[tokio::test]
async fn download_reports_progress_then_completion() {
    let body = vec![7_u8; 4_096];
    let server = MockServer::start().await;
    Mock::given(path("/ggml-base.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let models = Arc::new(LocalModels::new(dir.path()));
    let recorder = Recorder::default();
    let job = models
        .start(spec(&server, &body, &body), Box::new(recorder.clone()))
        .unwrap();
    wait_for(|| {
        recorder
            .events()
            .iter()
            .any(|e| matches!(e, ModelDownloadEvent::Completed { .. }))
    })
    .await;
    let events = recorder.events();
    assert!(matches!(
        &events[0],
        ModelDownloadEvent::Progress { job_id, received_bytes: 0, total_bytes: 4_096 } if *job_id == job
    ));
    assert_eq!(
        events.last(),
        Some(&ModelDownloadEvent::Completed { job_id: job })
    );
    assert!(!models.list().iter().any(|m| m.downloading));
}

#[tokio::test]
async fn failed_verification_is_reported_with_a_message_key() {
    let body = vec![1_u8; 64];
    let server = MockServer::start().await;
    Mock::given(path("/ggml-base.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let models = Arc::new(LocalModels::new(dir.path()));
    let recorder = Recorder::default();
    models
        .start(spec(&server, &body, b"other"), Box::new(recorder.clone()))
        .unwrap();
    wait_for(|| {
        recorder
            .events()
            .iter()
            .any(|e| matches!(e, ModelDownloadEvent::Failed { .. }))
    })
    .await;
    assert!(matches!(
        recorder.events().last(),
        Some(ModelDownloadEvent::Failed { message_key, .. }) if message_key == "errors.modelVerificationFailed"
    ));
}

#[tokio::test]
async fn one_download_per_model() {
    let body = vec![1_u8; 64];
    let server = MockServer::start().await;
    Mock::given(path("/ggml-base.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(body.clone())
                .set_delay(Duration::from_millis(300)),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let models = Arc::new(LocalModels::new(dir.path()));
    models
        .start(spec(&server, &body, &body), Box::new(Recorder::default()))
        .unwrap();
    assert!(
        models
            .list()
            .iter()
            .any(|m| m.id == "base" && m.downloading)
    );
    let err = models
        .start(spec(&server, &body, &body), Box::new(Recorder::default()))
        .unwrap_err();
    assert_eq!(err.code, "invalid_state");
}

#[test]
fn whisper_provider_is_usable_only_once_its_model_is_present() {
    let dir = tempfile::tempdir().unwrap();
    let factory = WhisperTranscriberFactory::new(dir.path());
    let provider = Provider::new(ProviderSpec {
        id: ProviderId::new(),
        role: ProviderRole::Transcription,
        adapter: ProviderAdapterKind::WhisperLocal,
        endpoint: None,
        model: "tiny".into(),
        credential_ref: None,
        data_categories: BTreeSet::from([DataCategory::Audio]),
    })
    .unwrap();
    assert!(factory.create(&provider, SessionLanguage::En).is_err());
    let tiny = devcopilot_intelligence::model_download::find("tiny").unwrap();
    // A sparse file of the pinned size stands in for the verified download.
    let file = std::fs::File::create(dir.path().join(tiny.file_name())).unwrap();
    file.set_len(tiny.size_bytes()).unwrap();
    assert!(factory.create(&provider, SessionLanguage::En).is_ok());
}
