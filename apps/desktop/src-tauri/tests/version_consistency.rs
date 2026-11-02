//! Release version consistency (002 T010, research R6, checklist item 2): the version in
//! `tauri.conf.json`, `apps/desktop/package.json` and the app's `Cargo.toml` is the same, and
//! when `RELEASE_TAG` is set (the release workflow sets it from the tag) the tag is
//! `v<version>` or `v<version>-rc.N`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

/// Version of the app crate as Cargo resolved it (`version.workspace = true` included).
const CARGO_VERSION: &str = env!("CARGO_PKG_VERSION");

fn json_version(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["version"]
        .as_str()
        .unwrap_or_else(|| panic!("{} has no string `version`", path.display()))
        .to_owned()
}

/// Whether `tag` names a release of `version`: `v<version>` or `v<version>-rc.N`, N ≥ 1.
fn tag_matches(tag: &str, version: &str) -> bool {
    let Some(rest) = tag.strip_prefix('v').and_then(|t| t.strip_prefix(version)) else {
        return false;
    };
    if rest.is_empty() {
        return true;
    }
    rest.strip_prefix("-rc.").is_some_and(|n| {
        !n.is_empty() && !n.starts_with('0') && n.bytes().all(|b| b.is_ascii_digit())
    })
}

#[test]
fn versions_agree_across_tauri_npm_and_cargo() {
    let tauri = json_version("tauri.conf.json");
    let npm = json_version("../package.json");
    assert_eq!(tauri, CARGO_VERSION, "tauri.conf.json vs Cargo.toml");
    assert_eq!(npm, CARGO_VERSION, "package.json vs Cargo.toml");
}

#[test]
fn release_tag_matches_the_version_when_set() {
    let Ok(tag) = std::env::var("RELEASE_TAG") else {
        return;
    };
    assert!(
        tag_matches(&tag, CARGO_VERSION),
        "RELEASE_TAG `{tag}` is neither v{CARGO_VERSION} nor v{CARGO_VERSION}-rc.N"
    );
}

#[test]
fn tag_rule() {
    for ok in ["v0.1.0", "v0.1.0-rc.1", "v0.1.0-rc.12"] {
        assert!(tag_matches(ok, "0.1.0"), "{ok}");
    }
    for bad in [
        "0.1.0",
        "v0.1.1",
        "v0.1.0-rc",
        "v0.1.0-rc.",
        "v0.1.0-rc.0",
        "v0.1.0-rc.01",
        "v0.1.0-rc.1a",
        "v0.1.0-beta.1",
        "v0.1.00",
        "v0.1.0.1",
    ] {
        assert!(!tag_matches(bad, "0.1.0"), "{bad}");
    }
}
