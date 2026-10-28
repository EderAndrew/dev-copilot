//! `export_session` (contracts/ipc-commands.md "Exportação", FR-041).
//!
//! The core opens the native save dialog — one per format — and writes each file only to the
//! path the user chose there; the frontend never sees or supplies a path, and has no file
//! system access. If any dialog is cancelled, nothing is written. Paths are never logged,
//! stored or returned: the response carries file names only.

use std::collections::BTreeSet;
use std::path::PathBuf;

use devcopilot_application::ApplicationError;
use devcopilot_application::export_service::ExportRequest;
use devcopilot_application::ports::{BoxFuture, RepositoryError};
use devcopilot_domain::export::{ExportContent, ExportFormat};
use devcopilot_domain::ids::SessionId;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;

use super::IpcError;
use crate::composition::{AppState, Core};
use crate::dto::Code;

/// `export_session` input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ExportSessionDto {
    pub session_id: String,
    pub formats: Vec<Code<ExportFormat>>,
    pub content: Vec<Code<ExportContent>>,
}

/// `export_session` output: names of the files written, without directories; empty when the
/// user cancelled.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(missing_docs)]
pub struct ExportResultDto {
    pub written: Vec<String>,
}

/// Asks the user where to save one file.
pub trait SaveDialog: Send + Sync {
    /// The path chosen for a file of `format` suggested as `file_name`; `None` when cancelled.
    fn choose(&self, file_name: &str, format: ExportFormat) -> BoxFuture<'_, Option<PathBuf>>;
}

/// The OS save dialog (`tauri-plugin-dialog`, opened from Rust).
pub struct NativeSaveDialog<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> NativeSaveDialog<R> {
    /// Dialogs parented to the app.
    #[must_use]
    pub const fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> SaveDialog for NativeSaveDialog<R> {
    fn choose(&self, file_name: &str, format: ExportFormat) -> BoxFuture<'_, Option<PathBuf>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let filter = match format {
            ExportFormat::Markdown => "Markdown",
            ExportFormat::Json => "JSON",
        };
        self.app
            .dialog()
            .file()
            .set_file_name(file_name)
            .add_filter(filter, &[format.extension()])
            .save_file(move |path| {
                // The command may be gone if the window closed; nothing to report to then.
                let _ = tx.send(path);
            });
        Box::pin(async move {
            rx.await
                .ok()
                .flatten()
                .and_then(|path| path.into_path().ok())
        })
    }
}

fn request_of(dto: ExportSessionDto) -> Result<ExportRequest, IpcError> {
    let session_id = SessionId::parse(&dto.session_id)?;
    let formats: BTreeSet<ExportFormat> = dto.formats.into_iter().map(|c| c.0).collect();
    let content: BTreeSet<ExportContent> = dto.content.into_iter().map(|c| c.0).collect();
    if formats.is_empty() {
        return Err(IpcError::invalid_input("formats", "empty"));
    }
    if content.is_empty() {
        return Err(IpcError::invalid_input("content", "empty"));
    }
    Ok(ExportRequest {
        session_id,
        formats,
        content,
    })
}

/// Asks where to save each format, then renders the persisted content and writes each file
/// to its chosen path, and records the export.
///
/// # Errors
///
/// `invalid_input` (id, empty formats or content, the same path chosen twice), `not_found`,
/// `storage_error` when a file cannot be written.
pub async fn export(
    core: &Core,
    dialog: &dyn SaveDialog,
    request: ExportSessionDto,
) -> Result<ExportResultDto, IpcError> {
    let request = request_of(request)?;
    let names = core
        .export
        .file_names(request.session_id, &request.formats)?;
    let mut targets = Vec::with_capacity(names.len());
    for (format, name) in names {
        let Some(path) = dialog.choose(&name, format).await else {
            tracing::info!(session = %request.session_id, "export cancelled");
            return Ok(ExportResultDto::default());
        };
        if targets.iter().any(|(_, chosen)| chosen == &path) {
            return Err(IpcError::invalid_input("path", "inconsistent"));
        }
        targets.push((format, path));
    }
    // Rendered after the dialogs, so the files hold what is persisted when they are written.
    let documents = core.export.render(&request)?;
    let mut written = Vec::with_capacity(targets.len());
    for (format, path) in &targets {
        let Some(document) = documents.iter().find(|doc| doc.format == *format) else {
            continue;
        };
        std::fs::write(path, document.contents.expose()).map_err(|err| {
            ApplicationError::Storage(RepositoryError::Storage(format!(
                "export file not written: {}",
                err.kind()
            )))
        })?;
        written.push(path.file_name().map_or_else(
            || document.file_name.clone(),
            |n| n.to_string_lossy().into_owned(),
        ));
    }
    core.export.record(&request)?;
    Ok(ExportResultDto { written })
}

/// Exports a session to files the user chooses (FR-041).
///
/// # Errors
///
/// See [`export`].
#[tauri::command]
pub async fn export_session(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ExportSessionDto,
) -> Result<ExportResultDto, IpcError> {
    export(state.core()?, &NativeSaveDialog::new(app), request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_rejects_unknown_formats_content_and_fields() {
        let ok: ExportSessionDto = serde_json::from_str(
            r#"{"sessionId":"x","formats":["markdown","json"],"content":["plan"]}"#,
        )
        .unwrap();
        assert_eq!(ok.formats.len(), 2);
        for bad in [
            r#"{"sessionId":"x","formats":["csv"],"content":["plan"]}"#,
            r#"{"sessionId":"x","formats":["json"],"content":["audio"]}"#,
            r#"{"sessionId":"x","formats":["json"],"content":["plan"],"path":"/tmp/x"}"#,
        ] {
            assert!(
                serde_json::from_str::<ExportSessionDto>(bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn empty_selections_are_invalid() {
        let id = SessionId::new().to_string();
        let err = request_of(ExportSessionDto {
            session_id: id.clone(),
            formats: Vec::new(),
            content: vec![Code(ExportContent::Plan)],
        })
        .unwrap_err();
        assert_eq!(
            err.details.get("field").map(String::as_str),
            Some("formats")
        );
        let err = request_of(ExportSessionDto {
            session_id: id,
            formats: vec![Code(ExportFormat::Json), Code(ExportFormat::Json)],
            content: Vec::new(),
        })
        .unwrap_err();
        assert_eq!(
            err.details.get("field").map(String::as_str),
            Some("content")
        );
    }
}
