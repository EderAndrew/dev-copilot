//! System audio ("loopback") sources per platform (research R2, US8).
//!
//! A platform's adapter is registered only after spike S1 validated it on real hardware and
//! the decision is recorded in ADR-002. Until then the source is listed as unavailable with
//! `not_validated_on_platform`, so the user sees why and can still capture the microphone
//! (FR-014, US8.2). No platform is validated yet.

use std::sync::Arc;

use devcopilot_application::ports::AudioSource;
use devcopilot_domain::audio_source::{AudioSourceKind, UnavailableReason};

use crate::unavailable::UnavailableSource;

/// Whether system audio capture is validated on the platform this build targets.
pub const VALIDATED_ON_THIS_PLATFORM: bool = false;

/// The system audio source registered on this platform.
#[must_use]
pub fn system_source() -> Arc<dyn AudioSource> {
    Arc::new(UnavailableSource::new(
        AudioSourceKind::System,
        UnavailableReason::NotValidatedOnPlatform,
    ))
}
