//! Every shared fixture in `specs/001-meeting-sessions/contracts/fixtures/` round-trips through
//! its Rust DTO unchanged (research R11).

// Test helpers outside `#[test]` fns are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use devcopilot_desktop_lib::commands::IpcError;
use devcopilot_desktop_lib::commands::audio::{AudioSourceDto, PermissionHelpDto};
use devcopilot_desktop_lib::commands::providers::{
    ProviderDto, ProviderSetDefaultDto, ProviderUpsertDto,
};
use devcopilot_desktop_lib::commands::sessions::{
    SessionCreateDto, SessionDto, SessionIdDto, SessionStartDto,
};
use devcopilot_desktop_lib::commands::settings::{SettingsDto, SettingsUpdateDto};
use devcopilot_desktop_lib::events::IpcSessionEvent;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../specs/001-meeting-sessions/contracts/fixtures")
}

fn fixtures(dto: &str) -> Vec<(PathBuf, Value)> {
    let dir = fixtures_dir().join(dto);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures for {dto}");
    files
        .into_iter()
        .map(|path| {
            let value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            (path, value)
        })
        .collect()
}

fn assert_round_trip<T: DeserializeOwned + Serialize>(dto: &str) {
    for (path, value) in fixtures(dto) {
        let parsed: T = serde_json::from_value(value.clone())
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back, value, "{} changed on round trip", path.display());
    }
}

#[test]
fn ipc_error() {
    assert_round_trip::<IpcError>("IpcError");
}

#[test]
fn settings() {
    assert_round_trip::<SettingsDto>("SettingsDto");
    assert_round_trip::<SettingsUpdateDto>("SettingsUpdateDto");
}

#[test]
fn providers() {
    assert_round_trip::<ProviderDto>("ProviderDto");
    assert_round_trip::<ProviderUpsertDto>("ProviderUpsertDto");
    assert_round_trip::<ProviderSetDefaultDto>("ProviderSetDefaultDto");
}

#[test]
fn sessions() {
    assert_round_trip::<SessionDto>("SessionDto");
    assert_round_trip::<SessionCreateDto>("SessionCreateDto");
    assert_round_trip::<SessionStartDto>("SessionStartDto");
    assert_round_trip::<SessionIdDto>("SessionIdDto");
}

#[test]
fn audio_sources() {
    assert_round_trip::<AudioSourceDto>("AudioSourceDto");
    assert_round_trip::<PermissionHelpDto>("PermissionHelpDto");
}

#[test]
fn session_events() {
    assert_round_trip::<IpcSessionEvent>("SessionEvent");
}

#[test]
fn every_fixture_directory_is_covered() {
    let covered = [
        "AudioSourceDto",
        "IpcError",
        "PermissionHelpDto",
        "ProviderDto",
        "ProviderSetDefaultDto",
        "ProviderUpsertDto",
        "SessionCreateDto",
        "SessionDto",
        "SessionEvent",
        "SessionIdDto",
        "SessionStartDto",
        "SettingsDto",
        "SettingsUpdateDto",
    ];
    let mut dirs: Vec<String> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    dirs.sort();
    assert_eq!(
        dirs, covered,
        "add a round-trip test for new fixture directories"
    );
}

#[test]
fn invalid_values_are_rejected() {
    let bad_event = serde_json::json!({ "type": "source_status", "seq": 1, "source": "microphone", "status": "muted" });
    assert!(serde_json::from_value::<IpcSessionEvent>(bad_event).is_err());
    let bad_settings = serde_json::json!({ "uiLanguage": "pt-PT" });
    assert!(serde_json::from_value::<SettingsUpdateDto>(bad_settings).is_err());
}
