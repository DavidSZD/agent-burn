use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::sleep;

use crate::app::{resolve_cli_path_with_override, AppState};

pub fn spawn_quota_collector(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Relevé initial rapide après 2 secondes pour archiver et mettre à jour le tray
        sleep(Duration::from_secs(2)).await;
        collect_and_archive(&app).await;

        loop {
            let minutes = app
                .state::<AppState>()
                .settings
                .read()
                .map(|settings| settings.refresh_minutes)
                .unwrap_or(1);
            sleep(Duration::from_secs(minutes.max(1) * 60)).await;
            collect_and_archive(&app).await;
        }
    });
}

async fn collect_and_archive(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = match state.settings.read() {
        Ok(settings) => settings.clone(),
        Err(_) => return,
    };
    let cli = resolve_cli_path_with_override(settings.custom_cli_path.as_deref())
        .or_else(|| state.cli_path.clone());
    if let Ok(data) = crate::commands::build_summary("all", &settings, cli.as_deref()).await {
        // Émission de l'événement vers l'UI
        let _ = app.emit("quotas_updated", &data);

        // Archivage persistant sur disque (%LOCALAPPDATA%/Agent Burn/)
        crate::archive::record_quota_sample(&data);
        crate::archive::record_metrics_snapshot(&data);

        // Mise à jour du tooltip du System Tray
        let sub = data.get("subscription");
        let sub_agents = sub.and_then(|s| s.get("agents")).and_then(|a| a.as_array());

        let mut parts = Vec::new();

        if let Some(agents) = sub_agents {
            for a in agents {
                let name = a.get("agent").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(used_pct) = a
                    .get("window")
                    .and_then(|win| win.get("usedPercent"))
                    .and_then(|u| u.as_f64())
                {
                    let remaining = (100.0f64 - used_pct).max(0.0f64);
                    parts.push(format!("{}: {:.0}%", name, remaining));
                }
            }
        }

        if !parts.is_empty() {
            let tooltip = format!("Agent Burn - {}", parts.join(" | "));
            crate::tray::update_tray_tooltip(app, &tooltip);
        }
    }
}
