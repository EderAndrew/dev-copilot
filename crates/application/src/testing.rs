//! In-memory fakes for every port, for use-case and integration tests (feature `test-support`).
//!
//! Fakes favour observability: each one records what happened so tests can assert on it.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::{AudioSourceKind, Availability, PermissionState};
use devcopilot_domain::ids::{ProviderId, SessionId};
use devcopilot_domain::provider::{CredentialRef, DataCategory, Locality, Provider};
use devcopilot_domain::session::Session;
use devcopilot_domain::time::{Duration, Timestamp};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::events::SessionEvent;
use crate::ports::{
    AnalysisProvider, AudioChunk, AudioChunkStream, AudioError, AudioSource, AuthorizationError,
    BoundedAudioSink, BoxFuture, CaptureHandle, Clock, EventPublisher, ProviderDescriptor,
    ProviderError, ProviderRepository, RemoteGate, RepositoryError, SecretStore, SecretStoreError,
    SessionRepository, SettingsRepository, SourceFault, SourceFaultNotifier, Transcriber,
    TranscriptSink, TranscriptUpdate,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------------------------
// Clock
// ---------------------------------------------------------------------------------------------

/// Manually driven clock.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<Timestamp>,
}

impl FakeClock {
    /// Starts at `unix_millis`; falls back to the epoch for out-of-range values.
    #[must_use]
    pub fn at(unix_millis: i64) -> Self {
        let start = Timestamp::from_unix_millis(unix_millis)
            .or_else(|_| Timestamp::from_unix_millis(0))
            .unwrap_or_else(|_| unreachable!("the epoch is always representable"));
        Self {
            now: Mutex::new(start),
        }
    }

    /// Moves time forward.
    pub fn advance(&self, by: Duration) {
        let mut now = lock(&self.now);
        *now = now.saturating_add(by);
    }

    /// Jumps to `instant`.
    pub fn set(&self, instant: Timestamp) {
        *lock(&self.now) = instant;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        // 2026-10-27T00:00:00Z
        Self::at(1_793_059_200_000)
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        *lock(&self.now)
    }
}

// ---------------------------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct CaptureState {
    starts: usize,
    stopped: Arc<AtomicBool>,
    cancel: Option<CancellationToken>,
    faults: Option<SourceFaultNotifier>,
}

/// Audio source whose permission, availability and captured chunks are scripted.
#[derive(Debug)]
pub struct ScriptedAudioSource {
    kind: AudioSourceKind,
    availability: Mutex<Availability>,
    permission: Mutex<PermissionState>,
    permission_after_request: Mutex<PermissionState>,
    start_error: Mutex<Option<AudioError>>,
    chunks: Mutex<Vec<AudioChunk>>,
    capture: Mutex<CaptureState>,
    permission_gate: Mutex<Option<Arc<Notify>>>,
    permission_requests: Mutex<usize>,
}

impl ScriptedAudioSource {
    /// Available source with permission granted and no chunks.
    #[must_use]
    pub fn granted(kind: AudioSourceKind) -> Self {
        Self {
            kind,
            availability: Mutex::new(Availability::Available),
            permission: Mutex::new(PermissionState::Granted),
            permission_after_request: Mutex::new(PermissionState::Granted),
            start_error: Mutex::new(None),
            chunks: Mutex::new(Vec::new()),
            capture: Mutex::new(CaptureState::default()),
            permission_gate: Mutex::new(None),
            permission_requests: Mutex::new(0),
        }
    }

    /// Makes `request_permission` wait, as if the OS prompt were open, until
    /// [`Self::answer_permission_prompt`].
    pub fn hold_permission_prompt(&self) {
        *lock(&self.permission_gate) = Some(Arc::new(Notify::new()));
    }

    /// Lets a held `request_permission` return.
    pub fn answer_permission_prompt(&self) {
        if let Some(gate) = lock(&self.permission_gate).take() {
            gate.notify_one();
        }
    }

    /// How many times the OS prompt was requested.
    #[must_use]
    pub fn permission_request_count(&self) -> usize {
        *lock(&self.permission_requests)
    }

    /// Simulates a fault of the running capture (device lost, default-device change, OS
    /// suspend/lock, permission revoked). Like a real backend, the stream stops delivering.
    pub fn trigger_fault(&self, fault: SourceFault) {
        let capture = lock(&self.capture);
        if let Some(faults) = &capture.faults {
            faults.notify(fault);
        }
        if fault == SourceFault::PermissionRevoked {
            *lock(&self.permission) = PermissionState::Revoked;
        }
    }

    /// Sets the permission reported now and the one returned by the next request.
    pub fn set_permission(&self, now: PermissionState, after_request: PermissionState) {
        *lock(&self.permission) = now;
        *lock(&self.permission_after_request) = after_request;
    }

    /// Sets availability.
    pub fn set_availability(&self, availability: Availability) {
        *lock(&self.availability) = availability;
    }

    /// Makes the next `start` calls fail with `error` (`None` clears it).
    pub fn fail_start_with(&self, error: Option<AudioError>) {
        *lock(&self.start_error) = error;
    }

    /// Chunks pushed into the sink on `start`.
    pub fn script_chunks(&self, chunks: Vec<AudioChunk>) {
        *lock(&self.chunks) = chunks;
    }

    /// How many times capture started.
    #[must_use]
    pub fn start_count(&self) -> usize {
        lock(&self.capture).starts
    }

    /// Whether a started capture is still open (not stopped, not cancelled).
    #[must_use]
    pub fn is_capturing(&self) -> bool {
        let capture = lock(&self.capture);
        capture.starts > 0
            && !capture.stopped.load(Ordering::SeqCst)
            && capture
                .cancel
                .as_ref()
                .is_some_and(|token| !token.is_cancelled())
    }
}

struct ScriptedCaptureHandle {
    stopped: Arc<AtomicBool>,
}

impl CaptureHandle for ScriptedCaptureHandle {
    fn stop(self: Box<Self>) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

impl Drop for ScriptedCaptureHandle {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
}

impl AudioSource for ScriptedAudioSource {
    fn kind(&self) -> AudioSourceKind {
        self.kind
    }

    fn availability(&self) -> Availability {
        *lock(&self.availability)
    }

    fn permission(&self) -> PermissionState {
        *lock(&self.permission)
    }

    fn request_permission(&self) -> BoxFuture<'_, PermissionState> {
        *lock(&self.permission_requests) += 1;
        let gate = lock(&self.permission_gate).clone();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            let next = *lock(&self.permission_after_request);
            *lock(&self.permission) = next;
            next
        })
    }

    fn start(
        &self,
        sink: BoundedAudioSink,
        cancel: CancellationToken,
        faults: SourceFaultNotifier,
    ) -> Result<Box<dyn CaptureHandle>, AudioError> {
        let availability = self.availability();
        if !availability.is_available() {
            return Err(AudioError::Unavailable(availability));
        }
        let permission = self.permission();
        if !permission.is_granted() {
            return Err(AudioError::PermissionNotGranted(permission));
        }
        if let Some(error) = lock(&self.start_error).clone() {
            return Err(error);
        }
        let stopped = Arc::new(AtomicBool::new(false));
        {
            let mut capture = lock(&self.capture);
            capture.starts += 1;
            capture.stopped = Arc::clone(&stopped);
            capture.cancel = Some(cancel);
            capture.faults = Some(faults);
        }
        for chunk in lock(&self.chunks).iter().cloned() {
            // Overload policy belongs to the pipeline; the fake just stops at capacity.
            if sink.try_send(chunk).is_err() {
                break;
            }
        }
        Ok(Box::new(ScriptedCaptureHandle { stopped }))
    }
}

// ---------------------------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------------------------

fn descriptor_for(
    provider_id: ProviderId,
    locality: Locality,
    categories: &[DataCategory],
) -> ProviderDescriptor {
    ProviderDescriptor {
        id: provider_id,
        locality,
        data_categories: categories.to_vec(),
    }
}

/// What a [`ScriptedTranscriber`] does after emitting its updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriberEnd {
    /// Waits until the input closes or the token is cancelled, then returns `Ok`.
    WaitForInput,
    /// Returns the error.
    Fail(ProviderError),
    /// Never returns and ignores cancellation (SC-006: controls must not wait for it).
    Hang,
}

/// Transcriber that emits scripted updates.
#[derive(Debug)]
pub struct ScriptedTranscriber {
    descriptor: ProviderDescriptor,
    updates: Mutex<VecDeque<TranscriptUpdate>>,
    end: Mutex<TranscriberEnd>,
    runs: Mutex<usize>,
}

impl ScriptedTranscriber {
    /// Local transcriber that emits `updates` then waits for input to close.
    #[must_use]
    pub fn local(updates: Vec<TranscriptUpdate>) -> Self {
        Self {
            descriptor: descriptor_for(ProviderId::new(), Locality::Local, &[DataCategory::Audio]),
            updates: Mutex::new(updates.into()),
            end: Mutex::new(TranscriberEnd::WaitForInput),
            runs: Mutex::new(0),
        }
    }

    /// Changes what happens after the updates.
    pub fn set_end(&self, end: TranscriberEnd) {
        *lock(&self.end) = end;
    }

    /// Number of `run` calls.
    #[must_use]
    pub fn run_count(&self) -> usize {
        *lock(&self.runs)
    }
}

impl Transcriber for ScriptedTranscriber {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    fn run(
        &self,
        mut input: AudioChunkStream,
        out: TranscriptSink,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), ProviderError>> {
        *lock(&self.runs) += 1;
        let updates: Vec<_> = lock(&self.updates).drain(..).collect();
        let end = lock(&self.end).clone();
        Box::pin(async move {
            for update in updates {
                if out.send(update).await.is_err() {
                    return Ok(());
                }
            }
            match end {
                TranscriberEnd::Fail(error) => Err(error),
                TranscriberEnd::Hang => std::future::pending().await,
                TranscriberEnd::WaitForInput => loop {
                    tokio::select! {
                        () = cancel.cancelled() => return Err(ProviderError::Cancelled),
                        chunk = input.recv() => if chunk.is_none() { return Ok(()) },
                    }
                },
            }
        })
    }
}

/// Analysis provider fake (behaviour scripted by US3/US5 tests as methods are added).
#[derive(Debug)]
pub struct ScriptedAnalysisProvider {
    descriptor: ProviderDescriptor,
}

impl ScriptedAnalysisProvider {
    /// Local analysis provider.
    #[must_use]
    pub fn local() -> Self {
        Self {
            descriptor: descriptor_for(
                ProviderId::new(),
                Locality::Local,
                &[
                    DataCategory::Transcript,
                    DataCategory::Records,
                    DataCategory::Questions,
                ],
            ),
        }
    }
}

impl AnalysisProvider for ScriptedAnalysisProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }
}

/// Remote gate allowing exactly the granted `(provider, category)` pairs; records every check.
#[derive(Debug, Default)]
pub struct FakeRemoteGate {
    granted: Mutex<BTreeMap<ProviderId, Vec<DataCategory>>>,
    checks: Mutex<Vec<(ProviderId, Vec<DataCategory>)>>,
}

impl FakeRemoteGate {
    /// Grants `categories` to `provider`.
    pub fn grant(&self, provider: ProviderId, categories: &[DataCategory]) {
        lock(&self.granted).insert(provider, categories.to_vec());
    }

    /// Revokes everything for `provider`.
    pub fn revoke(&self, provider: ProviderId) {
        lock(&self.granted).remove(&provider);
    }

    /// Every check performed, in order.
    #[must_use]
    pub fn checks(&self) -> Vec<(ProviderId, Vec<DataCategory>)> {
        lock(&self.checks).clone()
    }
}

impl RemoteGate for FakeRemoteGate {
    fn check(
        &self,
        provider: &ProviderId,
        categories: &[DataCategory],
    ) -> Result<(), AuthorizationError> {
        lock(&self.checks).push((*provider, categories.to_vec()));
        let granted = lock(&self.granted);
        match granted.get(provider) {
            Some(allowed) if categories.iter().all(|c| allowed.contains(c)) => Ok(()),
            _ => Err(AuthorizationError::NotAuthorized),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------------------------

/// In-memory session repository.
#[derive(Debug, Default)]
pub struct InMemorySessionRepository {
    sessions: Mutex<Vec<Session>>,
    interrupted_gaps: Mutex<Vec<SessionId>>,
    fail_updates: Mutex<Option<RepositoryError>>,
}

impl InMemorySessionRepository {
    /// Stores `session` as is (e.g. a session left active by a crash).
    pub fn seed(&self, session: Session) {
        lock(&self.sessions).push(session);
    }

    /// Makes `update` and `save_interrupted` fail with `error` (`None` restores them).
    pub fn fail_updates_with(&self, error: Option<RepositoryError>) {
        *lock(&self.fail_updates) = error;
    }

    /// Sessions that received a trailing `gap(interrupted)`, in order.
    #[must_use]
    pub fn interrupted_gaps(&self) -> Vec<SessionId> {
        lock(&self.interrupted_gaps).clone()
    }

    fn replace(&self, session: &Session) -> Result<(), RepositoryError> {
        if let Some(error) = lock(&self.fail_updates).clone() {
            return Err(error);
        }
        let mut sessions = lock(&self.sessions);
        let stored = sessions
            .iter_mut()
            .find(|s| s.id() == session.id())
            .ok_or(RepositoryError::NotFound)?;
        *stored = session.clone();
        Ok(())
    }
}

impl SessionRepository for InMemorySessionRepository {
    fn create(&self, session: &Session) -> Result<(), RepositoryError> {
        lock(&self.sessions).push(session.clone());
        Ok(())
    }

    fn get(&self, id: SessionId) -> Result<Session, RepositoryError> {
        lock(&self.sessions)
            .iter()
            .find(|s| s.id() == id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    fn update(&self, session: &Session) -> Result<(), RepositoryError> {
        self.replace(session)
    }

    fn list(&self) -> Result<Vec<Session>, RepositoryError> {
        let mut sessions = lock(&self.sessions).clone();
        sessions.sort_by_key(|s| std::cmp::Reverse((s.created_at(), s.id())));
        Ok(sessions)
    }

    fn list_active(&self) -> Result<Vec<Session>, RepositoryError> {
        Ok(lock(&self.sessions)
            .iter()
            .filter(|s| s.state().is_active())
            .cloned()
            .collect())
    }

    fn save_interrupted(&self, session: &Session) -> Result<(), RepositoryError> {
        self.replace(session)?;
        if session.started_at().is_some() {
            lock(&self.interrupted_gaps).push(session.id());
        }
        Ok(())
    }
}

/// In-memory provider repository.
#[derive(Debug, Default)]
pub struct InMemoryProviderRepository {
    providers: Mutex<Vec<Provider>>,
    fail_with: Mutex<Option<RepositoryError>>,
}

impl InMemoryProviderRepository {
    /// Makes every call fail with `error` (`None` restores normal behaviour).
    pub fn fail_with(&self, error: Option<RepositoryError>) {
        *lock(&self.fail_with) = error;
    }

    fn check(&self) -> Result<(), RepositoryError> {
        lock(&self.fail_with).clone().map_or(Ok(()), Err)
    }
}

impl ProviderRepository for InMemoryProviderRepository {
    fn list(&self) -> Result<Vec<Provider>, RepositoryError> {
        self.check()?;
        Ok(lock(&self.providers).clone())
    }

    fn get(&self, id: ProviderId) -> Result<Provider, RepositoryError> {
        self.check()?;
        lock(&self.providers)
            .iter()
            .find(|p| p.id() == id)
            .cloned()
            .ok_or(RepositoryError::NotFound)
    }

    fn upsert(&self, provider: &Provider) -> Result<(), RepositoryError> {
        self.check()?;
        let mut providers = lock(&self.providers);
        match providers.iter_mut().find(|p| p.id() == provider.id()) {
            Some(existing) => *existing = provider.clone(),
            None => providers.push(provider.clone()),
        }
        Ok(())
    }
}

/// In-memory settings repository.
#[derive(Debug, Default)]
pub struct InMemorySettingsRepository {
    values: Mutex<BTreeMap<String, String>>,
}

impl InMemorySettingsRepository {
    /// Raw stored values, for assertions.
    #[must_use]
    pub fn snapshot(&self) -> BTreeMap<String, String> {
        lock(&self.values).clone()
    }
}

impl SettingsRepository for InMemorySettingsRepository {
    fn get(&self, key: &str) -> Result<Option<String>, RepositoryError> {
        Ok(lock(&self.values).get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), RepositoryError> {
        lock(&self.values).insert(key.to_owned(), value.to_owned());
        Ok(())
    }
}

/// Secret store that can be switched off to simulate an unavailable OS vault (FR-039a).
#[derive(Debug)]
pub struct ToggleableSecretStore {
    available: AtomicBool,
    secrets: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl ToggleableSecretStore {
    /// Available, empty store.
    #[must_use]
    pub fn available() -> Self {
        Self {
            available: AtomicBool::new(true),
            secrets: Mutex::new(BTreeMap::new()),
        }
    }

    /// Unavailable store.
    #[must_use]
    pub fn unavailable() -> Self {
        let store = Self::available();
        store.set_available(false);
        store
    }

    /// Switches availability.
    pub fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::SeqCst);
    }

    /// Whether a secret exists under `reference` (ignores availability).
    #[must_use]
    pub fn contains(&self, reference: &CredentialRef) -> bool {
        lock(&self.secrets).contains_key(reference.as_str())
    }
}

impl SecretStore for ToggleableSecretStore {
    fn check_available(&self) -> Result<(), SecretStoreError> {
        if self.available.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(SecretStoreError::Unavailable)
        }
    }

    fn get(
        &self,
        reference: &CredentialRef,
    ) -> Result<Option<Redacted<Vec<u8>>>, SecretStoreError> {
        self.check_available()?;
        Ok(lock(&self.secrets)
            .get(reference.as_str())
            .cloned()
            .map(Redacted::new))
    }

    fn set(
        &self,
        reference: &CredentialRef,
        secret: &Redacted<Vec<u8>>,
    ) -> Result<(), SecretStoreError> {
        self.check_available()?;
        lock(&self.secrets).insert(reference.as_str().to_owned(), secret.expose().clone());
        Ok(())
    }

    fn delete(&self, reference: &CredentialRef) -> Result<(), SecretStoreError> {
        self.check_available()?;
        lock(&self.secrets).remove(reference.as_str());
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------------------------

/// Publisher that records every event.
#[derive(Debug, Default)]
pub struct RecordingEventPublisher {
    events: Mutex<Vec<(SessionId, SessionEvent)>>,
}

impl RecordingEventPublisher {
    /// Every event published so far.
    #[must_use]
    pub fn events(&self) -> Vec<(SessionId, SessionEvent)> {
        lock(&self.events).clone()
    }

    /// Events published for one session.
    #[must_use]
    pub fn events_for(&self, session: SessionId) -> Vec<SessionEvent> {
        lock(&self.events)
            .iter()
            .filter(|(id, _)| *id == session)
            .map(|(_, event)| event.clone())
            .collect()
    }
}

impl EventPublisher for RecordingEventPublisher {
    fn publish(&self, session: SessionId, event: SessionEvent) {
        lock(&self.events).push((session, event));
    }
}

#[cfg(test)]
mod tests {
    use devcopilot_domain::text::SegmentText;

    use super::*;

    fn chunk(start_ms: u64) -> AudioChunk {
        AudioChunk {
            source: AudioSourceKind::Microphone,
            start: Duration::from_millis(start_ms),
            samples: vec![0.0; 160],
        }
    }

    #[test]
    fn clock_advances() {
        let clock = FakeClock::at(1_000);
        clock.advance(Duration::from_secs(2));
        assert_eq!(clock.now().unix_millis(), 3_000);
    }

    #[test]
    fn audio_source_refuses_without_permission_and_tracks_capture() {
        let source = ScriptedAudioSource::granted(AudioSourceKind::Microphone);
        source.set_permission(PermissionState::Denied, PermissionState::Denied);
        let (sink, _rx) = BoundedAudioSink::channel(4);
        let (faults, _fault_rx) = SourceFaultNotifier::channel();
        assert_eq!(
            source
                .start(sink.clone(), CancellationToken::new(), faults.clone())
                .err(),
            Some(AudioError::PermissionNotGranted(PermissionState::Denied))
        );
        assert_eq!(source.start_count(), 0);

        source.set_permission(PermissionState::Granted, PermissionState::Granted);
        source.script_chunks(vec![chunk(0), chunk(10)]);
        let cancel = CancellationToken::new();
        let handle = source.start(sink, cancel.clone(), faults).unwrap();
        assert!(source.is_capturing());
        cancel.cancel();
        assert!(!source.is_capturing());
        handle.stop();
    }

    #[tokio::test]
    async fn audio_source_pushes_scripted_chunks_up_to_capacity() {
        let source = ScriptedAudioSource::granted(AudioSourceKind::Microphone);
        source.script_chunks(vec![chunk(0), chunk(10), chunk(20)]);
        let (sink, mut rx) = BoundedAudioSink::channel(2);
        let (faults, _fault_rx) = SourceFaultNotifier::channel();
        let _handle = source
            .start(sink, CancellationToken::new(), faults)
            .unwrap();
        assert_eq!(rx.recv().await.map(|c| c.start.as_millis()), Some(0));
        assert_eq!(rx.recv().await.map(|c| c.start.as_millis()), Some(10));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn audio_source_reports_scripted_faults_once() {
        let source = ScriptedAudioSource::granted(AudioSourceKind::Microphone);
        let (sink, _rx) = BoundedAudioSink::channel(2);
        let (faults, fault_rx) = SourceFaultNotifier::channel();
        let _handle = source
            .start(sink, CancellationToken::new(), faults)
            .unwrap();
        source.trigger_fault(SourceFault::PermissionRevoked);
        source.trigger_fault(SourceFault::Lost);
        assert_eq!(fault_rx.await, Ok(SourceFault::PermissionRevoked));
        assert_eq!(source.permission(), PermissionState::Revoked);
    }

    #[tokio::test]
    async fn transcriber_emits_updates_then_honours_cancel() {
        let update = TranscriptUpdate::Final {
            source: AudioSourceKind::Microphone,
            start: Duration::ZERO,
            end: Duration::from_secs(1),
            text: Redacted::new(SegmentText::parse("olá").unwrap()),
        };
        let transcriber = Arc::new(ScriptedTranscriber::local(vec![update.clone()]));
        let (_sink, input) = BoundedAudioSink::channel(1);
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task = tokio::spawn(async move { transcriber.run(input, tx, token).await });
        assert_eq!(rx.recv().await, Some(update));
        cancel.cancel();
        assert_eq!(task.await.unwrap(), Err(ProviderError::Cancelled));
    }

    #[test]
    fn secret_store_toggles() {
        let store = ToggleableSecretStore::available();
        let reference = CredentialRef::parse("db:key").unwrap();
        store.set(&reference, &Redacted::new(vec![1, 2])).unwrap();
        store.set_available(false);
        assert_eq!(store.get(&reference), Err(SecretStoreError::Unavailable));
        assert!(store.contains(&reference));
    }

    #[test]
    fn remote_gate_requires_every_category() {
        let gate = FakeRemoteGate::default();
        let provider = ProviderId::new();
        assert!(gate.check(&provider, &[DataCategory::Transcript]).is_err());
        gate.grant(provider, &[DataCategory::Transcript]);
        assert!(gate.check(&provider, &[DataCategory::Transcript]).is_ok());
        assert!(
            gate.check(
                &provider,
                &[DataCategory::Transcript, DataCategory::Records]
            )
            .is_err()
        );
        assert_eq!(gate.checks().len(), 3);
    }
}
