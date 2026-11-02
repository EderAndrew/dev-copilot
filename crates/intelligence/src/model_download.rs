//! Explicit download of local transcription models (research R4).
//!
//! - Only models in the pinned [`catalog`] can be downloaded; each entry fixes the URL (a
//!   specific repository revision), the size and the SHA-256 of the file.
//! - Nothing is requested until [`ModelDownloader::download`] is called by an explicit user
//!   action. The request is a plain `GET` of the pinned URL: no body, query, cookies or
//!   credentials, and no session data.
//! - The file is written to `<name>.part`, hashed while streaming and moved into place only if
//!   size and hash match; otherwise it is deleted.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

/// Revision of `huggingface.co/ggerganov/whisper.cpp` the catalog is pinned to.
const REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

macro_rules! pinned {
    ($id:literal, $name:literal, $file:literal, $size:literal, $sha:literal) => {
        ModelSpec {
            id: Cow::Borrowed($id),
            display_name: Cow::Borrowed($name),
            file_name: Cow::Borrowed($file),
            url: Cow::Borrowed(concat!(
                "https://huggingface.co/ggerganov/whisper.cpp/resolve/",
                "5359861c739e955e79d9a303bcbc70fb988958b1/",
                $file
            )),
            size_bytes: $size,
            sha256: Cow::Borrowed($sha),
            require_https: true,
        }
    };
}

/// Pinned whisper.cpp models (multilingual, so pt-BR and en both work).
static CATALOG: [ModelSpec; 4] = [
    pinned!(
        "tiny",
        "Whisper tiny",
        "ggml-tiny.bin",
        77_691_713,
        "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"
    ),
    pinned!(
        "base",
        "Whisper base",
        "ggml-base.bin",
        147_951_465,
        "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe"
    ),
    pinned!(
        "small",
        "Whisper small",
        "ggml-small.bin",
        487_601_967,
        "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b"
    ),
    pinned!(
        "large-v3-turbo-q5_0",
        "Whisper large-v3 turbo (q5_0)",
        "ggml-large-v3-turbo-q5_0.bin",
        574_041_195,
        "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2"
    ),
];

/// A downloadable model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSpec {
    id: Cow<'static, str>,
    display_name: Cow<'static, str>,
    file_name: Cow<'static, str>,
    url: Cow<'static, str>,
    size_bytes: u64,
    sha256: Cow<'static, str>,
    require_https: bool,
}

impl ModelSpec {
    /// Stable id, stored as the provider's `model`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Name shown to the user.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// File name inside the models directory.
    #[must_use]
    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    /// Pinned URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Host the model is downloaded from, shown to the user.
    #[must_use]
    pub fn origin(&self) -> &str {
        let rest = self
            .url
            .split_once("://")
            .map_or(&*self.url, |(_, rest)| rest);
        rest.split(['/', ':']).next().unwrap_or(rest)
    }

    /// Expected size.
    #[must_use]
    pub const fn size_bytes(&self) -> u64 {
        self.size_bytes
    }

    /// Expected SHA-256, lowercase hex.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Catalog entry pointing at a test server (plain HTTP allowed).
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn for_test(id: &str, url: String, size_bytes: u64, sha256_hex: String) -> Self {
        Self {
            id: Cow::Owned(id.to_owned()),
            display_name: Cow::Owned(id.to_owned()),
            file_name: Cow::Owned(format!("ggml-{id}.bin")),
            url: Cow::Owned(url),
            size_bytes,
            sha256: Cow::Owned(sha256_hex),
            require_https: false,
        }
    }
}

/// Every downloadable model.
#[must_use]
pub fn catalog() -> &'static [ModelSpec] {
    &CATALOG
}

/// The catalog entry for `id`.
#[must_use]
pub fn find(id: &str) -> Option<&'static ModelSpec> {
    CATALOG.iter().find(|spec| spec.id == id)
}

/// Repository revision the catalog is pinned to.
#[must_use]
pub const fn revision() -> &'static str {
    REVISION
}

/// Why a download failed; never contains file contents.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DownloadError {
    /// The server answered with a non-success status.
    #[error("download failed with HTTP status {0}")]
    Http(u16),
    /// Connection or TLS failure.
    #[error("network error: {0}")]
    Network(String),
    /// The file is not the expected size.
    #[error("downloaded model has an unexpected size")]
    SizeMismatch,
    /// The file does not match the pinned SHA-256; it was deleted.
    #[error("downloaded model failed hash verification")]
    HashMismatch,
    /// Local file-system failure.
    #[error("cannot write model file: {0}")]
    Io(String),
    /// Cancelled by the user.
    #[error("download cancelled")]
    Cancelled,
}

fn io_error(err: &std::io::Error) -> DownloadError {
    DownloadError::Io(err.kind().to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Downloads catalog models into one directory.
#[derive(Debug, Clone)]
pub struct ModelDownloader {
    models_dir: PathBuf,
}

impl ModelDownloader {
    /// Downloader writing into `models_dir`; sends nothing until [`Self::download`].
    #[must_use]
    pub fn new(models_dir: impl Into<PathBuf>) -> Self {
        Self {
            models_dir: models_dir.into(),
        }
    }

    /// Directory holding the models.
    #[must_use]
    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// Where `spec` is stored once downloaded.
    #[must_use]
    pub fn path_for(&self, spec: &ModelSpec) -> PathBuf {
        self.models_dir.join(spec.file_name())
    }

    /// Whether `spec` is downloaded (present with the expected size; the hash was checked
    /// when it was written).
    #[must_use]
    pub fn is_downloaded(&self, spec: &ModelSpec) -> bool {
        std::fs::metadata(self.path_for(spec))
            .is_ok_and(|meta| meta.is_file() && meta.len() == spec.size_bytes())
    }

    /// Downloads and verifies `spec`, reporting `(received, total)` bytes to `progress`.
    ///
    /// # Errors
    ///
    /// See [`DownloadError`]; no partial file is left behind on any error.
    pub async fn download(
        &self,
        spec: &ModelSpec,
        progress: impl Fn(u64, u64) + Send,
        cancel: CancellationToken,
    ) -> Result<PathBuf, DownloadError> {
        tokio::fs::create_dir_all(&self.models_dir)
            .await
            .map_err(|e| io_error(&e))?;
        let target = self.path_for(spec);
        let part = target.with_extension("bin.part");
        let result = tokio::select! {
            () = cancel.cancelled() => Err(DownloadError::Cancelled),
            result = fetch(spec, &part, &progress) => result,
        };
        match result {
            Ok(()) => {
                tokio::fs::rename(&part, &target)
                    .await
                    .map_err(|e| io_error(&e))?;
                tracing::info!(model = spec.id(), "model downloaded and verified");
                Ok(target)
            }
            Err(err) => {
                // Best effort: the part file may not exist yet.
                let _ = tokio::fs::remove_file(&part).await;
                tracing::warn!(model = spec.id(), error = %err, "model download failed");
                Err(err)
            }
        }
    }
}

async fn fetch(
    spec: &ModelSpec,
    part: &Path,
    progress: &(impl Fn(u64, u64) + Send),
) -> Result<(), DownloadError> {
    let client = reqwest::Client::builder()
        .https_only(spec.require_https)
        .build()
        .map_err(|e| DownloadError::Network(e.to_string()))?;
    let mut response = client
        .get(spec.url())
        .send()
        .await
        .map_err(|e| DownloadError::Network(e.without_url().to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::Http(status.as_u16()));
    }
    let total = spec.size_bytes();
    if response.content_length().is_some_and(|len| len != total) {
        return Err(DownloadError::SizeMismatch);
    }
    let mut file = tokio::fs::File::create(part)
        .await
        .map_err(|e| io_error(&e))?;
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    progress(0, total);
    while let Some(bytes) = response
        .chunk()
        .await
        .map_err(|e| DownloadError::Network(e.without_url().to_string()))?
    {
        received += bytes.len() as u64;
        if received > total {
            return Err(DownloadError::SizeMismatch);
        }
        hasher.update(&bytes);
        file.write_all(&bytes).await.map_err(|e| io_error(&e))?;
        progress(received, total);
    }
    file.flush().await.map_err(|e| io_error(&e))?;
    file.sync_all().await.map_err(|e| io_error(&e))?;
    if received != total {
        return Err(DownloadError::SizeMismatch);
    }
    if hex(&hasher.finalize()) != spec.sha256() {
        return Err(DownloadError::HashMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_pinned_to_https_revision_urls_with_sha256() {
        for spec in catalog() {
            assert!(spec.url().starts_with("https://huggingface.co/"));
            assert!(spec.url().contains(revision()));
            assert!(spec.url().ends_with(spec.file_name()));
            assert_eq!(spec.sha256().len(), 64);
            assert!(
                spec.sha256()
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            );
            assert!(spec.require_https);
            assert_eq!(spec.origin(), "huggingface.co");
            assert_eq!(find(spec.id()), Some(spec));
        }
        assert_eq!(find("../etc/passwd"), None);
    }

    #[test]
    fn is_downloaded_requires_the_expected_size() {
        let dir = std::env::temp_dir().join(format!("devcopilot-models-{}", std::process::id()));
        let downloader = ModelDownloader::new(&dir);
        let spec = &catalog()[0];
        assert!(!downloader.is_downloaded(spec));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(downloader.path_for(spec), b"short").unwrap();
        assert!(!downloader.is_downloaded(spec));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
