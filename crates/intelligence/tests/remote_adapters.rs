//! Remote adapter contracts against a fake server (T131): request shape, credentials, no tools,
//! declared categories only, timeouts and malformed replies mapped to errors.

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use devcopilot_application::ports::{
    AnalysisProvider, AnalysisRequest, AnalysisSegment, AudioChunk, ProviderError, QuestionRequest,
    RecordSummary, RemoteGate, Transcriber, TranscriptUpdate,
};
use devcopilot_application::testing::FakeRemoteGate;
use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{ProviderId, RecordId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{DataCategory, Provider, ProviderAdapterKind, ProviderSpec};
use devcopilot_domain::question::Certainty;
use devcopilot_domain::record::{RecordKind, RecordStatus};
use devcopilot_domain::text::{QuestionText, RecordText, SegmentText};
use devcopilot_domain::time::Duration;
use devcopilot_intelligence::anthropic::API_VERSION;
use devcopilot_intelligence::vad::ChunkerConfig;
use devcopilot_intelligence::{
    AnthropicAnalysisProvider, AnthropicChat, OpenAiChat, OpenAiChatAnalysisProvider,
    OpenAiSttTranscriber, RemoteTarget,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "sk-test-secret-123";

fn provider(adapter: ProviderAdapterKind, categories: &[DataCategory]) -> Provider {
    Provider::new(ProviderSpec {
        id: ProviderId::new(),
        role: adapter.role(),
        adapter,
        endpoint: Some("https://api.example.com".into()),
        model: "model-x".into(),
        credential_ref: None,
        data_categories: categories.iter().copied().collect::<BTreeSet<_>>(),
    })
    .unwrap()
}

const ANALYSIS: &[DataCategory] = &[
    DataCategory::Transcript,
    DataCategory::Records,
    DataCategory::Questions,
];

fn granted(provider: &Provider) -> Arc<FakeRemoteGate> {
    let gate = Arc::new(FakeRemoteGate::default());
    let categories: Vec<_> = provider.data_categories().iter().copied().collect();
    gate.grant(provider.id(), &categories);
    gate
}

fn target(
    provider: &Provider,
    server: &MockServer,
    key: Option<&str>,
    gate: Arc<FakeRemoteGate>,
    timeout: StdDuration,
) -> RemoteTarget {
    RemoteTarget::for_test(
        provider,
        &server.uri(),
        key,
        gate as Arc<dyn RemoteGate>,
        timeout,
    )
    .unwrap()
}

fn analysis_request(segment: SegmentId) -> AnalysisRequest {
    AnalysisRequest {
        session_id: SessionId::new(),
        language: SessionLanguage::PtBr,
        segments: vec![AnalysisSegment {
            id: segment,
            source: None,
            start: Duration::ZERO,
            text: Redacted::new(SegmentText::parse("O export precisa suportar CSV").unwrap()),
        }],
        records: vec![RecordSummary {
            id: RecordId::new(),
            kind: RecordKind::Decision,
            status: RecordStatus::Confirmed,
            text: Redacted::new(RecordText::parse("Usar Postgres-registro-sigiloso").unwrap()),
        }],
    }
}

fn proposals(segment: SegmentId) -> String {
    json!({ "schemaVersion": 1, "proposals": [{
        "kind": "requirement", "text": "Suportar CSV", "nature": "observed",
        "rationale": "", "evidenceSegmentIds": [segment.to_string()]
    }]})
    .to_string()
}

fn anthropic_reply(text: &str, stop_reason: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "msg_1", "type": "message", "role": "assistant", "model": "model-x",
        "content": [{ "type": "thinking", "thinking": "" }, { "type": "text", "text": text }],
        "stop_reason": stop_reason,
        "usage": { "input_tokens": 100, "output_tokens": 20 }
    }))
}

fn anthropic(
    server: &MockServer,
    provider: &Provider,
    timeout: StdDuration,
) -> AnthropicAnalysisProvider {
    AnthropicAnalysisProvider::new(
        AnthropicChat::new(
            target(provider, server, Some(KEY), granted(provider), timeout),
            "model-x",
        )
        .unwrap(),
    )
}

async fn body(server: &MockServer) -> Value {
    let received = server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    serde_json::from_slice(&received[0].body).unwrap()
}

#[tokio::test]
async fn anthropic_sends_key_version_and_delimited_data_without_tools() {
    let server = MockServer::start().await;
    let segment = SegmentId::new();
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", KEY))
        .and(header("anthropic-version", API_VERSION))
        .respond_with(anthropic_reply(&proposals(segment), "end_turn"))
        .expect(1)
        .mount(&server)
        .await;
    let provider = provider(ProviderAdapterKind::Anthropic, ANALYSIS);
    let adapter = anthropic(&server, &provider, StdDuration::from_secs(5));
    let output = adapter
        .analyze(analysis_request(segment), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(output.proposals.len(), 1);
    assert_eq!(output.tokens_used, 120);
    let body = body(&server).await;
    assert!(body.get("tools").is_none());
    assert!(body.get("temperature").is_none());
    assert_eq!(body["model"], "model-x");
    assert!(!body["system"].as_str().unwrap().contains("CSV"));
    assert_eq!(body["messages"].as_array().unwrap().len(), 1);
    assert_eq!(body["messages"][0]["role"], "user");
    assert!(
        body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("CSV")
    );
}

#[tokio::test]
async fn anthropic_answers_questions() {
    let server = MockServer::start().await;
    let segment = SegmentId::new();
    let answer = json!({
        "schemaVersion": 1, "answer": "CSV", "certainty": "supported",
        "evidenceSegmentIds": [segment.to_string()], "evidenceRecordIds": []
    })
    .to_string();
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(anthropic_reply(&answer, "end_turn"))
        .mount(&server)
        .await;
    let provider = provider(ProviderAdapterKind::Anthropic, ANALYSIS);
    let adapter = anthropic(&server, &provider, StdDuration::from_secs(5));
    let request = QuestionRequest {
        session_id: SessionId::new(),
        language: SessionLanguage::PtBr,
        question: Redacted::new(QuestionText::parse("Qual formato?").unwrap()),
        segments: analysis_request(segment).segments,
        records: Vec::new(),
    };
    let output = adapter
        .answer(request, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(output.answer.certainty, Certainty::Supported);
}

type ErrorCheck = fn(&ProviderError) -> bool;

#[tokio::test]
async fn anthropic_maps_failures() {
    let provider = provider(ProviderAdapterKind::Anthropic, ANALYSIS);
    let cases: Vec<(ResponseTemplate, ErrorCheck)> = vec![
        (anthropic_reply("{}", "refusal"), |e| {
            matches!(e, ProviderError::Other(_))
        }),
        (anthropic_reply("{\"schemaVersion\":", "max_tokens"), |e| {
            *e == ProviderError::InvalidOutput
        }),
        (anthropic_reply("not json", "end_turn"), |e| {
            *e == ProviderError::InvalidOutput
        }),
        (ResponseTemplate::new(200).set_body_string("<html>"), |e| {
            *e == ProviderError::InvalidOutput
        }),
        (ResponseTemplate::new(401), |e| {
            *e == ProviderError::Other("http status 401".into())
        }),
        (ResponseTemplate::new(529), |e| {
            matches!(e, ProviderError::Other(_))
        }),
        (
            anthropic_reply("{}", "end_turn").set_delay(StdDuration::from_secs(2)),
            |e| *e == ProviderError::Timeout,
        ),
    ];
    for (template, expected) in cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(template)
            .mount(&server)
            .await;
        let adapter = anthropic(&server, &provider, StdDuration::from_millis(300));
        let err = adapter
            .analyze(analysis_request(SegmentId::new()), CancellationToken::new())
            .await
            .unwrap_err();
        assert!(expected(&err), "{err:?}");
        assert!(!format!("{err}").contains(KEY));
    }
}

#[tokio::test]
async fn anthropic_requires_an_api_key() {
    let server = MockServer::start().await;
    let provider = provider(ProviderAdapterKind::Anthropic, ANALYSIS);
    let target = target(
        &provider,
        &server,
        None,
        granted(&provider),
        StdDuration::from_secs(1),
    );
    assert_eq!(
        AnthropicChat::new(target, "model-x").map(|_| ()),
        Err(ProviderError::Unavailable)
    );
}

#[tokio::test]
async fn only_declared_categories_are_sent() {
    let server = MockServer::start().await;
    let segment = SegmentId::new();
    Mock::given(method("POST"))
        .respond_with(anthropic_reply(&proposals(segment), "end_turn"))
        .mount(&server)
        .await;
    let provider = provider(ProviderAdapterKind::Anthropic, &[DataCategory::Transcript]);
    let gate = granted(&provider);
    let adapter = AnthropicAnalysisProvider::new(
        AnthropicChat::new(
            target(
                &provider,
                &server,
                Some(KEY),
                gate.clone(),
                StdDuration::from_secs(5),
            ),
            "model-x",
        )
        .unwrap(),
    );
    adapter
        .analyze(analysis_request(segment), CancellationToken::new())
        .await
        .unwrap();
    let body = body(&server).await.to_string();
    assert!(body.contains("CSV"));
    assert!(
        !body.contains("registro-sigiloso"),
        "records were not declared"
    );
    assert_eq!(
        gate.checks(),
        [(provider.id(), vec![DataCategory::Transcript])]
    );
}

#[tokio::test]
async fn cancellation_returns_promptly() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(anthropic_reply("{}", "end_turn").set_delay(StdDuration::from_secs(30)))
        .mount(&server)
        .await;
    let provider = provider(ProviderAdapterKind::Anthropic, ANALYSIS);
    let adapter = anthropic(&server, &provider, StdDuration::from_secs(60));
    let cancel = CancellationToken::new();
    let call = adapter.analyze(analysis_request(SegmentId::new()), cancel.clone());
    cancel.cancel();
    assert_eq!(call.await.unwrap_err(), ProviderError::Cancelled);
}

fn chat_reply(content: Option<&str>, finish_reason: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "object": "chat.completion",
        "choices": [{ "index": 0, "message": { "role": "assistant", "content": content },
                      "finish_reason": finish_reason }],
        "usage": { "prompt_tokens": 80, "completion_tokens": 10 }
    }))
}

fn openai(
    server: &MockServer,
    provider: &Provider,
    key: Option<&str>,
) -> OpenAiChatAnalysisProvider {
    OpenAiChatAnalysisProvider::new(OpenAiChat::new(
        target(
            provider,
            server,
            key,
            granted(provider),
            StdDuration::from_millis(500),
        ),
        "model-x",
    ))
}

#[tokio::test]
async fn openai_chat_sends_bearer_json_mode_and_no_tools() {
    let server = MockServer::start().await;
    let segment = SegmentId::new();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(chat_reply(Some(&proposals(segment)), "stop"))
        .expect(1)
        .mount(&server)
        .await;
    let provider = provider(ProviderAdapterKind::OpenAiCompatibleChat, ANALYSIS);
    let output = openai(&server, &provider, Some(KEY))
        .analyze(analysis_request(segment), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(output.tokens_used, 90);
    let body = body(&server).await;
    assert!(body.get("tools").is_none());
    assert_eq!(body["response_format"]["type"], "json_object");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert!(
        !body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("CSV")
    );
}

#[tokio::test]
async fn openai_chat_without_key_sends_no_authorization() {
    let server = MockServer::start().await;
    let segment = SegmentId::new();
    Mock::given(method("POST"))
        .respond_with(chat_reply(Some(&proposals(segment)), "stop"))
        .mount(&server)
        .await;
    let provider = provider(ProviderAdapterKind::OpenAiCompatibleChat, ANALYSIS);
    openai(&server, &provider, None)
        .analyze(analysis_request(segment), CancellationToken::new())
        .await
        .unwrap();
    let received = server.received_requests().await.unwrap();
    assert!(received[0].headers.get("authorization").is_none());
}

#[tokio::test]
async fn openai_chat_maps_malformed_replies() {
    let provider = provider(ProviderAdapterKind::OpenAiCompatibleChat, ANALYSIS);
    for template in [
        chat_reply(Some("{\"schemaVersion\""), "length"),
        chat_reply(None, "stop"),
        ResponseTemplate::new(200).set_body_json(json!({ "choices": [] })),
        chat_reply(Some("[1,2]"), "stop"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(template)
            .mount(&server)
            .await;
        assert_eq!(
            openai(&server, &provider, Some(KEY))
                .analyze(analysis_request(SegmentId::new()), CancellationToken::new())
                .await
                .unwrap_err(),
            ProviderError::InvalidOutput
        );
    }
}

/// 1 s of speech-level tone followed by 1 s of silence.
fn speech_then_silence(start: Duration) -> AudioChunk {
    let mut samples: Vec<f32> = (0..16_000).map(|i| 0.3 * (i as f32 * 0.05).sin()).collect();
    samples.extend(std::iter::repeat_n(0.0, 16_000));
    AudioChunk {
        source: AudioSourceKind::Microphone,
        start,
        samples,
    }
}

fn fast_chunker() -> ChunkerConfig {
    ChunkerConfig {
        silence_to_finalize: Duration::from_millis(300),
        partial_interval: Duration::from_secs(3_600),
        ..ChunkerConfig::default()
    }
}

async fn transcribe(
    transcriber: &OpenAiSttTranscriber,
    chunks: Vec<AudioChunk>,
) -> (Result<(), ProviderError>, Vec<TranscriptUpdate>) {
    let (tx, rx) = mpsc::channel(16);
    let (out_tx, mut out_rx) = mpsc::channel(16);
    for chunk in chunks {
        tx.send(chunk).await.unwrap();
    }
    drop(tx);
    let result = transcriber.run(rx, out_tx, CancellationToken::new()).await;
    let mut updates = Vec::new();
    while let Ok(update) = out_rx.try_recv() {
        updates.push(update);
    }
    (result, updates)
}

fn stt(
    server: &MockServer,
    provider: &Provider,
    gate: Arc<FakeRemoteGate>,
) -> OpenAiSttTranscriber {
    OpenAiSttTranscriber::new(
        target(
            provider,
            server,
            Some(KEY),
            gate,
            StdDuration::from_millis(500),
        ),
        "whisper-1",
        SessionLanguage::PtBr,
    )
    .with_chunker(fast_chunker())
}

#[tokio::test]
async fn stt_uploads_each_final_window_as_wav_and_emits_finals_only() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .and(header("authorization", format!("Bearer {KEY}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "text": " Olá, time. " })))
        .expect(2)
        .mount(&server)
        .await;
    let provider = provider(
        ProviderAdapterKind::OpenAiCompatibleStt,
        &[DataCategory::Audio],
    );
    let gate = granted(&provider);
    let transcriber = stt(&server, &provider, gate.clone());
    let (result, updates) = transcribe(
        &transcriber,
        vec![
            speech_then_silence(Duration::ZERO),
            speech_then_silence(Duration::from_secs(2)),
        ],
    )
    .await;
    result.unwrap();
    assert_eq!(updates.len(), 2);
    for update in &updates {
        match update {
            TranscriptUpdate::Final { text, .. } => {
                assert_eq!(text.expose().as_str(), "Olá, time.")
            }
            TranscriptUpdate::Partial { .. } => panic!("remote mode emits no partials"),
        }
    }
    assert_eq!(
        gate.checks(),
        vec![(provider.id(), vec![DataCategory::Audio]); 2]
    );
    let received = server.received_requests().await.unwrap();
    let content_type = received[0]
        .headers
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.starts_with("multipart/form-data; boundary="));
    let body = String::from_utf8_lossy(&received[0].body);
    assert!(body.contains("name=\"model\"\r\n\r\nwhisper-1"));
    assert!(body.contains("name=\"language\"\r\n\r\npt"));
    assert!(body.contains("filename=\"audio.wav\""));
    assert!(body.contains("RIFF"));
}

#[tokio::test]
async fn stt_silence_sends_nothing() {
    let server = MockServer::start().await;
    let provider = provider(
        ProviderAdapterKind::OpenAiCompatibleStt,
        &[DataCategory::Audio],
    );
    let transcriber = stt(&server, &provider, granted(&provider));
    let silence = AudioChunk {
        source: AudioSourceKind::Microphone,
        start: Duration::ZERO,
        samples: vec![0.0; 32_000],
    };
    let (result, updates) = transcribe(&transcriber, vec![silence]).await;
    result.unwrap();
    assert!(updates.is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn stt_failures_end_the_run_with_an_error() {
    let provider = provider(
        ProviderAdapterKind::OpenAiCompatibleStt,
        &[DataCategory::Audio],
    );
    let cases: Vec<(ResponseTemplate, ProviderError)> = vec![
        (
            ResponseTemplate::new(200).set_body_string("nope"),
            ProviderError::InvalidOutput,
        ),
        (
            ResponseTemplate::new(500),
            ProviderError::Other("http status 500".into()),
        ),
        (
            ResponseTemplate::new(200)
                .set_body_json(json!({ "text": "x" }))
                .set_delay(StdDuration::from_secs(2)),
            ProviderError::Timeout,
        ),
    ];
    for (template, expected) in cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(template)
            .mount(&server)
            .await;
        let transcriber = stt(&server, &provider, granted(&provider));
        let (result, updates) =
            transcribe(&transcriber, vec![speech_then_silence(Duration::ZERO)]).await;
        assert_eq!(result, Err(expected));
        assert!(updates.is_empty());
    }
}
