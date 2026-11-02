//! Shipped bundle configuration (002 T048, research R2–R4).
//!
//! The installers a user downloads are built from `bundle` in `tauri.conf.json`. A drift here
//! (an extra installer format, a missing entitlement, a per-machine Windows install) only shows
//! up on a clean machine during the release matrix, so it is pinned by this test instead.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn config() -> serde_json::Value {
    serde_json::from_str(&read("tauri.conf.json")).unwrap()
}

/// `<key>` names of a flat plist `<dict>`, in order.
fn plist_keys(plist: &str) -> Vec<String> {
    plist
        .split("<key>")
        .skip(1)
        .filter_map(|rest| {
            rest.split_once("</key>")
                .map(|(key, _)| key.trim().to_owned())
        })
        .collect()
}

#[test]
fn config_is_accepted_by_tauri() {
    // Tauri's config types deny unknown fields: a typo here would otherwise only fail in the
    // release build.
    serde_json::from_value::<tauri::utils::config::Config>(config())
        .expect("tauri.conf.json is a valid Tauri 2 config");
}

#[test]
fn installer_formats_are_exactly_the_declared_ones() {
    // No MSI: one Windows installer to validate, and the one the updater uses (R3).
    assert_eq!(
        config()["bundle"]["targets"],
        serde_json::json!(["app", "dmg", "nsis", "deb", "appimage"])
    );
}

#[test]
fn macos_minimum_version_and_hardened_runtime() {
    let macos = &config()["bundle"]["macOS"];
    assert_eq!(macos["minimumSystemVersion"], "13.0");
    assert_ne!(
        macos["hardenedRuntime"],
        serde_json::json!(false),
        "notarization requires the hardened runtime"
    );
    assert_eq!(macos["entitlements"], "./Entitlements.plist");
}

#[test]
fn entitlements_grant_only_the_microphone() {
    let plist = read("Entitlements.plist");
    assert_eq!(
        plist_keys(&plist),
        ["com.apple.security.device.audio-input"],
        "the app needs no sandbox, network or file entitlements (R2)"
    );
    let value = plist
        .split_once("</key>")
        .map(|(_, rest)| rest.trim_start())
        .unwrap();
    assert!(value.starts_with("<true/>"), "audio-input must be true");
}

#[test]
fn info_plist_explains_the_microphone_prompt() {
    let keys = plist_keys(&read("Info.plist"));
    assert!(
        keys.iter().any(|key| key == "NSMicrophoneUsageDescription"),
        "without it macOS never shows the microphone prompt"
    );
}

#[test]
fn windows_installs_per_user_with_embedded_webview_bootstrapper() {
    let windows = &config()["bundle"]["windows"];
    assert_eq!(windows["nsis"]["installMode"], "currentUser");
    assert_eq!(windows["webviewInstallMode"]["type"], "embedBootstrapper");
}

#[test]
fn windows_binaries_are_signed_by_the_release_tool() {
    let command = &config()["bundle"]["windows"]["signCommand"];
    let args: Vec<&str> = command["args"]
        .as_array()
        .expect("signCommand uses the object notation")
        .iter()
        .map(|arg| arg.as_str().unwrap())
        .collect();
    assert!(args.contains(&"%1"), "signCommand must receive the file");
    let script = args
        .iter()
        .find(|arg| arg.ends_with("windows-sign.mjs"))
        .expect("signCommand runs tooling/release/windows-sign.mjs");
    // The bundler runs signCommand from src-tauri.
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR")).join(script).is_file(),
        "{script} does not exist relative to src-tauri"
    );
}

#[test]
fn deb_dependencies() {
    let deb = &config()["bundle"]["linux"]["deb"];
    let depends = deb["depends"].as_array().unwrap();
    for package in ["libwebkit2gtk-4.1-0", "libssl3"] {
        assert!(depends.contains(&serde_json::json!(package)), "{package}");
    }
    // Recommended, not required: without a Secret Service the app blocks and explains why
    // (001/FR-039a).
    assert_eq!(
        deb["recommends"],
        serde_json::json!(["gnome-keyring | kwalletmanager"])
    );
}

#[test]
fn updater_artifacts_are_built() {
    assert_eq!(
        config()["bundle"]["createUpdaterArtifacts"],
        serde_json::json!(true)
    );
}
