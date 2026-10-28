//! SC-013 with a fake server (T129): the real remote adapters, guarded by the provider
//! service's `RemoteGate`, send zero requests without a current authorization — before
//! authorizing, after revoking, and after the endpoint or categories change.

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration as StdDuration;

use devcopilot_application::ports::{
    AnalysisProvider, AnalysisRequest, AnalysisSegment, AudioChunk, ProviderError, Transcriber,
};
use devcopilot_application::provider_service::ProviderService;
use devcopilot_application::testing::{
    FakeClock, InMemoryProviderRepository, InMemorySettingsRepository, ToggleableSecretStore,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{ProviderId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{Provider, ProviderAdapterKind, ProviderRole, ProviderSpec};
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::Duration;
use devcopilot_intelligence::vad::ChunkerConfig;
use devcopilot_intelligence::{
    AnthropicAnalysisProvider, AnthropicChat, OpenAiSttTranscriber, RemoteTarget,
};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

fn service() -> ProviderService {
    ProviderService::new(
        Arc::new(InMemoryProviderRepository::default()),
        Arc::new(InMemorySettingsRepository::default()),
        Arc::new(ToggleableSecretStore::available()),
        Arc::new(FakeClock::at(1_000)),
    )
}

fn spec(id: ProviderId, adapter: ProviderAdapterKind, endpoint: &str) -> ProviderSpec {
    ProviderSpec {
        id,
        role: adapter.role(),
        adapter,
        endpoint: Some(endpoint.into()),
        model: "model-x".into(),
        credential_ref: None,
        data_categories: adapter
            .role()
            .allowed_categories()
            .iter()
            .copied()
            .collect(),
    }
}

/// The adapter points at the fake server; the configured provider keeps its `https` endpoint.
fn analyzer(
    service: &ProviderService,
    provider: &Provider,
    server: &MockServer,
) -> AnthropicAnalysisProvider {
    let target = RemoteTarget::for_test(
        provider,
        &server.uri(),
        Some("sk-test"),
        service.gate(),
        StdDuration::from_secs(5),
    )
    .unwrap();
    AnthropicAnalysisProvider::new(AnthropicChat::new(target, "model-x").unwrap())
}

fn request() -> AnalysisRequest {
    AnalysisRequest {
        session_id: SessionId::new(),
        language: SessionLanguage::PtBr,
        segments: vec![AnalysisSegment {
            id: SegmentId::new(),
            source: None,
            start: Duration::ZERO,
            text: Redacted::new(SegmentText::parse("conteúdo confidencial").unwrap()),
        }],
        records: Vec::new(),
    }
}

async fn mount_ok(server: &MockServer) {
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{ "type": "text", "text": "{\"schemaVersion\":1,\"proposals\":[]}" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        })))
        .mount(server)
        .await;
}

async fn requests(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

#[tokio::test]
async fn analysis_reaches_the_network_only_while_authorized() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let service = service();
    let id = ProviderId::new();
    let provider = service
        .upsert(spec(
            id,
            ProviderAdapterKind::Anthropic,
            "https://api.example.com",
        ))
        .unwrap();
    let adapter = analyzer(&service, &provider, &server);

    // Configured but never authorized (US7.2): nothing leaves.
    assert_eq!(
        adapter.analyze(request(), CancellationToken::new()).await,
        Err(ProviderError::AuthorizationRequired)
    );
    assert_eq!(requests(&server).await, 0);

    service.authorize(id).unwrap();
    adapter
        .analyze(request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(requests(&server).await, 1);

    // Revoked (US7.4): the same adapter instance stops at its next request.
    service.revoke(id).unwrap();
    assert_eq!(
        adapter.analyze(request(), CancellationToken::new()).await,
        Err(ProviderError::AuthorizationRequired)
    );
    assert_eq!(requests(&server).await, 1);
}

#[tokio::test]
async fn reconfiguration_needs_a_new_authorization() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let service = service();
    let id = ProviderId::new();
    let provider = service
        .upsert(spec(
            id,
            ProviderAdapterKind::Anthropic,
            "https://api.example.com",
        ))
        .unwrap();
    service.authorize(id).unwrap();
    let adapter = analyzer(&service, &provider, &server);
    service
        .upsert(spec(
            id,
            ProviderAdapterKind::Anthropic,
            "https://other.example.com",
        ))
        .unwrap();
    assert_eq!(
        adapter.analyze(request(), CancellationToken::new()).await,
        Err(ProviderError::AuthorizationRequired)
    );
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn transcription_uploads_nothing_without_authorization() {
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let service = service();
    let id = ProviderId::new();
    let provider = service
        .upsert(ProviderSpec {
            role: ProviderRole::Transcription,
            ..spec(
                id,
                ProviderAdapterKind::OpenAiCompatibleStt,
                "https://stt.example.com/v1",
            )
        })
        .unwrap();
    let target = RemoteTarget::for_test(
        &provider,
        &server.uri(),
        Some("sk-test"),
        service.gate(),
        StdDuration::from_secs(5),
    )
    .unwrap();
    let transcriber = OpenAiSttTranscriber::new(target, "whisper-1", SessionLanguage::PtBr)
        .with_chunker(ChunkerConfig {
            silence_to_finalize: Duration::from_millis(300),
            ..ChunkerConfig::default()
        });
    let mut samples: Vec<f32> = (0..16_000).map(|i| 0.3 * (i as f32 * 0.05).sin()).collect();
    samples.extend(std::iter::repeat_n(0.0, 16_000));
    let (tx, rx) = mpsc::channel(4);
    tx.send(AudioChunk {
        source: AudioSourceKind::Microphone,
        start: Duration::ZERO,
        samples,
    })
    .await
    .unwrap();
    drop(tx);
    let (out, _updates) = mpsc::channel(4);
    assert_eq!(
        transcriber.run(rx, out, CancellationToken::new()).await,
        Err(ProviderError::AuthorizationRequired)
    );
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn production_targets_refuse_plain_http() {
    // Remote providers cannot be configured with `http` (domain rule), and a production target
    // never downgrades: pointed at a plain HTTP server, the TLS handshake fails and the server
    // parses no request — the session data is never sent in clear text.
    let server = MockServer::start().await;
    mount_ok(&server).await;
    let secrets = Arc::new(ToggleableSecretStore::available());
    let service = ProviderService::new(
        Arc::new(InMemoryProviderRepository::default()),
        Arc::new(InMemorySettingsRepository::default()),
        secrets.clone(),
        Arc::new(FakeClock::at(1_000)),
    );
    let id = ProviderId::new();
    assert!(
        service
            .upsert(spec(id, ProviderAdapterKind::Anthropic, &server.uri()))
            .is_err()
    );
    let port = server.address().port();
    service
        .upsert(spec(
            id,
            ProviderAdapterKind::Anthropic,
            &format!("https://127.0.0.1:{port}"),
        ))
        .unwrap();
    service
        .set_secret(id, Redacted::new(b"sk-test".to_vec()))
        .unwrap();
    service.authorize(id).unwrap();
    let provider = service.entry(id).unwrap().provider;
    let target = RemoteTarget::new(
        &provider,
        secrets.as_ref(),
        service.gate(),
        StdDuration::from_secs(5),
    )
    .unwrap();
    let adapter = AnthropicAnalysisProvider::new(AnthropicChat::new(target, "model-x").unwrap());
    let err = adapter
        .analyze(request(), CancellationToken::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, ProviderError::Other(_) | ProviderError::Unavailable),
        "{err:?}"
    );
    assert_eq!(requests(&server).await, 0);
}
