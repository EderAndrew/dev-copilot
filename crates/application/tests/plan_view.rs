//! Development plan derivation (T098, US4.1; FR-031, FR-032): one section per kind in plan
//! order, items ordered by status then creation, user records marked by their origin.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use devcopilot_application::ApplicationError;
use devcopilot_application::plan::{DevelopmentPlan, PLAN_KINDS};
use devcopilot_domain::Redacted;
use devcopilot_domain::ids::{AnalysisRunId, ProviderId, RecordId, SessionId};
use devcopilot_domain::record::{
    Nature, Origin, RecordFlags, RecordKind, RecordSpec, RecordStatus, TechnicalRecord,
};
use devcopilot_domain::text::{ListItem, Rationale, RecordText};
use devcopilot_domain::time::Timestamp;
use support::fixture;

fn record(
    session: SessionId,
    kind: RecordKind,
    status: RecordStatus,
    origin: Origin,
    created_ms: i64,
    text: &str,
) -> TechnicalRecord {
    let at = Timestamp::from_unix_millis(created_ms).unwrap();
    TechnicalRecord::restore(RecordSpec {
        id: RecordId::new(),
        session_id: session,
        kind,
        text: Redacted::new(RecordText::parse(text).unwrap()),
        nature: Nature::Inferred,
        status,
        rationale: Redacted::new(Rationale::default()),
        origin,
        evidence: Vec::new(),
        acceptance_criteria: if kind == RecordKind::Task {
            vec![Redacted::new(ListItem::parse("critério").unwrap())]
        } else {
            Vec::new()
        },
        alternatives: Vec::new(),
        flags: RecordFlags::default(),
        created_at: at,
        updated_at: at,
    })
}

fn ai() -> Origin {
    Origin::Ai {
        provider_id: ProviderId::new(),
        analysis_run_id: AnalysisRunId::new(),
    }
}

fn texts(items: &[TechnicalRecord]) -> Vec<&str> {
    items.iter().map(|r| r.text().expose().as_str()).collect()
}

#[test]
fn sections_follow_plan_order_and_include_empty_kinds() {
    let session = SessionId::new();
    let plan = DevelopmentPlan::assemble(
        session,
        vec![
            record(
                session,
                RecordKind::Risk,
                RecordStatus::Proposed,
                ai(),
                1,
                "risco",
            ),
            record(
                session,
                RecordKind::Task,
                RecordStatus::Proposed,
                ai(),
                2,
                "tarefa",
            ),
        ],
    );
    let kinds: Vec<RecordKind> = plan.sections.iter().map(|s| s.kind).collect();
    assert_eq!(kinds, PLAN_KINDS);
    assert_eq!(texts(plan.section(RecordKind::Task)), ["tarefa"]);
    assert_eq!(texts(plan.section(RecordKind::Risk)), ["risco"]);
    assert!(plan.section(RecordKind::Decision).is_empty());
    // Acceptance criteria travel with their task.
    assert_eq!(
        plan.section(RecordKind::Task)[0]
            .acceptance_criteria()
            .len(),
        1
    );
}

#[test]
fn items_are_ordered_by_status_then_creation() {
    let session = SessionId::new();
    let d = |status, at, text| record(session, RecordKind::Decision, status, ai(), at, text);
    let plan = DevelopmentPlan::assemble(
        session,
        vec![
            d(RecordStatus::Rejected, 1, "rejeitada"),
            d(RecordStatus::Proposed, 2, "proposta antiga"),
            d(RecordStatus::Confirmed, 9, "confirmada nova"),
            d(RecordStatus::Pending, 3, "pendente"),
            d(RecordStatus::Confirmed, 4, "confirmada antiga"),
            d(RecordStatus::Proposed, 8, "proposta nova"),
        ],
    );
    assert_eq!(
        texts(plan.section(RecordKind::Decision)),
        [
            "confirmada antiga",
            "confirmada nova",
            "pendente",
            "proposta antiga",
            "proposta nova",
            "rejeitada"
        ]
    );
}

#[test]
fn user_records_keep_their_origin_and_other_sessions_are_ignored() {
    let session = SessionId::new();
    let plan = DevelopmentPlan::assemble(
        session,
        vec![
            record(
                session,
                RecordKind::Assumption,
                RecordStatus::Confirmed,
                Origin::User,
                1,
                "minha premissa",
            ),
            record(
                SessionId::new(),
                RecordKind::Assumption,
                RecordStatus::Confirmed,
                ai(),
                2,
                "de outra sessão",
            ),
        ],
    );
    let assumptions = plan.section(RecordKind::Assumption);
    assert_eq!(texts(assumptions), ["minha premissa"]);
    assert_eq!(assumptions[0].origin(), Origin::User);
}

#[test]
fn service_builds_the_plan_from_stored_records() {
    let f = fixture();
    let session = f.create_mic().id();
    f.records.seed(record(
        session,
        RecordKind::OpenQuestion,
        RecordStatus::Proposed,
        ai(),
        1,
        "quem aprova?",
    ));
    let plan = f.record_service.plan(session).unwrap();
    assert_eq!(
        texts(plan.section(RecordKind::OpenQuestion)),
        ["quem aprova?"]
    );
    assert_eq!(
        f.record_service.plan(SessionId::new()),
        Err(ApplicationError::NotFound)
    );
}
