//! Composition root of the Dev Copilot desktop app.
//!
//! Commands registered here stay thin: they validate input and delegate to application use
//! cases (Constitution Art. VII.4).

/// Builds and runs the Tauri application until the last window closes.
///
/// # Errors
///
/// Returns the Tauri error when the runtime cannot be initialized or fails while running.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .run(tauri::generate_context!())
}
