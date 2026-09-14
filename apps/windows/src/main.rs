#![windows_subsystem = "windows"]

mod app;
mod archive;
mod autostart;
mod background;
mod commands;
mod projects;
mod antigravity;
mod tray;

use app::AppState;
use tauri::{Manager, WindowEvent};

fn main() {

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::new())
        .setup(|app| {
            let log_path = "startup.log";
            let mut log = format!("Setup démarré à {}\n", chrono::Local::now());
            let win = app.get_webview_window("main");
            log.push_str(&format!("Fenêtre 'main' trouvée : {}\n", win.is_some()));
            if let Some(ref w) = win {
                log.push_str(&format!("Visibilité initiale : {:?}\n", w.is_visible()));
                log.push_str(&format!("Résultat show() : {:?}\n", w.show()));
                log.push_str(&format!("Résultat set_focus() : {:?}\n", w.set_focus()));
            } else {
                log.push_str("ATTENTION: Fenêtre 'main' INTROUVABLE !\n");
            }
            let _ = std::fs::write(log_path, &log);

            let handle = app.handle();
            let _ = tray::setup_tray(handle);
            background::spawn_quota_collector(handle.clone());
            Ok(())
        })
        .on_window_event(|_window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                std::process::exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_summary,
            commands::get_harness,
            commands::get_cli_status,
            commands::get_autostart_status,
            commands::set_autostart,
            commands::get_quota_history,
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
