//! Validation of analysis output against `contracts/analysis-output.schema.json` and conversion
//! to domain [`Proposal`]s (FR-029, FR-030).
//!
//! Model output is untrusted. It is accepted only as one JSON document, no larger than
//! [`MAX_OUTPUT_BYTES`], valid against the embedded schema (unknown fields rejected, `observed`
//! requires evidence, bounded sizes, `schemaVersion = 1`) and convertible to domain values. Any
//! deviation discards the whole output as [`ProviderError::InvalidOutput`]. Evidence is
//! referenced by segment id only, so quoted evidence always comes from the stored original
//! segment and is never a model-written (or translated) copy (FR-053b).

use std::sync::OnceLock;

use devcopilot_application::ports::ProviderError;
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{RecordId, SegmentId};
use devcopilot_domain::record::{Nature, Proposal, RecordKind, Relation, RelationKind};
use devcopilot_domain::text::{ListItem, Rationale, RecordText};
use jsonschema::Validator;
use serde::Deserialize;
use serde_json::Value;

/// The analysis output schema, embedded from the contracts.
pub const SCHEMA_JSON: &str =
    include_str!("../../../specs/001-meeting-sessions/contracts/analysis-output.schema.json");

/// Largest output accepted, in bytes (30 proposals of maximal size fit comfortably).
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;

/// The schema as JSON (also sent to providers that support constrained output).
///
/// # Errors
///
/// [`ProviderError::Other`] if the embedded schema is not valid JSON (a build defect).
pub fn schema() -> Result<&'static Value, ProviderError> {
    static SCHEMA: OnceLock<Option<Value>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| serde_json::from_str(SCHEMA_JSON).ok())
        .as_ref()
        .ok_or_else(|| ProviderError::Other("analysis schema unavailable".into()))
}

fn validator() -> Result<&'static Validator, ProviderError> {
    static VALIDATOR: OnceLock<Option<Validator>> = OnceLock::new();
    VALIDATOR
        .get_or_init(|| {
            let schema = schema().ok()?;
            jsonschema::options()
                .should_validate_formats(true)
                .build(schema)
                .ok()
        })
        .as_ref()
        .ok_or_else(|| ProviderError::Other("analysis schema unavailable".into()))
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
    if raw.len() > MAX_OUTPUT_BYTES {
        return Err(ProviderError::InvalidOutput);
    }
    let value: Value =
        serde_json::from_str(raw.trim()).map_err(|_| ProviderError::InvalidOutput)?;
    if !validator()?.is_valid(&value) {
        return Err(ProviderError::InvalidOutput);
    }
    let output: WireOutput =
        serde_json::from_value(value).map_err(|_| ProviderError::InvalidOutput)?;
    output
        .proposals
        .into_iter()
        .map(WireProposal::into_proposal)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_schema_compiles() {
        assert!(validator().is_ok());
    }

    #[test]
    fn empty_output_is_valid() {
        assert_eq!(
            parse_analysis_output(r#"{"schemaVersion":1,"proposals":[]}"#),
            Ok(Vec::new())
        );
    }
}
