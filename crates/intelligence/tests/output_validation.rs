//! Analysis output and answers are accepted only when valid against
//! `analysis-output.schema.json` and `answer-output.schema.json` (T080, T114; FR-029, FR-030,
//! FR-036, FR-053b).

use devcopilot_application::ports::ProviderError;
use devcopilot_domain::ids::{RecordId, SegmentId};
use devcopilot_domain::question::{Certainty, EvidenceRef};
use devcopilot_domain::record::{Nature, RecordKind, RelationKind};
use devcopilot_intelligence::output_validation::{
    MAX_OUTPUT_BYTES, parse_analysis_output, parse_answer_output,
};
use serde_json::{Value, json};

fn proposal() -> Value {
    json!({
        "kind": "requirement",
        "text": "O export precisa suportar CSV",
        "nature": "observed",
        "rationale": "Dito explicitamente",
        "evidenceSegmentIds": [SegmentId::new().to_string()]
    })
}

fn output(proposals: Vec<Value>) -> String {
    json!({ "schemaVersion": 1, "proposals": proposals }).to_string()
}

fn invalid(raw: &str) {
    assert_eq!(
        parse_analysis_output(raw),
        Err(ProviderError::InvalidOutput),
        "{raw}"
    );
}

#[test]
fn valid_output_converts_to_proposals() {
    let segment = SegmentId::new();
    let record = RecordId::new();
    let raw = output(vec![
        json!({
            "kind": "task",
            "text": "Implementar export CSV",
            "nature": "inferred",
            "rationale": "",
            "evidenceSegmentIds": [segment.to_string()],
            "acceptanceCriteria": ["Exporta 10 mil linhas"],
            "relatesTo": { "recordId": record.to_string(), "relation": "refines" }
        }),
        json!({
            "kind": "decision",
            "text": "Usar Postgres",
            "nature": "suggested",
            "rationale": "Equipe conhece",
            "evidenceSegmentIds": [],
            "alternatives": ["SQLite", "MySQL"]
        }),
    ]);
    let proposals = parse_analysis_output(&raw).unwrap();
    assert_eq!(proposals.len(), 2);
    assert_eq!(proposals[0].kind, RecordKind::Task);
    assert_eq!(proposals[0].nature, Nature::Inferred);
    assert_eq!(proposals[0].evidence, vec![segment]);
    assert_eq!(proposals[0].acceptance_criteria.len(), 1);
    assert_eq!(
        proposals[0].relates_to.map(|r| (r.record_id, r.kind)),
        Some((record, RelationKind::Refines))
    );
    assert_eq!(proposals[1].alternatives.len(), 2);
    assert!(proposals[1].rationale.expose().as_str() == "Equipe conhece");
}

#[test]
fn unknown_fields_are_rejected() {
    invalid(&json!({ "schemaVersion": 1, "proposals": [], "actions": ["delete"] }).to_string());
    let mut extra = proposal();
    extra["status"] = json!("confirmed");
    invalid(&output(vec![extra]));
    let mut command = proposal();
    command["command"] = json!("rm -rf /");
    invalid(&output(vec![command]));
}

#[test]
fn observed_without_evidence_is_rejected() {
    let mut p = proposal();
    p["evidenceSegmentIds"] = json!([]);
    invalid(&output(vec![p]));
}

#[test]
fn oversize_output_is_rejected() {
    let mut long = proposal();
    long["text"] = json!("x".repeat(2001));
    invalid(&output(vec![long]));
    invalid(&output((0..31).map(|_| proposal()).collect()));
    let mut many = proposal();
    many["evidenceSegmentIds"] = json!(
        (0..21)
            .map(|_| SegmentId::new().to_string())
            .collect::<Vec<_>>()
    );
    invalid(&output(vec![many]));
    invalid(&" ".repeat(MAX_OUTPUT_BYTES + 1));
}

#[test]
fn wrong_version_or_shape_is_rejected() {
    invalid(&json!({ "schemaVersion": 2, "proposals": [] }).to_string());
    invalid(&json!({ "proposals": [] }).to_string());
    invalid("[]");
    invalid("Claro! Aqui está o JSON: {\"schemaVersion\":1,\"proposals\":[]}");
    invalid("```json\n{\"schemaVersion\":1,\"proposals\":[]}\n```");
    invalid("");
    let mut kind = proposal();
    kind["kind"] = json!("command");
    invalid(&output(vec![kind]));
    let mut nature = proposal();
    nature["nature"] = json!("certain");
    invalid(&output(vec![nature]));
}

#[test]
fn malformed_ids_and_control_characters_are_rejected() {
    let mut id = proposal();
    id["evidenceSegmentIds"] = json!(["segment-1"]);
    invalid(&output(vec![id]));
    let mut control = proposal();
    control["text"] = json!("texto\u{1b}[31m");
    invalid(&output(vec![control]));
}

#[test]
fn evidence_is_referenced_by_id_never_by_copied_text() {
    // A model cannot supply (possibly translated) quotes: evidence is ids of stored segments.
    let mut quoted = proposal();
    quoted["evidenceText"] = json!("The export must support CSV");
    invalid(&output(vec![quoted]));
    let segment = SegmentId::new();
    let mut p = proposal();
    p["evidenceSegmentIds"] = json!([segment.to_string()]);
    let proposals = parse_analysis_output(&output(vec![p])).unwrap();
    assert_eq!(proposals[0].evidence, vec![segment]);
}

// Answers (T114; FR-036, SC-011) -------------------------------------------------------------

/// A valid answer with `fields` replaced (`null` removes a field).
fn answer(fields: Value) -> String {
    let mut base = serde_json::Map::new();
    base.insert("schemaVersion".into(), json!(1));
    base.insert("answer".into(), json!("Foi decidido usar OAuth2."));
    base.insert("certainty".into(), json!("supported"));
    base.insert("evidenceSegmentIds".into(), json!([]));
    base.insert("evidenceRecordIds".into(), json!([]));
    if let Value::Object(fields) = fields {
        for (key, value) in fields {
            if value.is_null() {
                base.remove(&key);
            } else {
                base.insert(key, value);
            }
        }
    }
    Value::Object(base).to_string()
}

#[test]
fn valid_answer_converts_with_segments_before_records() {
    let segment = SegmentId::new();
    let record = RecordId::new();
    let parsed = parse_answer_output(&answer(json!({
        "certainty": "partial",
        "evidenceRecordIds": [record.to_string()],
        "evidenceSegmentIds": [segment.to_string()]
    })))
    .unwrap();
    assert_eq!(parsed.certainty, Certainty::Partial);
    assert_eq!(
        parsed.evidence,
        [EvidenceRef::Segment(segment), EvidenceRef::Record(record)]
    );
    assert_eq!(parsed.text.expose().as_str(), "Foi decidido usar OAuth2.");
}

#[test]
fn answers_outside_the_schema_are_rejected() {
    let bad = [
        answer(json!({ "certainty": "certain" })),
        answer(json!({ "answer": "" })),
        answer(json!({ "answer": "x".repeat(4001) })),
        answer(json!({ "evidenceSegmentIds": ["not-a-uuid"] })),
        answer(json!({ "evidenceRecordIds": null })),
        answer(json!({ "schemaVersion": 2 })),
        answer(json!({ "actions": [{ "type": "set_status", "status": "rejected" }] })),
        answer(json!({
            "evidenceSegmentIds": (0..21).map(|_| SegmentId::new().to_string()).collect::<Vec<_>>()
        })),
        "Claro! Foi decidido usar OAuth2.".to_owned(),
        format!("{} trailing", answer(json!({}))),
    ];
    for raw in bad {
        assert_eq!(
            parse_answer_output(&raw),
            Err(ProviderError::InvalidOutput),
            "{raw}"
        );
    }
}

#[test]
fn analysis_and_answer_documents_are_not_interchangeable() {
    assert_eq!(
        parse_answer_output(&output(vec![proposal()])),
        Err(ProviderError::InvalidOutput)
    );
    assert_eq!(
        parse_analysis_output(&answer(json!({}))),
        Err(ProviderError::InvalidOutput)
    );
}
