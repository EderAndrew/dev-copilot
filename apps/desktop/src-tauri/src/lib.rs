//! Composition root of the Dev Copilot desktop app.
//!
//! Commands registered here stay thin: they validate input and delegate to application use
//! cases (Constitution Art. VII.4).

pub mod commands;
pub mod composition;
pub mod dto;
pub mod events;

use tauri::Manager;

/// Builds and runs the Tauri application until the last window closes.
///
/// # Errors
///
/// Returns the Tauri error when the runtime cannot be initialized or fails while running.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = composition::build(app.handle());
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::settings_get,
            commands::settings::settings_update,
            commands::providers::provider_list,
            commands::providers::provider_upsert,
            commands::providers::provider_set_default,
        ])
        .run(tauri::generate_context!())
}
