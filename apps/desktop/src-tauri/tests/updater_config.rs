//! Shipped updater configuration (002 T039a, research R5).
//!
//! `plugins.updater` in `tauri.conf.json` must point at the one GitHub release endpoint over
//! HTTPS, enable no `dangerous*` option, bind signatures to versions, and embed the public
//! key whose id is recorded in `docs/adr/ADR-005.md`. A wrong key would only surface at the
//! next release, when updates would already be broken for everyone.
//!
//! Until T006 records the key, the config holds a placeholder and ADR-005 has no
//! `updater-pubkey-id` line. That is accepted in development, but fails when `RELEASE_TAG` is
//! set (the release workflow sets it from the tag), so no release ships without the real key.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use devcopilot_desktop_lib::updater::{pubkey_key_id, validate_config};

const ENDPOINT: &str =
    "https://github.com/EderAndrew/dev-copilot/releases/latest/download/latest.json";
const PLACEHOLDER: &str = "PENDING-T006-updater-public-key";

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn updater_config() -> serde_json::Value {
    let config: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    config["plugins"]["updater"].clone()
}

/// Key id from a `updater-pubkey-id: <16 hex>` line of ADR-005. The commented template line
/// (`<key id>`) does not count.
fn adr_key_id() -> Option<String> {
    read("../../../docs/adr/ADR-005.md")
        .lines()
        .find_map(|line| {
            let id = line.trim().strip_prefix("updater-pubkey-id:")?.trim();
            (id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| id.to_ascii_uppercase())
        })
}

#[test]
fn endpoint_is_the_github_release_manifest_over_https() {
    let updater = updater_config();
    assert_eq!(updater["endpoints"], serde_json::json!([ENDPOINT]));
    for endpoint in updater["endpoints"].as_array().unwrap() {
        assert!(endpoint.as_str().unwrap().starts_with("https://"));
    }
}

#[test]
fn no_dangerous_option_and_signed_versions_required() {
    let updater = updater_config();
    for option in [
        "dangerousInsecureTransportProtocol",
        "dangerousAcceptInvalidCerts",
        "dangerousAcceptInvalidHostnames",
        "allowDowngrades",
    ] {
        assert_ne!(updater[option], serde_json::json!(true), "{option}");
    }
    assert_eq!(updater["requireSignedVersion"], serde_json::json!(true));
    assert_eq!(updater["windows"]["installMode"], "passive");
}

#[test]
fn updater_artifacts_are_built() {
    let config: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    assert_eq!(
        config["bundle"]["createUpdaterArtifacts"],
        serde_json::json!(true)
    );
}

#[test]
fn public_key_matches_the_id_recorded_in_adr_005() {
    let updater = updater_config();
    let pubkey = updater["pubkey"].as_str().expect("pubkey is a string");
    match adr_key_id() {
        Some(recorded) => {
            assert_eq!(
                pubkey_key_id(pubkey).as_deref(),
                Some(recorded.as_str()),
                "plugins.updater.pubkey is not the key recorded in ADR-005"
            );
            assert_eq!(validate_config(&updater).as_deref(), Ok(recorded.as_str()));
        }
        None => {
            assert_eq!(
                pubkey, PLACEHOLDER,
                "a real pubkey needs its `updater-pubkey-id` line in ADR-005"
            );
            assert!(
                std::env::var_os("RELEASE_TAG").is_none(),
                "release build without the updater public key (T006)"
            );
        }
    }
}
