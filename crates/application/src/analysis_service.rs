//! Incremental technical analysis of a capturing session (FR-021–FR-030, research R7).
//!
//! ```text
//! transcription ─final persisted─▶ AnalysisFeed ─▶ scheduler ─▶ AnalysisProvider ─▶ proposals
//! ```
//!
//! - A run starts after [`AnalysisLimits::trigger_finals`] new finals or
//!   [`AnalysisLimits::trigger_speech_secs`] of new speech, at least
//!   [`AnalysisLimits::min_interval_secs`] after the previous run, with at most one run in
//!   flight per session. Calls per hour (across sessions) and tokens per session and provider
//!   are capped; a capped run is recorded once as `rate_limited` and the segments wait.
//! - A run sends the new finals plus a summary of the session's records. Output can only
//!   **create** `proposed` records (provider-ports.md rule 5): evidence outside the session is
//!   dropped (an `observed` proposal left without evidence becomes `inferred`), relations to
//!   records of the session become duplicate or conflict flags, and content already recorded
//!   is grouped with the existing record instead of repeated (FR-025, FR-027).
//! - Failures (invalid output, provider error) leave records untouched and keep the segments
//!   pending, so the next run retries them. Nothing here touches capture: the transcription
//!   only hands notices over without waiting (FR-028, SC-006).
//! - A **final** analysis of an ended or failed session runs only on explicit request
//!   ([`AnalysisService::run_final`], FR-033), under the same rules: it only creates proposals,
//!   counts against the hourly cap, and at most one runs per session.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};

use devcopilot_domain::Redacted;
use devcopilot_domain::analysis_run::{AnalysisOutcome, AnalysisRun, AnalysisTrigger};
use devcopilot_domain::ids::{AnalysisRunId, ProviderId, RecordId, SegmentId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::record::{Origin, Proposal, RecordFlags, RelationKind, TechnicalRecord};
use devcopilot_domain::session::{Session, SessionState};
use devcopilot_domain::text::RecordText;
use devcopilot_domain::time::{Duration, Timestamp};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::error::ApplicationError;
use crate::events::{AnalysisStatus, ProviderStage, SessionEvent};
use crate::ports::{
    AnalysisProvider, AnalysisProviderFactory, AnalysisRequest, AnalysisRunRepository,
    AnalysisSegment, BoxFuture, Clock, EventPublisher, ProviderError, RecordRepository,
    RecordSummary, SegmentRepository,
};
use crate::provider_service::ProviderService;
use crate::settings_service::{AnalysisLimits, SettingsService};

/// How often the scheduler re-evaluates time-based triggers.
pub const DEFAULT_TICK: std::time::Duration = std::time::Duration::from_secs(1);
/// Most final segments sent in one incremental run (the most recent ones win).
pub const MAX_SEGMENTS_PER_RUN: usize = 60;
/// Most final segments sent in a final analysis (the most recent ones win).
pub const MAX_SEGMENTS_FINAL: usize = 400;
/// Longest record text, in characters, in the summary sent to the provider.
pub const SUMMARY_CHARS: usize = 200;

/// Window of the per-hour call cap.
const HOUR: Duration = Duration::from_secs(3_600);
/// Notices queued between the transcription and the scheduler.
const FEED_CAPACITY: usize = 256;

/// Collaborators of [`AnalysisService`].
pub struct AnalysisDeps {
    /// Resolves the analysis provider.
    pub providers: Arc<ProviderService>,
    /// Builds the provider's adapter.
    pub analyzers: Arc<dyn AnalysisProviderFactory>,
    /// Record storage.
    pub records: Arc<dyn RecordRepository>,
    /// Run storage.
    pub runs: Arc<dyn AnalysisRunRepository>,
    /// Transcript storage.
    pub segments: Arc<dyn SegmentRepository>,
    /// Analysis limits (FR-028).
    pub settings: Arc<SettingsService>,
    /// Wall clock.
    pub clock: Arc<dyn Clock>,
    /// Session event delivery.
    pub events: Arc<dyn EventPublisher>,
    /// How often time-based triggers are re-evaluated ([`DEFAULT_TICK`] in the app).
    pub tick: std::time::Duration,
}

struct Inner {
    deps: AnalysisDeps,
    /// Start of every call in the last hour, across sessions.
    calls: Mutex<VecDeque<Timestamp>>,
    /// Sessions with a final analysis in flight.
    finals: Mutex<HashSet<SessionId>>,
}

/// Counts a call starting at `now` against the hourly cap; `false` when the cap is reached.
fn reserve_call(calls: &Mutex<VecDeque<Timestamp>>, now: Timestamp, max_per_hour: u32) -> bool {
    let mut calls = calls.lock().unwrap_or_else(PoisonError::into_inner);
    while calls
        .front()
        .is_some_and(|start| now.saturating_duration_since(*start) >= HOUR)
    {
        calls.pop_front();
    }
    if calls.len() >= usize::try_from(max_per_hour).unwrap_or(usize::MAX) {
        return false;
    }
    calls.push_back(now);
    true
}

/// Incremental analysis use cases.
#[derive(Clone)]
pub struct AnalysisService {
    inner: Arc<Inner>,
}

impl AnalysisService {
    /// Creates the service.
    #[must_use]
    pub fn new(deps: AnalysisDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                deps,
                calls: Mutex::new(VecDeque::new()),
                finals: Mutex::new(HashSet::new()),
            }),
        }
    }

    /// Runs a final analysis of an ended or failed session by explicit user request (FR-033)
    /// and returns the run id; results arrive as `analysis_status` and `record_proposed`
    /// events. Must be called within a Tokio runtime.
    ///
    /// # Errors
    ///
    /// [`ApplicationError::InvalidState`] unless the session ended or failed, or while another
    /// final analysis of it runs; [`ApplicationError::Provider`] when no analysis provider is
    /// usable; [`ApplicationError::Storage`].
    pub fn run_final(&self, session: &Session) -> Result<AnalysisRunId, ApplicationError> {
        let deps = &self.inner.deps;
        let id = session.id();
        if !session.state().is_terminal() {
            return Err(ApplicationError::InvalidState(
                "final analysis needs an ended or failed session".into(),
            ));
        }
        let runtime = Handle::try_current()
            .map_err(|_| ApplicationError::Internal("no async runtime".into()))?;
        let provider = deps
            .providers
            .resolve_analyzer()?
            .ok_or(ApplicationError::Provider(ProviderError::Unavailable))?;
        let analyzer = deps.analyzers.create(&provider)?;
        let next_seq = deps.segments.next_seq(id)?;
        let range = (0, next_seq.saturating_sub(1));
        let context = Arc::new(RunContext {
            inner: Arc::clone(&self.inner),
            session_id: id,
            session_state: session.state(),
            trigger: AnalysisTrigger::Final,
            language: session.language(),
            provider_id: provider.id(),
            max_segments: MAX_SEGMENTS_FINAL,
            analyzer,
        });
        {
            let mut finals = self
                .inner
                .finals
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if !finals.insert(id) {
                return Err(ApplicationError::InvalidState(
                    "final analysis already running".into(),
                ));
            }
        }
        let release = || {
            self.inner
                .finals
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
        };
        let now = deps.clock.now();
        let run = match context.create_run(range, now) {
            Ok(run) => run,
            Err(err) => {
                release();
                return Err(err);
            }
        };
        let run_id = run.id();
        deps.events.publish(
            id,
            SessionEvent::ProviderInUse {
                stage: ProviderStage::Analysis,
                locality: provider.locality(),
                provider_id: provider.id(),
            },
        );
        let max_per_hour = deps
            .settings
            .get()
            .map(|settings| settings.analysis_limits.max_calls_per_hour)
            .unwrap_or(AnalysisLimits::default().max_calls_per_hour);
        if !reserve_call(&self.inner.calls, now, max_per_hour) {
            tracing::warn!(session = %id, "final analysis rate limited");
            context.finish_run(run, AnalysisOutcome::RateLimited);
            release();
            return Ok(run_id);
        }
        let inner = Arc::clone(&self.inner);
        runtime.spawn(async move {
            context
                .execute_run(run, range, CancellationToken::new())
                .await;
            inner
                .finals
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&id);
        });
        tracing::info!(session = %id, run = %run_id, "final analysis started");
        Ok(run_id)
    }

    /// Starts incremental analysis of `session`, which has just started capturing, and
    /// publishes the provider in use (FR-050).
    ///
    /// `None` when no analysis provider is usable or outside a Tokio runtime: capture goes on
    /// without analysis.
    #[must_use]
    pub fn attach(&self, session: &Session) -> Option<SessionAnalysis> {
        let deps = &self.inner.deps;
        let id = session.id();
        let provider = match deps.providers.resolve_analyzer() {
            Ok(Some(provider)) => provider,
            Ok(None) => return None,
            Err(err) => {
                tracing::warn!(session = %id, error = %err, "analysis provider unresolved");
                return None;
            }
        };
        let analyzer = match deps.analyzers.create(&provider) {
            Ok(analyzer) => analyzer,
            Err(err) => {
                tracing::warn!(session = %id, provider = %provider.id(), error = %err, "analysis provider unavailable");
                return None;
            }
        };
        let runtime = Handle::try_current().ok()?;
        deps.events.publish(
            id,
            SessionEvent::ProviderInUse {
                stage: ProviderStage::Analysis,
                locality: provider.locality(),
                provider_id: provider.id(),
            },
        );
        let (tx, rx) = mpsc::channel(FEED_CAPACITY);
        let stop = CancellationToken::new();
        let cancel = CancellationToken::new();
        let scheduler = Scheduler {
            run: Arc::new(RunContext {
                inner: Arc::clone(&self.inner),
                session_id: id,
                // Incremental runs only start while the session captures; the scheduler
                // stops with capture.
                session_state: SessionState::Capturing,
                trigger: AnalysisTrigger::Incremental,
                language: session.language(),
                provider_id: provider.id(),
                max_segments: MAX_SEGMENTS_PER_RUN,
                analyzer,
            }),
            pending: Pending::default(),
            last_run_at: None,
            tokens_used: 0,
            limited_reported: false,
        };
        let handle = runtime.spawn(scheduler.run(rx, stop.clone(), cancel.clone()));
        tracing::info!(session = %id, provider = %provider.id(), "analysis attached");
        Some(SessionAnalysis {
            feed: AnalysisFeed { tx },
            stop,
            cancel,
            handle,
        })
    }
}

/// Non-blocking entry point for persisted finals.
#[derive(Debug, Clone)]
pub struct AnalysisFeed {
    tx: mpsc::Sender<FinalNotice>,
}

impl AnalysisFeed {
    /// Notes that the final segment `seq`, lasting `duration`, was persisted. Never waits: if
    /// the queue is full the notice is dropped, which only delays the next trigger (the run
    /// reads segments from storage).
    pub fn final_persisted(&self, seq: u64, duration: Duration) {
        let _ = self.tx.try_send(FinalNotice { seq, duration });
    }
}

/// Handle to one session's incremental analysis.
pub struct SessionAnalysis {
    feed: AnalysisFeed,
    stop: CancellationToken,
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

impl SessionAnalysis {
    /// Where the transcription reports persisted finals.
    #[must_use]
    pub fn feed(&self) -> AnalysisFeed {
        self.feed.clone()
    }

    /// Stops scheduling runs. A run in flight may finish for up to `grace`; then it is
    /// cancelled and recorded as `provider_error`. Never waits.
    pub fn finish(self, grace: std::time::Duration) {
        self.stop.cancel();
        let Self { cancel, handle, .. } = self;
        match Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    if tokio::time::timeout(grace, handle).await.is_err() {
                        cancel.cancel();
                    }
                });
            }
            Err(_) => cancel.cancel(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FinalNotice {
    seq: u64,
    duration: Duration,
}

/// Finals not yet analysed.
#[derive(Debug, Default)]
struct Pending {
    from: Option<u64>,
    to: u64,
    finals: u32,
    speech: Duration,
}

impl Pending {
    fn add(&mut self, notice: FinalNotice) {
        self.from = Some(self.from.map_or(notice.seq, |from| from.min(notice.seq)));
        self.to = self.to.max(notice.seq);
        self.finals = self.finals.saturating_add(1);
        self.speech = self.speech.saturating_add(notice.duration);
    }

    fn triggered(&self, limits: &AnalysisLimits) -> bool {
        self.from.is_some()
            && (self.finals >= limits.trigger_finals
                || self.speech >= Duration::from_secs(u64::from(limits.trigger_speech_secs)))
    }

    /// Puts a failed run's range back so the next run covers it again.
    fn retry(&mut self, (from, to): (u64, u64)) {
        self.from = Some(self.from.map_or(from, |current| current.min(from)));
        self.to = self.to.max(to);
    }
}

/// What a finished run tells the scheduler.
#[derive(Debug, Clone, Copy)]
struct RunResult {
    range: (u64, u64),
    succeeded: bool,
    tokens: u32,
}

/// Everything a run needs; shared by the scheduler and the run in flight.
struct RunContext {
    inner: Arc<Inner>,
    session_id: SessionId,
    /// Session state the runs are started in.
    session_state: SessionState,
    trigger: AnalysisTrigger,
    language: SessionLanguage,
    provider_id: ProviderId,
    /// Most final segments per request (the most recent ones win).
    max_segments: usize,
    analyzer: Arc<dyn AnalysisProvider>,
}

struct Scheduler {
    run: Arc<RunContext>,
    pending: Pending,
    last_run_at: Option<Timestamp>,
    tokens_used: u64,
    limited_reported: bool,
}

async fn finished(run: &mut Option<BoxFuture<'static, RunResult>>) -> RunResult {
    match run {
        Some(run) => run.await,
        None => std::future::pending().await,
    }
}

impl Scheduler {
    async fn run(
        mut self,
        mut feed: mpsc::Receiver<FinalNotice>,
        stop: CancellationToken,
        cancel: CancellationToken,
    ) {
        let mut tick = tokio::time::interval(self.run.inner.deps.tick);
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut in_flight: Option<BoxFuture<'static, RunResult>> = None;
        let mut stopping = false;
        loop {
            tokio::select! {
                () = stop.cancelled(), if !stopping => stopping = true,
                notice = feed.recv(), if !stopping => match notice {
                    Some(notice) => self.pending.add(notice),
                    None => stopping = true,
                },
                _ = tick.tick(), if !stopping => {}
                result = finished(&mut in_flight), if in_flight.is_some() => {
                    in_flight = None;
                    self.tokens_used = self.tokens_used.saturating_add(u64::from(result.tokens));
                    if !result.succeeded {
                        self.pending.retry(result.range);
                    }
                }
            }
            if stopping {
                if in_flight.is_none() {
                    return;
                }
                continue;
            }
            if in_flight.is_none()
                && let Some(range) = self.next_range()
            {
                in_flight = Some(Box::pin(
                    Arc::clone(&self.run).execute(range, cancel.child_token()),
                ));
            }
        }
    }

    /// The range of the run to start now, if any (research R7).
    fn next_range(&mut self) -> Option<(u64, u64)> {
        let deps = &self.run.inner.deps;
        let limits = match deps.settings.get() {
            Ok(settings) => settings.analysis_limits,
            Err(err) => {
                tracing::warn!(session = %self.run.session_id, error = %err, "analysis limits unavailable");
                return None;
            }
        };
        if !self.pending.triggered(&limits) {
            return None;
        }
        let now = deps.clock.now();
        let min_interval = Duration::from_secs(u64::from(limits.min_interval_secs));
        if self
            .last_run_at
            .is_some_and(|last| now.saturating_duration_since(last) < min_interval)
        {
            return None;
        }
        let from = self.pending.from?;
        let range = (from, self.pending.to);
        let token_cap = deps
            .settings
            .max_tokens_per_session(self.run.provider_id)
            .unwrap_or(limits.max_tokens_per_session);
        let allowed = self.tokens_used < u64::from(token_cap)
            && reserve_call(&self.run.inner.calls, now, limits.max_calls_per_hour);
        if !allowed {
            if !self.limited_reported {
                self.limited_reported = true;
                self.run.record_rate_limited(range, now);
            }
            return None;
        }
        self.limited_reported = false;
        self.last_run_at = Some(now);
        self.pending = Pending::default();
        Some(range)
    }
}

impl RunContext {
    fn publish(&self, event: SessionEvent) {
        self.inner.deps.events.publish(self.session_id, event);
    }

    fn publish_status(&self, run_id: AnalysisRunId, status: AnalysisStatus) {
        self.publish(SessionEvent::AnalysisStatus { run_id, status });
    }

    fn create_run(
        &self,
        range: (u64, u64),
        now: Timestamp,
    ) -> Result<AnalysisRun, ApplicationError> {
        let run = AnalysisRun::start(
            AnalysisRunId::new(),
            self.session_id,
            self.session_state,
            self.provider_id,
            self.trigger,
            range,
            now,
        )?;
        self.inner.deps.runs.insert(&run)?;
        Ok(run)
    }

    fn start_run(&self, range: (u64, u64), now: Timestamp) -> Option<AnalysisRun> {
        match self.create_run(range, now) {
            Ok(run) => Some(run),
            Err(err) => {
                tracing::error!(session = %self.session_id, error = %err, "analysis run not stored");
                None
            }
        }
    }

    fn finish_run(&self, mut run: AnalysisRun, outcome: AnalysisOutcome) {
        if run.finish(outcome, self.inner.deps.clock.now()).is_ok()
            && let Err(err) = self.inner.deps.runs.update(&run)
        {
            tracing::error!(session = %self.session_id, error = %err, "analysis run outcome not stored");
        }
        self.publish_status(
            run.id(),
            match outcome {
                AnalysisOutcome::Ok { created } => AnalysisStatus::Ok { created },
                AnalysisOutcome::InvalidOutput => AnalysisStatus::InvalidOutput,
                AnalysisOutcome::ProviderError => AnalysisStatus::ProviderError,
                AnalysisOutcome::RateLimited => AnalysisStatus::RateLimited,
            },
        );
        tracing::info!(session = %self.session_id, run = %run.id(), outcome = outcome.code(), created = outcome.created(), "analysis run finished");
    }

    fn record_rate_limited(&self, range: (u64, u64), now: Timestamp) {
        tracing::warn!(session = %self.session_id, "analysis rate limited");
        if let Some(run) = self.start_run(range, now) {
            self.finish_run(run, AnalysisOutcome::RateLimited);
        }
    }

    async fn execute(self: Arc<Self>, range: (u64, u64), cancel: CancellationToken) -> RunResult {
        let Some(run) = self.start_run(range, self.inner.deps.clock.now()) else {
            return RunResult {
                range,
                succeeded: false,
                tokens: 0,
            };
        };
        self.execute_run(run, range, cancel).await
    }

    async fn execute_run(
        &self,
        run: AnalysisRun,
        range: (u64, u64),
        cancel: CancellationToken,
    ) -> RunResult {
        let failed = RunResult {
            range,
            succeeded: false,
            tokens: 0,
        };
        self.publish_status(run.id(), AnalysisStatus::Running);
        let (request, finals) = match self.request(range) {
            Ok(request) => request,
            Err(err) => {
                tracing::error!(session = %self.session_id, error = %err, "analysis input unavailable");
                self.finish_run(run, AnalysisOutcome::ProviderError);
                return failed;
            }
        };
        if request.segments.is_empty() {
            self.finish_run(run, AnalysisOutcome::Ok { created: 0 });
            return RunResult {
                succeeded: true,
                ..failed
            };
        }
        match self.analyzer.analyze(request, cancel).await {
            Ok(output) => {
                let created = self.propose(&run, output.proposals, &finals);
                self.finish_run(run, AnalysisOutcome::Ok { created });
                RunResult {
                    range,
                    succeeded: true,
                    tokens: output.tokens_used,
                }
            }
            Err(ProviderError::InvalidOutput) => {
                tracing::warn!(session = %self.session_id, "analysis output discarded");
                self.finish_run(run, AnalysisOutcome::InvalidOutput);
                failed
            }
            Err(err) => {
                tracing::warn!(session = %self.session_id, error = %err, "analysis provider failed");
                self.finish_run(run, AnalysisOutcome::ProviderError);
                failed
            }
        }
    }

    /// The request for `range` and the ids of every final of the session.
    fn request(
        &self,
        (from, to): (u64, u64),
    ) -> Result<(AnalysisRequest, HashSet<SegmentId>), ApplicationError> {
        let deps = &self.inner.deps;
        let all = deps.segments.list_by_session(self.session_id)?;
        let finals: HashSet<SegmentId> = all
            .iter()
            .filter(|segment| segment.text().is_some())
            .map(|segment| segment.id())
            .collect();
        let mut segments: Vec<AnalysisSegment> = all
            .iter()
            .filter(|segment| (from..=to).contains(&segment.seq()))
            .filter_map(|segment| {
                Some(AnalysisSegment {
                    id: segment.id(),
                    source: segment.source(),
                    start: segment.start(),
                    text: segment.text()?.clone(),
                })
            })
            .collect();
        let excess = segments.len().saturating_sub(self.max_segments);
        segments.drain(..excess);
        let records = deps
            .records
            .list_by_session(self.session_id)?
            .iter()
            .map(summary)
            .collect();
        Ok((
            AnalysisRequest {
                session_id: self.session_id,
                language: self.language,
                segments,
                records,
            },
            finals,
        ))
    }

    /// Creates the proposals of `run`; returns how many were stored.
    fn propose(
        &self,
        run: &AnalysisRun,
        proposals: Vec<Proposal>,
        finals: &HashSet<SegmentId>,
    ) -> u32 {
        let deps = &self.inner.deps;
        let existing = match deps.records.list_by_session(self.session_id) {
            Ok(existing) => existing,
            Err(err) => {
                tracing::error!(session = %self.session_id, error = %err, "records unavailable");
                return 0;
            }
        };
        let known: HashSet<RecordId> = existing.iter().map(TechnicalRecord::id).collect();
        let mut contents: HashSet<String> =
            existing.iter().map(TechnicalRecord::dedup_key).collect();
        let origin = Origin::Ai {
            provider_id: self.provider_id,
            analysis_run_id: run.id(),
        };
        let now = deps.clock.now();
        let mut created = 0;
        for proposal in proposals {
            let proposal = proposal.grounded(|id| finals.contains(&id));
            if !contents.insert(proposal.dedup_key()) {
                // Same content already recorded: grouped with the existing record (FR-027).
                continue;
            }
            let mut flags = RecordFlags::default();
            if let Some(relation) = proposal
                .relates_to
                .filter(|relation| known.contains(&relation.record_id))
            {
                match relation.kind {
                    RelationKind::Duplicate => flags.duplicate_of = Some(relation.record_id),
                    RelationKind::Conflict => flags.conflicts_with = Some(relation.record_id),
                    RelationKind::Refines => {}
                }
            }
            let record = match TechnicalRecord::propose(
                RecordId::new(),
                self.session_id,
                proposal,
                origin,
                flags,
                now,
            ) {
                Ok(record) => record,
                Err(err) => {
                    tracing::warn!(session = %self.session_id, error = %err, "proposal rejected");
                    continue;
                }
            };
            if let Err(err) = deps.records.insert(&record, &record.creation()) {
                tracing::error!(session = %self.session_id, error = %err, "proposal not stored");
                break;
            }
            created += 1;
            self.publish(SessionEvent::RecordProposed(Box::new(record)));
        }
        created
    }
}

/// Summary of `record` for the provider, its text cut at [`SUMMARY_CHARS`].
pub(crate) fn summary(record: &TechnicalRecord) -> RecordSummary {
    let text = record.text().expose();
    let short = if text.as_str().chars().count() > SUMMARY_CHARS {
        let cut: String = text.as_str().chars().take(SUMMARY_CHARS).collect();
        RecordText::parse(format!("{}…", cut.trim_end())).unwrap_or_else(|_| text.clone())
    } else {
        text.clone()
    };
    RecordSummary {
        id: record.id(),
        kind: record.kind(),
        status: record.status(),
        text: Redacted::new(short),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: i64) -> Timestamp {
        Timestamp::from_unix_millis(millis).unwrap()
    }

    #[test]
    fn pending_triggers_on_finals_or_speech() {
        let limits = AnalysisLimits::default();
        let mut pending = Pending::default();
        assert!(!pending.triggered(&limits));
        for seq in 0..5 {
            pending.add(FinalNotice {
                seq,
                duration: Duration::from_secs(2),
            });
        }
        assert!(!pending.triggered(&limits));
        pending.add(FinalNotice {
            seq: 5,
            duration: Duration::from_secs(2),
        });
        assert!(pending.triggered(&limits));

        let mut long = Pending::default();
        long.add(FinalNotice {
            seq: 9,
            duration: Duration::from_secs(90),
        });
        assert!(long.triggered(&limits));
        assert_eq!((long.from, long.to), (Some(9), 9));
    }

    #[test]
    fn retry_widens_the_pending_range() {
        let mut pending = Pending::default();
        pending.add(FinalNotice {
            seq: 10,
            duration: Duration::ZERO,
        });
        pending.retry((3, 8));
        assert_eq!((pending.from, pending.to), (Some(3), 10));
    }

    #[test]
    fn hourly_cap_slides() {
        let calls = Mutex::new(VecDeque::new());
        assert!(reserve_call(&calls, at(0), 2));
        assert!(reserve_call(&calls, at(1_000), 2));
        assert!(!reserve_call(&calls, at(2_000), 2));
        assert!(reserve_call(&calls, at(3_600_000), 2));
    }
}
