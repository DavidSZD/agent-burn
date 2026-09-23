#![windows_subsystem = "windows"]

mod antigravity;
mod antigravity_cloud;
mod app;
mod archive;
mod autostart;
mod background;
mod commands;
mod pricing;
mod projects;
mod tray;

use app::AppState;
use tauri::{Manager, WindowEvent};

#[derive(Debug, PartialEq, Eq)]
enum CloseAction {
    HideWindow,
}

fn close_action() -> CloseAction {
    CloseAction::HideWindow
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState::new())
        .setup(|app| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
            let handle = app.handle();
            tray::setup_tray(handle)?;
            background::spawn_quota_collector(handle.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                match close_action() {
                    CloseAction::HideWindow => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_summary,
            commands::get_summary_since,
            commands::get_harness,
            commands::get_cli_status,
            commands::get_settings,
            commands::set_settings,
            commands::get_autostart_status,
            commands::set_autostart,
            commands::get_quota_history,
            commands::get_refresh_snapshot,
            commands::get_refresh_revision,
            commands::open_data_folder,
            commands::get_projects_usage,
            commands::open_project_folder,
            commands::get_report_cache,
            commands::save_report_cache,
            commands::get_antigravity_summary,
        ])
        .run(tauri::generate_context!())
        .expect("Erreur lors de l'exécution de l'application Agent Burn Windows");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_dashboard_keeps_the_tray_process_running() {
        assert_eq!(close_action(), CloseAction::HideWindow);
    }
}
