use crate::app::{
    execute_cli_json_with_settings, resolve_cli_path_with_override, save_settings, AppSettings,
    AppState,
};
use std::path::Path;
use tauri::State;

#[tauri::command]
pub async fn get_summary(
    period: Option<String>,
    range: Option<String>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let period_val = period.or(range).unwrap_or_else(|| "all".to_string());
    let settings = state
        .settings
        .read()
        .map_err(|error| error.to_string())?
        .clone();
    let cli = resolve_cli_path_with_override(settings.custom_cli_path.as_deref())
        .or_else(|| state.cli_path.clone());
    let mut timeline_settings = settings;
    timeline_settings.offline = true;
    build_summary(&period_val, &timeline_settings, cli.as_deref()).await
}

#[tauri::command]
pub async fn get_summary_since(
    since: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
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
    let mut timeline_settings = settings.clone();
    timeline_settings.offline = true;
    let mut summary =
        execute_cli_json_with_settings(cli.as_deref(), &args, &timeline_settings).await?;
    let min_date = chrono::NaiveDate::parse_from_str(&since, "%Y-%m-%d")
        .ok()
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|date| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(date, chrono::Utc));
    let antigravity = tokio::task::spawn_blocking(move || {
        crate::antigravity::get_antigravity_data_with_bounds("rtd", min_date, None)
    })
    .await
    .map_err(|error| format!("Erreur tâche d'analyse Antigravity: {error}"))??;
    let mut antigravity = antigravity;
    if let Some(plan) = antigravity.plan.as_mut() {
        plan.price_per_month = crate::antigravity::antigravity_plan_price(
            &plan.plan,
            settings.antigravity_ultra_price,
        );
    }
    merge_antigravity(&mut summary, &antigravity)?;
    Ok(summary)
}

pub(crate) async fn build_summary(
    period: &str,
    settings: &AppSettings,
    cli: Option<&Path>,
) -> Result<serde_json::Value, String> {
    let mut args = vec!["summary", "--value"];
    if period != "all" && !period.is_empty() {
        args.push(period);
    }
    let mut summary = execute_cli_json_with_settings(cli, &args, settings).await?;
    let antigravity_period = period.to_string();
    let antigravity_result = tokio::task::spawn_blocking(move || {
        crate::antigravity::get_antigravity_data(Some(&antigravity_period))
    })
    .await
    .map_err(|error| format!("Erreur tâche d'analyse Antigravity: {error}"))?;
    if let Ok(mut antigravity) = antigravity_result {
        if let Some(plan) = antigravity.plan.as_mut() {
            plan.price_per_month = crate::antigravity::antigravity_plan_price(
                &plan.plan,
                settings.antigravity_ultra_price,
            );
        }
        merge_antigravity(&mut summary, &antigravity)?;
    }
    Ok(summary)
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
        if let Some(index) = agents
            .iter()
            .position(|value| value.get("agent").and_then(|v| v.as_str()) == Some("antigravity"))
        {
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
            if let Some(existing) = root_daily
                .iter_mut()
                .find(|value| value.get("date").and_then(|v| v.as_str()) == Some(day.date.as_str()))
            {
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
    if let Some(plan) = &data.plan {
        let root = summary
            .as_object_mut()
            .ok_or_else(|| "Le résumé CLI doit être un objet JSON.".to_string())?;
        let limiting = plan
            .quotas
            .iter()
            .min_by(|a, b| a.remaining.total_cmp(&b.remaining));

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
            limiting.map(|quota| {
                let elapsed_mins = quota
                    .reset_time
                    .as_deref()
                    .and_then(|rt| chrono::DateTime::parse_from_rfc3339(rt).ok())
                    .map(|reset_dt| {
                        let total_mins = 7.0 * 24.0 * 60.0;
                        let rem_mins = (reset_dt.with_timezone(&chrono::Utc) - chrono::Utc::now())
                            .num_seconds() as f64
                            / 60.0;
                        (total_mins - rem_mins).clamp(0.0, total_mins)
                    });
                serde_json::json!({
                    "usedPercent": (100.0 - quota.remaining).clamp(0.0, 100.0),
                    "resetDate": quota.reset_time,
                    "elapsedMinutes": elapsed_mins,
                })
            })
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
    let args = vec!["harness", &agent, "--value"];
    let settings = state
        .settings
        .read()
        .map_err(|error| error.to_string())?
        .clone();
    let cli = resolve_cli_path_with_override(settings.custom_cli_path.as_deref())
        .or_else(|| state.cli_path.clone());
    execute_cli_json_with_settings(cli.as_deref(), &args, &settings).await
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
pub fn get_quota_history() -> Result<serde_json::Value, String> {
    Ok(crate::archive::load_quota_history())
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let win = &ag["window"];
        assert!((win["usedPercent"].as_f64().unwrap() - 4.5).abs() < 0.01);
        assert!(win["elapsedMinutes"].as_f64().is_some());
        let swin = &ag["shortWindow"];
        assert!((swin["usedPercent"].as_f64().unwrap() - 20.0).abs() < 0.01);
        assert!(swin["elapsedMinutes"].as_f64().is_some());
    }
}
