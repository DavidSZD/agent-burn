use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};
use chrono::{DateTime, Datelike, Duration, Local, Utc, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AntigravitySession {
    pub id: String,
    pub project_name: String,
    pub project_path: String,
    pub date: String,
    pub steps_count: usize,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost: f64,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AntigravityModelUsage {
    pub model: String,
    pub tokens: i64,
    pub cost: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AntigravitySummary {
    pub period: String,
    pub session_count: usize,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_cost: f64,
    pub top_models: Vec<AntigravityModelUsage>,
    pub sessions: Vec<AntigravitySession>,
}

pub fn get_antigravity_data(period_str: Option<&str>) -> Result<AntigravitySummary, String> {
    let period = period_str.unwrap_or("mtd");
    let now = Utc::now();

    let min_date: Option<DateTime<Utc>> = match period {
        "today" => {
            let local_now = Local::now();
            local_now
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .and_then(|naive| local_now.offset().from_local_datetime(&naive).single())
                .map(|dt| dt.with_timezone(&Utc))
        }
        "week" => Some(now - Duration::days(7)),
        "mtd" => {
            let local_now = Local::now();
            chrono::NaiveDate::from_ymd_opt(local_now.year(), local_now.month(), 1)
                .and_then(|date| date.and_hms_opt(0, 0, 0))
                .and_then(|naive| local_now.offset().from_local_datetime(&naive).single())
                .map(|dt| dt.with_timezone(&Utc))
        }
        "all" => None,
        _ => None,
    };

    let home = std::env::var("USERPROFILE")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from));

    let mut sessions: Vec<AntigravitySession> = Vec::new();
    let mut model_tokens: HashMap<String, (i64, f64)> = HashMap::new();

    if let Some(home) = home {
        let brain_dir = home.join(".gemini").join("antigravity").join("brain");
        if brain_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(brain_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let conv_id = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                        let transcript_path = path.join(".system_generated").join("logs").join("transcript.jsonl");
                        if transcript_path.is_file() {
                            if let Some(session) = parse_transcript(&transcript_path, &conv_id, min_date) {
                                let entry = model_tokens.entry(session.model.clone()).or_insert((0, 0.0));
                                entry.0 += session.total_tokens;
                                entry.1 += session.cost;
                                sessions.push(session);
                            }
                        }
                    }
                }
            }
        }
    }

    // Trier les sessions par date décroissante
    sessions.sort_by(|a, b| b.date.cmp(&a.date));

    let mut total_tokens = 0;
    let mut input_tokens = 0;
    let mut output_tokens = 0;
    let mut total_cost = 0.0;

    for s in &sessions {
        total_tokens += s.total_tokens;
        input_tokens += s.input_tokens;
        output_tokens += s.output_tokens;
        total_cost += s.cost;
    }

    let mut top_models: Vec<AntigravityModelUsage> = model_tokens
        .into_iter()
        .map(|(model, (tokens, cost))| AntigravityModelUsage {
            model,
            tokens,
            cost: (cost * 100.0).round() / 100.0,
        })
        .collect();
    top_models.sort_by(|a, b| b.tokens.cmp(&a.tokens));

    Ok(AntigravitySummary {
        period: period.to_string(),
        session_count: sessions.len(),
        total_tokens,
        input_tokens,
        output_tokens,
        total_cost: (total_cost * 100.0).round() / 100.0,
        top_models,
        sessions,
    })
}

fn parse_transcript(path: &Path, conv_id: &str, min_date: Option<DateTime<Utc>>) -> Option<AntigravitySession> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);

    let mut session_date: Option<DateTime<Utc>> = None;
    let mut detected_project_path: Option<String> = None;
    let mut steps_count = 0;
    let mut input_chars: i64 = 0;
    let mut output_chars: i64 = 0;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }

        steps_count += 1;

        if let Ok(val) = serde_json::from_str::<Value>(&line) {
            // Extraire la date
            if session_date.is_none() {
                if let Some(dt_str) = val.get("created_at").and_then(|v| v.as_str()) {
                    if let Ok(dt) = DateTime::parse_from_rfc3339(dt_str) {
                        session_date = Some(dt.with_timezone(&Utc));
                    }
                }
            }

            // Détecter le projet dans les tool_calls ou content
            if detected_project_path.is_none() {
                if let Some(tool_calls) = val.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        if let Some(args) = tc.get("args") {
                            for key in &["DirectoryPath", "Cwd", "SearchDirectory", "SearchPath", "TargetFile"] {
                                if let Some(p_str) = args.get(key).and_then(|v| v.as_str()) {
                                    let clean = p_str.trim_matches('"').trim();
                                    if clean.len() >= 3 && clean.chars().nth(1) == Some(':') {
                                        let p = Path::new(clean);
                                        let dir = if p.extension().is_some() {
                                            p.parent().unwrap_or(p)
                                        } else {
                                            p
                                        };
                                        detected_project_path = Some(dir.to_string_lossy().to_string());
                                        break;
                                    }
                                }
                            }
                        }
                        if detected_project_path.is_some() {
                            break;
                        }
                    }
                }
            }

            // Décompte de tokens par source
            let source = val.get("source").and_then(|v| v.as_str()).unwrap_or("");
            if source == "MODEL" {
                if let Some(content) = val.get("content").and_then(|v| v.as_str()) {
                    output_chars += content.len() as i64;
                }
                if let Some(thinking) = val.get("thinking").and_then(|v| v.as_str()) {
                    output_chars += thinking.len() as i64;
                }
            } else {
                if let Some(content) = val.get("content").and_then(|v| v.as_str()) {
                    input_chars += content.len() as i64;
                }
            }
        }
    }

    let date = session_date?;
    if let Some(min) = min_date {
        if date < min {
            return None;
        }
    }

    let input_tokens = (input_chars / 4).max(100);
    let output_tokens = (output_chars / 4).max(50);
    let total_tokens = input_tokens + output_tokens;

    // Tarification officielle LiteLLM Google Gemini 2.5 Pro :
    // input = $1.25 / 1M, output = $5.00 / 1M
    let cost = (input_tokens as f64 * 1.25 + output_tokens as f64 * 5.00) / 1_000_000.0;

    let project_path = detected_project_path.unwrap_or_else(|| "Projet général (Workspace Antigravity)".to_string());
    let project_name = Path::new(&project_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Antigravity")
        .to_string();

    Some(AntigravitySession {
        id: conv_id.to_string(),
        project_name,
        project_path,
        date: date.to_rfc3339(),
        steps_count,
        input_tokens,
        output_tokens,
        total_tokens,
        cost: (cost * 100.0).round() / 100.0,
        model: "gemini-2.5-pro".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_antigravity_scan_all() {
        let result = get_antigravity_data(Some("all"));
        assert!(result.is_ok());
        let data = result.unwrap();
        println!("Antigravity total sessions: {}", data.session_count);
        println!("Antigravity total tokens: {}", data.total_tokens);
        println!("Antigravity total cost: ${}", data.total_cost);
        assert!(data.session_count > 0);
    }
}

