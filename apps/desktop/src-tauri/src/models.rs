//! Local transcription models: the pinned catalog, local state and explicit downloads
//! (research R4, ADR-003).
//!
//! Downloads start only from `local_model_download` and report progress to the caller's
//! channel. At most one download runs per model.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use devcopilot_intelligence::model_download::{self, DownloadError, ModelDownloader, ModelSpec};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::commands::IpcError;

/// A catalog model as listed to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct LocalModelDto {
    /// Catalog id; used as the `whisper_local` provider's `model`.
    pub id: String,
    pub display_name: String,
    /// Host the file is downloaded from.
    pub origin: String,
    pub size_bytes: u64,
    pub downloaded: bool,
    pub downloading: bool,
}

/// Progress of one download, delivered over the command's channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
#[allow(missing_docs)]
pub enum ModelDownloadEvent {
    Progress {
        job_id: String,
        received_bytes: u64,
        total_bytes: u64,
    },
    Completed {
        job_id: String,
    },
    Failed {
        job_id: String,
        message_key: String,
    },
}

/// Receiver of download progress (the IPC channel in the app, a recorder in tests).
pub trait DownloadSink: Send + Sync {
    /// Delivers `event`; `false` once the receiver is gone.
    fn deliver(&self, event: ModelDownloadEvent) -> bool;
}

impl DownloadSink for tauri::ipc::Channel<ModelDownloadEvent> {
    fn deliver(&self, event: ModelDownloadEvent) -> bool {
        self.send(event).is_ok()
    }
}

/// i18n key for a failed download.
fn failure_key(error: &DownloadError) -> &'static str {
    match error {
        DownloadError::Http(_) | DownloadError::Network(_) => "errors.modelDownloadNetwork",
        DownloadError::SizeMismatch | DownloadError::HashMismatch => {
            "errors.modelVerificationFailed"
        }
        DownloadError::Io(_) => "errors.modelDownloadStorage",
        DownloadError::Cancelled => "errors.modelDownloadCancelled",
    }
}

/// Catalog, local state and downloads.
pub struct LocalModels {
    downloader: ModelDownloader,
    /// Model id → cancellation of its running download.
    running: Mutex<HashMap<String, CancellationToken>>,
}

impl LocalModels {
    /// Models stored in `models_dir`.
    #[must_use]
    pub fn new(models_dir: impl Into<PathBuf>) -> Self {
        Self {
            downloader: ModelDownloader::new(models_dir),
            running: Mutex::new(HashMap::new()),
        }
    }

    fn running(&self) -> MutexGuard<'_, HashMap<String, CancellationToken>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every catalog model with its local state.
    #[must_use]
    pub fn list(&self) -> Vec<LocalModelDto> {
        let running = self.running();
        model_download::catalog()
            .iter()
            .map(|spec| LocalModelDto {
                id: spec.id().to_owned(),
                display_name: spec.display_name().to_owned(),
                origin: spec.origin().to_owned(),
                size_bytes: spec.size_bytes(),
                downloaded: self.downloader.is_downloaded(spec),
                downloading: running.contains_key(spec.id()),
            })
            .collect()
    }

    /// Starts downloading catalog model `model_id`; returns the job id.
    ///
    /// # Errors
    ///
    /// `invalid_input` (field `model_id`) for ids outside the catalog; `invalid_state` when
    /// it is already downloading; `internal` outside an async runtime.
    pub fn start_download(
        self: &Arc<Self>,
        model_id: &str,
        sink: Box<dyn DownloadSink>,
    ) -> Result<String, IpcError> {
        let spec = model_download::find(model_id)
            .ok_or_else(|| IpcError::invalid_input("model_id", "malformed"))?;
        self.start(spec.clone(), sink)
    }

    /// [`Self::start_download`] for any spec (tests point it at a local server).
    ///
    /// # Errors
    ///
    /// See [`Self::start_download`].
    pub fn start(
        self: &Arc<Self>,
        spec: ModelSpec,
        sink: Box<dyn DownloadSink>,
    ) -> Result<String, IpcError> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            IpcError::from(devcopilot_application::ApplicationError::Internal(
                "no async runtime".into(),
            ))
        })?;
        let cancel = CancellationToken::new();
        {
            let mut running = self.running();
            if running.contains_key(spec.id()) {
                return Err(IpcError::from(
                    devcopilot_application::ApplicationError::InvalidState(
                        "model is already downloading".into(),
                    ),
                ));
            }
            running.insert(spec.id().to_owned(), cancel.clone());
        }
        let job_id = Uuid::now_v7().to_string();
        let this = Arc::clone(self);
        let job = job_id.clone();
        runtime.spawn(async move {
            let last = Mutex::new(0_u64);
            let result = this
                .downloader
                .download(
                    &spec,
                    |received, total| {
                        // About one event per percent, plus the first and the last.
                        let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
                        let step = (total / 100).max(1);
                        if received == 0 || received == total || received >= *last + step {
                            *last = received;
                            sink.deliver(ModelDownloadEvent::Progress {
                                job_id: job.clone(),
                                received_bytes: received,
                                total_bytes: total,
                            });
                        }
                    },
                    cancel,
                )
                .await;
            this.running().remove(spec.id());
            sink.deliver(match result {
                Ok(_) => ModelDownloadEvent::Completed { job_id: job },
                Err(err) => ModelDownloadEvent::Failed {
                    job_id: job,
                    message_key: failure_key(&err).to_owned(),
                },
            });
        });
        Ok(job_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_the_catalog_without_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let models = LocalModels::new(dir.path());
        let listed = models.list();
        assert_eq!(listed.len(), model_download::catalog().len());
        assert!(listed.iter().all(|m| !m.downloaded && !m.downloading));
        assert!(listed.iter().all(|m| m.origin == "huggingface.co"));
    }

    #[tokio::test]
    async fn unknown_models_are_rejected() {
        struct Nothing;
        impl DownloadSink for Nothing {
            fn deliver(&self, _: ModelDownloadEvent) -> bool {
                true
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let models = Arc::new(LocalModels::new(dir.path()));
        let err = models
            .start_download("../../etc/passwd", Box::new(Nothing))
            .unwrap_err();
        assert_eq!(err.code, "invalid_input");
    }

    #[test]
    fn event_shape_follows_the_contract() {
        let json = serde_json::to_value(ModelDownloadEvent::Progress {
            job_id: "j".into(),
            received_bytes: 1,
            total_bytes: 2,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"type":"progress","jobId":"j","receivedBytes":1,"totalBytes":2})
        );
    }
}
