use crate::pricing::get_model_pricing;
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProjectSummary {
    pub name: String,
    pub path: String,
    pub session_count: usize,
    pub last_active: String,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_tokens: i64,
    pub estimated_cost: f64,
    pub relative_percent: f64,
}

#[derive(Default)]
struct ProjectAccumulator {
    name: String,
    path: String,
    session_count: usize,
    last_active: Option<DateTime<Utc>>,
    total_tokens: i64,
    input_tokens: i64,
    output_tokens: i64,
    cached_tokens: i64,
    total_cost: f64,
}

pub fn get_projects_usage(period_str: Option<&str>) -> Result<Value, String> {
    let period = period_str.unwrap_or("mtd");
    let (min_date, max_date) = crate::antigravity::period_bounds(period, Local::now());

    let mut projects_map: HashMap<String, ProjectAccumulator> = HashMap::new();

    // Collecter les fichiers de session Codex
    let mut session_files = Vec::new();
    let home = std::env::var("USERPROFILE")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from));

    if let Some(home) = home {
        let codex_base = home.join(".codex");

        let sessions_dir = codex_base.join("sessions");
        let archived_dir = codex_base.join("archived_sessions");

        collect_jsonl_files(&sessions_dir, &mut session_files);
        collect_jsonl_files(&archived_dir, &mut session_files);
    }

    for file_path in session_files {
        process_codex_session(&file_path, min_date, max_date, &mut projects_map);
    }

    // Agrégation des sessions Google Antigravity (avec les coûts exacts LiteLLM déjà calculés)
    if let Ok(agy) =
        crate::antigravity::get_antigravity_data_with_bounds(period, min_date, max_date)
    {
        for s in agy.sessions {
            if s.project_path.starts_with("Projet général") {
                continue;
            }
            let key = s.project_path.to_lowercase().replace('/', "\\");
            let entry = projects_map
                .entry(key)
                .or_insert_with(|| ProjectAccumulator {
                    name: s.project_name.clone(),
                    path: s.project_path.clone(),
                    session_count: 0,
                    last_active: None,
                    total_tokens: 0,
                    input_tokens: 0,
                    output_tokens: 0,
                    cached_tokens: 0,
                    total_cost: 0.0,
                });
            entry.session_count += 1;
            entry.total_tokens += s.total_tokens;
            entry.input_tokens += s.input_tokens;
            entry.output_tokens += s.output_tokens;
            entry.cached_tokens += s.cache_read_tokens;
            entry.total_cost += s.cost;
            if let Ok(dt) = DateTime::parse_from_rfc3339(&s.date) {
                let utc_dt = dt.with_timezone(&Utc);
                if entry.last_active.is_none_or(|cur| utc_dt > cur) {
                    entry.last_active = Some(utc_dt);
                }
            }
        }
    }

    // Calculer le total et le maximum pour les pourcentages relatifs
    let mut project_list: Vec<ProjectSummary> = Vec::new();
    let mut max_tokens: i64 = 0;
    let mut total_tokens_all: i64 = 0;
    let mut total_cost_all: f64 = 0.0;

    for (_, acc) in projects_map {
        if acc.total_tokens > max_tokens {
            max_tokens = acc.total_tokens;
        }
        total_tokens_all += acc.total_tokens;
        total_cost_all += acc.total_cost;

        let last_active_str = acc
            .last_active
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default();

        project_list.push(ProjectSummary {
            name: acc.name,
            path: acc.path,
            session_count: acc.session_count,
            last_active: last_active_str,
            total_tokens: acc.total_tokens,
            input_tokens: acc.input_tokens,
            output_tokens: acc.output_tokens,
            cached_tokens: acc.cached_tokens,
            estimated_cost: (acc.total_cost * 100.0).round() / 100.0,
            relative_percent: 0.0,
        });
    }

    // Mettre à jour les pourcentages relatifs
    if max_tokens > 0 {
        for proj in &mut project_list {
            proj.relative_percent = (proj.total_tokens as f64 / max_tokens as f64) * 100.0;
        }
    }

    // Trier par tokens décroissants
    project_list.sort_by_key(|project| std::cmp::Reverse(project.total_tokens));

    Ok(json!({
        "period": period,
        "totalProjects": project_list.len(),
        "totalTokens": total_tokens_all,
        "totalCost": (total_cost_all * 100.0).round() / 100.0,
        "projects": project_list,
    }))
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("rollout-") && name.ends_with(".jsonl") {
                    files.push(path);
                }
            }
        } else if path.is_dir() {
            collect_jsonl_files(&path, files);
        }
    }
}

fn process_codex_session(
    path: &Path,
    min_date: Option<DateTime<Utc>>,
    max_date: Option<DateTime<Utc>>,
    projects_map: &mut HashMap<String, ProjectAccumulator>,
) {
    let Ok(file) = File::open(path) else {
        return;
    };
    let mut reader = BufReader::new(file);
    let mut first_line = String::new();

    if reader.read_line(&mut first_line).is_err() || first_line.is_empty() {
        return;
    }

    let Ok(meta) = serde_json::from_str::<Value>(&first_line) else {
        return;
    };

    // Extraire timestamp
    let session_timestamp = meta
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc));

    if let (Some(min), Some(sess_ts)) = (min_date, session_timestamp) {
        if sess_ts < min {
            return;
        }
    }
    if let (Some(max), Some(sess_ts)) = (max_date, session_timestamp) {
        if sess_ts >= max {
            return;
        }
    }

    // Extraire cwd
    let cwd_str = meta
        .get("payload")
        .and_then(|p| p.get("cwd"))
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let cwd = match cwd_str {
        Some(c) => c,
        None => return,
    };

    // Nettoyer et normaliser le chemin
    let normalized_key = cwd.to_lowercase().replace('/', "\\");
    let folder_name = Path::new(&cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(&cwd)
        .to_string();

    // Modèle de la session (si spécifié dans le header)
    let mut session_model = meta
        .get("payload")
        .and_then(|p| p.get("model"))
        .and_then(|m| m.as_str())
        .map(str::to_string);

    // Parcourir le reste du fichier pour trouver les tokens et le modèle
    let mut line = String::new();
    let mut max_total_tokens: i64 = 0;
    let mut max_input_tokens: i64 = 0;
    let mut max_output_tokens: i64 = 0;
    let mut max_cached_tokens: i64 = 0;

    let mut last_event_ts = session_timestamp;

    while let Ok(bytes) = reader.read_line(&mut line) {
        if bytes == 0 {
            break;
        }

        if session_model.is_none() && line.contains("\"model\"") {
            if let Ok(entry) = serde_json::from_str::<Value>(&line) {
                if let Some(m) = entry
                    .get("payload")
                    .and_then(|p| p.get("model"))
                    .and_then(|s| s.as_str())
                {
                    session_model = Some(m.to_string());
                } else if let Some(m) = entry
                    .get("payload")
                    .and_then(|p| p.get("turn_context"))
                    .and_then(|tc| tc.get("model"))
                    .and_then(|s| s.as_str())
                {
                    session_model = Some(m.to_string());
                }
            }
        }

        if line.contains("\"total_token_usage\"") {
            if let Ok(entry) = serde_json::from_str::<Value>(&line) {
                if let Some(ts_str) = entry.get("timestamp").and_then(|t| t.as_str()) {
                    if let Ok(dt) = DateTime::parse_from_rfc3339(ts_str) {
                        let dt_utc = dt.with_timezone(&Utc);
                        if last_event_ts.map(|curr| dt_utc > curr).unwrap_or(true) {
                            last_event_ts = Some(dt_utc);
                        }
                    }
                }

                if let Some(info) = entry
                    .get("payload")
                    .and_then(|p| p.get("info"))
                    .and_then(|i| i.get("total_token_usage"))
                {
                    let total = info
                        .get("total_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let input = info
                        .get("input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let output = info
                        .get("output_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let cached = info
                        .get("cached_input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);

                    if total > max_total_tokens {
                        max_total_tokens = total;
                        max_input_tokens = input;
                        max_output_tokens = output;
                        max_cached_tokens = cached;
                    }
                }
            }
        }
        line.clear();
    }

    // Calcul du coût exact basé sur le modèle réel LiteLLM de la session
    let net_input = (max_input_tokens - max_cached_tokens).max(0) as f64;
    let cached = max_cached_tokens as f64;
    let output = max_output_tokens as f64;
    let session_cost = session_model
        .as_deref()
        .and_then(get_model_pricing)
        .map(|price| {
            (net_input * price.input_per_m
                + cached * price.cache_read_per_m
                + output * price.output_per_m)
                / 1_000_000.0
        })
        .unwrap_or(0.0);

    let entry = projects_map
        .entry(normalized_key)
        .or_insert_with(|| ProjectAccumulator {
            name: folder_name,
            path: cwd,
            session_count: 0,
            last_active: None,
            total_tokens: 0,
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            total_cost: 0.0,
        });

    entry.session_count += 1;
    entry.total_tokens += max_total_tokens;
    entry.input_tokens += max_input_tokens;
    entry.output_tokens += max_output_tokens;
    entry.cached_tokens += max_cached_tokens;
    entry.total_cost += session_cost;

    if let Some(ts) = last_event_ts {
        if entry.last_active.map(|curr| ts > curr).unwrap_or(true) {
            entry.last_active = Some(ts);
        }
    }
}

pub fn open_folder_in_explorer(path: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::process::Command;
        let p = Path::new(path);
        if p.exists() {
            Command::new("explorer.exe")
                .arg(path)
                .spawn()
                .map_err(|e| format!("Impossible d'ouvrir l'explorateur Windows: {e}"))?;
            Ok(())
        } else {
            if let Some(parent) = p.parent() {
                if parent.exists() {
                    Command::new("explorer.exe")
                        .arg(parent)
                        .spawn()
                        .map_err(|e| format!("Impossible d'ouvrir l'explorateur Windows: {e}"))?;
                    return Ok(());
                }
            }
            Err(format!(
                "Le dossier n'existe pas ou n'est plus accessible: {path}"
            ))
        }
    }
    #[cfg(not(windows))]
    {
        Err("Disponible uniquement sous Windows".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_projects_usage() {
        let res = get_projects_usage(Some("mtd"));
        assert!(res.is_ok(), "L'extraction des projets ne doit pas échouer");
        let val = res.unwrap();
        println!("Résultat projets MTD: totalCost=${}", val["totalCost"]);
        assert!(val.get("projects").is_some());
    }

    #[test]
    #[ignore = "requires local Codex or Antigravity session data"]
    fn test_extract_projects_all() {
        let res = get_projects_usage(Some("all"));
        assert!(res.is_ok(), "L'extraction totale ne doit pas échouer");
        let val = res.unwrap();
        let count = val
            .get("totalProjects")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let cost = val.get("totalCost").and_then(|v| v.as_f64()).unwrap_or(0.0);
        println!(
            "Nombre total de projets détectés: {}, coût total: ${}",
            count, cost
        );
        assert!(
            count > 0,
            "Au moins un projet doit être extrait des sessions"
        );
        assert!(cost > 0.0, "Le coût calculé doit être strictement positif");
    }
}
