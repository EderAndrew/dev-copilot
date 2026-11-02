// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    match devcopilot_desktop_lib::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("Dev Copilot failed to start: {err}");
            ExitCode::FAILURE
        }
    }
}
