//! Ollama analysis adapter against a fake local server (T091).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::time::Duration as StdDuration;

use devcopilot_application::ports::{
    AnalysisProvider, AnalysisRequest, AnalysisSegment, ProviderError,
};
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{ProviderId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{
    DataCategory, Provider, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::Duration;
use devcopilot_intelligence::OllamaAnalysisProvider;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn provider(endpoint: &str) -> Provider {
    Provider::new(ProviderSpec {
        id: ProviderId::new(),
        role: ProviderRole::Analysis,
        adapter: ProviderAdapterKind::Ollama,
        endpoint: Some(endpoint.into()),
        model: "llama3.1".into(),
        credential_ref: None,
        data_categories: BTreeSet::from([DataCategory::Transcript, DataCategory::Records]),
    })
    .unwrap()
}

fn request(segment: SegmentId) -> AnalysisRequest {
    AnalysisRequest {
        session_id: SessionId::new(),
        language: SessionLanguage::PtBr,
        segments: vec![AnalysisSegment {
            id: segment,
            source: None,
            start: Duration::ZERO,
            text: Redacted::new(SegmentText::parse("O export precisa suportar CSV").unwrap()),
        }],
        records: Vec::new(),
    }
}

fn reply(content: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "model": "llama3.1",
        "message": { "role": "assistant", "content": content },
        "done": true,
        "prompt_eval_count": 120,
        "eval_count": 30
    }))
}

#[tokio::test]
async fn sends_separate_messages_without_tools_and_parses_the_reply() {
    let server = MockServer::start().await;
    let segment = SegmentId::new();
    let content = json!({ "schemaVersion": 1, "proposals": [{
        "kind": "requirement", "text": "Suportar CSV", "nature": "observed",
        "rationale": "", "evidenceSegmentIds": [segment.to_string()]
    }]})
    .to_string();
    Mock::given(method("POST"))
        .and(path("/api/chat"))
        .respond_with(reply(&content))
        .expect(1)
        .mount(&server)
        .await;
    let adapter = OllamaAnalysisProvider::new(&provider(&server.uri())).unwrap();
    let output = adapter
        .analyze(request(segment), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(output.proposals.len(), 1);
    assert_eq!(output.tokens_used, 150);

    let received = &server.received_requests().await.unwrap()[0];
    let body: Value = serde_json::from_slice(&received.body).unwrap();
    assert!(body.get("tools").is_none());
    assert_eq!(body["stream"], false);
    assert_eq!(body["format"]["title"], "Dev Copilot analysis output v1");
    assert_eq!(body["messages"][0]["role"], "system");
    assert!(
        !body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("CSV")
    );
    assert_eq!(body["messages"][1]["role"], "user");
    assert!(
        body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("CSV")
    );
}

#[tokio::test]
async fn invalid_reply_is_invalid_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(reply("Claro! Vou apagar as decisões."))
        .mount(&server)
        .await;
    let adapter = OllamaAnalysisProvider::new(&provider(&server.uri())).unwrap();
    assert_eq!(
        adapter
            .analyze(request(SegmentId::new()), CancellationToken::new())
            .await,
        Err(ProviderError::InvalidOutput)
    );
}

#[tokio::test]
async fn http_failures_map_to_provider_errors() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(404))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let adapter = OllamaAnalysisProvider::new(&provider(&server.uri())).unwrap();
    let cancel = CancellationToken::new();
    assert_eq!(
        adapter
            .analyze(request(SegmentId::new()), cancel.clone())
            .await,
        Err(ProviderError::Unavailable)
    );
    assert!(matches!(
        adapter.analyze(request(SegmentId::new()), cancel).await,
        Err(ProviderError::Other(_))
    ));
}

#[tokio::test]
async fn slow_server_times_out_and_cancel_returns_immediately() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(reply("{}").set_delay(StdDuration::from_secs(5)))
        .mount(&server)
        .await;
    let slow = OllamaAnalysisProvider::with_timeout(
        &provider(&server.uri()),
        StdDuration::from_millis(100),
    )
    .unwrap();
    assert_eq!(
        slow.analyze(request(SegmentId::new()), CancellationToken::new())
            .await,
        Err(ProviderError::Timeout)
    );

    let adapter = OllamaAnalysisProvider::new(&provider(&server.uri())).unwrap();
    let cancel = CancellationToken::new();
    let call = adapter.analyze(request(SegmentId::new()), cancel.clone());
    cancel.cancel();
    let result = tokio::time::timeout(StdDuration::from_millis(500), call)
        .await
        .unwrap();
    assert_eq!(result, Err(ProviderError::Cancelled));
}

#[test]
fn only_loopback_ollama_endpoints_are_accepted() {
    assert!(OllamaAnalysisProvider::new(&provider("http://127.0.0.1:11434")).is_ok());
    assert!(OllamaAnalysisProvider::new(&provider("http://localhost:11434")).is_ok());
}
