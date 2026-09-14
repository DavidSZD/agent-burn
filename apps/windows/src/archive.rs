use std::{
    env,
    fs::{self, File},
    io::{BufReader, Write},
    path::PathBuf,
};
use chrono::Utc;
use serde_json::{json, Value};

const MAX_ARCHIVE_SAMPLES: usize = 10_000;

pub fn get_data_dir() -> PathBuf {
    let base = env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            env::var("USERPROFILE")
                .map(|p| PathBuf::from(p).join("AppData").join("Local"))
                .unwrap_or_else(|_| PathBuf::from("."))
        });

    let dir = base.join("Agent Burn");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn record_quota_sample(data: &Value) {
    let data_dir = get_data_dir();
    let archive_path = data_dir.join("quota-archive.json");
    let bak_path = data_dir.join("quota-archive.json.bak");
    let tmp_path = data_dir.join("quota-archive.json.tmp");

    let sub = data.get("subscription");
    let agents = sub.and_then(|s| s.get("agents")).and_then(|a| a.as_array());

    let mut sample_agents = Vec::new();

    if let Some(list) = agents {
        for item in list {
            let name = item.get("agent").and_then(|v| v.as_str()).unwrap_or("unknown");
            let mut remaining = 100.0;
            let mut short_remaining = None;
            let mut elapsed_mins = None;

            if let Some(w) = item.get("window") {
                if let Some(used) = w.get("usedPercent").and_then(|u| u.as_f64()) {
                    remaining = (100.0 - used).max(0.0);
                }
                elapsed_mins = w.get("elapsedMinutes").and_then(|e| e.as_f64());
            }

            if let Some(sw) = item.get("shortWindow") {
                if let Some(used) = sw.get("usedPercent").and_then(|u| u.as_f64()) {
                    short_remaining = Some((100.0 - used).max(0.0));
                }
            }

            let period_cost = item.get("periodUsage").and_then(|c| c.as_f64()).unwrap_or(0.0);
            let reset_credits = item.get("resetCreditsAvailable").and_then(|r| r.as_i64());

            sample_agents.push(json!({
                "agent": name,
                "plan": item.get("plan").and_then(|p| p.as_str()).unwrap_or(""),
                "remainingPercent": remaining,
                "shortRemainingPercent": short_remaining,
                "elapsedMinutes": elapsed_mins,
                "periodCost": period_cost,
                "resetCredits": reset_credits,
            }));
        }
    }

    if sample_agents.is_empty() {
        return;
    }

    let new_entry = json!({
        "timestamp": Utc::now().to_rfc3339(),
        "agents": sample_agents,
    });

    let mut samples: Vec<Value> = load_json_array_with_fallback(&archive_path, &bak_path);
    samples.push(new_entry);

    if samples.len() > MAX_ARCHIVE_SAMPLES {
        let trim_count = samples.len() - MAX_ARCHIVE_SAMPLES;
        samples.drain(0..trim_count);
    }

    save_atomic_with_backup(&archive_path, &bak_path, &tmp_path, &Value::Array(samples));
}

pub fn record_metrics_snapshot(data: &Value) {
    let data_dir = get_data_dir();
    let metrics_path = data_dir.join("metrics-history.json");
    let bak_path = data_dir.join("metrics-history.json.bak");
    let tmp_path = data_dir.join("metrics-history.json.tmp");

    let daily = match data.get("daily").and_then(|d| d.as_array()) {
        Some(d) => d,
        None => return,
    };

    let mut history_map = serde_json::Map::new();

    // Charger l'historique existant
    if let Ok(file) = File::open(&metrics_path).or_else(|_| File::open(&bak_path)) {
        let reader = BufReader::new(file);
        if let Ok(Value::Object(map)) = serde_json::from_reader(reader) {
            history_map = map;
        }
    }

    // Fusionner les jours (préserve les jours passés même si les logs sources expirent)
    for day_val in daily {
        if let Some(date_str) = day_val.get("date").and_then(|d| d.as_str()) {
            history_map.insert(date_str.to_string(), day_val.clone());
        }
    }

    save_atomic_with_backup(&metrics_path, &bak_path, &tmp_path, &Value::Object(history_map));
}

pub fn load_quota_history() -> Value {
    let data_dir = get_data_dir();
    let archive_path = data_dir.join("quota-archive.json");
    let bak_path = data_dir.join("quota-archive.json.bak");

    let samples = load_json_array_with_fallback(&archive_path, &bak_path);
    Value::Array(samples)
}

pub fn open_data_folder() -> Result<(), String> {
    let dir = get_data_dir();
    #[cfg(windows)]
    {
        std::process::Command::new("explorer.exe")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("Impossible d'ouvrir l'explorateur: {e}"))?;
    }
    Ok(())
}

pub fn save_report_cache(data: &Value) {
    let data_dir = get_data_dir();
    let cache_path = data_dir.join("report-cache.json");
    let bak_path = data_dir.join("report-cache.json.bak");
    let tmp_path = data_dir.join("report-cache.json.tmp");
    save_atomic_with_backup(&cache_path, &bak_path, &tmp_path, data);
}

pub fn load_report_cache() -> Value {
    let data_dir = get_data_dir();
    let cache_path = data_dir.join("report-cache.json");
    let bak_path = data_dir.join("report-cache.json.bak");

    if let Ok(file) = File::open(&cache_path).or_else(|_| File::open(&bak_path)) {
        let reader = BufReader::new(file);
        if let Ok(val) = serde_json::from_reader(reader) {
            return val;
        }
    }
    json!(null)
}


fn load_json_array_with_fallback(primary: &PathBuf, fallback: &PathBuf) -> Vec<Value> {
    if let Ok(file) = File::open(primary).or_else(|_| File::open(fallback)) {
        let reader = BufReader::new(file);
        if let Ok(Value::Array(arr)) = serde_json::from_reader(reader) {
            return arr;
        }
    }
    Vec::new()
}

fn save_atomic_with_backup(primary: &PathBuf, backup: &PathBuf, tmp: &PathBuf, val: &Value) {
    if let Ok(bytes) = serde_json::to_vec_pretty(val) {
        if let Ok(mut f) = File::create(tmp) {
            if f.write_all(&bytes).is_ok() && f.flush().is_ok() {
                drop(f);

                if primary.exists() {
                    let _ = fs::copy(primary, backup);
                    let _ = fs::remove_file(primary);
                }

                if fs::rename(tmp, primary).is_err() {
                    let _ = fs::write(primary, &bytes);
                    let _ = fs::remove_file(tmp);
                }
            }
        } else {
            if primary.exists() {
                let _ = fs::copy(primary, backup);
            }
            let _ = fs::write(primary, &bytes);
        }
    }
}
