//! Append-only history of a technical record (data-model.md "RecordChange", FR-026).
//!
//! The AI may only appear as the actor of a record's creation: every later change is a user
//! action (FR-024, FR-025).

use crate::Redacted;
use crate::error::DomainError;
use crate::ids::{AnalysisRunId, ChangeId, RecordId};
use crate::time::Timestamp;

/// Who made a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeActor {
    /// The user, through an explicit action.
    User,
    /// An analysis run; only for `created`.
    Ai {
        /// Run that proposed the record.
        analysis_run_id: AnalysisRunId,
    },
}

impl ChangeActor {
    /// Stable code (`user` or `ai`).
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Ai { .. } => "ai",
        }
    }
}

/// Which part of the record changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeField {
    /// The record was created.
    Created,
    /// Text.
    Text,
    /// Status.
    Status,
    /// Rationale.
    Rationale,
    /// Acceptance criteria.
    AcceptanceCriteria,
    /// Alternatives.
    Alternatives,
}

code_enum!(ChangeField, "field", {
    Created => "created",
    Text => "text",
    Status => "status",
    Rationale => "rationale",
    AcceptanceCriteria => "acceptance_criteria",
    Alternatives => "alternatives",
});

/// Fields of a change before validation, also used to restore stored rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordChangeSpec {
    /// Identifier.
    pub id: ChangeId,
    /// Changed record.
    pub record_id: RecordId,
    /// Who changed it.
    pub actor: ChangeActor,
    /// What changed.
    pub field: ChangeField,
    /// Previous value (session content when the field is text-like).
    pub old_value: Option<Redacted<String>>,
    /// New value.
    pub new_value: Option<Redacted<String>>,
    /// When.
    pub at: Timestamp,
}

/// One entry of a record's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordChange {
    id: ChangeId,
    record_id: RecordId,
    actor: ChangeActor,
    field: ChangeField,
    old_value: Option<Redacted<String>>,
    new_value: Option<Redacted<String>>,
    at: Timestamp,
}

impl RecordChange {
    /// Validates a change.
    ///
    /// # Errors
    ///
    /// [`DomainError::InvariantViolation`] when the AI is the actor of anything but the
    /// creation (FR-024, FR-025).
    pub fn new(spec: RecordChangeSpec) -> Result<Self, DomainError> {
        if matches!(spec.actor, ChangeActor::Ai { .. }) && spec.field != ChangeField::Created {
            return Err(DomainError::InvariantViolation(
                "the AI cannot change existing records",
            ));
        }
        Ok(Self::restore(spec))
    }

    /// Rebuilds a stored change.
    #[must_use]
    pub fn restore(spec: RecordChangeSpec) -> Self {
        Self {
            id: spec.id,
            record_id: spec.record_id,
            actor: spec.actor,
            field: spec.field,
            old_value: spec.old_value,
            new_value: spec.new_value,
            at: spec.at,
        }
    }

    /// Identifier.
    #[must_use]
    pub const fn id(&self) -> ChangeId {
        self.id
    }

    /// Changed record.
    #[must_use]
    pub const fn record_id(&self) -> RecordId {
        self.record_id
    }

    /// Who changed it.
    #[must_use]
    pub const fn actor(&self) -> ChangeActor {
        self.actor
    }

    /// What changed.
    #[must_use]
    pub const fn field(&self) -> ChangeField {
        self.field
    }

    /// Previous value.
    #[must_use]
    pub const fn old_value(&self) -> Option<&Redacted<String>> {
        self.old_value.as_ref()
    }

    /// New value.
    #[must_use]
    pub const fn new_value(&self) -> Option<&Redacted<String>> {
        self.new_value.as_ref()
    }

    /// When.
    #[must_use]
    pub const fn at(&self) -> Timestamp {
        self.at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(actor: ChangeActor, field: ChangeField) -> RecordChangeSpec {
        RecordChangeSpec {
            id: ChangeId::new(),
            record_id: RecordId::new(),
            actor,
            field,
            old_value: None,
            new_value: Some(Redacted::new("confirmed".into())),
            at: Timestamp::from_unix_millis(1_000).unwrap(),
        }
    }

    #[test]
    fn ai_may_only_create() {
        let ai = ChangeActor::Ai {
            analysis_run_id: AnalysisRunId::new(),
        };
        assert!(RecordChange::new(spec(ai, ChangeField::Created)).is_ok());
        for field in [
            ChangeField::Text,
            ChangeField::Status,
            ChangeField::Rationale,
            ChangeField::AcceptanceCriteria,
            ChangeField::Alternatives,
        ] {
            assert!(
                matches!(
                    RecordChange::new(spec(ai, field)),
                    Err(DomainError::InvariantViolation(_))
                ),
                "{field:?}"
            );
            assert!(RecordChange::new(spec(ChangeActor::User, field)).is_ok());
        }
    }

    #[test]
    fn field_codes_round_trip() {
        for field in [
            ChangeField::Created,
            ChangeField::Text,
            ChangeField::Status,
            ChangeField::Rationale,
            ChangeField::AcceptanceCriteria,
            ChangeField::Alternatives,
        ] {
            assert_eq!(ChangeField::parse(field.code()), Ok(field));
        }
    }

    #[test]
    fn debug_hides_values() {
        let mut spec = spec(ChangeActor::User, ChangeField::Text);
        spec.new_value = Some(Redacted::new("texto sigiloso".into()));
        let change = RecordChange::new(spec).unwrap();
        assert!(!format!("{change:?}").contains("sigiloso"));
    }
}
