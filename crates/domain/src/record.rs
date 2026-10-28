//! Technical records proposed by analysis or created by the user (data-model.md
//! "TechnicalRecord", FR-021–FR-027).
//!
//! Human control is enforced here: AI records are born `proposed`, `observed` records always
//! cite evidence, and only the user changes an existing record. AI output reaches the domain
//! as a [`Proposal`], which can only become a *new* record.

use std::collections::BTreeSet;

use crate::Redacted;
use crate::error::{DomainError, InputProblem};
use crate::ids::{AnalysisRunId, ChangeId, ProviderId, RecordId, SegmentId, SessionId};
use crate::record_change::{ChangeActor, ChangeField, RecordChange, RecordChangeSpec};
use crate::text::{ListItem, Rationale, RecordText};
use crate::time::Timestamp;

/// Most evidence references, acceptance criteria or alternatives on one record.
pub const MAX_EVIDENCE: usize = 20;
/// Most acceptance criteria or alternatives on one record.
pub const MAX_LIST_ITEMS: usize = 10;

/// What a record describes. `Assumption` covers Art. VI.4 (premises in the plan).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecordKind {
    /// Requirement.
    Requirement,
    /// Technical decision.
    Decision,
    /// Risk.
    Risk,
    /// Open question.
    OpenQuestion,
    /// Technical task.
    Task,
    /// Assumption.
    Assumption,
}

code_enum!(RecordKind, "kind", {
    Requirement => "requirement",
    Decision => "decision",
    Risk => "risk",
    OpenQuestion => "open_question",
    Task => "task",
    Assumption => "assumption",
});

/// How the record relates to the conversation (FR-022, FR-023).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Nature {
    /// Stated in the conversation; cites the segments that say it.
    Observed,
    /// Deduced from the conversation.
    Inferred,
    /// Suggested by the assistant.
    Suggested,
}

code_enum!(Nature, "nature", {
    Observed => "observed",
    Inferred => "inferred",
    Suggested => "suggested",
});

/// Review status (FR-024).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecordStatus {
    /// Awaiting review.
    Proposed,
    /// Accepted by the user.
    Confirmed,
    /// Discarded by the user.
    Rejected,
    /// Kept for later.
    Pending,
}

code_enum!(RecordStatus, "status", {
    Proposed => "proposed",
    Confirmed => "confirmed",
    Rejected => "rejected",
    Pending => "pending",
});

impl RecordStatus {
    /// Whether the user may move a record from `self` to `to` (data-model.md "Transições de
    /// status"). Nothing ever returns to `proposed`.
    #[must_use]
    pub const fn can_become(self, to: Self) -> bool {
        matches!(
            (self, to),
            (
                Self::Proposed,
                Self::Confirmed | Self::Rejected | Self::Pending
            ) | (Self::Pending, Self::Confirmed | Self::Rejected)
                | (Self::Confirmed, Self::Pending | Self::Rejected)
                | (Self::Rejected, Self::Pending | Self::Confirmed)
        )
    }
}

/// Who acts on a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actor {
    /// The user, through an explicit action.
    User,
    /// Analysis output.
    Ai,
}

/// Where a record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Origin {
    /// Proposed by an analysis run.
    Ai {
        /// Provider that produced it.
        provider_id: ProviderId,
        /// Run that produced it.
        analysis_run_id: AnalysisRunId,
    },
    /// Created by the user (FR-032).
    User,
}

impl Origin {
    /// Stable code (`ai` or `user`).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Ai { .. } => "ai",
            Self::User => "user",
        }
    }
}

/// Markers set when a proposal duplicates or contradicts an existing record (FR-025, FR-027).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RecordFlags {
    /// Record this one contradicts.
    pub conflicts_with: Option<RecordId>,
    /// Record this one repeats.
    pub duplicate_of: Option<RecordId>,
}

/// How a proposal says it relates to an existing record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationKind {
    /// Same content.
    Duplicate,
    /// Contradicts it.
    Conflict,
    /// Adds detail to it.
    Refines,
}

code_enum!(RelationKind, "relation", {
    Duplicate => "duplicate",
    Conflict => "conflict",
    Refines => "refines",
});

/// Relation of a proposal to an existing record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Relation {
    /// Existing record.
    pub record_id: RecordId,
    /// How.
    pub kind: RelationKind,
}

/// A record suggested by analysis output, already schema-validated. It can only ever become
/// a new `proposed` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// Kind.
    pub kind: RecordKind,
    /// Text, in the session language (FR-053b).
    pub text: Redacted<RecordText>,
    /// Nature claimed by the model.
    pub nature: Nature,
    /// Justification.
    pub rationale: Redacted<Rationale>,
    /// Segments cited as evidence.
    pub evidence: Vec<SegmentId>,
    /// Acceptance criteria (tasks only).
    pub acceptance_criteria: Vec<Redacted<ListItem>>,
    /// Alternatives (decisions only).
    pub alternatives: Vec<Redacted<ListItem>>,
    /// Claimed relation to an existing record.
    pub relates_to: Option<Relation>,
}

impl Proposal {
    /// Keeps only evidence for which `is_session_final` holds; an `observed` proposal left
    /// without evidence becomes `inferred` (FR-023, provider-ports.md rule 4).
    #[must_use]
    pub fn grounded(mut self, is_session_final: impl Fn(SegmentId) -> bool) -> Self {
        let mut seen = BTreeSet::new();
        self.evidence
            .retain(|id| is_session_final(*id) && seen.insert(*id));
        self.evidence.truncate(MAX_EVIDENCE);
        if self.nature == Nature::Observed && self.evidence.is_empty() {
            self.nature = Nature::Inferred;
        }
        self
    }

    /// Key used to detect duplicates of the same content (FR-027).
    #[must_use]
    pub fn dedup_key(&self) -> String {
        dedup_key(self.kind, self.text.expose().as_str())
    }
}

/// Lowercase alphanumeric words of `text`, prefixed by the kind.
#[must_use]
pub fn dedup_key(kind: RecordKind, text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    format!("{}:{}", kind.code(), words.join(" "))
}

/// A record written by the user (FR-032).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRecord {
    /// Kind.
    pub kind: RecordKind,
    /// Text.
    pub text: Redacted<RecordText>,
    /// Justification.
    pub rationale: Redacted<Rationale>,
    /// Initial status (the user may create records already confirmed).
    pub status: RecordStatus,
}

/// A user edit; `None` keeps the current value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecordEdit {
    /// New text.
    pub text: Option<Redacted<RecordText>>,
    /// New rationale.
    pub rationale: Option<Redacted<Rationale>>,
    /// New acceptance criteria (tasks only).
    pub acceptance_criteria: Option<Vec<Redacted<ListItem>>>,
    /// New alternatives (decisions only).
    pub alternatives: Option<Vec<Redacted<ListItem>>>,
}

impl RecordEdit {
    /// Whether the edit names no field.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.text.is_none()
            && self.rationale.is_none()
            && self.acceptance_criteria.is_none()
            && self.alternatives.is_none()
    }
}

/// Fields of a record before validation, also used to restore stored rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordSpec {
    /// Identifier.
    pub id: RecordId,
    /// Owning session.
    pub session_id: SessionId,
    /// Kind.
    pub kind: RecordKind,
    /// Text.
    pub text: Redacted<RecordText>,
    /// Nature.
    pub nature: Nature,
    /// Status.
    pub status: RecordStatus,
    /// Justification.
    pub rationale: Redacted<Rationale>,
    /// Origin.
    pub origin: Origin,
    /// Segments cited as evidence.
    pub evidence: Vec<SegmentId>,
    /// Acceptance criteria (tasks only).
    pub acceptance_criteria: Vec<Redacted<ListItem>>,
    /// Alternatives (decisions only).
    pub alternatives: Vec<Redacted<ListItem>>,
    /// Duplicate and conflict markers.
    pub flags: RecordFlags,
    /// Creation instant.
    pub created_at: Timestamp,
    /// Last change.
    pub updated_at: Timestamp,
}

/// A technical record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechnicalRecord {
    spec: RecordSpec,
}

impl TechnicalRecord {
    /// Validates a new record.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvalidInput`] when an `observed` record has no evidence (`evidence`),
    /// lists exceed their bounds or belong to another kind (`acceptance_criteria`,
    /// `alternatives`), or a flag points at the record itself (`flags`);
    /// [`DomainError::InvariantViolation`] when an AI record is not `proposed` (FR-024).
    pub fn new(spec: RecordSpec) -> Result<Self, DomainError> {
        if matches!(spec.origin, Origin::Ai { .. }) && spec.status != RecordStatus::Proposed {
            return Err(DomainError::InvariantViolation(
                "AI records start as proposed",
            ));
        }
        if spec.nature == Nature::Observed && spec.evidence.is_empty() {
            return Err(DomainError::invalid("evidence", InputProblem::Empty));
        }
        if spec.evidence.len() > MAX_EVIDENCE {
            return Err(DomainError::invalid("evidence", InputProblem::TooLong));
        }
        check_list(
            "acceptance_criteria",
            &spec.acceptance_criteria,
            spec.kind == RecordKind::Task,
        )?;
        check_list(
            "alternatives",
            &spec.alternatives,
            spec.kind == RecordKind::Decision,
        )?;
        if [spec.flags.conflicts_with, spec.flags.duplicate_of].contains(&Some(spec.id)) {
            return Err(DomainError::invalid("flags", InputProblem::Inconsistent));
        }
        Ok(Self::restore(spec))
    }

    /// Turns a grounded proposal into a new `proposed` record from analysis run `run`.
    ///
    /// # Errors
    ///
    /// As [`Self::new`] (e.g. an `observed` proposal that was not grounded).
    pub fn propose(
        id: RecordId,
        session_id: SessionId,
        proposal: Proposal,
        origin: Origin,
        flags: RecordFlags,
        now: Timestamp,
    ) -> Result<Self, DomainError> {
        Self::new(RecordSpec {
            id,
            session_id,
            kind: proposal.kind,
            text: proposal.text,
            nature: proposal.nature,
            status: RecordStatus::Proposed,
            rationale: proposal.rationale,
            origin,
            evidence: proposal.evidence,
            acceptance_criteria: proposal.acceptance_criteria,
            alternatives: proposal.alternatives,
            flags,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a stored record without validation.
    #[must_use]
    pub const fn restore(spec: RecordSpec) -> Self {
        Self { spec }
    }

    /// The history entry for this record's creation.
    #[must_use]
    pub fn creation(&self) -> RecordChange {
        RecordChange::restore(RecordChangeSpec {
            id: ChangeId::new(),
            record_id: self.spec.id,
            actor: match self.spec.origin {
                Origin::Ai {
                    analysis_run_id, ..
                } => ChangeActor::Ai { analysis_run_id },
                Origin::User => ChangeActor::User,
            },
            field: ChangeField::Created,
            old_value: None,
            new_value: Some(Redacted::new(self.spec.status.code().to_owned())),
            at: self.spec.created_at,
        })
    }

    /// A record created by the user (FR-032): any status, no evidence, nature `suggested`.
    /// The origin `user` is what identifies it as the user's.
    ///
    /// # Errors
    ///
    /// As [`Self::new`].
    pub fn create_by_user(
        id: RecordId,
        session_id: SessionId,
        draft: UserRecord,
        now: Timestamp,
    ) -> Result<Self, DomainError> {
        Self::new(RecordSpec {
            id,
            session_id,
            kind: draft.kind,
            text: draft.text,
            nature: Nature::Suggested,
            status: draft.status,
            rationale: draft.rationale,
            origin: Origin::User,
            evidence: Vec::new(),
            acceptance_criteria: Vec::new(),
            alternatives: Vec::new(),
            flags: RecordFlags::default(),
            created_at: now,
            updated_at: now,
        })
    }

    /// Applies a user edit and returns one history entry per field that actually changed
    /// (FR-026, FR-032); an edit that changes nothing returns no entries.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvariantViolation`] when `actor` is the AI (FR-025);
    /// [`DomainError::InvalidInput`] when lists exceed their bounds or belong to another kind.
    pub fn edit(
        &mut self,
        actor: Actor,
        edit: RecordEdit,
        now: Timestamp,
    ) -> Result<Vec<RecordChange>, DomainError> {
        if actor == Actor::Ai {
            return Err(DomainError::InvariantViolation(
                "the AI cannot change existing records",
            ));
        }
        if let Some(items) = &edit.acceptance_criteria {
            check_list(
                "acceptance_criteria",
                items,
                self.spec.kind == RecordKind::Task,
            )?;
        }
        if let Some(items) = &edit.alternatives {
            check_list(
                "alternatives",
                items,
                self.spec.kind == RecordKind::Decision,
            )?;
        }
        let mut changes = Vec::new();
        let mut change = |field: ChangeField, old: String, new: String| {
            if old != new {
                changes.push(RecordChange::restore(RecordChangeSpec {
                    id: ChangeId::new(),
                    record_id: self.spec.id,
                    actor: ChangeActor::User,
                    field,
                    old_value: Some(Redacted::new(old)),
                    new_value: Some(Redacted::new(new)),
                    at: now,
                }));
            }
        };
        if let Some(text) = edit.text {
            change(
                ChangeField::Text,
                self.spec.text.expose().as_str().to_owned(),
                text.expose().as_str().to_owned(),
            );
            self.spec.text = text;
        }
        if let Some(rationale) = edit.rationale {
            change(
                ChangeField::Rationale,
                self.spec.rationale.expose().as_str().to_owned(),
                rationale.expose().as_str().to_owned(),
            );
            self.spec.rationale = rationale;
        }
        if let Some(items) = edit.acceptance_criteria {
            change(
                ChangeField::AcceptanceCriteria,
                joined(&self.spec.acceptance_criteria),
                joined(&items),
            );
            self.spec.acceptance_criteria = items;
        }
        if let Some(items) = edit.alternatives {
            change(
                ChangeField::Alternatives,
                joined(&self.spec.alternatives),
                joined(&items),
            );
            self.spec.alternatives = items;
        }
        if !changes.is_empty() {
            self.spec.updated_at = now.max(self.spec.updated_at);
        }
        Ok(changes)
    }

    /// Changes the status by `actor` and returns the history entry (FR-024–FR-026).
    ///
    /// # Errors
    ///
    /// [`DomainError::InvariantViolation`] when `actor` is the AI: analysis output never
    /// changes an existing record (FR-025, FR-029); [`DomainError::InvalidTransition`] for
    /// transitions outside the table.
    pub fn set_status(
        &mut self,
        actor: Actor,
        to: RecordStatus,
        now: Timestamp,
    ) -> Result<RecordChange, DomainError> {
        if actor == Actor::Ai {
            return Err(DomainError::InvariantViolation(
                "the AI cannot change existing records",
            ));
        }
        let from = self.spec.status;
        if !from.can_become(to) {
            return Err(DomainError::InvalidTransition {
                from: from.code(),
                action: to.code(),
            });
        }
        self.spec.status = to;
        self.spec.updated_at = now.max(self.spec.updated_at);
        RecordChange::new(RecordChangeSpec {
            id: ChangeId::new(),
            record_id: self.spec.id,
            actor: ChangeActor::User,
            field: ChangeField::Status,
            old_value: Some(Redacted::new(from.code().to_owned())),
            new_value: Some(Redacted::new(to.code().to_owned())),
            at: now,
        })
    }

    /// Key used to detect duplicates of the same content (FR-027).
    #[must_use]
    pub fn dedup_key(&self) -> String {
        dedup_key(self.spec.kind, self.spec.text.expose().as_str())
    }

    /// All fields.
    #[must_use]
    pub const fn spec(&self) -> &RecordSpec {
        &self.spec
    }

    /// Identifier.
    #[must_use]
    pub const fn id(&self) -> RecordId {
        self.spec.id
    }

    /// Owning session.
    #[must_use]
    pub const fn session_id(&self) -> SessionId {
        self.spec.session_id
    }

    /// Kind.
    #[must_use]
    pub const fn kind(&self) -> RecordKind {
        self.spec.kind
    }

    /// Text.
    #[must_use]
    pub const fn text(&self) -> &Redacted<RecordText> {
        &self.spec.text
    }

    /// Nature.
    #[must_use]
    pub const fn nature(&self) -> Nature {
        self.spec.nature
    }

    /// Status.
    #[must_use]
    pub const fn status(&self) -> RecordStatus {
        self.spec.status
    }

    /// Justification.
    #[must_use]
    pub const fn rationale(&self) -> &Redacted<Rationale> {
        &self.spec.rationale
    }

    /// Origin.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.spec.origin
    }

    /// Segments cited as evidence.
    #[must_use]
    pub fn evidence(&self) -> &[SegmentId] {
        &self.spec.evidence
    }

    /// Acceptance criteria.
    #[must_use]
    pub fn acceptance_criteria(&self) -> &[Redacted<ListItem>] {
        &self.spec.acceptance_criteria
    }

    /// Alternatives.
    #[must_use]
    pub fn alternatives(&self) -> &[Redacted<ListItem>] {
        &self.spec.alternatives
    }

    /// Duplicate and conflict markers.
    #[must_use]
    pub const fn flags(&self) -> RecordFlags {
        self.spec.flags
    }

    /// Creation instant.
    #[must_use]
    pub const fn created_at(&self) -> Timestamp {
        self.spec.created_at
    }

    /// Last change.
    #[must_use]
    pub const fn updated_at(&self) -> Timestamp {
        self.spec.updated_at
    }
}

/// List items one per line, as stored in the history.
fn joined(items: &[Redacted<ListItem>]) -> String {
    items
        .iter()
        .map(|item| item.expose().as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn check_list(
    field: &'static str,
    items: &[Redacted<ListItem>],
    allowed: bool,
) -> Result<(), DomainError> {
    if items.is_empty() {
        return Ok(());
    }
    if !allowed {
        return Err(DomainError::invalid(field, InputProblem::Inconsistent));
    }
    if items.len() > MAX_LIST_ITEMS {
        return Err(DomainError::invalid(field, InputProblem::TooLong));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_STATUSES: [RecordStatus; 4] = [
        RecordStatus::Proposed,
        RecordStatus::Confirmed,
        RecordStatus::Rejected,
        RecordStatus::Pending,
    ];

    fn at(millis: i64) -> Timestamp {
        Timestamp::from_unix_millis(millis).unwrap()
    }

    fn ai() -> Origin {
        Origin::Ai {
            provider_id: ProviderId::new(),
            analysis_run_id: AnalysisRunId::new(),
        }
    }

    fn item(text: &str) -> Redacted<ListItem> {
        Redacted::new(ListItem::parse(text).unwrap())
    }

    fn proposal(kind: RecordKind, nature: Nature, evidence: Vec<SegmentId>) -> Proposal {
        Proposal {
            kind,
            text: Redacted::new(RecordText::parse("O export precisa suportar CSV").unwrap()),
            nature,
            rationale: Redacted::new(Rationale::parse("dito na reunião").unwrap()),
            evidence,
            acceptance_criteria: Vec::new(),
            alternatives: Vec::new(),
            relates_to: None,
        }
    }

    fn spec(origin: Origin, status: RecordStatus) -> RecordSpec {
        RecordSpec {
            id: RecordId::new(),
            session_id: SessionId::new(),
            kind: RecordKind::Requirement,
            text: Redacted::new(RecordText::parse("Suportar CSV").unwrap()),
            nature: Nature::Observed,
            status,
            rationale: Redacted::new(Rationale::default()),
            origin,
            evidence: vec![SegmentId::new()],
            acceptance_criteria: Vec::new(),
            alternatives: Vec::new(),
            flags: RecordFlags::default(),
            created_at: at(1_000),
            updated_at: at(1_000),
        }
    }

    fn record(status: RecordStatus) -> TechnicalRecord {
        TechnicalRecord::restore(spec(ai(), status))
    }

    #[test]
    fn codes_round_trip() {
        for kind in [
            RecordKind::Requirement,
            RecordKind::Decision,
            RecordKind::Risk,
            RecordKind::OpenQuestion,
            RecordKind::Task,
            RecordKind::Assumption,
        ] {
            assert_eq!(RecordKind::parse(kind.code()), Ok(kind));
        }
        for nature in [Nature::Observed, Nature::Inferred, Nature::Suggested] {
            assert_eq!(Nature::parse(nature.code()), Ok(nature));
        }
        for status in ALL_STATUSES {
            assert_eq!(RecordStatus::parse(status.code()), Ok(status));
        }
        assert!(RecordStatus::parse("deleted").is_err());
    }

    #[test]
    fn status_transitions_follow_the_table() {
        use RecordStatus::{Confirmed, Pending, Proposed, Rejected};
        let allowed = [
            (Proposed, Confirmed),
            (Proposed, Rejected),
            (Proposed, Pending),
            (Pending, Confirmed),
            (Pending, Rejected),
            (Confirmed, Pending),
            (Confirmed, Rejected),
            (Rejected, Pending),
            (Rejected, Confirmed),
        ];
        for from in ALL_STATUSES {
            for to in ALL_STATUSES {
                let mut record = record(from);
                let result = record.set_status(Actor::User, to, at(2_000));
                if allowed.contains(&(from, to)) {
                    assert!(result.is_ok(), "{from:?} -> {to:?}");
                    assert_eq!(record.status(), to);
                } else {
                    assert_eq!(
                        result,
                        Err(DomainError::InvalidTransition {
                            from: from.code(),
                            action: to.code()
                        }),
                        "{from:?} -> {to:?}"
                    );
                    assert_eq!(record.status(), from);
                }
            }
        }
    }

    #[test]
    fn user_status_change_produces_history() {
        let mut record = record(RecordStatus::Proposed);
        let change = record
            .set_status(Actor::User, RecordStatus::Confirmed, at(5_000))
            .unwrap();
        assert_eq!(change.record_id(), record.id());
        assert_eq!(change.actor(), ChangeActor::User);
        assert_eq!(change.field(), ChangeField::Status);
        assert_eq!(
            change.old_value().map(|v| v.expose().as_str()),
            Some("proposed")
        );
        assert_eq!(
            change.new_value().map(|v| v.expose().as_str()),
            Some("confirmed")
        );
        assert_eq!(change.at(), at(5_000));
        assert_eq!(record.updated_at(), at(5_000));
    }

    #[test]
    fn ai_cannot_change_existing_records() {
        for from in ALL_STATUSES {
            for to in ALL_STATUSES {
                let mut record = record(from);
                assert!(
                    matches!(
                        record.set_status(Actor::Ai, to, at(2_000)),
                        Err(DomainError::InvariantViolation(_))
                    ),
                    "{from:?} -> {to:?}"
                );
                assert_eq!(record.status(), from);
            }
        }
    }

    #[test]
    fn ai_records_start_proposed() {
        assert!(TechnicalRecord::new(spec(ai(), RecordStatus::Proposed)).is_ok());
        for status in [
            RecordStatus::Confirmed,
            RecordStatus::Rejected,
            RecordStatus::Pending,
        ] {
            assert!(matches!(
                TechnicalRecord::new(spec(ai(), status)),
                Err(DomainError::InvariantViolation(_))
            ));
            // The user may create records already confirmed (FR-032).
            assert!(TechnicalRecord::new(spec(Origin::User, status)).is_ok());
        }
    }

    #[test]
    fn observed_requires_evidence() {
        let mut no_evidence = spec(ai(), RecordStatus::Proposed);
        no_evidence.evidence.clear();
        assert_eq!(
            TechnicalRecord::new(no_evidence.clone()),
            Err(DomainError::invalid("evidence", InputProblem::Empty))
        );
        no_evidence.nature = Nature::Inferred;
        assert!(TechnicalRecord::new(no_evidence.clone()).is_ok());
        no_evidence.nature = Nature::Suggested;
        assert!(TechnicalRecord::new(no_evidence).is_ok());
    }

    #[test]
    fn grounding_drops_foreign_evidence_and_downgrades_observed() {
        let known = SegmentId::new();
        let foreign = SegmentId::new();
        let grounded = proposal(
            RecordKind::Requirement,
            Nature::Observed,
            vec![foreign, known, known],
        )
        .grounded(|id| id == known);
        assert_eq!(grounded.evidence, vec![known]);
        assert_eq!(grounded.nature, Nature::Observed);

        let ungrounded = proposal(RecordKind::Requirement, Nature::Observed, vec![foreign])
            .grounded(|id| id == known);
        assert!(ungrounded.evidence.is_empty());
        assert_eq!(ungrounded.nature, Nature::Inferred);

        let suggested =
            proposal(RecordKind::Risk, Nature::Suggested, Vec::new()).grounded(|_| true);
        assert_eq!(suggested.nature, Nature::Suggested);
    }

    #[test]
    fn proposals_become_proposed_records_with_ai_creation_history() {
        let segment = SegmentId::new();
        let origin = ai();
        let record = TechnicalRecord::propose(
            RecordId::new(),
            SessionId::new(),
            proposal(RecordKind::Requirement, Nature::Observed, vec![segment]),
            origin,
            RecordFlags::default(),
            at(3_000),
        )
        .unwrap();
        assert_eq!(record.status(), RecordStatus::Proposed);
        assert_eq!(record.evidence(), [segment]);
        let creation = record.creation();
        assert_eq!(creation.field(), ChangeField::Created);
        let Origin::Ai {
            analysis_run_id, ..
        } = origin
        else {
            unreachable!()
        };
        assert_eq!(creation.actor(), ChangeActor::Ai { analysis_run_id });
    }

    #[test]
    fn lists_belong_to_their_kinds() {
        let mut task = spec(ai(), RecordStatus::Proposed);
        task.kind = RecordKind::Task;
        task.acceptance_criteria = vec![item("exporta 10k linhas")];
        assert!(TechnicalRecord::new(task.clone()).is_ok());
        task.alternatives = vec![item("JSON")];
        assert_eq!(
            TechnicalRecord::new(task),
            Err(DomainError::invalid(
                "alternatives",
                InputProblem::Inconsistent
            ))
        );

        let mut decision = spec(ai(), RecordStatus::Proposed);
        decision.kind = RecordKind::Decision;
        decision.alternatives = (0..11).map(|_| item("x")).collect();
        assert_eq!(
            TechnicalRecord::new(decision),
            Err(DomainError::invalid("alternatives", InputProblem::TooLong))
        );
    }

    #[test]
    fn user_edits_produce_one_change_per_changed_field() {
        let mut record = record(RecordStatus::Confirmed);
        let changes = record
            .edit(
                Actor::User,
                RecordEdit {
                    text: Some(Redacted::new(
                        RecordText::parse("Suportar CSV e XLSX").unwrap(),
                    )),
                    // Same value: no history entry.
                    rationale: Some(Redacted::new(Rationale::default())),
                    ..RecordEdit::default()
                },
                at(7_000),
            )
            .unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field(), ChangeField::Text);
        assert_eq!(changes[0].actor(), ChangeActor::User);
        assert_eq!(
            changes[0].old_value().map(|v| v.expose().as_str()),
            Some("Suportar CSV")
        );
        assert_eq!(record.text().expose().as_str(), "Suportar CSV e XLSX");
        assert_eq!(record.status(), RecordStatus::Confirmed);
        assert_eq!(record.updated_at(), at(7_000));
        assert!(
            record
                .edit(Actor::User, RecordEdit::default(), at(8_000))
                .unwrap()
                .is_empty()
        );
        assert_eq!(record.updated_at(), at(7_000));
    }

    #[test]
    fn edits_keep_lists_on_their_kinds_and_refuse_the_ai() {
        let mut record = record(RecordStatus::Proposed);
        let lists = RecordEdit {
            acceptance_criteria: Some(vec![item("x")]),
            ..RecordEdit::default()
        };
        assert_eq!(
            record.edit(Actor::User, lists.clone(), at(2_000)),
            Err(DomainError::invalid(
                "acceptance_criteria",
                InputProblem::Inconsistent
            ))
        );
        assert!(matches!(
            record.edit(Actor::Ai, lists, at(2_000)),
            Err(DomainError::InvariantViolation(_))
        ));
    }

    #[test]
    fn user_records_may_start_in_any_status() {
        for status in ALL_STATUSES {
            let record = TechnicalRecord::create_by_user(
                RecordId::new(),
                SessionId::new(),
                UserRecord {
                    kind: RecordKind::Assumption,
                    text: Redacted::new(RecordText::parse("Equipe de 3 pessoas").unwrap()),
                    rationale: Redacted::new(Rationale::default()),
                    status,
                },
                at(1_000),
            )
            .unwrap();
            assert_eq!(record.origin(), Origin::User);
            assert_eq!(record.status(), status);
            assert_eq!(record.creation().actor(), ChangeActor::User);
        }
    }

    #[test]
    fn flags_cannot_point_at_the_record_itself() {
        let mut spec = spec(ai(), RecordStatus::Proposed);
        spec.flags.conflicts_with = Some(spec.id);
        assert!(TechnicalRecord::new(spec).is_err());
    }

    #[test]
    fn dedup_key_ignores_case_and_punctuation() {
        assert_eq!(
            dedup_key(RecordKind::Requirement, "  O export precisa suportar CSV!"),
            dedup_key(RecordKind::Requirement, "o EXPORT precisa, suportar csv")
        );
        assert_ne!(
            dedup_key(RecordKind::Requirement, "suportar CSV"),
            dedup_key(RecordKind::Task, "suportar CSV")
        );
    }

    #[test]
    fn debug_hides_content() {
        let record = record(RecordStatus::Proposed);
        assert!(!format!("{record:?}").contains("CSV"));
    }
}
