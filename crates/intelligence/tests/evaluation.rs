//! Analysis evaluation harness (T083; SC-008, Art. VI.6, research R18).
//!
//! Annotated synthetic transcripts in `fixtures/eval/` list the items a good analysis finds, with
//! the segments that support each one. The harness measures:
//!
//! - **recall** of explicit requirements and decisions (SC-008: ≥ 80 %);
//! - **grounding**: share of `observed` proposals that match an annotated item and cite at least
//!   one of its supporting segments (SC-008: 100 %);
//! - **invented rate**: share of `observed` proposals that match no annotated item.
//!
//! By default it runs against scripted providers as a self-test of the metrics. With
//! `DEVCOPILOT_EVAL_PROVIDER=ollama` it calls a local Ollama server
//! (`DEVCOPILOT_EVAL_ENDPOINT`, default `http://127.0.0.1:11434`; `DEVCOPILOT_EVAL_MODEL`,
//! default `llama3.1`) and prints the metrics to record in `docs/adr/ADR-003.md`. Remote
//! reference providers join with US7. Only synthetic, non-confidential transcripts are used.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::collections::BTreeSet;

use devcopilot_application::ports::{AnalysisProvider, AnalysisRequest, AnalysisSegment};
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{ProviderId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{
    DataCategory, Provider, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
use devcopilot_domain::record::{Nature, Proposal, RecordKind};
use devcopilot_domain::text::{Rationale, RecordText, SegmentText};
use devcopilot_domain::time::Duration;
use devcopilot_intelligence::OllamaAnalysisProvider;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
struct Expected {
    kind: String,
    keywords: Vec<String>,
    segments: Vec<usize>,
}

#[derive(Deserialize)]
struct Transcript {
    id: String,
    language: String,
    segments: Vec<String>,
    expected: Vec<Expected>,
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
    request: AnalysisRequest,
}

fn fixture(transcript: Transcript) -> Fixture {
    let ids: Vec<SegmentId> = transcript
        .segments
        .iter()
        .map(|_| SegmentId::new())
        .collect();
    let request = AnalysisRequest {
        session_id: SessionId::new(),
        language: SessionLanguage::parse(&transcript.language).unwrap(),
        segments: transcript
            .segments
            .iter()
            .zip(&ids)
            .enumerate()
            .map(|(i, (text, id))| AnalysisSegment {
                id: *id,
                source: None,
                start: Duration::from_secs(i as u64 * 4),
                text: Redacted::new(SegmentText::parse(text.as_str()).unwrap()),
            })
            .collect(),
        records: Vec::new(),
    };
    Fixture {
        transcript,
        ids,
        request,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Metrics {
    recall: f64,
    grounding: f64,
    invented_rate: f64,
}

fn matches(expected: &Expected, proposal: &Proposal) -> bool {
    let text = proposal.text.expose().as_str().to_lowercase();
    RecordKind::parse(&expected.kind) == Ok(proposal.kind)
        && expected
            .keywords
            .iter()
            .all(|k| text.contains(&k.to_lowercase()))
}

fn measure(fixture: &Fixture, proposals: &[Proposal]) -> Metrics {
    let mut used = BTreeSet::new();
    // Annotated item index matched by each proposal (each item matched at most once).
    let matched: Vec<Option<usize>> = proposals
        .iter()
        .map(|proposal| {
            let found = fixture
                .transcript
                .expected
                .iter()
                .enumerate()
                .find(|(i, expected)| !used.contains(i) && matches(expected, proposal))
                .map(|(i, _)| i);
            if let Some(i) = found {
                used.insert(i);
            }
            found
        })
        .collect();
    let explicit: Vec<usize> = fixture
        .transcript
        .expected
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "requirement" || e.kind == "decision")
        .map(|(i, _)| i)
        .collect();
    let found_explicit = explicit.iter().filter(|i| used.contains(i)).count();
    let observed: Vec<(&Proposal, Option<usize>)> = proposals
        .iter()
        .zip(matched)
        .filter(|(p, _)| p.nature == Nature::Observed)
        .collect();
    let grounded = observed
        .iter()
        .filter(|(proposal, item)| {
            item.is_some_and(|i| {
                fixture.transcript.expected[i]
                    .segments
                    .iter()
                    .any(|s| proposal.evidence.contains(&fixture.ids[*s]))
            })
        })
        .count();
    let invented = observed.iter().filter(|(_, item)| item.is_none()).count();
    let ratio = |part: usize, whole: usize| {
        if whole == 0 {
            1.0
        } else {
            part as f64 / whole as f64
        }
    };
    Metrics {
        recall: ratio(found_explicit, explicit.len()),
        grounding: ratio(grounded, observed.len()),
        invented_rate: if observed.is_empty() {
            0.0
        } else {
            invented as f64 / observed.len() as f64
        },
    }
}

fn proposal(kind: &str, text: &str, nature: Nature, evidence: Vec<SegmentId>) -> Proposal {
    Proposal {
        kind: RecordKind::parse(kind).unwrap(),
        text: Redacted::new(RecordText::parse(text).unwrap()),
        nature,
        rationale: Redacted::new(Rationale::default()),
        evidence,
        acceptance_criteria: Vec::new(),
        alternatives: Vec::new(),
        relates_to: None,
    }
}

/// What an ideal provider returns: every annotated item, observed, citing its segments.
fn ideal(fixture: &Fixture) -> Vec<Proposal> {
    fixture
        .transcript
        .expected
        .iter()
        .map(|e| {
            proposal(
                &e.kind,
                &e.keywords.join(" "),
                Nature::Observed,
                e.segments.iter().map(|s| fixture.ids[*s]).collect(),
            )
        })
        .collect()
}

#[test]
fn harness_scores_an_ideal_provider_perfectly() {
    for transcript in transcripts() {
        let fixture = fixture(transcript);
        let metrics = measure(&fixture, &ideal(&fixture));
        assert_eq!(
            metrics,
            Metrics {
                recall: 1.0,
                grounding: 1.0,
                invented_rate: 0.0
            },
            "{}",
            fixture.transcript.id
        );
    }
}

#[test]
fn harness_detects_misses_bad_citations_and_inventions() {
    let fixture = fixture(transcripts().remove(0));
    let mut proposals = ideal(&fixture);
    // Drop one explicit requirement.
    let first_requirement = proposals
        .iter()
        .position(|p| p.kind == RecordKind::Requirement)
        .unwrap();
    proposals.remove(first_requirement);
    // Cite the wrong segment for one item.
    proposals[0].evidence = vec![fixture.ids[0]];
    // Invent an "observed" fact.
    proposals.push(proposal(
        "decision",
        "Migrar tudo para Kubernetes",
        Nature::Observed,
        vec![fixture.ids[8]],
    ));
    // Inferred and suggested items never count as inventions.
    proposals.push(proposal(
        "risk",
        "Prazo apertado",
        Nature::Suggested,
        Vec::new(),
    ));
    let metrics = measure(&fixture, &proposals);
    assert!(metrics.recall < 0.8, "{metrics:?}");
    assert!(metrics.grounding < 1.0, "{metrics:?}");
    assert!(metrics.invented_rate > 0.0, "{metrics:?}");
}

/// Runs the reference provider named by `DEVCOPILOT_EVAL_PROVIDER`; skipped when unset.
#[tokio::test]
async fn reference_provider_meets_sc_008() {
    let Ok(kind) = std::env::var("DEVCOPILOT_EVAL_PROVIDER") else {
        println!("DEVCOPILOT_EVAL_PROVIDER not set; reference evaluation skipped");
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
        data_categories: BTreeSet::from([DataCategory::Transcript, DataCategory::Records]),
    })
    .unwrap();
    let adapter = OllamaAnalysisProvider::new(&provider).unwrap();
    for transcript in transcripts() {
        let fixture = fixture(transcript);
        let output = adapter
            .analyze(fixture.request.clone(), CancellationToken::new())
            .await
            .unwrap();
        let proposals: Vec<Proposal> = output
            .proposals
            .into_iter()
            .map(|p| p.grounded(|id| fixture.ids.contains(&id)))
            .collect();
        let metrics = measure(&fixture, &proposals);
        println!(
            "eval {} model={model}: recall={:.2} grounding={:.2} invented={:.2} tokens={}",
            fixture.transcript.id,
            metrics.recall,
            metrics.grounding,
            metrics.invented_rate,
            output.tokens_used
        );
        assert!(
            metrics.recall >= 0.8,
            "{}: {metrics:?}",
            fixture.transcript.id
        );
        assert!(
            (metrics.grounding - 1.0).abs() < f64::EPSILON,
            "{}: {metrics:?}",
            fixture.transcript.id
        );
    }
}
