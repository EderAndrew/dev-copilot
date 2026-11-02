//! Audio source vocabulary (data-model.md "AudioSource").
//!
//! Permission state is never persisted as truth: it is queried from the OS on every
//! start/resume.

/// Kind of audio source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AudioSourceKind {
    /// A microphone input device.
    Microphone,
    /// Audio played by the system (P2, platform dependent).
    System,
}

code_enum!(AudioSourceKind, "source", {
    Microphone => "microphone",
    System => "system",
});

/// Why a source cannot be used on this machine (FR-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnavailableReason {
    /// The platform offers no supported capture API.
    PlatformUnsupported,
    /// Capture exists but has not passed the platform spike (ADR-002).
    NotValidatedOnPlatform,
    /// No matching device is connected.
    NoDevice,
    /// The OS version is below the supported minimum.
    OsVersion,
}

code_enum!(UnavailableReason, "reason", {
    PlatformUnsupported => "platform_unsupported",
    NotValidatedOnPlatform => "not_validated_on_platform",
    NoDevice => "no_device",
    OsVersion => "os_version",
});

/// Whether a source can be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Availability {
    /// Can be selected.
    Available,
    /// Cannot be selected, with the reason shown to the user.
    Unavailable(UnavailableReason),
}

impl Availability {
    /// Whether the source can be selected.
    #[must_use]
    pub const fn is_available(self) -> bool {
        matches!(self, Self::Available)
    }
}

/// OS permission to capture from a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionState {
    /// The OS has not asked the user yet.
    NotRequested,
    /// Capture allowed.
    Granted,
    /// The user refused.
    Denied,
    /// Previously granted, then withdrawn in OS settings.
    Revoked,
}

code_enum!(PermissionState, "permission", {
    NotRequested => "not_requested",
    Granted => "granted",
    Denied => "denied",
    Revoked => "revoked",
});

impl PermissionState {
    /// Whether capture may start.
    #[must_use]
    pub const fn is_granted(self) -> bool {
        matches!(self, Self::Granted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_granted_allows_capture() {
        assert!(PermissionState::Granted.is_granted());
        for state in [
            PermissionState::NotRequested,
            PermissionState::Denied,
            PermissionState::Revoked,
        ] {
            assert!(!state.is_granted());
        }
    }

    #[test]
    fn availability_and_codes() {
        assert!(Availability::Available.is_available());
        assert!(!Availability::Unavailable(UnavailableReason::NoDevice).is_available());
        assert_eq!(
            UnavailableReason::parse("not_validated_on_platform"),
            Ok(UnavailableReason::NotValidatedOnPlatform)
        );
        assert_eq!(
            AudioSourceKind::parse("system"),
            Ok(AudioSourceKind::System)
        );
        assert!(AudioSourceKind::parse("speaker").is_err());
    }
}
