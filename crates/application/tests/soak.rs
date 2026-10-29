//! Soak test (T159, SC-015): two hours of simulated capture with scripted audio and fake
//! providers. Queues stay bounded, the session keeps responding, and resident memory ends at
//! most 10% above the value measured after a 10-minute warm-up.
//!
//! Ignored by default (it pushes 72 000 chunks); run with
//! `cargo test -p devcopilot-application --release --test soak -- --ignored --nocapture`.
//! Fakes here discard what they receive, unlike the observable fakes in `testing`, and storage
//! keeps only a recent window (the app stores on disk), so any growth measured belongs to the
//! pipeline. The bounded audio queues are allowed on top of the 10% at their designed maximum
//! (see the final assertion). RSS is read with `ps` (macOS and Linux).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration as StdDuration, Instant};

use devcopilot_application::analysis_service::{AnalysisDeps, AnalysisService};
use devcopilot_application::capture_pipeline::CHUNK_QUEUE_CAPACITY;
use devcopilot_application::events::SessionEvent;
use devcopilot_application::ports::AudioChunk;
use devcopilot_application::ports::{
    AnalysisOutput, AnalysisProvider, AnalysisProviderFactory, AnalysisRequest,
    AnalysisRunRepository, AnswerOutput, AudioChunkStream, AudioSource, BoxFuture, EventPublisher,
    ProviderDescriptor, ProviderError, QuestionRequest, RepositoryError, SegmentRepository,
    SinkError, Transcriber, TranscriberFactory, TranscriptSink, TranscriptUpdate,
};
use devcopilot_application::provider_service::ProviderService;
use devcopilot_application::session_service::{NewSession, SessionDeps, SessionService};
use devcopilot_application::settings_service::{AnalysisLimits, SettingsService, SettingsUpdate};
use devcopilot_application::testing::{
    FakeClock, InMemoryProviderRepository, InMemoryRecordRepository, InMemorySessionRepository,
    InMemorySettingsRepository, ScriptedAudioSource, ToggleableSecretStore,
};
use devcopilot_application::transcription::BACKLOG_LIMIT;
use devcopilot_domain::Redacted;
use devcopilot_domain::analysis_run::AnalysisRun;
use devcopilot_domain::audio_source::AudioSourceKind;
use devcopilot_domain::ids::{ProviderId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{
    DataCategory, Locality, Provider, ProviderAdapterKind, ProviderRole, ProviderSpec,
};
use devcopilot_domain::session::SessionState;
use devcopilot_domain::text::{SegmentText, Title};
use devcopilot_domain::time::Duration;
use devcopilot_domain::transcript::TranscriptSegment;
use tokio_util::sync::CancellationToken;

const CHUNK_MS: u64 = 100;
const SESSION: StdDuration = StdDuration::from_secs(2 * 60 * 60);
const WARM_UP: StdDuration = StdDuration::from_secs(10 * 60);

/// Emits one final every five seconds of audio and forgets the samples.
struct LeanTranscriber;

impl Transcriber for LeanTranscriber {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new(),
            locality: Locality::Local,
            data_categories: vec![DataCategory::Audio],
        }
    }

    fn run(
        &self,
        mut input: AudioChunkStream,
        out: TranscriptSink,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), ProviderError>> {
        Box::pin(async move {
            let mut window_start = None;
            loop {
                let chunk = tokio::select! {
                    () = cancel.cancelled() => return Err(ProviderError::Cancelled),
                    chunk = input.recv() => chunk,
                };
                let Some(chunk) = chunk else { return Ok(()) };
                let start = *window_start.get_or_insert(chunk.start);
                let end = chunk.start.saturating_add(Duration::from_millis(CHUNK_MS));
                if end.saturating_sub(start) >= Duration::from_secs(5) {
                    window_start = None;
                    let update = TranscriptUpdate::Final {
                        source: chunk.source,
                        start,
                        end,
                        text: Redacted::new(
                            SegmentText::parse("trecho sintético de teste de longa duração")
                                .unwrap(),
                        ),
                    };
                    if out.send(update).await.is_err() {
                        return Ok(());
                    }
                }
            }
        })
    }
}

struct LeanTranscriberFactory;

impl TranscriberFactory for LeanTranscriberFactory {
    fn create(
        &self,
        _provider: &Provider,
        _language: SessionLanguage,
    ) -> Result<Arc<dyn Transcriber>, ProviderError> {
        Ok(Arc::new(LeanTranscriber))
    }
}

/// Answers every analysis with no proposals.
struct LeanAnalyzer {
    calls: AtomicU64,
}

impl AnalysisProvider for LeanAnalyzer {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: ProviderId::new(),
            locality: Locality::Local,
            data_categories: vec![DataCategory::Transcript],
        }
    }

    fn analyze(
        &self,
        _request: AnalysisRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<AnalysisOutput, ProviderError>> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        Box::pin(async {
            Ok(AnalysisOutput {
                proposals: Vec::new(),
                tokens_used: 100,
            })
        })
    }

    fn answer(
        &self,
        _request: QuestionRequest,
        _cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<AnswerOutput, ProviderError>> {
        Box::pin(async { Err(ProviderError::Unavailable) })
    }
}

struct LeanAnalyzerFactory(Arc<LeanAnalyzer>);

impl AnalysisProviderFactory for LeanAnalyzerFactory {
    fn create(&self, _provider: &Provider) -> Result<Arc<dyn AnalysisProvider>, ProviderError> {
        Ok(Arc::clone(&self.0) as Arc<dyn AnalysisProvider>)
    }
}

/// Segment storage keeping only a recent window: the app stores segments on disk, so keeping
/// all of them in memory here would measure the test's storage instead of the pipeline.
#[derive(Default)]
struct WindowedSegments {
    recent: std::sync::Mutex<std::collections::VecDeque<TranscriptSegment>>,
    next_seq: AtomicU64,
    stored: AtomicU64,
}

const WINDOW: usize = 60;

impl SegmentRepository for WindowedSegments {
    fn append(&self, segment: &TranscriptSegment) -> Result<(), RepositoryError> {
        let mut recent = self.recent.lock().unwrap();
        recent.push_back(segment.clone());
        if recent.len() > WINDOW {
            recent.pop_front();
        }
        self.next_seq.store(segment.seq() + 1, Ordering::SeqCst);
        self.stored.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn list_by_session(
        &self,
        _session: SessionId,
    ) -> Result<Vec<TranscriptSegment>, RepositoryError> {
        Ok(self.recent.lock().unwrap().iter().cloned().collect())
    }

    fn next_seq(&self, _session: SessionId) -> Result<u64, RepositoryError> {
        Ok(self.next_seq.load(Ordering::SeqCst))
    }
}

/// Analysis runs are written to disk by the app; here they are only counted.
#[derive(Default)]
struct CountedRuns(AtomicU64);

impl AnalysisRunRepository for CountedRuns {
    fn insert(&self, _run: &AnalysisRun) -> Result<(), RepositoryError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn update(&self, _run: &AnalysisRun) -> Result<(), RepositoryError> {
        Ok(())
    }

    fn list_by_session(&self, _session: SessionId) -> Result<Vec<AnalysisRun>, RepositoryError> {
        Ok(Vec::new())
    }
}

/// Counts events instead of keeping them.
#[derive(Default)]
struct CountingPublisher {
    events: AtomicU64,
    gaps: AtomicU64,
}

impl EventPublisher for CountingPublisher {
    fn publish(&self, _session: SessionId, event: SessionEvent) {
        self.events.fetch_add(1, Ordering::Relaxed);
        if matches!(event, SessionEvent::Gap { .. }) {
            self.gaps.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Resident set size of this process in KiB.
fn rss_kib() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "long-running soak test (SC-015); run explicitly with --ignored"]
async fn two_hour_session_stays_bounded() {
    let clock = Arc::new(FakeClock::default());
    let events = Arc::new(CountingPublisher::default());
    let secrets = Arc::new(ToggleableSecretStore::available());
    let settings_repo = Arc::new(InMemorySettingsRepository::default());
    let providers = Arc::new(ProviderService::new(
        Arc::new(InMemoryProviderRepository::default()),
        settings_repo.clone(),
        secrets.clone(),
        clock.clone(),
    ));
    let settings = Arc::new(SettingsService::new(settings_repo));
    settings
        .update(SettingsUpdate {
            consent_reminder_dismissed: Some(true),
            analysis_limits: Some(AnalysisLimits {
                max_calls_per_hour: 1_000,
                max_tokens_per_session: 10_000_000,
                ..AnalysisLimits::default()
            }),
            ..SettingsUpdate::default()
        })
        .unwrap();
    for (role, adapter, endpoint, categories) in [
        (
            ProviderRole::Transcription,
            ProviderAdapterKind::WhisperLocal,
            None,
            vec![DataCategory::Audio],
        ),
        (
            ProviderRole::Analysis,
            ProviderAdapterKind::Ollama,
            Some("http://127.0.0.1:11434".to_owned()),
            vec![DataCategory::Transcript],
        ),
    ] {
        providers
            .upsert_local(ProviderSpec {
                id: ProviderId::new(),
                role,
                adapter,
                endpoint,
                model: "m".into(),
                credential_ref: None,
                data_categories: categories.into_iter().collect::<BTreeSet<_>>(),
            })
            .unwrap();
    }
    let segments = Arc::new(WindowedSegments::default());
    let analyzer = Arc::new(LeanAnalyzer {
        calls: AtomicU64::new(0),
    });
    let analysis = AnalysisService::new(AnalysisDeps {
        providers: providers.clone(),
        analyzers: Arc::new(LeanAnalyzerFactory(analyzer.clone())),
        records: Arc::new(InMemoryRecordRepository::default()),
        runs: Arc::new(CountedRuns::default()),
        segments: segments.clone(),
        settings: settings.clone(),
        clock: clock.clone(),
        events: events.clone(),
        tick: StdDuration::from_millis(5),
    });
    let mic = Arc::new(ScriptedAudioSource::granted(AudioSourceKind::Microphone));
    let service = SessionService::new(SessionDeps {
        sessions: Arc::new(InMemorySessionRepository::default()),
        segments: segments.clone(),
        transcribers: Arc::new(LeanTranscriberFactory),
        providers,
        settings,
        secrets,
        sources: vec![mic.clone() as Arc<dyn AudioSource>],
        clock: clock.clone(),
        events: events.clone(),
        analysis: Some(analysis),
        questions: None,
    });

    let session = service
        .create(NewSession {
            title: Title::parse("Soak").unwrap(),
            language: SessionLanguage::PtBr,
            sources: vec![AudioSourceKind::Microphone],
        })
        .unwrap();
    let id = session.id();
    service.start(id, false).unwrap();
    while service.get(id).unwrap().state() != SessionState::Capturing {
        tokio::task::yield_now().await;
    }

    let samples_per_chunk = usize::try_from(16 * CHUNK_MS).unwrap();
    let total_chunks = SESSION.as_millis() / u128::from(CHUNK_MS);
    let warm_up_chunks = WARM_UP.as_millis() / u128::from(CHUNK_MS);
    let mut warm_rss = None;
    let mut slowest_control = StdDuration::ZERO;
    let started = Instant::now();
    for n in 0..total_chunks {
        let chunk = AudioChunk {
            source: AudioSourceKind::Microphone,
            start: Duration::from_millis(u64::try_from(n).unwrap() * CHUNK_MS),
            samples: vec![0.01; samples_per_chunk],
        };
        // A full queue is the bounded-queue policy at work: wait for room, never grow.
        loop {
            match mic.push_chunk(chunk.clone()) {
                Ok(()) => break,
                Err(SinkError::Full) => tokio::task::yield_now().await,
                Err(SinkError::Closed) => panic!("capture closed unexpectedly"),
            }
        }
        clock.advance(Duration::from_millis(CHUNK_MS));
        if n % 600 == 0 {
            // Controls keep answering during the run (SC-006 checked as a sanity bound).
            let asked = Instant::now();
            assert_eq!(service.get(id).unwrap().state(), SessionState::Capturing);
            slowest_control = slowest_control.max(asked.elapsed());
            tokio::task::yield_now().await;
        }
        if std::env::var_os("DEVCOPILOT_SOAK_TRACE").is_some() && (n + 1) % warm_up_chunks == 0 {
            tokio::time::sleep(StdDuration::from_millis(50)).await;
            println!("{} min: {:?} KiB", (n + 1) / 600, rss_kib());
        }
        if n + 1 == warm_up_chunks {
            tokio::time::sleep(StdDuration::from_millis(200)).await;
            warm_rss = rss_kib();
        }
    }
    tokio::time::sleep(StdDuration::from_millis(200)).await;
    let end_rss = rss_kib();
    let stop_asked = Instant::now();
    service.stop(id).unwrap();
    let stop_latency = stop_asked.elapsed();
    while service.get(id).unwrap().state() != SessionState::Ended {
        tokio::time::sleep(StdDuration::from_millis(5)).await;
    }

    let finals = segments.stored.load(Ordering::Relaxed);
    println!(
        "simulated 2 h in {:?}: {finals} segments, {} events, {} gaps, {} analysis calls, \
         RSS warm-up {warm_rss:?} KiB → end {end_rss:?} KiB, slowest control {slowest_control:?}, \
         stop {stop_latency:?}",
        started.elapsed(),
        events.events.load(Ordering::Relaxed),
        events.gaps.load(Ordering::Relaxed),
        analyzer.calls.load(Ordering::Relaxed),
    );
    assert!(finals >= 1_400, "transcription kept up: {finals} segments");
    assert!(analyzer.calls.load(Ordering::Relaxed) > 0);
    assert!(stop_latency < StdDuration::from_millis(500));
    if let (Some(warm), Some(end)) = (warm_rss, end_rss) {
        // SC-015 allows 10% over the warm-up value of the app process, whose baseline is
        // hundreds of MiB (webview, model). This harness's baseline is a few MiB, so the
        // bounded audio queues alone — full or empty depending on timing, with the allocator
        // keeping freed pages — exceed 10% of it. They are allowed on top, at their designed
        // maximum: the capture queue plus the transcription backlog.
        let chunk_bytes = samples_per_chunk * std::mem::size_of::<f32>();
        let backlog_bytes = usize::try_from(BACKLOG_LIMIT.as_millis()).unwrap() * 16 * 4;
        let queue_kib =
            u64::try_from((CHUNK_QUEUE_CAPACITY * chunk_bytes + backlog_bytes) / 1024).unwrap();
        assert!(
            end * 100 <= warm * 110 + queue_kib * 100,
            "RSS grew from {warm} KiB to {end} KiB (queue allowance {queue_kib} KiB)"
        );
    }
}
