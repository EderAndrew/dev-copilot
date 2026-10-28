//! Q&A evaluation harness (T111a; SC-011, FR-035, FR-036).
//!
//! The annotated transcripts of T083 (`fixtures/eval/`) carry questions: answerable ones list
//! the keywords a correct answer mentions and the segments that support it; unanswerable ones
//! list terms an invented answer would likely use. Every answer is grounded in the session the
//! way `QuestionService` does it before it is measured:
//!
//! - **cited or declared**: share of answers that cite session evidence or declare
//!   `no_evidence` (SC-011: 100 %);
//! - **invented**: unanswerable questions answered with anything but `no_evidence`, or whose
//!   text uses a forbidden term (SC-011: 0);
//! - **accuracy**: answerable questions answered with the keywords and at least one supporting
//!   segment (informative; recorded in ADR-003 with the reference provider).
//!
//! By default it runs against scripted answers as a self-test of the metrics. With
//! `DEVCOPILOT_EVAL_PROVIDER=ollama` it asks a local Ollama server (`DEVCOPILOT_EVAL_ENDPOINT`,
//! default `http://127.0.0.1:11434`; `DEVCOPILOT_EVAL_MODEL`, default `llama3.1`), like the
//! analysis harness. Only synthetic, non-confidential transcripts are used.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::collections::BTreeSet;

use devcopilot_application::ports::{AnalysisProvider, AnalysisSegment, QuestionRequest};
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{ProviderId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{
    DataCategory, Provider, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
use devcopilot_domain::question::{Answer, Certainty, EvidenceRef};
use devcopilot_domain::text::{AnswerText, QuestionText, SegmentText};
use devcopilot_domain::time::Duration;
use devcopilot_intelligence::OllamaAnalysisProvider;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct Question {
    question: String,
    answerable: bool,
    #[serde(default)]
    keywords: Vec<String>,
    #[serde(default)]
    segments: Vec<usize>,
    #[serde(default)]
    forbidden: Vec<String>,
}

#[derive(Deserialize)]
struct Transcript {
    id: String,
    language: String,
    segments: Vec<String>,
    questions: Vec<Question>,
}

fn transcripts() -> Vec<Transcript> {
    [
        include_str!("fixtures/eval/export-planning.pt-BR.json"),
        include_str!("fixtures/eval/queue-migration.en.json"),
    ]
    .iter()
    .map(|raw| serde_json::from_str(raw).unwrap())
    .collect()
}

struct Fixture {
    transcript: Transcript,
    ids: Vec<SegmentId>,
}

impl Fixture {
    fn new(transcript: Transcript) -> Self {
        let ids = transcript
            .segments
            .iter()
            .map(|_| SegmentId::new())
            .collect();
        Self { transcript, ids }
    }

    fn request(&self, question: &Question) -> QuestionRequest {
        QuestionRequest {
            session_id: SessionId::new(),
            language: SessionLanguage::parse(&self.transcript.language).unwrap(),
            question: Redacted::new(QuestionText::parse(question.question.as_str()).unwrap()),
            segments: self
                .transcript
                .segments
                .iter()
                .zip(&self.ids)
                .enumerate()
                .map(|(i, (text, id))| AnalysisSegment {
                    id: *id,
                    source: None,
                    start: Duration::from_secs(i as u64 * 4),
                    text: Redacted::new(SegmentText::parse(text.as_str()).unwrap()),
                })
                .collect(),
            records: Vec::new(),
        }
    }

    /// Grounds `answer` in this session, as `QuestionService` does.
    fn ground(&self, answer: Answer) -> Answer {
        answer.grounded(|evidence| match evidence {
            EvidenceRef::Segment(segment) => self.ids.contains(&segment),
            EvidenceRef::Record(_) => false,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Metrics {
    cited_or_declared: f64,
    invented: usize,
    accuracy: f64,
}

/// Lowercase alphanumeric words of `text`, joined by single spaces and padded.
fn words(text: &str) -> String {
    let joined = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    format!(" {joined} ")
}

/// Whether `text` uses `term` as whole words.
fn mentions(text: &str, term: &str) -> bool {
    words(text).contains(&words(term))
}

/// Measures grounded `answers`, one per question in fixture order.
fn measure(fixture: &Fixture, answers: &[Answer]) -> Metrics {
    let questions = &fixture.transcript.questions;
    assert_eq!(answers.len(), questions.len());
    let cited_or_declared = answers
        .iter()
        .filter(|a| (a.certainty == Certainty::NoEvidence) == a.evidence.is_empty())
        .count();
    let mut invented = 0;
    let mut correct = 0;
    for (question, answer) in questions.iter().zip(answers) {
        let text = answer.text.expose().as_str();
        if question.answerable {
            let cites_support = question.segments.iter().any(|s| {
                answer
                    .evidence
                    .contains(&EvidenceRef::Segment(fixture.ids[*s]))
            });
            if cites_support && question.keywords.iter().all(|k| mentions(text, k)) {
                correct += 1;
            }
        } else if answer.certainty != Certainty::NoEvidence
            || question.forbidden.iter().any(|term| mentions(text, term))
        {
            invented += 1;
        }
    }
    let answerable = questions.iter().filter(|q| q.answerable).count();
    Metrics {
        cited_or_declared: cited_or_declared as f64 / answers.len() as f64,
        invented,
        accuracy: if answerable == 0 {
            1.0
        } else {
            correct as f64 / answerable as f64
        },
    }
}

fn answer(text: &str, certainty: Certainty, evidence: Vec<EvidenceRef>) -> Answer {
    Answer {
        text: Redacted::new(AnswerText::parse(text).unwrap()),
        certainty,
        evidence,
    }
}

/// What an ideal provider returns: keywords cited from the supporting segments, or an honest
/// `no_evidence`.
fn ideal(fixture: &Fixture) -> Vec<Answer> {
    fixture
        .transcript
        .questions
        .iter()
        .map(|q| {
            if q.answerable {
                answer(
                    &q.keywords.join(" "),
                    Certainty::Supported,
                    q.segments
                        .iter()
                        .map(|s| EvidenceRef::Segment(fixture.ids[*s]))
                        .collect(),
                )
            } else {
                answer(
                    "A sessão não trata disso. / The session does not cover this.",
                    Certainty::NoEvidence,
                    Vec::new(),
                )
            }
        })
        .map(|a| fixture.ground(a))
        .collect()
}

#[test]
fn every_fixture_has_answerable_and_unanswerable_questions() {
    for transcript in transcripts() {
        let fixture = Fixture::new(transcript);
        let questions = &fixture.transcript.questions;
        assert!(questions.iter().filter(|q| q.answerable).count() >= 3);
        assert!(questions.iter().filter(|q| !q.answerable).count() >= 3);
        for q in questions {
            if q.answerable {
                assert!(!q.keywords.is_empty() && !q.segments.is_empty());
                // Keywords appear in the segments that support them.
                let support: String = q
                    .segments
                    .iter()
                    .map(|s| fixture.transcript.segments[*s].as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                for k in &q.keywords {
                    assert!(mentions(&support, k), "{}: {k}", q.question);
                }
            } else {
                assert!(!q.forbidden.is_empty(), "{}", q.question);
            }
        }
    }
}

#[test]
fn harness_scores_an_ideal_provider_perfectly() {
    for transcript in transcripts() {
        let fixture = Fixture::new(transcript);
        let metrics = measure(&fixture, &ideal(&fixture));
        assert_eq!(
            metrics,
            Metrics {
                cited_or_declared: 1.0,
                invented: 0,
                accuracy: 1.0
            },
            "{}",
            fixture.transcript.id
        );
    }
}

#[test]
fn grounding_turns_fabricated_citations_into_no_evidence() {
    let fixture = Fixture::new(transcripts().remove(1));
    // A provider that answers everything confidently with made-up ids.
    let answers: Vec<Answer> = fixture
        .transcript
        .questions
        .iter()
        .map(|_| {
            fixture.ground(answer(
                "It was decided.",
                Certainty::Supported,
                vec![EvidenceRef::Segment(SegmentId::new())],
            ))
        })
        .collect();
    let metrics = measure(&fixture, &answers);
    // Still 100 %: every stored answer either cites the session or says it has no evidence.
    assert!((metrics.cited_or_declared - 1.0).abs() < f64::EPSILON);
    assert!(answers.iter().all(|a| a.certainty == Certainty::NoEvidence));
    assert_eq!(metrics.invented, 0);
    assert!(metrics.accuracy < f64::EPSILON);
}

#[test]
fn harness_detects_inventions_and_wrong_citations() {
    let fixture = Fixture::new(transcripts().remove(0));
    let mut answers = ideal(&fixture);
    let unanswerable: Vec<usize> = fixture
        .transcript
        .questions
        .iter()
        .enumerate()
        .filter(|(_, q)| !q.answerable)
        .map(|(i, _)| i)
        .collect();
    // Invents a framework, citing a real but unrelated segment.
    answers[unanswerable[0]] = fixture.ground(answer(
        "O frontend será feito em React.",
        Certainty::Supported,
        vec![EvidenceRef::Segment(fixture.ids[0])],
    ));
    // Says "no evidence" but slips in an invented fact.
    answers[unanswerable[1]] = fixture.ground(answer(
        "Não há evidência, mas devem ser uns 50 mil reais.",
        Certainty::NoEvidence,
        Vec::new(),
    ));
    // Right words, wrong citation.
    answers[0] = fixture.ground(answer(
        "Postgres",
        Certainty::Supported,
        vec![EvidenceRef::Segment(fixture.ids[8])],
    ));
    let metrics = measure(&fixture, &answers);
    assert_eq!(metrics.invented, 2, "{metrics:?}");
    assert!(metrics.accuracy < 1.0, "{metrics:?}");
    assert!((metrics.cited_or_declared - 1.0).abs() < f64::EPSILON);
}

/// Runs the reference provider named by `DEVCOPILOT_EVAL_PROVIDER`; skipped when unset.
#[tokio::test]
async fn reference_provider_meets_sc_011() {
    let Ok(kind) = std::env::var("DEVCOPILOT_EVAL_PROVIDER") else {
        println!("DEVCOPILOT_EVAL_PROVIDER not set; reference Q&A evaluation skipped");
        return;
    };
    assert_eq!(kind, "ollama", "only `ollama` is supported until US7");
    let endpoint = std::env::var("DEVCOPILOT_EVAL_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:11434".into());
    let model = std::env::var("DEVCOPILOT_EVAL_MODEL").unwrap_or_else(|_| "llama3.1".into());
    let provider = Provider::new(ProviderSpec {
        id: ProviderId::new(),
        role: ProviderRole::Analysis,
        adapter: ProviderAdapterKind::Ollama,
        endpoint: Some(endpoint),
        model: model.clone(),
        credential_ref: None,
        data_categories: BTreeSet::from([
            DataCategory::Transcript,
            DataCategory::Records,
            DataCategory::Questions,
        ]),
    })
    .unwrap();
    let adapter = OllamaAnalysisProvider::new(&provider).unwrap();
    for transcript in transcripts() {
        let fixture = Fixture::new(transcript);
        let mut answers = Vec::new();
        for question in &fixture.transcript.questions {
            let output = adapter
                .answer(fixture.request(question), CancellationToken::new())
                .await
                .unwrap();
            answers.push(fixture.ground(output.answer));
        }
        let metrics = measure(&fixture, &answers);
        println!(
            "qa eval {} model={model}: cited_or_declared={:.2} invented={} accuracy={:.2}",
            fixture.transcript.id, metrics.cited_or_declared, metrics.invented, metrics.accuracy
        );
        assert!(
            (metrics.cited_or_declared - 1.0).abs() < f64::EPSILON,
            "{}: {metrics:?}",
            fixture.transcript.id
        );
        assert_eq!(
            metrics.invented, 0,
            "{}: {metrics:?}",
            fixture.transcript.id
        );
    }
}
