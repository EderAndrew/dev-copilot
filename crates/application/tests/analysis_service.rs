//! Incremental analysis of a capturing session (T082, US3): scheduling and limits (R7),
//! conflict and duplicate handling, grounding, and failures that never touch records or
//! capture (FR-021–FR-030, SC-006, SC-009).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::time::{Duration as StdDuration, Instant};

use devcopilot_application::events::{AnalysisStatus, ProviderStage, SessionEvent};
use devcopilot_application::ports::{
    AnalysisOutput, AnalysisRequest, ProviderError, RecordRepository, SegmentRepository,
    TranscriptUpdate,
};
use devcopilot_application::settings_service::AnalysisLimits;
use devcopilot_domain::Redacted;
use devcopilot_domain::analysis_run::AnalysisOutcome;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{AnalysisRunId, ProviderId, RecordId, SegmentId, SessionId};
use devcopilot_domain::record::{
    Nature, Origin, Proposal, RecordFlags, RecordKind, RecordSpec, RecordStatus, Relation,
    RelationKind, TechnicalRecord,
};
use devcopilot_domain::session::SessionState;
use devcopilot_domain::text::{Rationale, RecordText, SegmentText};
use devcopilot_domain::time::{Duration, Timestamp};
use support::{Fixture, fixture, wait_until};

fn secs(value: u64) -> Duration {
    Duration::from_secs(value)
}

fn text_final(start_s: u64, end_s: u64, text: &str) -> TranscriptUpdate {
    TranscriptUpdate::Final {
        source: AudioSourceKind::Microphone,
        start: secs(start_s),
        end: secs(end_s),
        text: Redacted::new(SegmentText::parse(text).unwrap()),
    }
}

/// Emits `count` two-second finals starting at `from_s` and waits until they are stored.
async fn emit_finals(f: &Fixture, id: SessionId, from_s: u64, count: u64) {
    let before = f.segments.list_by_session(id).unwrap().len();
    for i in 0..count {
        let start = from_s + i * 2;
        f.transcriber
            .emit(text_final(start, start + 2, &format!("trecho {start}")));
    }
    let expected = before + usize::try_from(count).unwrap();
    wait_until(|| f.segments.list_by_session(id).unwrap().len() >= expected).await;
}

fn proposal(kind: RecordKind, text: &str, nature: Nature, evidence: Vec<SegmentId>) -> Proposal {
    Proposal {
        kind,
        text: Redacted::new(RecordText::parse(text).unwrap()),
        nature,
        rationale: Redacted::new(Rationale::default()),
        evidence,
        acceptance_criteria: Vec::new(),
        alternatives: Vec::new(),
        relates_to: None,
    }
}

fn output(proposals: Vec<Proposal>) -> AnalysisOutput {
    AnalysisOutput {
        proposals,
        tokens_used: 100,
    }
}

fn statuses(f: &Fixture, id: SessionId) -> Vec<AnalysisStatus> {
    f.events(id)
        .into_iter()
        .filter_map(|event| match event {
            SessionEvent::AnalysisStatus { status, .. } => Some(status),
            _ => None,
        })
        .collect()
}

fn proposed_events(f: &Fixture, id: SessionId) -> usize {
    f.events(id)
        .iter()
        .filter(|event| matches!(event, SessionEvent::RecordProposed(_)))
        .count()
}

fn seed_record(
    f: &Fixture,
    session: SessionId,
    status: RecordStatus,
    text: &str,
) -> TechnicalRecord {
    let at = Timestamp::from_unix_millis(500).unwrap();
    let record = TechnicalRecord::restore(RecordSpec {
        id: RecordId::new(),
        session_id: session,
        kind: RecordKind::Decision,
        text: Redacted::new(RecordText::parse(text).unwrap()),
        nature: Nature::Inferred,
        status,
        rationale: Redacted::new(Rationale::default()),
        origin: Origin::Ai {
            provider_id: ProviderId::new(),
            analysis_run_id: AnalysisRunId::new(),
        },
        evidence: Vec::new(),
        acceptance_criteria: Vec::new(),
        alternatives: Vec::new(),
        flags: RecordFlags::default(),
        created_at: at,
        updated_at: at,
    });
    f.records.seed(record.clone());
    record
}

/// Fixture with an analyzer, capturing a mic session.
async fn capturing() -> (Fixture, SessionId) {
    let f = fixture();
    f.add_analyzer();
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    (f, id)
}

async fn settle() {
    tokio::time::sleep(StdDuration::from_millis(40)).await;
}

#[tokio::test]
async fn runs_after_six_finals_and_creates_proposed_records() {
    let (f, id) = capturing().await;
    assert!(f.events(id).iter().any(|e| matches!(
        e,
        SessionEvent::ProviderInUse {
            stage: ProviderStage::Analysis,
            ..
        }
    )));
    f.analyzer
        .respond_with(std::sync::Arc::new(|req: &AnalysisRequest| {
            Ok(output(vec![proposal(
                RecordKind::Requirement,
                "O export precisa suportar CSV",
                Nature::Observed,
                vec![req.segments[0].id],
            )]))
        }));
    emit_finals(&f, id, 0, 5).await;
    settle().await;
    assert_eq!(f.analyzer.call_count(), 0, "five finals do not trigger");

    emit_finals(&f, id, 10, 1).await;
    wait_until(|| f.records.all().len() == 1).await;
    let request = &f.analyzer.requests()[0];
    assert_eq!(request.segments.len(), 6);
    assert_eq!(request.session_id, id);
    let record = &f.records.all()[0];
    assert_eq!(record.status(), RecordStatus::Proposed);
    assert_eq!(record.nature(), Nature::Observed);
    assert_eq!(record.evidence(), [request.segments[0].id]);
    wait_until(|| statuses(&f, id).contains(&AnalysisStatus::Ok { created: 1 })).await;
    assert_eq!(statuses(&f, id)[0], AnalysisStatus::Running);
    assert_eq!(proposed_events(&f, id), 1);
    let runs = f.runs.all();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].outcome(), Some(AnalysisOutcome::Ok { created: 1 }));
    assert_eq!(runs[0].segment_range(), (0, 5));
}

#[tokio::test]
async fn ninety_seconds_of_speech_trigger_a_run() {
    let (f, _id) = capturing().await;
    f.transcriber.emit(text_final(0, 90, "monólogo longo"));
    wait_until(|| f.analyzer.call_count() == 1).await;
}

#[tokio::test]
async fn runs_are_at_least_the_minimum_interval_apart() {
    let (f, id) = capturing().await;
    emit_finals(&f, id, 0, 6).await;
    wait_until(|| f.runs.all().iter().any(|r| r.outcome().is_some())).await;
    emit_finals(&f, id, 20, 6).await;
    settle().await;
    assert_eq!(f.analyzer.call_count(), 1, "within 30 s of the last run");
    f.clock.advance(secs(30));
    wait_until(|| f.analyzer.call_count() == 2).await;
    assert_eq!(f.analyzer.requests()[1].segments.len(), 6);
}

#[tokio::test]
async fn at_most_one_run_is_in_flight() {
    let (f, id) = capturing().await;
    f.analyzer.hold();
    emit_finals(&f, id, 0, 6).await;
    wait_until(|| f.analyzer.call_count() == 1).await;
    for round in 0..3 {
        f.clock.advance(secs(60));
        emit_finals(&f, id, 100 + round * 20, 6).await;
    }
    settle().await;
    assert_eq!(f.analyzer.call_count(), 1);
    f.analyzer.release();
    wait_until(|| f.analyzer.call_count() == 2).await;
    assert_eq!(f.analyzer.max_in_flight(), 1);
    // The second run covers everything that waited.
    assert_eq!(f.analyzer.requests()[1].segments.len(), 18);
}

#[tokio::test]
async fn hourly_cap_reports_rate_limited_once_and_keeps_segments() {
    let (f, id) = capturing().await;
    f.set_analysis_limits(AnalysisLimits {
        max_calls_per_hour: 1,
        ..AnalysisLimits::default()
    });
    emit_finals(&f, id, 0, 6).await;
    wait_until(|| f.analyzer.call_count() == 1).await;
    f.clock.advance(secs(60));
    emit_finals(&f, id, 20, 6).await;
    wait_until(|| statuses(&f, id).contains(&AnalysisStatus::RateLimited)).await;
    f.clock.advance(secs(60));
    emit_finals(&f, id, 40, 6).await;
    settle().await;
    assert_eq!(f.analyzer.call_count(), 1);
    let limited = statuses(&f, id)
        .into_iter()
        .filter(|s| *s == AnalysisStatus::RateLimited)
        .count();
    assert_eq!(limited, 1, "reported once");
    assert!(
        f.runs
            .all()
            .iter()
            .any(|r| r.outcome() == Some(AnalysisOutcome::RateLimited))
    );
    // An hour later the waiting segments are analysed together.
    f.clock.advance(secs(3_600));
    wait_until(|| f.analyzer.call_count() == 2).await;
    assert_eq!(f.analyzer.requests()[1].segments.len(), 12);
}

#[tokio::test]
async fn token_budget_per_session_rate_limits() {
    let (f, id) = capturing().await;
    f.set_analysis_limits(AnalysisLimits {
        max_tokens_per_session: 1_000,
        ..AnalysisLimits::default()
    });
    f.analyzer.push(Ok(AnalysisOutput {
        proposals: Vec::new(),
        tokens_used: 1_500,
    }));
    emit_finals(&f, id, 0, 6).await;
    wait_until(|| statuses(&f, id).contains(&AnalysisStatus::Ok { created: 0 })).await;
    f.clock.advance(secs(60));
    emit_finals(&f, id, 20, 6).await;
    wait_until(|| statuses(&f, id).contains(&AnalysisStatus::RateLimited)).await;
    assert_eq!(f.analyzer.call_count(), 1);
}

#[tokio::test]
async fn conflicting_suggestion_becomes_a_new_flagged_proposal() {
    let (f, id) = capturing().await;
    let confirmed = seed_record(&f, id, RecordStatus::Confirmed, "Usar Postgres");
    let target = confirmed.id();
    f.analyzer
        .respond_with(std::sync::Arc::new(move |_: &AnalysisRequest| {
            let mut conflicting = proposal(
                RecordKind::Decision,
                "Usar MongoDB",
                Nature::Suggested,
                Vec::new(),
            );
            conflicting.relates_to = Some(Relation {
                record_id: target,
                kind: RelationKind::Conflict,
            });
            Ok(output(vec![conflicting]))
        }));
    emit_finals(&f, id, 0, 6).await;
    wait_until(|| f.records.all().len() == 2).await;
    assert_eq!(f.records.get(confirmed.id()).unwrap(), confirmed);
    let new = &f.records.all()[1];
    assert_eq!(new.status(), RecordStatus::Proposed);
    assert_eq!(new.flags().conflicts_with, Some(target));
    // The request summarized the existing record so the model could point at it.
    assert_eq!(f.analyzer.requests()[0].records[0].id, target);
}

#[tokio::test]
async fn duplicates_are_flagged_or_grouped() {
    let (f, id) = capturing().await;
    let existing = seed_record(&f, id, RecordStatus::Pending, "Usar Postgres");
    let target = existing.id();
    let foreign = RecordId::new();
    f.analyzer
        .respond_with(std::sync::Arc::new(move |_: &AnalysisRequest| {
            let mut reworded = proposal(
                RecordKind::Decision,
                "Banco será PostgreSQL",
                Nature::Suggested,
                Vec::new(),
            );
            reworded.relates_to = Some(Relation {
                record_id: target,
                kind: RelationKind::Duplicate,
            });
            let mut other_session = proposal(
                RecordKind::Risk,
                "Prazo curto",
                Nature::Suggested,
                Vec::new(),
            );
            other_session.relates_to = Some(Relation {
                record_id: foreign,
                kind: RelationKind::Conflict,
            });
            Ok(output(vec![
                reworded,
                // Same content as the existing record: grouped, not repeated (FR-027).
                proposal(
                    RecordKind::Decision,
                    "usar  POSTGRES!",
                    Nature::Suggested,
                    Vec::new(),
                ),
                other_session.clone(),
                other_session,
            ]))
        }));
    emit_finals(&f, id, 0, 6).await;
    wait_until(|| {
        statuses(&f, id)
            .iter()
            .any(|s| matches!(s, AnalysisStatus::Ok { .. }))
    })
    .await;
    let records = f.records.all();
    assert_eq!(records.len(), 3);
    assert_eq!(records[1].flags().duplicate_of, Some(target));
    assert_eq!(
        records[2].flags(),
        RecordFlags::default(),
        "relation outside the session ignored"
    );
    assert_eq!(
        statuses(&f, id).last(),
        Some(&AnalysisStatus::Ok { created: 2 })
    );
}

#[tokio::test]
async fn evidence_from_other_sessions_is_discarded() {
    let (f, id) = capturing().await;
    let foreign = SegmentId::new();
    f.analyzer
        .respond_with(std::sync::Arc::new(move |req: &AnalysisRequest| {
            Ok(output(vec![
                proposal(
                    RecordKind::Requirement,
                    "Requisito sem base",
                    Nature::Observed,
                    vec![foreign],
                ),
                proposal(
                    RecordKind::Risk,
                    "Risco citado",
                    Nature::Observed,
                    vec![foreign, req.segments[1].id],
                ),
            ]))
        }));
    emit_finals(&f, id, 0, 6).await;
    wait_until(|| f.records.all().len() == 2).await;
    let records = f.records.all();
    assert_eq!(records[0].nature(), Nature::Inferred);
    assert!(records[0].evidence().is_empty());
    assert_eq!(records[1].nature(), Nature::Observed);
    assert_eq!(records[1].evidence().len(), 1);
    assert_ne!(records[1].evidence()[0], foreign);
}

#[tokio::test]
async fn provider_failure_leaves_records_and_capture_intact_and_retries() {
    let (f, id) = capturing().await;
    let existing = seed_record(&f, id, RecordStatus::Confirmed, "Usar Postgres");
    f.analyzer.push(Err(ProviderError::Unavailable));
    f.analyzer.push(Err(ProviderError::InvalidOutput));
    emit_finals(&f, id, 0, 6).await;
    wait_until(|| statuses(&f, id).contains(&AnalysisStatus::ProviderError)).await;
    assert_eq!(f.state(id), SessionState::Capturing);
    assert_eq!(f.records.all(), vec![existing.clone()]);

    f.clock.advance(secs(60));
    emit_finals(&f, id, 20, 6).await;
    wait_until(|| statuses(&f, id).contains(&AnalysisStatus::InvalidOutput)).await;
    assert_eq!(f.records.all(), vec![existing]);
    // Failed segments stay pending and are sent again.
    assert_eq!(f.analyzer.requests()[1].segments.len(), 12);
    assert_eq!(f.state(id), SessionState::Capturing);
}

#[tokio::test]
async fn stop_is_immediate_while_analysis_hangs() {
    let (f, id) = capturing().await;
    f.analyzer.hold();
    emit_finals(&f, id, 0, 6).await;
    wait_until(|| f.analyzer.call_count() == 1).await;
    let started = Instant::now();
    f.service.stop(id).unwrap();
    assert!(started.elapsed() < StdDuration::from_millis(500), "SC-006");
    f.wait_for_state(id, SessionState::Ended).await;
}

#[tokio::test]
async fn no_analysis_without_a_usable_provider() {
    let f = fixture();
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    emit_finals(&f, id, 0, 12).await;
    settle().await;
    assert_eq!(f.analyzer.call_count(), 0);

    let f = fixture();
    f.add_analyzer();
    f.analyzers.set_unavailable(true);
    let id = f.create_mic().id();
    f.start_capturing(id).await;
    emit_finals(&f, id, 0, 6).await;
    settle().await;
    assert_eq!(f.analyzer.call_count(), 0);
    assert_eq!(f.state(id), SessionState::Capturing);
}
