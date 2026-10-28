//! A source listed with its unavailability reason but never opened (FR-008, FR-014).
//!
//! System audio is registered this way on every platform until spike S1 approves an adapter
//! (US8, ADR-002).

use devcopilot_application::ports::{
    AudioError, AudioSource, BoundedAudioSink, BoxFuture, CaptureHandle, SourceFaultNotifier,
};
use devcopilot_domain::audio_source::{
    AudioSourceKind, Availability, PermissionState, UnavailableReason,
};
use tokio_util::sync::CancellationToken;

/// A source that is always unavailable for `reason`.
#[derive(Debug, Clone, Copy)]
pub struct UnavailableSource {
    kind: AudioSourceKind,
    reason: UnavailableReason,
}

impl UnavailableSource {
    /// Lists `kind` as unavailable for `reason`.
    #[must_use]
    pub const fn new(kind: AudioSourceKind, reason: UnavailableReason) -> Self {
        Self { kind, reason }
    }
}

impl AudioSource for UnavailableSource {
    fn kind(&self) -> AudioSourceKind {
        self.kind
    }

    fn availability(&self) -> Availability {
        Availability::Unavailable(self.reason)
    }

    fn permission(&self) -> PermissionState {
        PermissionState::NotRequested
    }

    fn request_permission(&self) -> BoxFuture<'_, PermissionState> {
        Box::pin(async { PermissionState::NotRequested })
    }

    fn start(
        &self,
        _sink: BoundedAudioSink,
        _cancel: CancellationToken,
        _faults: SourceFaultNotifier,
    ) -> Result<Box<dyn CaptureHandle>, AudioError> {
        Err(AudioError::Unavailable(self.availability()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_opens() {
        let source = UnavailableSource::new(
            AudioSourceKind::System,
            UnavailableReason::NotValidatedOnPlatform,
        );
        assert!(!source.availability().is_available());
        let (sink, _rx) = BoundedAudioSink::channel(1);
        let (faults, _f) = SourceFaultNotifier::channel();
        assert!(
            source
                .start(sink, CancellationToken::new(), faults)
                .is_err()
        );
    }
}
