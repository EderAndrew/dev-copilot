//! Ports implemented by infrastructure crates (contracts/provider-ports.md).
//!
//! Vendor types never cross these traits (Constitution Art. VI.2). Ports used across `.await`
//! points return [`BoxFuture`] so they stay object-safe and can be composed as `Arc<dyn _>`.
//!
//! Repository ports for entities introduced by user stories grow with those stories:
//! `RecordRepository` (T088),
//! `QuestionRepository` (T113); `AnalysisProvider::analyze`/`answer` arrive with T089–T091 and
//! T114; authorizations on `ProviderRepository` with T135.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};

use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::{AudioSourceKind, Availability, PermissionState};
use devcopilot_domain::ids::{ProviderId, SessionId};
use devcopilot_domain::language::SessionLanguage;
use devcopilot_domain::provider::{CredentialRef, DataCategory, Locality, Provider};
use devcopilot_domain::session::Session;
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::{Duration, Timestamp};
use devcopilot_domain::transcript::TranscriptSegment;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::events::SessionEvent;

/// Boxed `Send` future returned by asynchronous ports.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// ---------------------------------------------------------------------------------------------
// Audio
// ---------------------------------------------------------------------------------------------

/// A block of 16 kHz mono `f32` samples from one source.
#[derive(Clone, PartialEq)]
pub struct AudioChunk {
    /// Source that produced the samples.
    pub source: AudioSourceKind,
    /// Offset of the first sample from the start of capture.
    pub start: Duration,
    /// Samples at 16 kHz, mono, in `[-1.0, 1.0]`.
    pub samples: Vec<f32>,
}

impl std::fmt::Debug for AudioChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioChunk")
            .field("source", &self.source)
            .field("start", &self.start)
            .field("samples", &self.samples.len())
            .finish()
    }
}

/// Sample rate of every [`AudioChunk`].
pub const SAMPLE_RATE_HZ: u32 = 16_000;

/// Why a chunk could not be handed to the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SinkError {
    /// The bounded queue is full; the caller applies the overload policy (FR-019).
    #[error("audio sink is full")]
    Full,
    /// The pipeline is gone (capture stopped).
    #[error("audio sink is closed")]
    Closed,
}

/// Bounded, non-blocking entry point of the capture pipeline (Art. V.3).
#[derive(Debug, Clone)]
pub struct BoundedAudioSink {
    tx: mpsc::Sender<AudioChunk>,
}

impl BoundedAudioSink {
    /// Creates a sink and the receiving end with room for `capacity` chunks.
    #[must_use]
    pub fn channel(capacity: usize) -> (Self, AudioChunkStream) {
        let (tx, rx) = mpsc::channel(capacity.max(1));
        (Self { tx }, rx)
    }

    /// Hands a chunk over without waiting.
    ///
    /// # Errors
    ///
    /// [`SinkError::Full`] when the queue is at capacity, [`SinkError::Closed`] when the
    /// pipeline has stopped.
    pub fn try_send(&self, chunk: AudioChunk) -> Result<(), SinkError> {
        self.tx.try_send(chunk).map_err(|err| match err {
            mpsc::error::TrySendError::Full(_) => SinkError::Full,
            mpsc::error::TrySendError::Closed(_) => SinkError::Closed,
        })
    }
}

/// Receiving end of the audio queue consumed by the transcriber.
pub type AudioChunkStream = mpsc::Receiver<AudioChunk>;

/// Keeps a started capture alive; dropping it or calling [`CaptureHandle::stop`] closes the
/// OS stream.
pub trait CaptureHandle: Send {
    /// Closes the stream immediately.
    fn stop(self: Box<Self>);
}

/// Failures of an audio source.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AudioError {
    /// Permission not granted; nothing was captured.
    #[error("audio permission not granted ({})", .0.code())]
    PermissionNotGranted(PermissionState),
    /// The source cannot be used.
    #[error("audio source unavailable")]
    Unavailable(Availability),
    /// The selected device disappeared; capture never switches devices silently (FR-011).
    #[error("audio device lost")]
    DeviceLost,
    /// Platform backend error, described without session content.
    #[error("audio backend error: {0}")]
    Backend(String),
}

/// Why a running capture ended on its own. Capture never resumes or switches device after a
/// fault (FR-011); the user starts again explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFault {
    /// The device disappeared, the default device changed, or the OS suspended or locked the
    /// session.
    Lost,
    /// The OS permission was withdrawn while capturing (FR-010).
    PermissionRevoked,
}

/// One-shot fault report from a running capture to the pipeline.
///
/// Cloneable and callable from any thread (e.g. a platform error callback); only the first
/// report is delivered.
#[derive(Debug, Clone)]
pub struct SourceFaultNotifier {
    tx: Arc<Mutex<Option<oneshot::Sender<SourceFault>>>>,
}

impl SourceFaultNotifier {
    /// Creates a notifier and the receiving end.
    #[must_use]
    pub fn channel() -> (Self, oneshot::Receiver<SourceFault>) {
        let (tx, rx) = oneshot::channel();
        (
            Self {
                tx: Arc::new(Mutex::new(Some(tx))),
            },
            rx,
        )
    }

    /// Reports `fault`; later reports are ignored.
    pub fn notify(&self, fault: SourceFault) {
        let sender = self
            .tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(sender) = sender {
            // The pipeline may already be gone; nothing to report to then.
            let _ = sender.send(fault);
        }
    }
}

/// An OS audio source (microphone or system audio).
pub trait AudioSource: Send + Sync {
    /// Kind of source.
    fn kind(&self) -> AudioSourceKind;

    /// Whether the source can be used on this machine.
    fn availability(&self) -> Availability;

    /// Queries the OS without prompting.
    fn permission(&self) -> PermissionState;

    /// May show the OS permission prompt.
    fn request_permission(&self) -> BoxFuture<'_, PermissionState>;

    /// Starts writing 16 kHz mono chunks into `sink`.
    ///
    /// Guarantees: fails without capturing when permission is not granted; stops capturing as
    /// soon as `cancel` fires; never switches device silently (FR-011); reports device loss
    /// and permission revocation through `faults` and stops capturing.
    ///
    /// # Errors
    ///
    /// See [`AudioError`].
    fn start(
        &self,
        sink: BoundedAudioSink,
        cancel: CancellationToken,
        faults: SourceFaultNotifier,
    ) -> Result<Box<dyn CaptureHandle>, AudioError>;
}

// ---------------------------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------------------------

/// Non-sensitive description of a provider adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDescriptor {
    /// Configured provider.
    pub id: ProviderId,
    /// Local or remote.
    pub locality: Locality,
    /// Data categories the adapter sends.
    pub data_categories: Vec<DataCategory>,
}

/// Provider failures, free of vendor types and session content.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProviderError {
    /// No remote authorization covers the request (FR-046).
    #[error("remote provider not authorized")]
    AuthorizationRequired,
    /// The provider cannot be reached or is not ready (e.g. model missing).
    #[error("provider unavailable")]
    Unavailable,
    /// The request exceeded its deadline.
    #[error("provider timed out")]
    Timeout,
    /// The output failed schema validation (FR-030).
    #[error("provider returned invalid output")]
    InvalidOutput,
    /// Cancelled through the session token.
    #[error("provider call cancelled")]
    Cancelled,
    /// Any other provider-side failure, described without content or secrets.
    #[error("provider error: {0}")]
    Other(String),
}

/// Transcriber output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptUpdate {
    /// Provisional text; never persisted (FR-017).
    Partial {
        /// Source of the audio.
        source: AudioSourceKind,
        /// Offset of the segment start.
        start: Duration,
        /// Provisional text.
        text: Redacted<String>,
    },
    /// Final text, persisted on arrival.
    Final {
        /// Source of the audio.
        source: AudioSourceKind,
        /// Offset of the segment start.
        start: Duration,
        /// Offset of the segment end.
        end: Duration,
        /// Final text.
        text: Redacted<SegmentText>,
    },
}

/// Where a transcriber writes its updates.
pub type TranscriptSink = mpsc::Sender<TranscriptUpdate>;

/// Streaming speech-to-text.
pub trait Transcriber: Send + Sync {
    /// Non-sensitive description.
    fn descriptor(&self) -> ProviderDescriptor;

    /// Consumes chunks and emits partial and final updates until `input` closes or `cancel`
    /// fires; honours `cancel` within 5 s.
    fn run(
        &self,
        input: AudioChunkStream,
        out: TranscriptSink,
        cancel: CancellationToken,
    ) -> BoxFuture<'_, Result<(), ProviderError>>;
}

/// Builds the [`Transcriber`] for a configured transcription provider.
///
/// `create` must be cheap (no model loading or network I/O): it runs on the `session_start`
/// path, which answers within 500 ms (SC-006). Heavy work happens inside
/// [`Transcriber::run`].
pub trait TranscriberFactory: Send + Sync {
    /// Transcriber for `provider` transcribing `language`.
    ///
    /// # Errors
    ///
    /// [`ProviderError::Unavailable`] when the provider cannot transcribe on this machine
    /// (e.g. its local model is not downloaded).
    fn create(
        &self,
        provider: &Provider,
        language: SessionLanguage,
    ) -> Result<Arc<dyn Transcriber>, ProviderError>;
}

/// Technical analysis and contextual questions (methods added by T089–T091 and T114).
pub trait AnalysisProvider: Send + Sync {
    /// Non-sensitive description.
    fn descriptor(&self) -> ProviderDescriptor;
}

/// Refusal by the [`RemoteGate`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AuthorizationError {
    /// No granted authorization covers destination and categories.
    #[error("no authorization covers this request")]
    NotAuthorized,
}

/// Single network enforcement point for remote adapters (R9).
pub trait RemoteGate: Send + Sync {
    /// `Ok` only when a granted authorization covers the provider's destination and every
    /// category in `categories`.
    ///
    /// # Errors
    ///
    /// [`AuthorizationError::NotAuthorized`] otherwise.
    fn check(
        &self,
        provider: &ProviderId,
        categories: &[DataCategory],
    ) -> Result<(), AuthorizationError>;
}

// ---------------------------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------------------------

/// Persistence failures, described without content.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RepositoryError {
    /// The requested row does not exist.
    #[error("not found")]
    NotFound,
    /// The encryption key cannot be obtained (FR-039a).
    #[error("secret store unavailable")]
    SecretStoreUnavailable,
    /// Stored data cannot be mapped back to domain types.
    #[error("corrupt stored data in `{0}`")]
    Corrupt(&'static str),
    /// Database or file-system error.
    #[error("storage error: {0}")]
    Storage(String),
}

/// Sessions and their selected sources (data-model.md "Session").
pub trait SessionRepository: Send + Sync {
    /// Inserts a new session.
    ///
    /// # Errors
    ///
    /// See [`RepositoryError`].
    fn create(&self, session: &Session) -> Result<(), RepositoryError>;

    /// One session.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::NotFound`] when absent.
    fn get(&self, id: SessionId) -> Result<Session, RepositoryError>;

    /// Stores the session's title, state, failure, timestamps and active duration.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::NotFound`] when absent.
    fn update(&self, session: &Session) -> Result<(), RepositoryError>;

    /// Every session, newest first (cursor pagination arrives with US4, T105).
    ///
    /// # Errors
    ///
    /// See [`RepositoryError`].
    fn list(&self) -> Result<Vec<Session>, RepositoryError>;

    /// Sessions stored in `starting`, `capturing`, `paused` or `stopping`.
    ///
    /// # Errors
    ///
    /// See [`RepositoryError`].
    fn list_active(&self) -> Result<Vec<Session>, RepositoryError>;

    /// Stores a session failed as `interrupted` on startup and, if it ever captured, appends a
    /// trailing `gap(interrupted)` after its last persisted segment, in one transaction
    /// (FR-007, US2.3).
    ///
    /// # Errors
    ///
    /// [`RepositoryError::NotFound`] when absent.
    fn save_interrupted(&self, session: &Session) -> Result<(), RepositoryError>;
}

/// Final segments and gaps of a session's transcript. Each append is its own transaction, so
/// a later failure or crash never removes what was stored (FR-017).
pub trait SegmentRepository: Send + Sync {
    /// Stores a final segment or gap.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::NotFound`] when the session does not exist; [`RepositoryError::Storage`]
    /// (e.g. disk full, or a duplicate `seq`).
    fn append(&self, segment: &TranscriptSegment) -> Result<(), RepositoryError>;

    /// Every segment of `session`, in `seq` order.
    ///
    /// # Errors
    ///
    /// See [`RepositoryError`].
    fn list_by_session(
        &self,
        session: SessionId,
    ) -> Result<Vec<TranscriptSegment>, RepositoryError>;

    /// The `seq` the next segment of `session` takes (0 for an empty transcript).
    ///
    /// # Errors
    ///
    /// See [`RepositoryError`].
    fn next_seq(&self, session: SessionId) -> Result<u64, RepositoryError>;
}

/// Configured providers (authorizations are added with US7, T135).
pub trait ProviderRepository: Send + Sync {
    /// Every provider, oldest first.
    ///
    /// # Errors
    ///
    /// See [`RepositoryError`].
    fn list(&self) -> Result<Vec<Provider>, RepositoryError>;

    /// One provider.
    ///
    /// # Errors
    ///
    /// [`RepositoryError::NotFound`] when absent.
    fn get(&self, id: ProviderId) -> Result<Provider, RepositoryError>;

    /// Inserts or replaces a provider.
    ///
    /// # Errors
    ///
    /// See [`RepositoryError`].
    fn upsert(&self, provider: &Provider) -> Result<(), RepositoryError>;
}

/// Non-sensitive key/value application settings.
pub trait SettingsRepository: Send + Sync {
    /// Stored value for `key`.
    ///
    /// # Errors
    ///
    /// See [`RepositoryError`].
    fn get(&self, key: &str) -> Result<Option<String>, RepositoryError>;

    /// Stores `value` under `key`.
    ///
    /// # Errors
    ///
    /// See [`RepositoryError`].
    fn set(&self, key: &str, value: &str) -> Result<(), RepositoryError>;
}

/// Credential-store failures.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SecretStoreError {
    /// The OS credential store cannot be used (FR-039a).
    #[error("secret store unavailable")]
    Unavailable,
    /// Platform failure, described without the secret.
    #[error("secret store error: {0}")]
    Backend(String),
}

/// OS credential store holding the database key and provider API keys (FR-049).
pub trait SecretStore: Send + Sync {
    /// Fails with [`SecretStoreError::Unavailable`] when the store cannot be used.
    ///
    /// # Errors
    ///
    /// See [`SecretStoreError`].
    fn check_available(&self) -> Result<(), SecretStoreError>;

    /// The secret stored under `reference`, if any.
    ///
    /// # Errors
    ///
    /// See [`SecretStoreError`].
    fn get(&self, reference: &CredentialRef)
    -> Result<Option<Redacted<Vec<u8>>>, SecretStoreError>;

    /// Stores or replaces a secret.
    ///
    /// # Errors
    ///
    /// See [`SecretStoreError`].
    fn set(
        &self,
        reference: &CredentialRef,
        secret: &Redacted<Vec<u8>>,
    ) -> Result<(), SecretStoreError>;

    /// Deletes a secret; deleting a missing one succeeds.
    ///
    /// # Errors
    ///
    /// See [`SecretStoreError`].
    fn delete(&self, reference: &CredentialRef) -> Result<(), SecretStoreError>;
}

// ---------------------------------------------------------------------------------------------
// Time and events
// ---------------------------------------------------------------------------------------------

/// Wall clock; injected so tests control time.
pub trait Clock: Send + Sync {
    /// Current instant.
    fn now(&self) -> Timestamp;
}

/// Delivers session events to subscribers (the UI channel at the composition root).
pub trait EventPublisher: Send + Sync {
    /// Publishes `event` for `session`. Never blocks and never fails the caller.
    fn publish(&self, session: SessionId, event: SessionEvent);
}
