//! Ports implemented by infrastructure crates (contracts/provider-ports.md).
//!
//! Vendor types never cross these traits (Constitution Art. VI.2). Ports used across `.await`
//! points return [`BoxFuture`] so they stay object-safe and can be composed as `Arc<dyn _>`.
//!
//! Repository ports for entities introduced by user stories grow with those stories:
//! `SessionRepository` (T048), `SegmentRepository` (T069), `RecordRepository` (T088),
//! `QuestionRepository` (T113); `AnalysisProvider::analyze`/`answer` arrive with T089–T091 and
//! T114; authorizations on `ProviderRepository` with T135.

use std::future::Future;
use std::pin::Pin;

use devcopilot_domain::Redacted;
use devcopilot_domain::audio_source::{AudioSourceKind, Availability, PermissionState};
use devcopilot_domain::ids::{ProviderId, SessionId};
use devcopilot_domain::provider::{CredentialRef, DataCategory, Locality, Provider};
use devcopilot_domain::text::SegmentText;
use devcopilot_domain::time::{Duration, Timestamp};
use thiserror::Error;
use tokio::sync::mpsc;
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
    /// soon as `cancel` fires; never switches device silently (FR-011).
    ///
    /// # Errors
    ///
    /// See [`AudioError`].
    fn start(
        &self,
        sink: BoundedAudioSink,
        cancel: CancellationToken,
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
