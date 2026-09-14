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

#[cfg(windows)]
fn ensure_single_instance() -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    extern "system" {
        fn CreateMutexW(lpMutexAttributes: *mut std::ffi::c_void, bInitialOwner: i32, lpName: *const u16) -> *mut std::ffi::c_void;
        fn GetLastError() -> u32;
        fn FindWindowW(lpClassName: *const u16, lpWindowName: *const u16) -> *mut std::ffi::c_void;
        fn ShowWindow(hWnd: *mut std::ffi::c_void, nCmdShow: i32) -> i32;
        fn SetForegroundWindow(hWnd: *mut std::ffi::c_void) -> i32;
    }

    let mutex_name: Vec<u16> = OsStr::new("Local\\AgentBurn_Windows_SingleInstance_Mutex")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let handle = CreateMutexW(std::ptr::null_mut(), 1, mutex_name.as_ptr());
        if !handle.is_null() && GetLastError() == 183 {
            // Une instance active est déjà en cours d'exécution : la restaurer et la mettre au premier plan
            let class_name: Vec<u16> = OsStr::new("Tauri Window")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let title: Vec<u16> = OsStr::new("Agent Burn")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let mut hwnd = FindWindowW(class_name.as_ptr(), title.as_ptr());
            if hwnd.is_null() {
                hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
            }
            if !hwnd.is_null() {
                ShowWindow(hwnd, 9); // SW_RESTORE = 9
                SetForegroundWindow(hwnd);
            }
            return false;
        }
    }
    true
}

fn main() {
    #[cfg(windows)]
    if !ensure_single_instance() {
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::new())
        .setup(|app| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.unminimize();
                let _ = win.show();
                let _ = win.set_focus();
            }
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
