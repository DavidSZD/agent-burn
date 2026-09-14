use crate::app::{execute_cli_json, AppState};
use tauri::State;

#[tauri::command]
pub async fn get_summary(
    period: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let mut args = vec!["summary", "--value"];
    let period_val = period.unwrap_or_else(|| "mtd".to_string());
    if period_val != "all" && !period_val.is_empty() {
        args.push(&period_val);
    }

    execute_cli_json(state.cli_path.as_deref(), &args).await
}

#[tauri::command]
pub async fn get_harness(
    agent: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let args = vec!["harness", &agent, "--value"];
    execute_cli_json(state.cli_path.as_deref(), &args).await
}

#[tauri::command]
pub fn get_cli_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "available": state.cli_path.is_some(),
        "path": state.cli_path.as_ref().map(|p| p.to_string_lossy().to_string()),
        "dataDir": crate::archive::get_data_dir().to_string_lossy().to_string(),
    }))
}

#[tauri::command]
pub fn get_autostart_status() -> Result<bool, String> {
    Ok(crate::autostart::is_autostart_enabled())
}

#[tauri::command]
pub fn set_autostart(enabled: bool) -> Result<bool, String> {
    crate::autostart::set_autostart_enabled(enabled)?;
    Ok(crate::autostart::is_autostart_enabled())
}

#[tauri::command]
pub fn get_quota_history() -> Result<serde_json::Value, String> {
    Ok(crate::archive::load_quota_history())
}

#[tauri::command]
pub fn open_data_folder() -> Result<(), String> {
    crate::archive::open_data_folder()
}

#[tauri::command]
pub async fn get_projects_usage(period: Option<String>) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        crate::projects::get_projects_usage(period.as_deref())
    })
    .await
    .map_err(|e| format!("Erreur tâche d'analyse projets: {e}"))?
}

#[tauri::command]
pub fn open_project_folder(path: String) -> Result<(), String> {
    crate::projects::open_folder_in_explorer(&path)
}

#[tauri::command]
pub fn get_report_cache() -> Result<serde_json::Value, String> {
    Ok(crate::archive::load_report_cache())
}

#[tauri::command]
pub fn save_report_cache(data: serde_json::Value) -> Result<(), String> {
    crate::archive::save_report_cache(&data);
    Ok(())
}

#[tauri::command]
pub async fn get_antigravity_summary(period: Option<String>) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || {
        crate::antigravity::get_antigravity_data(period.as_deref())
            .map(|data| serde_json::to_value(data).unwrap_or(serde_json::Value::Null))
    })
    .await
    .map_err(|e| format!("Erreur tâche d'analyse Antigravity: {e}"))?
}

