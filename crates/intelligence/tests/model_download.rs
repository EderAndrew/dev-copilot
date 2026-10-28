//! Local model download (T064, T072): pinned hash enforced, nothing requested without an
//! explicit call, and the request carries no session data.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use devcopilot_intelligence::model_download::{self, DownloadError, ModelDownloader, ModelSpec};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BODY: &[u8] = b"not really a ggml model, but bytes with a known hash";

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn server_with_model() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models/ggml-test.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(BODY))
        .mount(&server)
        .await;
    server
}

fn spec(server: &MockServer, sha256: String) -> ModelSpec {
    ModelSpec::for_test(
        "test",
        format!("{}/models/ggml-test.bin", server.uri()),
        BODY.len() as u64,
        sha256,
    )
}

fn files_in(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[tokio::test]
async fn downloads_verifies_and_reports_progress() {
    let server = server_with_model().await;
    let dir = tempfile::tempdir().unwrap();
    let downloader = ModelDownloader::new(dir.path().join("models"));
    let spec = spec(&server, sha256_hex(BODY));
    let progress = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&progress);
    let path = downloader
        .download(
            &spec,
            move |received, total| seen.lock().unwrap().push((received, total)),
            CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), BODY);
    assert!(downloader.is_downloaded(&spec));
    let progress = progress.lock().unwrap();
    assert_eq!(progress.first(), Some(&(0, BODY.len() as u64)));
    assert_eq!(
        progress.last(),
        Some(&(BODY.len() as u64, BODY.len() as u64))
    );
    assert_eq!(files_in(&dir.path().join("models")), ["ggml-test.bin"]);
}

#[tokio::test]
async fn hash_mismatch_is_rejected_and_leaves_no_file() {
    let server = server_with_model().await;
    let dir = tempfile::tempdir().unwrap();
    let downloader = ModelDownloader::new(dir.path());
    let spec = spec(&server, sha256_hex(b"something else"));
    let result = downloader
        .download(&spec, |_, _| {}, CancellationToken::new())
        .await;
    assert_eq!(result, Err(DownloadError::HashMismatch));
    assert!(!downloader.is_downloaded(&spec));
    assert!(files_in(dir.path()).is_empty(), "no partial or final file");
}

#[tokio::test]
async fn size_mismatch_and_http_errors_are_rejected() {
    let server = MockServer::start().await;
    Mock::given(path("/models/ggml-test.bin"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(&BODY[..10]))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let downloader = ModelDownloader::new(dir.path());
    let result = downloader
        .download(
            &spec(&server, sha256_hex(BODY)),
            |_, _| {},
            CancellationToken::new(),
        )
        .await;
    assert_eq!(result, Err(DownloadError::SizeMismatch));

    let missing = ModelSpec::for_test(
        "missing",
        format!("{}/nope", server.uri()),
        1,
        sha256_hex(b"x"),
    );
    let result = downloader
        .download(&missing, |_, _| {}, CancellationToken::new())
        .await;
    assert_eq!(result, Err(DownloadError::Http(404)));
    assert!(files_in(dir.path()).is_empty());
}

#[tokio::test]
async fn nothing_is_requested_without_an_explicit_download() {
    let server = server_with_model().await;
    let dir = tempfile::tempdir().unwrap();
    let downloader = ModelDownloader::new(dir.path());
    let spec = spec(&server, sha256_hex(BODY));
    // Browsing the catalog and checking local state never touches the network.
    assert!(!model_download::catalog().is_empty());
    assert!(!downloader.is_downloaded(&spec));
    let _ = downloader.path_for(&spec);
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn the_request_carries_no_session_data() {
    let server = server_with_model().await;
    let dir = tempfile::tempdir().unwrap();
    let downloader = ModelDownloader::new(dir.path());
    downloader
        .download(
            &spec(&server, sha256_hex(BODY)),
            |_, _| {},
            CancellationToken::new(),
        )
        .await
        .unwrap();
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method.as_str(), "GET");
    assert_eq!(request.url.path(), "/models/ggml-test.bin");
    assert_eq!(request.url.query(), None);
    assert!(request.body.is_empty());
    for header in ["authorization", "cookie", "x-session-id", "content-type"] {
        assert!(
            !request.headers.contains_key(header),
            "unexpected header {header}"
        );
    }
}

#[tokio::test]
async fn cancelled_download_leaves_no_file() {
    let server = MockServer::start().await;
    Mock::given(path("/models/ggml-test.bin"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(BODY)
                .set_delay(std::time::Duration::from_secs(10)),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let downloader = ModelDownloader::new(dir.path());
    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        token.cancel();
    });
    let result = downloader
        .download(&spec(&server, sha256_hex(BODY)), |_, _| {}, cancel)
        .await;
    assert_eq!(result, Err(DownloadError::Cancelled));
    assert!(files_in(dir.path()).is_empty());
}

#[test]
fn pinned_catalog_requires_https() {
    for spec in model_download::catalog() {
        assert!(spec.url().starts_with("https://"));
    }
}
