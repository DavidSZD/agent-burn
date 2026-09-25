use std::{sync::atomic::Ordering, time::Duration};
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::{sleep_until, timeout, Instant};

fn refresh_delay(minutes: u64) -> Duration {
    Duration::from_secs(minutes.max(1) * 60)
}

fn next_refresh_at(anchor: Instant, minutes: u64) -> Instant {
    anchor + refresh_delay(minutes)
}

use crate::app::{resolve_cli_path_with_override, AppState, RefreshSnapshot};

const FAST_AGENT_LIST: &str =
    "codex,claude,cursor,gemini,hermes,opencode,openclaw,pi,kimi,qwen,amp,codebuff,droid,goose,kilo,copilot";

pub fn spawn_quota_collector(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let mut next_refresh = Instant::now() + Duration::from_secs(2);
        loop {
            tokio::select! {
                _ = sleep_until(next_refresh) => {
                    let scan_lock = state.scan_lock.lock();
                    tokio::pin!(scan_lock);
                    let _scan = tokio::select! {
                        _ = state.refresh_schedule_changed.notified() => {
                            let minutes = current_refresh_minutes(&state);
                            next_refresh = next_refresh_at(Instant::now(), minutes);
                            continue;
                        }
                        guard = &mut scan_lock => guard,
                    };
                    let cycle_started = Instant::now();
                    let cycle_started_at_ms = chrono::Utc::now().timestamp_millis();
                    run_refresh(&app, false).await;
                    drop(_scan);
                    next_refresh = next_refresh_at(cycle_started, current_refresh_minutes(&state));
                    emit_next_refresh(&app, cycle_started_at_ms + refresh_delay(current_refresh_minutes(&state)).as_millis() as i64);
                }
                _ = state.refresh_schedule_changed.notified() => {
                    next_refresh = next_refresh_at(Instant::now(), current_refresh_minutes(&state));
                    emit_next_refresh(&app, chrono::Utc::now().timestamp_millis() + refresh_delay(current_refresh_minutes(&state)).as_millis() as i64);
                }
            }
        }
    });
}

fn current_refresh_minutes(state: &AppState) -> u64 {
    state
        .settings
        .read()
        .map(|settings| settings.refresh_minutes)
        .unwrap_or(1)
}

pub(crate) async fn run_refresh(app: &AppHandle, manual: bool) -> bool {
    let started_at_ms = chrono::Utc::now().timestamp_millis();
    let generation = app
        .state::<AppState>()
        .refresh_generation
        .fetch_add(1, Ordering::Relaxed)
        + 1;
    let _ = app.emit(
        "refresh_started",
        refresh_started_payload(generation, started_at_ms, manual),
    );
    let success = collect_and_archive(app, generation).await;
    let finished_at_ms = chrono::Utc::now().timestamp_millis();
    let _ = app.emit(
        "refresh_finished",
        refresh_finished_payload(generation, started_at_ms, finished_at_ms, success, manual),
    );
    success
}

fn emit_next_refresh(app: &AppHandle, next_refresh_at_ms: i64) {
    let _ = app.emit(
        "refresh_schedule_updated",
        serde_json::json!({ "nextRefreshAtMs": next_refresh_at_ms }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_refresh_waits_for_the_full_interval_after_collection() {
        let finished = Instant::now();
        assert_eq!(
            next_refresh_at(finished, 5),
            finished + Duration::from_secs(300)
        );
    }

    #[test]
    fn a_slow_scan_does_not_push_the_next_refresh_past_its_interval() {
        let started = Instant::now();
        assert_eq!(
            next_refresh_at(started, 1),
            started + Duration::from_secs(60)
        );
    }

    #[test]
    fn changing_the_interval_reschedules_from_the_setting_change() {
        let changed_at = Instant::now();
        assert_eq!(
            next_refresh_at(changed_at, 1),
            changed_at + Duration::from_secs(60)
        );
    }

    #[test]
    fn manual_refresh_reschedules_from_scan_completion() {
        let completed_at = Instant::now();
        assert_eq!(
            next_refresh_at(completed_at, 5),
            completed_at + Duration::from_secs(300)
        );
    }

    #[test]
    fn automatic_refresh_start_event_contains_its_timestamp() {
        assert_eq!(
            refresh_started_payload(7, 42, true),
            serde_json::json!({ "generation": 7, "startedAtMs": 42, "manual": true }),
        );
    }
}

fn refresh_started_payload(generation: u64, started_at_ms: i64, manual: bool) -> serde_json::Value {
    serde_json::json!({ "generation": generation, "startedAtMs": started_at_ms, "manual": manual })
}

fn refresh_finished_payload(
    generation: u64,
    started_at_ms: i64,
    finished_at_ms: i64,
    success: bool,
    manual: bool,
) -> serde_json::Value {
    serde_json::json!({
        "generation": generation,
        "startedAtMs": started_at_ms,
        "finishedAtMs": finished_at_ms,
        "success": success,
        "manual": manual,
    })
}

async fn collect_and_archive(app: &AppHandle, generation: u64) -> bool {
    let state = app.state::<AppState>();
    let settings = match state.settings.read() {
        Ok(settings) => settings.clone(),
        Err(_) => return false,
    };
    let cli = resolve_cli_path_with_override(settings.custom_cli_path.as_deref())
        .or_else(|| state.cli_path.clone());
    let fast_settings = settings.clone();
    let fast_cli = cli.clone();
    let antigravity_settings = settings.clone();
    let antigravity_cli = cli.clone();
    let fast_future = crate::commands::build_cli_summary_for_agents(
        "all",
        &fast_settings,
        fast_cli.as_deref(),
        Some(FAST_AGENT_LIST),
        &state.scan_control,
    );
    let antigravity_future = collect_antigravity_source(
        &antigravity_settings,
        antigravity_cli.as_deref(),
        &state.scan_control,
    );
    tokio::pin!(fast_future);
    tokio::pin!(antigravity_future);
    let mut combined = state
        .latest_refresh
        .read()
        .ok()
        .and_then(|snapshot| snapshot.as_ref().map(|snapshot| snapshot.report.clone()))
        .unwrap_or_else(empty_report);
    let mut any_success = false;
    let mut fast_done = false;
    let mut antigravity_done = false;
    while !fast_done || !antigravity_done {
        tokio::select! {
            result = &mut fast_future, if !fast_done => {
                fast_done = true;
                any_success |= publish_source_result(&app, generation, "fast", result, &mut combined);
            }
            result = &mut antigravity_future, if !antigravity_done => {
                antigravity_done = true;
                any_success |= publish_source_result(&app, generation, "antigravity", result, &mut combined);
            }
        }
    }
    if !any_success {
        return false;
    }
    let data = combined;

    let snapshot = RefreshSnapshot {
        generation,
        refreshed_at_ms: chrono::Utc::now().timestamp_millis(),
        report: data.clone(),
    };
    if let Ok(mut latest) = state.latest_refresh.write() {
        *latest = Some(snapshot);
    }
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

    true
}

fn publish_source_result(
    app: &AppHandle,
    generation: u64,
    source: &str,
    result: Result<serde_json::Value, String>,
    combined: &mut serde_json::Value,
) -> bool {
    let Ok(mut report) = result else {
        return false;
    };
    crate::commands::attach_model_pricing(&mut report);
    let refreshed_at_ms = chrono::Utc::now().timestamp_millis();
    let _ = app.emit(
        "refresh_source_completed",
        source_payload(generation, source, refreshed_at_ms, &report),
    );
    let merged = crate::commands::merge_source_reports(combined, &report).is_ok();
    merged
}

async fn collect_antigravity_source(
    settings: &crate::app::AppSettings,
    cli: Option<&std::path::Path>,
    scan_control: &crate::scan_control::ScanControl,
) -> Result<serde_json::Value, String> {
    // Keep the slow Antigravity source independent from the fast providers.
    // A stalled live-quota endpoint must not hold the whole refresh cycle (or
    // the UI's Updating state) forever. The historical report remains valid;
    // merge_source_reports keeps the last known live subscription alongside it.
    let source = async {
        let mut report = crate::commands::build_cli_summary_for_agents(
            "all",
            settings,
            cli,
            Some("antigravity"),
            scan_control,
        )
        .await
        .unwrap_or_else(|_| empty_report());
        if scan_control.is_stopping_for_update() {
            return Err("Scan annulé pour préparer la mise à jour.".to_string());
        }
        match timeout(
            std::time::Duration::from_secs(20),
            crate::commands::attach_live_antigravity(&mut report, "all", settings),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                // Historical usage is still published. The previous live
                // quota remains in the aggregate until a later fetch works.
            }
        }
        Ok::<_, String>(report)
    };
    timeout(std::time::Duration::from_secs(90), source)
        .await
        .map_err(|_| "Antigravity refresh timed out after 90 seconds.".to_string())?
}

fn empty_report() -> serde_json::Value {
    serde_json::json!({
        "totals": { "totalCost": 0.0, "totalTokens": 0 },
        "agents": [],
        "models": [],
    })
}

fn source_payload(
    generation: u64,
    source: &str,
    refreshed_at_ms: i64,
    report: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "generation": generation,
        "source": source,
        "refreshedAtMs": refreshed_at_ms,
        "report": report,
    })
}
