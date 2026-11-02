//! System audio availability (T149, FR-014, US8.2): until spike S1 validates a platform, its
//! system source is listed as `not_validated_on_platform`, never prompts and never captures.

use devcopilot_application::ports::{AudioError, BoundedAudioSink, SourceFaultNotifier};
use devcopilot_audio::system::{VALIDATED_ON_THIS_PLATFORM, system_source};
use devcopilot_domain::audio_source::{
    AudioSourceKind, Availability, PermissionState, UnavailableReason,
};
use tokio_util::sync::CancellationToken;

const NOT_VALIDATED: Availability =
    Availability::Unavailable(UnavailableReason::NotValidatedOnPlatform);

#[tokio::test]
async fn unvalidated_platform_reports_the_reason_and_captures_nothing() {
    if VALIDATED_ON_THIS_PLATFORM {
        return;
    }
    let source = system_source();
    assert_eq!(source.kind(), AudioSourceKind::System);
    assert_eq!(source.availability(), NOT_VALIDATED);
    assert!(!source.availability().is_available());
    // Listing the source must not trigger an OS prompt.
    assert_eq!(source.permission(), PermissionState::NotRequested);
    assert_eq!(
        source.request_permission().await,
        PermissionState::NotRequested
    );
    let (sink, mut chunks) = BoundedAudioSink::channel(4);
    let (faults, _) = SourceFaultNotifier::channel();
    let started = source.start(sink, CancellationToken::new(), faults);
    assert!(matches!(
        started.err(),
        Some(AudioError::Unavailable(availability)) if availability == NOT_VALIDATED
    ));
    assert!(chunks.try_recv().is_err());
}
