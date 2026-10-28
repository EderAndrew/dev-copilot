//! Development plan of a session (data-model.md "DevelopmentPlan", FR-031).
//!
//! A derived view, never stored: the session's records grouped by kind (tasks first, then
//! requirements, decisions, assumptions, risks and open questions), each group ordered by status
//! (`confirmed`, `pending`, `proposed`, `rejected`) and then by creation. Acceptance criteria and
//! alternatives travel with their tasks and decisions; user records keep `origin = user`, which
//! is how the plan marks them (FR-032).

use devcopilot_domain::ids::SessionId;
use devcopilot_domain::record::{RecordKind, RecordStatus, TechnicalRecord};

/// Order of the plan sections.
pub const PLAN_KINDS: [RecordKind; 6] = [
    RecordKind::Task,
    RecordKind::Requirement,
    RecordKind::Decision,
    RecordKind::Assumption,
    RecordKind::Risk,
    RecordKind::OpenQuestion,
];

/// Records of one kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSection {
    /// Kind of every item.
    pub kind: RecordKind,
    /// Items in plan order.
    pub items: Vec<TechnicalRecord>,
}

/// The plan of one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentPlan {
    /// Session.
    pub session_id: SessionId,
    /// One section per kind, in [`PLAN_KINDS`] order, possibly empty.
    pub sections: Vec<PlanSection>,
}

const fn status_rank(status: RecordStatus) -> u8 {
    match status {
        RecordStatus::Confirmed => 0,
        RecordStatus::Pending => 1,
        RecordStatus::Proposed => 2,
        RecordStatus::Rejected => 3,
    }
}

impl DevelopmentPlan {
    /// Builds the plan from the records of `session_id`; records of other sessions are ignored.
    #[must_use]
    pub fn assemble(session_id: SessionId, records: Vec<TechnicalRecord>) -> Self {
        let mut records: Vec<TechnicalRecord> = records
            .into_iter()
            .filter(|record| record.session_id() == session_id)
            .collect();
        records.sort_by_key(|record| {
            (
                status_rank(record.status()),
                record.created_at(),
                record.id(),
            )
        });
        let sections = PLAN_KINDS
            .iter()
            .map(|kind| PlanSection {
                kind: *kind,
                items: records
                    .iter()
                    .filter(|record| record.kind() == *kind)
                    .cloned()
                    .collect(),
            })
            .collect();
        Self {
            session_id,
            sections,
        }
    }

    /// Items of `kind`.
    #[must_use]
    pub fn section(&self, kind: RecordKind) -> &[TechnicalRecord] {
        self.sections
            .iter()
            .find(|section| section.kind == kind)
            .map_or(&[], |section| section.items.as_slice())
    }
}
