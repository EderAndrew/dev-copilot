//! Production CSP invariants (002 T017, research R1, F-1): `app.security.csp` in
//! `tauri.conf.json` follows `specs/002-desktop-release-readiness/contracts/csp-policy.md`.
//!
//! 1. No directive allows `'unsafe-inline'`, `'unsafe-eval'`, `'unsafe-hashes'`,
//!    `'wasm-unsafe-eval'`, a wildcard, or a bare `http:`/`https:` scheme.
//! 2. `connect-src` is exactly `'self' ipc: http://ipc.localhost`. Update requests leave from the
//!    core, never from the webview, so they are not listed here.
//! 3. `object-src 'none'`, `base-uri 'self'` and `form-action 'none'` are present.
//! 4. A `'sha256-…'` source is accepted only in `style-src`, and only when it is in
//!    [`STYLE_HASH_EXCEPTIONS`] with the ADR that approved it. Nonces are never accepted.
//!
//! `devCsp` is not checked: it is never used to validate this feature (FR-003).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// `(hash source, ADR)` pairs approved through the Constitution's exception process. Empty: any
/// hash needs an ADR before it is added here (research R1).
const STYLE_HASH_EXCEPTIONS: &[(&str, &str)] = &[];

const UNSAFE_KEYWORDS: [&str; 4] = [
    "'unsafe-inline'",
    "'unsafe-eval'",
    "'unsafe-hashes'",
    "'wasm-unsafe-eval'",
];

const CONNECT_SRC: [&str; 3] = ["'self'", "ipc:", "http://ipc.localhost"];

const REQUIRED: [(&str, &str); 3] = [
    ("object-src", "'none'"),
    ("base-uri", "'self'"),
    ("form-action", "'none'"),
];

type Policy = BTreeMap<String, Vec<String>>;

fn parse(csp: &str) -> Result<Policy, String> {
    let mut policy = Policy::new();
    for directive in csp.split(';').map(str::trim).filter(|d| !d.is_empty()) {
        let mut tokens = directive.split_whitespace();
        let name = tokens.next().unwrap().to_ascii_lowercase();
        let sources: Vec<String> = tokens.map(str::to_owned).collect();
        if policy.insert(name.clone(), sources).is_some() {
            return Err(format!("directive `{name}` appears twice"));
        }
    }
    Ok(policy)
}

fn is_hash(source: &str) -> bool {
    let lower = source.to_ascii_lowercase();
    ["'sha256-", "'sha384-", "'sha512-"]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// Every broken invariant of `csp`; empty when it follows the contract.
fn violations(csp: &str) -> Vec<String> {
    let policy = match parse(csp) {
        Ok(policy) => policy,
        Err(err) => return vec![err],
    };
    let mut found = Vec::new();
    for (name, sources) in &policy {
        for source in sources {
            let lower = source.to_ascii_lowercase();
            if UNSAFE_KEYWORDS.contains(&lower.as_str()) {
                found.push(format!("1: {name} allows {source}"));
            }
            if source.contains('*') {
                found.push(format!("1: {name} has wildcard {source}"));
            }
            if lower == "http:" || lower == "https:" {
                found.push(format!("1: {name} allows any host over {source}"));
            }
            if lower.starts_with("'nonce-") {
                found.push(format!("4: {name} has nonce {source}"));
            }
            if is_hash(source) {
                let approved = name == "style-src"
                    && STYLE_HASH_EXCEPTIONS
                        .iter()
                        .any(|(hash, adr)| hash == source && !adr.is_empty());
                if !approved {
                    found.push(format!("4: {name} has unapproved hash {source}"));
                }
            }
        }
    }
    let connect: BTreeSet<&str> = policy
        .get("connect-src")
        .map(|s| s.iter().map(String::as_str).collect())
        .unwrap_or_default();
    if connect != BTreeSet::from(CONNECT_SRC) {
        found.push(format!(
            "2: connect-src is {connect:?}, expected {CONNECT_SRC:?}"
        ));
    }
    for (name, value) in REQUIRED {
        if policy.get(name).map(Vec::as_slice) != Some(&[value.to_owned()][..]) {
            found.push(format!("3: `{name} {value}` missing"));
        }
    }
    found
}

fn production_csp() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    json["app"]["security"]["csp"]
        .as_str()
        .expect("app.security.csp is a string")
        .to_owned()
}

/// The policy in the fenced `text` block of the contract's production CSP section.
fn contract_csp() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../specs/002-desktop-release-readiness/contracts/csp-policy.md");
    let text = std::fs::read_to_string(&path).unwrap();
    let block = text
        .split("```text")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("contract has a ```text block");
    block.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn production_csp_follows_the_invariants() {
    let found = violations(&production_csp());
    assert!(found.is_empty(), "production CSP breaks: {found:#?}");
}

#[test]
fn production_csp_equals_the_contract() {
    assert_eq!(
        parse(&production_csp()).unwrap(),
        parse(&contract_csp()).unwrap()
    );
}

#[test]
fn the_contract_itself_follows_the_invariants() {
    assert_eq!(violations(&contract_csp()), Vec::<String>::new());
}

#[test]
fn invariants_catch_weakened_policies() {
    let good = contract_csp();
    let cases = [
        // 1: unsafe keywords, wildcards and bare schemes, in any directive.
        good.replace("style-src 'self'", "style-src 'self' 'unsafe-inline'"),
        good.replace("script-src 'self'", "script-src 'self' 'unsafe-eval'"),
        good.replace("script-src 'self'", "script-src 'self' 'wasm-unsafe-eval'"),
        good.replace("style-src 'self'", "style-src 'self' 'UNSAFE-HASHES'"),
        good.replace("img-src 'self'", "img-src *"),
        good.replace("img-src 'self'", "img-src https://*.example.com"),
        good.replace("font-src 'self'", "font-src 'self' https:"),
        // 2: connect-src grows or shrinks.
        good.replace(
            "http://ipc.localhost;",
            "http://ipc.localhost https://github.com;",
        ),
        good.replace(
            "connect-src 'self' ipc: http://ipc.localhost",
            "connect-src 'self'",
        ),
        // 3: required directives missing or relaxed.
        good.replace("object-src 'none';", ""),
        good.replace("base-uri 'self'", "base-uri 'self' https://example.com"),
        good.replace("form-action 'none'", "form-action 'self'"),
        // 4: hashes outside the exception list, and nonces.
        good.replace("style-src 'self'", "style-src 'self' 'sha256-AAAA'"),
        good.replace("script-src 'self'", "script-src 'self' 'sha256-AAAA'"),
        good.replace("style-src 'self'", "style-src 'self' 'nonce-abc'"),
        // Duplicate directives are ambiguous.
        format!("{good}; style-src 'unsafe-inline'"),
    ];
    for case in cases {
        assert_ne!(case, good, "replacement did not apply");
        assert!(!violations(&case).is_empty(), "accepted: {case}");
    }
}

#[test]
fn exceptions_cite_an_adr() {
    for (hash, adr) in STYLE_HASH_EXCEPTIONS {
        assert!(is_hash(hash), "{hash} is not a hash source");
        assert!(adr.starts_with("ADR-"), "{hash} has no ADR reference");
    }
}
