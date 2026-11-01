//! Contract tests of `TauriUpdater` against the real `tauri-plugin-updater` under the
//! `tauri::test` mock runtime (002 T029, research R5).
//!
//! `latest.json` and packages are served by `wiremock`. The packages in
//! `tests/fixtures/updater/` were signed with throwaway test keys whose private halves were
//! discarded (see the fixture README); the mock app's installed version is `0.1.0`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read as _, Write as _};
use std::net::TcpListener;

use devcopilot_application::ports::{AppUpdater, AvailableUpdate, UpdateFailure};
use devcopilot_desktop_lib::updater::{TauriUpdater, to_available};
use tauri::test::{MockRuntime, mock_builder, mock_context, noop_assets};
use tauri_plugin_updater::UpdaterExt;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/updater");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap()
}

fn package() -> Vec<u8> {
    std::fs::read(format!("{FIXTURES}/package.bin")).unwrap()
}

fn target() -> String {
    tauri_plugin_updater::target().expect("supported test platform")
}

/// Mock app whose updater plugin trusts the test key and reads `endpoint`. Plain HTTP is
/// enabled only because `wiremock` cannot serve TLS; the shipped config is checked by
/// `updater_config.rs`.
fn app(endpoint: &str) -> tauri::App<MockRuntime> {
    let mut context = mock_context(noop_assets());
    context.config_mut().plugins.0.insert(
        "updater".into(),
        serde_json::json!({
            "pubkey": fixture("test.key.pub").trim(),
            "endpoints": [endpoint],
            "dangerousInsecureTransportProtocol": true,
            "requireSignedVersion": true,
        }),
    );
    mock_builder()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .build(context)
        .unwrap()
}

fn manifest(version: &str, url: &str, signature: &str, notes: &str) -> serde_json::Value {
    serde_json::json!({
        "version": version,
        "notes": notes,
        "pub_date": "2026-11-01T12:00:00Z",
        "platforms": { target(): { "url": url, "signature": signature } }
    })
}

struct Server {
    mock: MockServer,
}

impl Server {
    async fn start() -> Self {
        Self {
            mock: MockServer::start().await,
        }
    }

    fn url(&self, route: &str) -> String {
        format!("{}{route}", self.mock.uri())
    }

    async fn serve_manifest(&self, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path("/latest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.mock)
            .await;
    }

    async fn serve_package(&self, bytes: Vec<u8>) {
        Mock::given(method("GET"))
            .and(path("/package.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes))
            .mount(&self.mock)
            .await;
    }

    /// Adapter over a mock app reading this server's manifest.
    fn updater(&self) -> (tauri::App<MockRuntime>, TauriUpdater<MockRuntime>) {
        let app = app(&self.url("/latest.json"));
        let updater = TauriUpdater::new(app.handle().clone()).allowing_plain_http();
        (app, updater)
    }
}

async fn download(
    updater: &TauriUpdater<MockRuntime>,
    update: &AvailableUpdate,
) -> Result<(), UpdateFailure> {
    updater
        .download(update, &|_, _| {}, CancellationToken::new())
        .await
}

// --- check -------------------------------------------------------------------------------------

#[tokio::test]
async fn newer_signed_update_is_available() {
    let server = Server::start().await;
    server
        .serve_manifest(manifest(
            "0.2.0",
            "https://example.invalid/package.bin",
            &fixture("package.sig"),
            "Correções",
        ))
        .await;
    let (_app, updater) = server.updater();
    let update = updater.check().await.unwrap().expect("update offered");
    assert_eq!(update.version, "0.2.0");
    assert_eq!(update.notes, "Correções");
    assert_eq!(
        update.published_at.map(|at| at.to_rfc3339()).as_deref(),
        Some("2026-11-01T12:00:00Z")
    );
}

#[tokio::test]
async fn same_or_lower_version_is_not_offered() {
    for version in ["0.1.0", "0.0.9"] {
        let server = Server::start().await;
        server
            .serve_manifest(manifest(
                version,
                "https://example.invalid/package.bin",
                &fixture("package.sig"),
                "",
            ))
            .await;
        let (_app, updater) = server.updater();
        assert_eq!(updater.check().await.unwrap(), None, "{version}");
    }
}

#[tokio::test]
async fn malformed_manifest_is_rejected() {
    let server = Server::start().await;
    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{ not json"))
        .mount(&server.mock)
        .await;
    let (_app, updater) = server.updater();
    assert_eq!(
        updater.check().await.unwrap_err(),
        UpdateFailure::ManifestInvalid
    );

    let server = Server::start().await;
    server
        .serve_manifest(serde_json::json!({ "version": "0.2.0" }))
        .await;
    let (_app, updater) = server.updater();
    assert_eq!(
        updater.check().await.unwrap_err(),
        UpdateFailure::ManifestInvalid
    );
}

#[tokio::test]
async fn unreachable_manifest_is_a_network_failure() {
    let server = Server::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server.mock)
        .await;
    let (_app, updater) = server.updater();
    assert_eq!(updater.check().await.unwrap_err(), UpdateFailure::Network);
}

#[test]
fn non_https_package_url_is_rejected() {
    let app = app("https://example.invalid/latest.json");
    let updater = app
        .handle()
        .updater_builder()
        .build()
        .expect("updater for the mock app");
    let manifest = manifest(
        "0.2.0",
        "http://example.invalid/package.bin",
        &fixture("package.sig"),
        "",
    );
    let update = updater
        .restore_update(manifest)
        .unwrap()
        .expect("newer version");
    assert_eq!(
        to_available(&update, false).unwrap_err(),
        UpdateFailure::ManifestInvalid
    );
}

#[tokio::test]
async fn non_https_endpoint_is_rejected_without_a_request() {
    // Debug builds of the plugin accept a plain-HTTP endpoint; the shipped adapter does not.
    let server = Server::start().await;
    let app = app(&server.url("/latest.json"));
    let strict = TauriUpdater::new(app.handle().clone());
    assert_eq!(
        strict.check().await.unwrap_err(),
        UpdateFailure::ManifestInvalid
    );
    assert!(server.mock.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn long_notes_are_truncated() {
    let server = Server::start().await;
    server
        .serve_manifest(manifest(
            "0.2.0",
            "https://example.invalid/package.bin",
            &fixture("package.sig"),
            &"n".repeat(5_000),
        ))
        .await;
    let (_app, updater) = server.updater();
    let update = updater.check().await.unwrap().unwrap();
    assert_eq!(update.notes.chars().count(), 4_000);
}

// --- download and signature --------------------------------------------------------------------

async fn checked(
    server: &Server,
    signature: &str,
    bytes: Vec<u8>,
) -> (
    tauri::App<MockRuntime>,
    TauriUpdater<MockRuntime>,
    AvailableUpdate,
) {
    server
        .serve_manifest(manifest(
            "0.2.0",
            &server.url("/package.bin"),
            signature,
            "",
        ))
        .await;
    server.serve_package(bytes).await;
    let (app, updater) = server.updater();
    let update = updater.check().await.unwrap().unwrap();
    (app, updater, update)
}

#[tokio::test]
async fn valid_package_downloads_with_progress() {
    let server = Server::start().await;
    let (_app, updater, update) = checked(&server, &fixture("package.sig"), package()).await;
    let seen = std::sync::Mutex::new(Vec::new());
    updater
        .download(
            &update,
            &|received, total| seen.lock().unwrap().push((received, total)),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let size = package().len() as u64;
    assert_eq!(seen.lock().unwrap().last(), Some(&(size, Some(size))));
}

#[tokio::test]
async fn tampered_package_is_rejected() {
    let server = Server::start().await;
    let mut tampered = package();
    tampered[0] ^= 0xFF;
    let (_app, updater, update) = checked(&server, &fixture("package.sig"), tampered).await;
    assert_eq!(
        download(&updater, &update).await.unwrap_err(),
        UpdateFailure::SignatureInvalid
    );
    assert_eq!(
        updater.install(&update).unwrap_err(),
        UpdateFailure::SignatureInvalid,
        "nothing verified, nothing to install"
    );
}

#[tokio::test]
async fn package_signed_by_another_key_is_rejected() {
    let server = Server::start().await;
    let (_app, updater, update) =
        checked(&server, &fixture("package.other-key.sig"), package()).await;
    assert_eq!(
        download(&updater, &update).await.unwrap_err(),
        UpdateFailure::SignatureInvalid
    );
}

#[tokio::test]
async fn signature_for_another_version_is_rejected() {
    // A genuine signature of an older release paired with a newer announced version.
    let server = Server::start().await;
    let (_app, updater, update) = checked(&server, &fixture("package.v0.1.5.sig"), package()).await;
    assert_eq!(
        download(&updater, &update).await.unwrap_err(),
        UpdateFailure::SignatureInvalid
    );
}

#[tokio::test]
async fn garbage_signature_is_rejected() {
    let server = Server::start().await;
    let (_app, updater, update) = checked(&server, "bm90IGEgc2lnbmF0dXJl", package()).await;
    assert_eq!(
        download(&updater, &update).await.unwrap_err(),
        UpdateFailure::SignatureInvalid
    );
}

/// Serves one response announcing `Content-Length: 1000` and closes after a few bytes.
fn truncating_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nContent-Type: application/octet-stream\r\n\r\npartial",
            );
        }
    });
    format!("http://{address}/package.bin")
}

#[tokio::test]
async fn truncated_download_is_interrupted() {
    let server = Server::start().await;
    server
        .serve_manifest(manifest(
            "0.2.0",
            &truncating_server(),
            &fixture("package.sig"),
            "",
        ))
        .await;
    let (_app, updater) = server.updater();
    let update = updater.check().await.unwrap().unwrap();
    assert_eq!(
        download(&updater, &update).await.unwrap_err(),
        UpdateFailure::DownloadInterrupted
    );
}

#[tokio::test]
async fn cancelled_download_is_interrupted() {
    let server = Server::start().await;
    let (_app, updater, update) = checked(&server, &fixture("package.sig"), package()).await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        updater
            .download(&update, &|_, _| {}, cancel)
            .await
            .unwrap_err(),
        UpdateFailure::DownloadInterrupted
    );
}

#[tokio::test]
async fn download_of_an_unchecked_version_is_refused() {
    let server = Server::start().await;
    let (_app, updater, _update) = checked(&server, &fixture("package.sig"), package()).await;
    let other = AvailableUpdate {
        version: "9.9.9".into(),
        notes: String::new(),
        published_at: None,
    };
    assert_eq!(
        download(&updater, &other).await.unwrap_err(),
        UpdateFailure::ManifestInvalid
    );
}
