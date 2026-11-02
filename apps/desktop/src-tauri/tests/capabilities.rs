//! Webview capability guard (002 T030, contracts/ipc-updates.md): the updater, like the
//! export dialog, is driven from the core only, so every capability grants exactly
//! `core:default` and nothing from the updater, dialog, file system, HTTP or shell plugins.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

const FORBIDDEN_PREFIXES: [&str; 5] = ["updater:", "dialog:", "fs:", "http:", "shell:"];

#[test]
fn every_capability_grants_only_core_default() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let capability: serde_json::Value = match path.extension().and_then(|e| e.to_str()) {
            Some("json") => serde_json::from_str(&text).unwrap(),
            other => panic!("{}: unexpected capability format {other:?}", path.display()),
        };
        let permissions = capability["permissions"]
            .as_array()
            .unwrap_or_else(|| panic!("{}: no permissions array", path.display()));
        let names: Vec<String> = permissions
            .iter()
            .map(|permission| match permission {
                serde_json::Value::String(name) => name.clone(),
                // Scoped permission objects carry their name in `identifier`.
                other => other["identifier"].as_str().unwrap_or_default().to_owned(),
            })
            .collect();
        for name in &names {
            assert!(
                !FORBIDDEN_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix)),
                "{}: forbidden permission {name}",
                path.display()
            );
        }
        assert_eq!(names, ["core:default"], "{}", path.display());
        checked += 1;
    }
    assert!(checked > 0, "no capability files found");
}
