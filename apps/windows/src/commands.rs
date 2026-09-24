use crate::app::{
    execute_cli_json_with_settings, resolve_cli_path_with_override, save_settings, AppSettings,
    AppState,
};
use std::{collections::BTreeMap, path::Path};
use tauri::State;

const GEMINI_QUOTA_SCOPE: &str = "gemini";

#[tauri::command]
pub async fn get_summary(
    period: Option<String>,
    range: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _scan = state.summary_scan.lock().await;
    let period_val = period.or(range).unwrap_or_else(|| "all".to_string());
    let settings = state
        .settings
        .read()
        .map_err(|error| error.to_string())?
        .clone();
    let cli = resolve_cli_path_with_override(settings.custom_cli_path.as_deref())
        .or_else(|| state.cli_path.clone());
    if state.scan_control.is_stopping_for_update() {
        return Err("Scan annulé pour préparer la mise à jour.".to_string());
    }
    build_summary(&period_val, &settings, cli.as_deref(), &state.scan_control).await
}

#[tauri::command]
pub async fn get_summary_since(
    since: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _scan = state.summary_scan.lock().await;
    chrono::NaiveDate::parse_from_str(&since, "%Y-%m-%d")
        .map_err(|_| "La date de début doit utiliser le format YYYY-MM-DD.".to_string())?;
    let settings = state
        .settings
        .read()
        .map_err(|error| error.to_string())?
        .clone();
    let cli = resolve_cli_path_with_override(settings.custom_cli_path.as_deref())
        .or_else(|| state.cli_path.clone());
    let args = ["summary", "--value", "--since", since.as_str()];
    let mut summary =
        execute_cli_json_with_settings(&state.scan_control, cli.as_deref(), &args, &settings)
            .await?;
    if state.scan_control.is_stopping_for_update() {
        return Err("Scan annulé pour préparer la mise à jour.".to_string());
    }
    let min_date = chrono::NaiveDate::parse_from_str(&since, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(date, chrono::Utc));
    let antigravity = tokio::task::spawn_blocking(move || {
        let _ = min_date;
        crate::antigravity::get_live_antigravity_data("rtd")
    })
    .await
    .map_err(|error| format!("Erreur tâche d'analyse Antigravity: {error}"))?;
    let mut antigravity = antigravity;
    if let Some(plan) = antigravity.plan.as_mut() {
        plan.price_per_month = crate::antigravity::antigravity_plan_price(
            &plan.plan,
            settings.antigravity_ultra_price,
        );
    }
    merge_antigravity(&mut summary, &antigravity)?;
    attach_model_pricing(&mut summary);
    Ok(summary)
}

pub(crate) async fn build_summary(
    period: &str,
    settings: &AppSettings,
    cli: Option<&Path>,
    scan_control: &crate::scan_control::ScanControl,
) -> Result<serde_json::Value, String> {
    let mut summary =
        build_cli_summary_for_agents(period, settings, cli, None, scan_control).await?;
    if scan_control.is_stopping_for_update() {
        return Err("Scan annulé pour préparer la mise à jour.".to_string());
    }
    attach_live_antigravity(&mut summary, period, settings).await?;
    attach_model_pricing(&mut summary);
    Ok(summary)
}

/// Runs the bundled CLI for only the requested agents. Keeping this separate
/// from the Antigravity live-plan lookup allows fast providers to publish
/// their result while the Antigravity history scan is still running.
pub(crate) async fn build_cli_summary_for_agents(
    period: &str,
    settings: &AppSettings,
    cli: Option<&Path>,
    agents: Option<&str>,
    scan_control: &crate::scan_control::ScanControl,
) -> Result<serde_json::Value, String> {
    let mut args = vec!["summary", "--value"];
    if period != "all" && !period.is_empty() {
        args.push(period);
    }
    if let Some(agents) = agents {
        args.push("--agents");
        args.push(agents);
    }
    execute_cli_json_with_settings(scan_control, cli, &args, settings).await
}

pub(crate) async fn attach_live_antigravity(
    summary: &mut serde_json::Value,
    period: &str,
    settings: &AppSettings,
) -> Result<(), String> {
    let antigravity_period = period.to_string();
    let mut antigravity = tokio::task::spawn_blocking(move || {
        crate::antigravity::get_live_antigravity_data(&antigravity_period)
    })
    .await
    .map_err(|error| format!("Erreur tâche d'analyse Antigravity: {error}"))?;
    if let Some(plan) = antigravity.plan.as_mut() {
        plan.price_per_month = crate::antigravity::antigravity_plan_price(
            &plan.plan,
            settings.antigravity_ultra_price,
        );
    }
    merge_antigravity(summary, &antigravity)?;
    Ok(())
}

/// Merge a report containing one or more provider slices into an aggregate
/// report. Existing rows for those providers are replaced, never added twice.
/// This is used by the refresh coordinator and deliberately keeps unrelated
/// provider data intact when one slow source is still in flight.
pub(crate) fn merge_source_reports(
    target: &mut serde_json::Value,
    source: &serde_json::Value,
) -> Result<(), String> {
    let target_object = target
        .as_object_mut()
        .ok_or_else(|| "Le résumé cible doit être un objet JSON.".to_string())?;
    let Some(source_agents) = source.get("agents").and_then(|value| value.as_array()) else {
        return Ok(());
    };
    let source_names = source_agents
        .iter()
        .filter_map(|agent| agent.get("agent").and_then(|value| value.as_str()))
        .collect::<std::collections::HashSet<_>>();
    {
        let target_agents = target_object
            .entry("agents")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| "Le champ agents doit être un tableau JSON.".to_string())?;
        target_agents.retain(|agent| {
            !agent
                .get("agent")
                .and_then(|value| value.as_str())
                .is_some_and(|name| source_names.contains(name))
        });
        target_agents.extend(source_agents.iter().cloned());
    }
    recompute_daily_from_agents(target_object);

    let source_models = source.get("models").and_then(|value| value.as_array());
    if let Some(source_models) = source_models {
        let target_models = target_object
            .entry("models")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| "Le champ models doit être un tableau JSON.".to_string())?;
        let names = source_models
            .iter()
            .filter_map(|model| model.get("model").and_then(|value| value.as_str()))
            .collect::<std::collections::HashSet<_>>();
        target_models.retain(|model| {
            !model
                .get("model")
                .and_then(|value| value.as_str())
                .is_some_and(|name| names.contains(name))
        });
        target_models.extend(source_models.iter().cloned());
        let total_cost = target_models
            .iter()
            .filter_map(|model| model.get("totalCost").and_then(|value| value.as_f64()))
            .sum::<f64>();
        for model in target_models {
            let cost = model
                .get("totalCost")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            model["percentage"] = serde_json::json!(if total_cost > 0.0 {
                cost / total_cost * 100.0
            } else {
                0.0
            });
        }
    }

    // `summary --value` also carries a per-period matrix when the Windows
    // shell enables AGENT_BURN_TIMELINE_CACHE. Merge each period with the
    // same provider-replacement rules as the top-level report so a fast
    // source never erases a slower source's cached timeline.
    if let Some(source_timelines) = source
        .get("timelineReports")
        .and_then(|value| value.as_object())
    {
        let target_timelines = target_object
            .entry("timelineReports")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| "Le champ timelineReports doit être un objet JSON.".to_string())?;
        for (period, source_report) in source_timelines {
            let target_report = target_timelines.entry(period.clone()).or_insert_with(|| {
                serde_json::json!({
                    "totals": { "totalCost": 0.0, "totalTokens": 0 },
                    "agents": [],
                    "models": [],
                })
            });
            merge_source_reports(target_report, source_report)?;
        }
    }

    let target_agents = target_object
        .get("agents")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "Le champ agents doit être un tableau JSON.".to_string())?;
    let total_cost = target_agents
        .iter()
        .filter_map(|agent| agent.get("totalCost").and_then(|value| value.as_f64()))
        .sum::<f64>();
    let total_tokens = target_agents
        .iter()
        .filter_map(|agent| agent.get("totalTokens").and_then(|value| value.as_u64()))
        .sum::<u64>();
    target_object["totals"] = serde_json::json!({
        "totalCost": total_cost,
        "totalTokens": total_tokens,
    });
    if let Some(subscription) = source.get("subscription") {
        let mut merged_subscription = target_object
            .get("subscription")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(incoming_agents) = subscription
            .get("agents")
            .and_then(|value| value.as_array())
        {
            let incoming_names = incoming_agents
                .iter()
                .filter_map(|agent| agent.get("agent").and_then(|value| value.as_str()))
                .collect::<std::collections::HashSet<_>>();
            let existing_agents = merged_subscription
                .get("agents")
                .and_then(|value| value.as_array())
                .map(|agents| {
                    agents
                        .iter()
                        .filter(|agent| {
                            !agent
                                .get("agent")
                                .and_then(|value| value.as_str())
                                .is_some_and(|name| incoming_names.contains(name))
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            merged_subscription = subscription.clone();
            merged_subscription["agents"] =
                serde_json::json!([existing_agents, incoming_agents.clone()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>());
        } else {
            merged_subscription = subscription.clone();
        }
        target_object.insert("subscription".to_string(), merged_subscription);
    }
    Ok(())
}

fn recompute_daily_from_agents(target: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(agents) = target.get("agents").and_then(|value| value.as_array()) else {
        return;
    };

    let mut by_date = BTreeMap::<String, (f64, u64)>::new();
    for agent in agents {
        let Some(days) = agent.get("daily").and_then(|value| value.as_array()) else {
            continue;
        };
        for day in days {
            let Some(date) = day.get("date").and_then(|value| value.as_str()) else {
                continue;
            };
            let cost = day
                .get("cost")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let tokens = day
                .get("tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            let entry = by_date.entry(date.to_string()).or_default();
            entry.0 += cost;
            entry.1 += tokens;
        }
    }

    if !by_date.is_empty() {
        target.insert(
            "daily".to_string(),
            serde_json::Value::Array(
                by_date
                    .into_iter()
                    .map(|(date, (cost, tokens))| {
                        serde_json::json!({ "date": date, "cost": cost, "tokens": tokens })
                    })
                    .collect(),
            ),
        );
    }
}

pub(crate) fn attach_model_pricing(summary: &mut serde_json::Value) {
    fn attach(models: Option<&mut Vec<serde_json::Value>>) {
        for model in models.into_iter().flatten() {
            let Some(name) = model.get("model").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(price) = crate::pricing::get_model_pricing(name) else {
                continue;
            };
            model["pricing"] = serde_json::json!({
                "inputPerM": price.input_per_m,
                "outputPerM": price.output_per_m,
                "cacheReadPerM": price.cache_read_per_m,
                "cacheWritePerM": price.cache_write_per_m,
            });
        }
    }

    attach(
        summary
            .get_mut("models")
            .and_then(|value| value.as_array_mut()),
    );
    if let Some(agents) = summary
        .get_mut("agents")
        .and_then(|value| value.as_array_mut())
    {
        for agent in agents {
            attach(
                agent
                    .get_mut("models")
                    .and_then(|value| value.as_array_mut()),
            );
        }
    }
    if let Some(reports) = summary
        .get_mut("timelineReports")
        .and_then(|value| value.as_object_mut())
    {
        for report in reports.values_mut() {
            attach_model_pricing(report);
        }
    }
}

fn merge_antigravity(
    summary: &mut serde_json::Value,
    data: &crate::antigravity::AntigravitySummary,
) -> Result<(), String> {
    if data.session_count == 0 && data.plan.is_none() {
        return Ok(());
    }
    if !summary.is_object() {
        return Err("Le résumé CLI doit être un objet JSON.".to_string());
    }
    let historical_already_present = summary
        .get("agents")
        .and_then(|value| value.as_array())
        .is_some_and(|agents| {
            agents.iter().any(|value| {
                value.get("agent").and_then(|value| value.as_str()) == Some("antigravity")
            })
        });
    if !historical_already_present {
        let models = data
            .top_models
            .iter()
            .map(|model| {
                serde_json::json!({
                    "model": model.model,
                    "totalTokens": model.tokens,
                    "totalCost": model.cost,
                })
            })
            .collect::<Vec<_>>();
        let daily = data
            .daily
            .iter()
            .map(|day| {
                serde_json::json!({
                    "date": day.date, "tokens": day.tokens, "cost": day.cost,
                })
            })
            .collect::<Vec<_>>();
        let agent = serde_json::json!({
            "agent": "antigravity",
            "totalCost": data.total_cost,
            "totalTokens": data.total_tokens,
            "models": models,
            "daily": daily,
            "tokenBreakdown": {
                "input": data.input_tokens,
                "output": data.output_tokens,
                "cacheRead": data.cache_read_tokens,
            }
        });
        if let Some(agents) = summary
            .get_mut("agents")
            .and_then(|value| value.as_array_mut())
        {
            if let Some(index) = agents.iter().position(|value| {
                value.get("agent").and_then(|v| v.as_str()) == Some("antigravity")
            }) {
                agents[index] = agent;
            } else {
                agents.push(agent);
            }
        }
        if let Some(totals) = summary.get_mut("totals") {
            let cost = totals
                .get("totalCost")
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0);
            let tokens = totals
                .get("totalTokens")
                .and_then(|value| value.as_i64())
                .unwrap_or(0);
            totals["totalCost"] = serde_json::json!(cost + data.total_cost);
            totals["totalTokens"] = serde_json::json!(tokens + data.total_tokens);
        }
        if let Some(root_models) = summary
            .get_mut("models")
            .and_then(|value| value.as_array_mut())
        {
            for model in &data.top_models {
                if let Some(existing) = root_models.iter_mut().find(|value| {
                    value.get("model").and_then(|v| v.as_str()) == Some(model.model.as_str())
                }) {
                    let tokens = existing
                        .get("totalTokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let cost = existing
                        .get("totalCost")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0);
                    existing["totalTokens"] = serde_json::json!(tokens + model.tokens);
                    existing["totalCost"] = serde_json::json!(cost + model.cost);
                } else {
                    root_models.push(serde_json::json!({
                        "model": model.model,
                        "totalTokens": model.tokens,
                        "totalCost": model.cost,
                        "percentage": 0.0,
                    }));
                }
            }
            let total_cost: f64 = root_models
                .iter()
                .filter_map(|value| value.get("totalCost")?.as_f64())
                .sum();
            for model in root_models {
                let cost = model
                    .get("totalCost")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                model["percentage"] = serde_json::json!(if total_cost > 0.0 {
                    cost / total_cost * 100.0
                } else {
                    0.0
                });
            }
        }
        if let Some(root_daily) = summary
            .get_mut("daily")
            .and_then(|value| value.as_array_mut())
        {
            for day in &data.daily {
                if let Some(existing) = root_daily.iter_mut().find(|value| {
                    value.get("date").and_then(|v| v.as_str()) == Some(day.date.as_str())
                }) {
                    let tokens = existing.get("tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                    let cost = existing.get("cost").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    existing["tokens"] = serde_json::json!(tokens + day.tokens);
                    existing["cost"] = serde_json::json!(cost + day.cost);
                } else {
                    root_daily.push(
                    serde_json::json!({"date": day.date, "tokens": day.tokens, "cost": day.cost}),
                );
                }
            }
            root_daily.sort_by(|a, b| {
                a.get("date")
                    .and_then(|v| v.as_str())
                    .cmp(&b.get("date").and_then(|v| v.as_str()))
            });
        }
    }
    if let Some(plan) = &data.plan {
        let root = summary
            .as_object_mut()
            .ok_or_else(|| "Le résumé CLI doit être un objet JSON.".to_string())?;
        let window_val = if let (Some(rem), Some(reset_time)) =
            (plan.weekly_remaining, &plan.weekly_reset_time)
        {
            let elapsed_mins =
                chrono::DateTime::parse_from_rfc3339(reset_time)
                    .ok()
                    .map(|reset_dt| {
                        let total_mins = 7.0 * 24.0 * 60.0;
                        let rem_mins = (reset_dt.with_timezone(&chrono::Utc) - chrono::Utc::now())
                            .num_seconds() as f64
                            / 60.0;
                        (total_mins - rem_mins).clamp(0.0, total_mins)
                    });

            Some(serde_json::json!({
                "usedPercent": (100.0 - rem).clamp(0.0, 100.0),
                "resetDate": reset_time,
                "elapsedMinutes": elapsed_mins,
            }))
        } else {
            None
        };

        let short_window_val = if let (Some(rem), Some(reset_time)) =
            (plan.session_remaining, &plan.session_reset_time)
        {
            let elapsed_mins =
                chrono::DateTime::parse_from_rfc3339(reset_time)
                    .ok()
                    .map(|reset_dt| {
                        let total_mins = 5.0 * 60.0;
                        let rem_mins = (reset_dt.with_timezone(&chrono::Utc) - chrono::Utc::now())
                            .num_seconds() as f64
                            / 60.0;
                        (total_mins - rem_mins).clamp(0.0, total_mins)
                    });
            Some(serde_json::json!({
                "usedPercent": (100.0 - rem).clamp(0.0, 100.0),
                "resetDate": reset_time,
                "elapsedMinutes": elapsed_mins,
            }))
        } else {
            None
        };

        let subscription = serde_json::json!({
            "agent": "antigravity",
            "plan": plan.plan,
            "pricePerMonth": if plan.price_per_month > 0.0 { Some(plan.price_per_month) } else { None },
            "account": plan.email.as_ref().or(plan.name.as_ref()),
            "quotaScope": GEMINI_QUOTA_SCOPE,
            "liveLimits": plan.quotas,
            "window": window_val,
            "shortWindow": short_window_val,
        });
        let subscriptions = root
            .entry("subscription")
            .or_insert_with(|| serde_json::json!({"agents": []}));
        let agents = subscriptions
            .get_mut("agents")
            .and_then(|value| value.as_array_mut());
        if let Some(agents) = agents {
            agents
                .retain(|value| value.get("agent").and_then(|v| v.as_str()) != Some("antigravity"));
            agents.push(subscription);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn get_harness(
    agent: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let _scan = state.summary_scan.lock().await;
    let args = vec!["harness", &agent, "--value"];
    let settings = state
        .settings
        .read()
        .map_err(|error| error.to_string())?
        .clone();
    let cli = resolve_cli_path_with_override(settings.custom_cli_path.as_deref())
        .or_else(|| state.cli_path.clone());
    execute_cli_json_with_settings(&state.scan_control, cli.as_deref(), &args, &settings).await
}

#[tauri::command]
pub async fn prepare_for_update(state: State<'_, AppState>) -> Result<(), String> {
    state.scan_control.stop_for_update().await;
    Ok(())
}

#[tauri::command]
pub fn resume_scans_after_cancelled_update(state: State<'_, AppState>) {
    state.scan_control.resume_after_cancelled_update();
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    state
        .settings
        .read()
        .map(|settings| settings.clone())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_settings(
    settings: AppSettings,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    let settings = settings.normalized();
    save_settings(&settings)?;
    *state.settings.write().map_err(|error| error.to_string())? = settings.clone();
    Ok(settings)
}

#[tauri::command]
pub fn get_cli_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let settings = state
        .settings
        .read()
        .map_err(|error| error.to_string())?
        .clone();
    let resolved = resolve_cli_path_with_override(settings.custom_cli_path.as_deref())
        .or_else(|| state.cli_path.clone());
    Ok(serde_json::json!({
        "available": resolved.is_some(),
        "path": resolved.as_ref().map(|p| p.to_string_lossy().to_string()),
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
pub async fn get_quota_history() -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(crate::archive::load_quota_history)
        .await
        .map_err(|error| format!("Erreur lecture historique quotas: {error}"))
}

#[tauri::command]
pub fn get_refresh_snapshot(
    state: State<'_, AppState>,
) -> Result<Option<crate::app::RefreshSnapshot>, String> {
    state
        .latest_refresh
        .read()
        .map(|snapshot| snapshot.clone())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_refresh_revision(state: State<'_, AppState>) -> Result<Option<i64>, String> {
    state
        .latest_refresh
        .read()
        .map(|snapshot| snapshot.as_ref().map(|value| value.refreshed_at_ms))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn open_data_folder() -> Result<(), String> {
    crate::archive::open_data_folder()
}

#[tauri::command]
pub async fn get_projects_usage(period: Option<String>) -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(move || crate::projects::get_projects_usage(period.as_deref()))
        .await
        .map_err(|e| format!("Erreur tâche d'analyse projets: {e}"))?
}

#[tauri::command]
pub fn open_project_folder(path: String) -> Result<(), String> {
    crate::projects::open_folder_in_explorer(&path)
}

#[tauri::command]
pub async fn get_report_cache() -> Result<serde_json::Value, String> {
    tokio::task::spawn_blocking(|| {
        let mut cache = crate::archive::load_report_cache();
        remove_unverified_antigravity_quotas(&mut cache);
        cache
    })
    .await
    .map_err(|error| format!("Erreur lecture cache rapports: {error}"))
}

fn remove_unverified_antigravity_quotas(cache: &mut serde_json::Value) {
    fn sanitize_report(report: &mut serde_json::Value) {
        if let Some(agents) = report
            .pointer_mut("/subscription/agents")
            .and_then(serde_json::Value::as_array_mut)
        {
            agents.retain(|agent| {
                agent.get("agent").and_then(serde_json::Value::as_str) != Some("antigravity")
                    || agent.get("quotaScope").and_then(serde_json::Value::as_str)
                        == Some(GEMINI_QUOTA_SCOPE)
            });
        }
    }

    if let Some(summary) = cache.get_mut("summary") {
        sanitize_report(summary);
    }
    if let Some(periods) = cache
        .get_mut("periods")
        .and_then(serde_json::Value::as_object_mut)
    {
        for period in periods.values_mut() {
            if let Some(report) = period.get_mut("reportData") {
                sanitize_report(report);
            }
        }
    }
}

#[tauri::command]
pub async fn save_report_cache(data: serde_json::Value) -> Result<(), String> {
    tokio::task::spawn_blocking(move || crate::archive::save_report_cache(&data))
        .await
        .map_err(|error| format!("Erreur écriture cache rapports: {error}"))?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_pricing_is_attached_to_general_and_harness_rows() {
        let mut summary = serde_json::json!({
            "models": [{"model": "gpt-5.5"}],
            "agents": [{"agent": "codex", "models": [{"model": "gpt-5.5"}]}],
            "timelineReports": {"today": {"models": [{"model": "gpt-5.5"}], "agents": []}}
        });

        attach_model_pricing(&mut summary);

        assert_eq!(summary["models"][0]["pricing"]["inputPerM"], 5.0);
        assert_eq!(summary["models"][0]["pricing"]["outputPerM"], 30.0);
        assert_eq!(
            summary["agents"][0]["models"][0]["pricing"]["cacheReadPerM"],
            0.5
        );
        assert_eq!(
            summary["timelineReports"]["today"]["models"][0]["pricing"]["inputPerM"],
            5.0
        );
    }

    #[test]
    fn antigravity_is_merged_into_the_standard_summary_shape() {
        let mut summary = serde_json::json!({
            "totals": {"totalCost": 1.0, "totalTokens": 10},
            "agents": [], "models": [], "daily": []
        });
        let antigravity = crate::antigravity::AntigravitySummary {
            period: "all".into(),
            session_count: 1,
            total_tokens: 30,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 15,
            total_cost: 2.0,
            top_models: vec![crate::antigravity::AntigravityModelUsage {
                model: "gemini-test".into(),
                tokens: 30,
                cost: 2.0,
                cache_read_tokens: 15,
            }],
            sessions: vec![],
            daily: vec![crate::antigravity::AntigravityDailyUsage {
                date: "2026-09-15".into(),
                tokens: 30,
                cost: 2.0,
            }],
            plan: None,
        };

        merge_antigravity(&mut summary, &antigravity).expect("merge summary");

        assert_eq!(summary["agents"][0]["agent"], "antigravity");
        assert_eq!(summary["agents"][0]["tokenBreakdown"]["cacheRead"], 15);
        assert!(summary["agents"][0]["tokenBreakdown"]
            .get("cacheWrite")
            .is_none());
        assert_eq!(summary["totals"]["totalTokens"], 40);
        assert_eq!(summary["models"][0]["model"], "gemini-test");
        assert_eq!(summary["daily"][0]["tokens"], 30);
    }

    #[test]
    fn live_antigravity_merge_preserves_usage_already_loaded_by_the_cli() {
        let mut summary = serde_json::json!({
            "totals": {"totalCost": 12.0, "totalTokens": 120},
            "agents": [{
                "agent": "antigravity",
                "totalCost": 7.0,
                "totalTokens": 70,
                "models": [{"model": "gemini-3-pro", "totalTokens": 70}],
                "daily": []
            }],
            "models": [{"model": "gemini-3-pro", "totalTokens": 70, "totalCost": 7.0}],
            "daily": []
        });
        let live = crate::antigravity::AntigravitySummary {
            period: "all".into(),
            session_count: 0,
            total_tokens: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            total_cost: 0.0,
            top_models: vec![],
            sessions: vec![],
            daily: vec![],
            plan: None,
        };

        merge_antigravity(&mut summary, &live).expect("merge live status");

        assert_eq!(summary["totals"]["totalTokens"], 120);
        assert_eq!(summary["agents"][0]["totalTokens"], 70);
        assert_eq!(summary["models"][0]["totalTokens"], 70);
    }

    #[test]
    fn antigravity_merge_rejects_a_non_object_summary() {
        let mut summary = serde_json::json!([]);
        let antigravity = crate::antigravity::AntigravitySummary {
            period: "all".into(),
            session_count: 1,
            total_tokens: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            total_cost: 0.0,
            top_models: vec![],
            sessions: vec![],
            daily: vec![],
            plan: None,
        };

        assert!(merge_antigravity(&mut summary, &antigravity).is_err());
    }

    #[test]
    fn source_merge_replaces_rows_without_dropping_other_subscriptions() {
        let mut target = serde_json::json!({
            "totals": {"totalCost": 3.0, "totalTokens": 30},
            "agents": [
                {"agent": "codex", "totalCost": 1.0, "totalTokens": 10, "daily": [{"date": "2026-09-18", "cost": 1.0, "tokens": 10}]},
                {"agent": "antigravity", "totalCost": 2.0, "totalTokens": 20, "daily": [{"date": "2026-09-18", "cost": 2.0, "tokens": 20}]}
            ],
            "models": [],
            "subscription": {"agents": [
                {"agent": "codex", "plan": "Free"},
                {"agent": "antigravity", "plan": "Pro"}
            ]}
        });
        let source = serde_json::json!({
            "totals": {"totalCost": 2.0, "totalTokens": 20},
            "agents": [{"agent": "antigravity", "totalCost": 2.5, "totalTokens": 25}],
            "models": [],
            "subscription": {"agents": [{"agent": "antigravity", "plan": "Ultra"}]}
        });

        merge_source_reports(&mut target, &source).expect("merge source");
        assert_eq!(target["totals"]["totalTokens"], 35);
        assert_eq!(
            target["subscription"]["agents"].as_array().unwrap().len(),
            2
        );
        assert_eq!(target["subscription"]["agents"][0]["agent"], "codex");
        assert_eq!(target["subscription"]["agents"][1]["plan"], "Ultra");
    }

    #[test]
    fn source_merge_updates_each_timeline_without_dropping_other_sources() {
        let mut target = serde_json::json!({
            "totals": {"totalCost": 3.0, "totalTokens": 30},
            "agents": [
                {"agent": "codex", "totalCost": 1.0, "totalTokens": 10},
                {"agent": "antigravity", "totalCost": 2.0, "totalTokens": 20}
            ],
            "models": [],
            "timelineReports": {
                "today": {
                    "totals": {"totalCost": 2.0, "totalTokens": 20},
                    "agents": [{"agent": "antigravity", "totalCost": 2.0, "totalTokens": 20}],
                    "models": []
                }
            }
        });
        let source = serde_json::json!({
            "totals": {"totalCost": 1.0, "totalTokens": 10},
            "agents": [{"agent": "codex", "totalCost": 3.0, "totalTokens": 30, "daily": [{"date": "2026-09-18", "cost": 3.0, "tokens": 30}]}],
            "models": [],
            "timelineReports": {
                "today": {
                    "totals": {"totalCost": 3.0, "totalTokens": 30},
                    "agents": [{"agent": "codex", "totalCost": 3.0, "totalTokens": 30}],
                    "models": []
                },
                "ytd": {
                    "totals": {"totalCost": 3.0, "totalTokens": 30},
                    "agents": [{"agent": "codex", "totalCost": 3.0, "totalTokens": 30}],
                    "models": []
                }
            }
        });

        merge_source_reports(&mut target, &source).expect("merge timeline source");

        assert_eq!(
            target["timelineReports"]["today"]["totals"]["totalTokens"],
            50
        );
        assert_eq!(
            target["timelineReports"]["today"]["agents"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            target["timelineReports"]["ytd"]["totals"]["totalTokens"],
            30
        );
        assert_eq!(target["daily"][0]["cost"], 3.0);
        assert_eq!(target["daily"][0]["tokens"], 30);
    }

    #[test]
    fn antigravity_merge_includes_weekly_and_session_window_with_elapsed_minutes() {
        let mut summary = serde_json::json!({
            "totals": {"totalCost": 0.0, "totalTokens": 0},
            "agents": [], "models": [], "daily": []
        });
        let future_reset = (chrono::Utc::now() + chrono::Duration::hours(24)).to_rfc3339();
        let antigravity = crate::antigravity::AntigravitySummary {
            period: "all".into(),
            session_count: 1,
            total_tokens: 10,
            input_tokens: 5,
            output_tokens: 5,
            cache_read_tokens: 0,
            total_cost: 0.1,
            top_models: vec![],
            sessions: vec![],
            daily: vec![],
            plan: Some(crate::antigravity::AntigravityPlan {
                plan: "Pro".into(),
                price_per_month: 20.0,
                email: Some("test@example.com".into()),
                name: Some("Test User".into()),
                quotas: vec![],
                weekly_remaining: Some(95.5),
                weekly_reset_time: Some(future_reset.clone()),
                session_remaining: Some(80.0),
                session_reset_time: Some(future_reset.clone()),
            }),
        };

        merge_antigravity(&mut summary, &antigravity).expect("merge summary");

        let agents = summary["subscription"]["agents"]
            .as_array()
            .expect("agents array");
        let ag = agents
            .iter()
            .find(|a| a["agent"] == "antigravity")
            .expect("antigravity in subscription");
        assert_eq!(ag["plan"], "Pro");
        assert_eq!(ag["quotaScope"], "gemini");
        let win = &ag["window"];
        assert!((win["usedPercent"].as_f64().unwrap() - 4.5).abs() < 0.01);
        assert!(win["elapsedMinutes"].as_f64().is_some());
        let swin = &ag["shortWindow"];
        assert!((swin["usedPercent"].as_f64().unwrap() - 20.0).abs() < 0.01);
        assert!(swin["elapsedMinutes"].as_f64().is_some());
    }

    #[test]
    fn antigravity_merge_never_promotes_an_untyped_model_quota_to_weekly() {
        let mut summary = serde_json::json!({
            "totals": {"totalCost": 0.0, "totalTokens": 0},
            "agents": [], "models": [], "daily": []
        });
        let antigravity = crate::antigravity::AntigravitySummary {
            period: "all".into(),
            session_count: 0,
            total_tokens: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            total_cost: 0.0,
            top_models: vec![],
            sessions: vec![],
            daily: vec![],
            plan: Some(crate::antigravity::AntigravityPlan {
                plan: "Pro".into(),
                price_per_month: 20.0,
                email: None,
                name: None,
                quotas: vec![crate::antigravity::AntigravityQuotaInfo {
                    label: "Gemini Pro".into(),
                    remaining: 92.0,
                    reset_time: Some("2026-09-25T02:00:00Z".into()),
                    model_id: Some("gemini-pro".into()),
                }],
                weekly_remaining: None,
                weekly_reset_time: None,
                session_remaining: Some(92.0),
                session_reset_time: Some("2026-09-25T02:00:00Z".into()),
            }),
        };

        merge_antigravity(&mut summary, &antigravity).expect("merge summary");

        let agent = &summary["subscription"]["agents"][0];
        assert!(agent["window"].is_null());
        assert_eq!(agent["shortWindow"]["usedPercent"], 8.0);
    }

    #[test]
    fn report_cache_hides_legacy_antigravity_quota_but_keeps_scoped_data() {
        let mut cache = serde_json::json!({
            "summary": {"subscription": {"agents": [
                {"agent": "antigravity", "window": {"usedPercent": 60.4}},
                {"agent": "codex", "window": {"usedPercent": 12.0}}
            ]}},
            "periods": {
                "all": {"reportData": {"subscription": {"agents": [
                    {"agent": "antigravity", "window": {"usedPercent": 24.0}, "quotaScope": "gemini"},
                    {"agent": "cursor", "window": null}
                ]}}}
            }
        });

        remove_unverified_antigravity_quotas(&mut cache);

        assert_eq!(
            cache["summary"]["subscription"]["agents"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            cache["summary"]["subscription"]["agents"][0]["agent"],
            "codex"
        );
        assert_eq!(
            cache["periods"]["all"]["reportData"]["subscription"]["agents"][0]["quotaScope"],
            "gemini"
        );
    }
}
