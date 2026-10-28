//! Validation of model output against `contracts/analysis-output.schema.json` and
//! `contracts/answer-output.schema.json`, and conversion to domain [`Proposal`]s and
//! [`Answer`]s (FR-029, FR-030, FR-036).
//!
//! Model output is untrusted. It is accepted only as one JSON document, no larger than
//! [`MAX_OUTPUT_BYTES`], valid against the embedded schema (unknown fields rejected, `observed`
//! requires evidence, bounded sizes, `schemaVersion = 1`) and convertible to domain values. Any
//! deviation discards the whole output as [`ProviderError::InvalidOutput`]. Evidence is
//! referenced by id only, so quoted evidence always comes from the stored original segment or
//! record and is never a model-written (or translated) copy (FR-053b). Whether cited ids belong
//! to the session is checked by the application, not here.

use std::sync::OnceLock;

use devcopilot_application::ports::ProviderError;
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{RecordId, SegmentId};
use devcopilot_domain::question::{Answer, Certainty, EvidenceRef};
use devcopilot_domain::record::{Nature, Proposal, RecordKind, Relation, RelationKind};
use devcopilot_domain::text::{AnswerText, ListItem, Rationale, RecordText};
use jsonschema::Validator;
use serde::Deserialize;
use serde_json::Value;

/// The analysis output schema, embedded from the contracts.
pub const SCHEMA_JSON: &str =
    include_str!("../../../specs/001-meeting-sessions/contracts/analysis-output.schema.json");

/// The answer output schema, embedded from the contracts.
pub const ANSWER_SCHEMA_JSON: &str =
    include_str!("../../../specs/001-meeting-sessions/contracts/answer-output.schema.json");

/// Largest output accepted, in bytes (30 proposals of maximal size fit comfortably).
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;

/// A schema parsed once and its compiled validator.
struct Compiled {
    schema: Value,
    validator: Validator,
}

fn compile(raw: &str) -> Option<Compiled> {
    let schema: Value = serde_json::from_str(raw).ok()?;
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)
        .ok()?;
    Some(Compiled { schema, validator })
}

fn analysis_compiled() -> Result<&'static Compiled, ProviderError> {
    static COMPILED: OnceLock<Option<Compiled>> = OnceLock::new();
    COMPILED
        .get_or_init(|| compile(SCHEMA_JSON))
        .as_ref()
        .ok_or_else(|| ProviderError::Other("analysis schema unavailable".into()))
}

fn answer_compiled() -> Result<&'static Compiled, ProviderError> {
    static COMPILED: OnceLock<Option<Compiled>> = OnceLock::new();
    COMPILED
        .get_or_init(|| compile(ANSWER_SCHEMA_JSON))
        .as_ref()
        .ok_or_else(|| ProviderError::Other("answer schema unavailable".into()))
}

/// The analysis schema as JSON (also sent to providers that support constrained output).
///
/// # Errors
///
/// [`ProviderError::Other`] if the embedded schema is not valid (a build defect).
pub fn schema() -> Result<&'static Value, ProviderError> {
    analysis_compiled().map(|compiled| &compiled.schema)
}

/// The answer schema as JSON (also sent to providers that support constrained output).
///
/// # Errors
///
/// [`ProviderError::Other`] if the embedded schema is not valid (a build defect).
pub fn answer_schema() -> Result<&'static Value, ProviderError> {
    answer_compiled().map(|compiled| &compiled.schema)
}

/// Parses `raw` as one JSON document valid against `compiled` and deserializes it.
fn validated<T: serde::de::DeserializeOwned>(
    raw: &str,
    compiled: &Compiled,
) -> Result<T, ProviderError> {
    if raw.len() > MAX_OUTPUT_BYTES {
        return Err(ProviderError::InvalidOutput);
    }
    let value: Value =
        serde_json::from_str(raw.trim()).map_err(|_| ProviderError::InvalidOutput)?;
    if !compiled.validator.is_valid(&value) {
        return Err(ProviderError::InvalidOutput);
    }
    serde_json::from_value(value).map_err(|_| ProviderError::InvalidOutput)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireOutput {
    #[allow(dead_code)]
    schema_version: u8,
    proposals: Vec<WireProposal>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireProposal {
    kind: String,
    text: String,
    nature: String,
    rationale: String,
    evidence_segment_ids: Vec<String>,
    #[serde(default)]
    acceptance_criteria: Vec<String>,
    #[serde(default)]
    alternatives: Vec<String>,
    relates_to: Option<WireRelation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireAnswer {
    #[allow(dead_code)]
    schema_version: u8,
    answer: String,
    certainty: String,
    evidence_segment_ids: Vec<String>,
    evidence_record_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireRelation {
    record_id: String,
    relation: String,
}

fn items(values: Vec<String>) -> Result<Vec<Redacted<ListItem>>, ProviderError> {
    values
        .into_iter()
        .map(|value| {
            ListItem::parse(value)
                .map(Redacted::new)
                .map_err(|_| ProviderError::InvalidOutput)
        })
        .collect()
}

impl WireProposal {
    fn into_proposal(self) -> Result<Proposal, ProviderError> {
        let invalid = |_| ProviderError::InvalidOutput;
        Ok(Proposal {
            kind: RecordKind::parse(&self.kind).map_err(invalid)?,
            text: Redacted::new(RecordText::parse(self.text).map_err(invalid)?),
            nature: Nature::parse(&self.nature).map_err(invalid)?,
            rationale: Redacted::new(Rationale::parse(self.rationale).map_err(invalid)?),
            evidence: self
                .evidence_segment_ids
                .iter()
                .map(|id| SegmentId::parse(id).map_err(invalid))
                .collect::<Result<_, _>>()?,
            acceptance_criteria: items(self.acceptance_criteria)?,
            alternatives: items(self.alternatives)?,
            relates_to: self
                .relates_to
                .map(|relation| {
                    Ok::<_, ProviderError>(Relation {
                        record_id: RecordId::parse(&relation.record_id).map_err(invalid)?,
                        kind: RelationKind::parse(&relation.relation).map_err(invalid)?,
                    })
                })
                .transpose()?,
        })
    }
}

/// Validates raw model output and converts it to proposals.
///
/// # Errors
///
/// [`ProviderError::InvalidOutput`] for anything but a valid, schema-conforming document.
pub fn parse_analysis_output(raw: &str) -> Result<Vec<Proposal>, ProviderError> {
    let output: WireOutput = validated(raw, analysis_compiled()?)?;
    output
        .proposals
        .into_iter()
        .map(WireProposal::into_proposal)
        .collect()
}

/// Validates raw model output and converts it to an answer. Segment ids are cited before
/// record ids; whether they belong to the session is checked later
/// ([`Answer::grounded`]).
///
/// # Errors
///
/// [`ProviderError::InvalidOutput`] for anything but a valid, schema-conforming document.
pub fn parse_answer_output(raw: &str) -> Result<Answer, ProviderError> {
    let output: WireAnswer = validated(raw, answer_compiled()?)?;
    let invalid = |_| ProviderError::InvalidOutput;
    let segments = output.evidence_segment_ids.iter().map(|id| {
        SegmentId::parse(id)
            .map(EvidenceRef::Segment)
            .map_err(invalid)
    });
    let records = output.evidence_record_ids.iter().map(|id| {
        RecordId::parse(id)
            .map(EvidenceRef::Record)
            .map_err(invalid)
    });
    Ok(Answer {
        text: Redacted::new(AnswerText::parse(output.answer).map_err(invalid)?),
        certainty: Certainty::parse(&output.certainty).map_err(invalid)?,
        evidence: segments.chain(records).collect::<Result<_, _>>()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_schemas_compile() {
        assert!(schema().is_ok());
        assert!(answer_schema().is_ok());
    }

    #[test]
    fn minimal_answer_is_valid() {
        let answer = parse_answer_output(
            r#"{"schemaVersion":1,"answer":"Não há evidência.","certainty":"no_evidence","evidenceSegmentIds":[],"evidenceRecordIds":[]}"#,
        )
        .unwrap();
        assert_eq!(answer.certainty, Certainty::NoEvidence);
        assert!(answer.evidence.is_empty());
    }

    #[test]
    fn empty_output_is_valid() {
        assert_eq!(
            parse_analysis_output(r#"{"schemaVersion":1,"proposals":[]}"#),
            Ok(Vec::new())
        );
    }
}
