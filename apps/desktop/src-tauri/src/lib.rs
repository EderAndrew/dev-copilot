//! Composition root of the Dev Copilot desktop app.
//!
//! Commands registered here stay thin: they validate input and delegate to application use
//! cases (Constitution Art. VII.4).

pub mod commands;
pub mod composition;
pub mod dto;
pub mod events;
pub mod metrics;
pub mod models;
pub mod updater;

use tauri::Manager;

/// Builds and runs the Tauri application until the last window closes.
///
/// # Errors
///
/// Returns the Tauri error when the runtime cannot be initialized or fails while running.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Driven from the core only (`updater::TauriUpdater`); no capability grants
        // `updater:*` to the webview (002 research R5, tests/capabilities.rs).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(commands::diagnostics::CspReports::default())
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
            commands::providers::provider_set_secret,
            commands::providers::provider_authorization_preview,
            commands::providers::provider_authorize,
            commands::providers::provider_revoke,
            commands::providers::local_model_list,
            commands::providers::local_model_download,
            commands::records::record_set_status,
            commands::records::record_edit,
            commands::records::record_create,
            commands::records::plan_get,
            commands::records::analysis_run_final,
            commands::questions::question_ask,
            commands::questions::question_to_record,
            commands::sessions::session_list,
            commands::sessions::session_get,
            commands::sessions::session_rename,
            commands::sessions::session_delete,
            commands::export::export_session,
            commands::sessions::session_create,
            commands::sessions::session_start,
            commands::sessions::session_cancel_start,
            commands::sessions::session_pause,
            commands::sessions::session_resume,
            commands::sessions::session_stop,
            commands::sessions::session_subscribe,
            commands::audio::audio_sources_list,
            commands::audio::audio_permission_request,
            commands::audio::audio_permission_help,
            commands::diagnostics::diagnostics_report_csp_violation,
            commands::updates::update_status_get,
            commands::updates::update_check,
            commands::updates::update_install,
            commands::updates::update_cancel,
            commands::updates::update_notice_acknowledge,
            commands::updates::update_subscribe,
        ])
        .build(tauri::generate_context!())?;
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event
            && let Some(state) = handle.try_state::<composition::AppState>()
        {
            state.shutdown.cancel();
        }
    });
    Ok(())
}
