use crate::pricing::get_model_pricing;
use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AntigravitySession {
    pub id: String,
    pub project_name: String,
    pub project_path: String,
    pub date: String,
    pub steps_count: usize,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_tokens: i64,
    pub cost: f64,
    pub model: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AntigravityModelUsage {
    pub model: String,
    pub tokens: i64,
    pub cost: f64,
    pub cache_read_tokens: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AntigravityDailyUsage {
    pub date: String,
    pub tokens: i64,
    pub cost: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AntigravityQuotaInfo {
    pub label: String,
    pub remaining: f64,
    #[serde(rename = "resetTime")]
    pub reset_time: Option<String>,
    #[serde(rename = "modelId")]
    pub model_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AntigravityPlan {
    pub plan: String,
    #[serde(rename = "pricePerMonth")]
    pub price_per_month: f64,
    pub email: Option<String>,
    pub name: Option<String>,
    pub quotas: Vec<AntigravityQuotaInfo>,
    #[serde(default)]
    pub weekly_remaining: Option<f64>,
    #[serde(default)]
    pub weekly_reset_time: Option<String>,
    #[serde(default)]
    pub session_remaining: Option<f64>,
    #[serde(default)]
    pub session_reset_time: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AntigravitySummary {
    pub period: String,
    pub session_count: usize,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub total_cost: f64,
    pub top_models: Vec<AntigravityModelUsage>,
    pub sessions: Vec<AntigravitySession>,
    pub daily: Vec<AntigravityDailyUsage>,
    pub plan: Option<AntigravityPlan>,
}

pub fn get_antigravity_data(period_str: Option<&str>) -> Result<AntigravitySummary, String> {
    let period = period_str.unwrap_or("mtd");
    let (min_date, max_date) = period_bounds(period, Local::now());
    get_antigravity_data_with_bounds(period, min_date, max_date)
}

pub(crate) fn get_antigravity_data_with_bounds(
    period: &str,
    min_date: Option<DateTime<Utc>>,
    max_date: Option<DateTime<Utc>>,
) -> Result<AntigravitySummary, String> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from));

    let mut sessions: Vec<AntigravitySession> = Vec::new();
    let mut model_totals: HashMap<String, (i64, f64, i64)> = HashMap::new();
    let mut daily_totals: HashMap<String, (i64, f64)> = HashMap::new();

    if let Some(home) = home {
        let conv_dir = home
            .join(".gemini")
            .join("antigravity")
            .join("conversations");
        let brain_dir = home.join(".gemini").join("antigravity").join("brain");

        if conv_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&conv_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|s| s.to_str()) == Some("db") {
                        let conv_id = path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string();

                        if conv_id.is_empty() {
                            continue;
                        }

                        let transcript_path = brain_dir
                            .join(&conv_id)
                            .join(".system_generated")
                            .join("logs")
                            .join("transcript.jsonl");

                        let (step_dates, last_activity) = load_step_dates(&transcript_path, &path);

                        if let Some(session) = parse_conversation_db(
                            &path,
                            &conv_id,
                            min_date,
                            max_date,
                            &step_dates,
                            last_activity,
                            &mut daily_totals,
                        ) {
                            for m in &session.model_breakdown {
                                let entry =
                                    model_totals.entry(m.model.clone()).or_insert((0, 0.0, 0));
                                entry.0 += m.tokens;
                                entry.1 += m.cost;
                                entry.2 += m.cache_read_tokens;
                            }
                            sessions.push(session.session);
                        }
                    }
                }
            }
        }
    }

    sessions.sort_by(|a, b| b.date.cmp(&a.date));

    let total_tokens: i64 = sessions.iter().map(|s| s.total_tokens).sum();
    let input_tokens: i64 = sessions.iter().map(|s| s.input_tokens).sum();
    let output_tokens: i64 = sessions.iter().map(|s| s.output_tokens).sum();
    let cache_read_tokens: i64 = sessions.iter().map(|s| s.cache_read_tokens).sum();
    let total_cost: f64 = sessions.iter().map(|s| s.cost).sum();

    let mut top_models: Vec<AntigravityModelUsage> = model_totals
        .into_iter()
        .map(
            |(model, (tokens, cost, cache_read_tokens))| AntigravityModelUsage {
                model,
                tokens,
                cost: (cost * 100.0).round() / 100.0,
                cache_read_tokens,
            },
        )
        .collect();
    top_models.sort_by_key(|model| std::cmp::Reverse(model.tokens));

    let mut daily: Vec<AntigravityDailyUsage> = daily_totals
        .into_iter()
        .map(|(date, (tokens, cost))| AntigravityDailyUsage {
            date,
            tokens,
            cost: (cost * 100.0).round() / 100.0,
        })
        .collect();
    daily.sort_by(|a, b| a.date.cmp(&b.date));

    let plan = get_live_antigravity_plan();

    Ok(AntigravitySummary {
        period: period.to_string(),
        session_count: sessions.len(),
        total_tokens,
        input_tokens,
        output_tokens,
        cache_read_tokens,
        total_cost: (total_cost * 100.0).round() / 100.0,
        top_models,
        sessions,
        daily,
        plan,
    })
}

struct ParsedDBSession {
    session: AntigravitySession,
    model_breakdown: Vec<AntigravityModelUsage>,
}

fn parse_conversation_db(
    db_path: &Path,
    conv_id: &str,
    min_date: Option<DateTime<Utc>>,
    max_date: Option<DateTime<Utc>>,
    step_dates: &HashMap<usize, DateTime<Utc>>,
    last_activity: Option<DateTime<Utc>>,
    daily_totals: &mut HashMap<String, (i64, f64)>,
) -> Option<ParsedDBSession> {
    let connection = sqlite::open(db_path).ok()?;

    // 1. Extraire le workspace
    let mut project_path = "Projet général (Workspace Antigravity)".to_string();
    let mut project_name = "Antigravity".to_string();

    let query_meta = "SELECT data FROM trajectory_metadata_blob WHERE id = 'main' LIMIT 1;";
    if let Ok(mut stmt) = connection.prepare(query_meta) {
        if let Ok(sqlite::State::Row) = stmt.next() {
            if let Ok(blob) = stmt.read::<Vec<u8>, _>(0) {
                if let Some(ws) = extract_workspace_from_blob(&blob) {
                    project_path = ws.clone();
                    project_name = Path::new(&ws)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&ws)
                        .to_string();
                }
            }
        }
    }

    // 2. Extraire les métadonnées de génération (modèles réels et tokens exacts ventilés par étape)
    let query_gen = "SELECT idx, data FROM gen_metadata ORDER BY idx ASC;";
    let mut total_in: i64 = 0;
    let mut total_out: i64 = 0;
    let mut total_cache_read: i64 = 0;
    let mut total_cost: f64 = 0.0;
    let mut steps_count: usize = 0;
    let mut model_stats: HashMap<String, (i64, f64, i64)> = HashMap::new();
    let mut primary_model = None;
    let mut max_model_tokens = 0;

    if let Ok(mut stmt) = connection.prepare(query_gen) {
        while let Ok(sqlite::State::Row) = stmt.next() {
            let idx = stmt.read::<i64, _>(0).unwrap_or(0) as usize;
            if let Ok(blob) = stmt.read::<Vec<u8>, _>(1) {
                let step_date = step_dates.get(&idx).cloned().or(last_activity);

                // Filtrage temporel strict de l'étape
                if let Some(min) = min_date {
                    if let Some(sd) = step_date {
                        if sd < min {
                            continue;
                        }
                    }
                }
                if let Some(max) = max_date {
                    if let Some(sd) = step_date {
                        if sd >= max {
                            continue;
                        }
                    }
                }

                if let Some(usage) = parse_gen_metadata_step(&blob) {
                    steps_count += 1;
                    total_in += usage.input_tokens;
                    total_out += usage.output_tokens;
                    total_cache_read += usage.cache_read_tokens;
                    let step_cost = get_model_pricing(&usage.model)
                        .map(|price| {
                            (usage.input_tokens as f64 * price.input_per_m
                                + usage.cache_read_tokens as f64 * price.cache_read_per_m
                                + usage.output_tokens as f64 * price.output_per_m)
                                / 1_000_000.0
                        })
                        .unwrap_or(0.0);
                    total_cost += step_cost;

                    let stat = model_stats
                        .entry(usage.model.clone())
                        .or_insert((0, 0.0, 0));
                    stat.0 += usage.input_tokens + usage.output_tokens + usage.cache_read_tokens;
                    stat.1 += step_cost;
                    stat.2 += usage.cache_read_tokens;

                    if stat.0 > max_model_tokens {
                        max_model_tokens = stat.0;
                        primary_model = Some(usage.model);
                    }

                    // Agréger dans le daily global
                    if let Some(sd) = step_date {
                        let day_str = sd.format("%Y-%m-%d").to_string();
                        let d_entry = daily_totals.entry(day_str).or_insert((0, 0.0));
                        d_entry.0 +=
                            usage.input_tokens + usage.output_tokens + usage.cache_read_tokens;
                        d_entry.1 += step_cost;
                    }
                }
            }
        }
    }

    if steps_count == 0 {
        return None;
    }

    let date_str = last_activity.unwrap_or_else(Utc::now).to_rfc3339();

    let model_breakdown = model_stats
        .into_iter()
        .map(|(m, (tok, c, cache_read_tokens))| AntigravityModelUsage {
            model: m,
            tokens: tok,
            cost: c,
            cache_read_tokens,
        })
        .collect();

    Some(ParsedDBSession {
        session: AntigravitySession {
            id: conv_id.to_string(),
            project_name,
            project_path,
            date: date_str,
            steps_count,
            input_tokens: total_in,
            output_tokens: total_out,
            cache_read_tokens: total_cache_read,
            total_tokens: total_in + total_out + total_cache_read,
            cost: (total_cost * 100.0).round() / 100.0,
            model: primary_model?,
        },
        model_breakdown,
    })
}

pub(crate) fn period_bounds(
    period: &str,
    local_now: DateTime<Local>,
) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>) {
    let local_midnight = |date: chrono::NaiveDate| {
        date.and_hms_opt(0, 0, 0)
            .and_then(|naive| local_now.offset().from_local_datetime(&naive).single())
            .map(|dt| dt.with_timezone(&Utc))
    };
    let today = local_now.date_naive();
    let start_today = local_midnight(today);
    match period {
        "today" => (start_today, None),
        "yesterday" => (local_midnight(today - Duration::days(1)), start_today),
        "wtd" => {
            let days = i64::from(local_now.weekday().num_days_from_monday());
            (local_midnight(today - Duration::days(days)), None)
        }
        "mtd" => (
            chrono::NaiveDate::from_ymd_opt(local_now.year(), local_now.month(), 1)
                .and_then(local_midnight),
            None,
        ),
        "ytd" => (
            chrono::NaiveDate::from_ymd_opt(local_now.year(), 1, 1).and_then(local_midnight),
            None,
        ),
        "week" | "rtd" => (
            Some(local_now.with_timezone(&Utc) - Duration::days(7)),
            None,
        ),
        "month" => (
            Some(local_now.with_timezone(&Utc) - Duration::days(30)),
            None,
        ),
        _ => (None, None),
    }
}

fn load_step_dates(
    transcript_path: &Path,
    db_path: &Path,
) -> (HashMap<usize, DateTime<Utc>>, Option<DateTime<Utc>>) {
    let mut step_dates = HashMap::new();
    let mut last_date = None;

    if let Ok(file) = fs::File::open(transcript_path) {
        use std::io::{BufRead, BufReader};
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                let step_idx = val
                    .get("step_index")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);
                if let Some(dt_str) = val.get("created_at").and_then(|v| v.as_str()) {
                    if let Ok(dt) = DateTime::parse_from_rfc3339(dt_str) {
                        let dt_utc = dt.with_timezone(&Utc);
                        if let Some(idx) = step_idx {
                            step_dates.insert(idx, dt_utc);
                        }
                        last_date = Some(dt_utc);
                    }
                }
            }
        }
    }

    // Repli sur la date de modification du fichier db si aucune date n'a été lue
    if last_date.is_none() {
        if let Ok(meta) = fs::metadata(db_path) {
            if let Ok(modified) = meta.modified() {
                let dt: DateTime<Utc> = modified.into();
                last_date = Some(dt);
            }
        }
    }

    (step_dates, last_date)
}

fn get_live_antigravity_plan() -> Option<AntigravityPlan> {
    let script = r#"$OutputEncoding=[Console]::OutputEncoding=[Text.UTF8Encoding]::new();$p=Get-CimInstance Win32_Process -Filter "Name = 'language_server.exe'"|Select-Object -First 1;if(-not $p){exit 1};$m=[regex]::Match($p.CommandLine,'--csrf_token\s+([^\s]+)');if(-not $m.Success){exit 1};$token=$m.Groups[1].Value;$ports=Get-NetTCPConnection -State Listen|Where-Object{$_.OwningProcess -eq $p.ProcessId -and $_.LocalAddress -eq '127.0.0.1'}|Select-Object -ExpandProperty LocalPort -Unique;[System.Net.ServicePointManager]::ServerCertificateValidationCallback={$true};foreach($port in $ports){foreach($scheme in @('http','https')){try{$headers=@{'x-codeium-csrf-token'=$token;'Connect-Protocol-Version'='1'};$s=Invoke-RestMethod -Uri "${scheme}://127.0.0.1:${port}/exa.language_server_pb.LanguageServerService/GetUserStatus" -Method Post -Headers $headers -ContentType 'application/json' -Body '{}' -TimeoutSec 2 -ErrorAction Stop;$q=Invoke-RestMethod -Uri "${scheme}://127.0.0.1:${port}/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary" -Method Post -Headers $headers -ContentType 'application/json' -Body '{}' -TimeoutSec 2 -ErrorAction SilentlyContinue;@{userStatus=$s.userStatus;quotaSummary=$q.response}|ConvertTo-Json -Depth 30 -Compress;exit 0}catch{}}};exit 1"#;
    let mut command = std::process::Command::new("powershell.exe");
    command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(powershell_creation_flags());
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return unavailable_live_plan();
    }
    let status = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok()?;
    parse_live_status(&status)
}

#[cfg(windows)]
fn powershell_creation_flags() -> u32 {
    0x08000000 // CREATE_NO_WINDOW
}

fn parse_live_status(status: &serde_json::Value) -> Option<AntigravityPlan> {
    let user = status.get("userStatus").unwrap_or(status);
    let plan = user
        .pointer("/planStatus/planInfo/planName")
        .and_then(|v| v.as_str())?
        .to_string();
    let quotas = user
        .pointer("/cascadeModelConfigData/clientModelConfigs")?
        .as_array()?
        .iter()
        .filter_map(|config| {
            let quota = config.get("quotaInfo")?;
            Some(AntigravityQuotaInfo {
                label: config.get("label")?.as_str()?.to_string(),
                remaining: quota.get("remainingFraction")?.as_f64()? * 100.0,
                reset_time: quota
                    .get("resetTime")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                model_id: config
                    .get("modelId")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
            })
        })
        .collect();

    let mut weekly_remaining: Option<f64> = None;
    let mut weekly_reset_time: Option<String> = None;
    let mut session_remaining: Option<f64> = None;
    let mut session_reset_time: Option<String> = None;

    if let Some(quota_summary) = status.get("quotaSummary") {
        if let Some(groups) = quota_summary.get("groups").and_then(|g| g.as_array()) {
            for group in groups {
                if let Some(buckets) = group.get("buckets").and_then(|b| b.as_array()) {
                    for bucket in buckets {
                        let window = bucket.get("window").and_then(|w| w.as_str()).unwrap_or("");
                        let frac = bucket.get("remainingFraction").and_then(|f| f.as_f64());
                        let reset = bucket.get("resetTime").and_then(|r| r.as_str()).map(str::to_string);

                        if window == "weekly" {
                            if let Some(rem_pct) = frac.map(|f| f * 100.0) {
                                if weekly_remaining.map_or(true, |curr| rem_pct < curr) {
                                    weekly_remaining = Some(rem_pct);
                                    weekly_reset_time = reset;
                                }
                            }
                        } else if window == "5h" {
                            if let Some(rem_pct) = frac.map(|f| f * 100.0) {
                                if session_remaining.map_or(true, |curr| rem_pct < curr) {
                                    session_remaining = Some(rem_pct);
                                    session_reset_time = reset;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Some(AntigravityPlan {
        plan,
        price_per_month: 0.0,
        email: user
            .get("email")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        name: user
            .get("name")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        quotas,
        weekly_remaining,
        weekly_reset_time,
        session_remaining,
        session_reset_time,
    })
}

pub(crate) fn antigravity_plan_price(plan: &str, ultra_price: Option<f64>) -> f64 {
    if plan.eq_ignore_ascii_case("pro") {
        20.0
    } else if plan.eq_ignore_ascii_case("ultra") {
        ultra_price
            .filter(|price| matches!(*price as u64, 100 | 200))
            .unwrap_or(0.0)
    } else {
        0.0
    }
}

fn unavailable_live_plan() -> Option<AntigravityPlan> {
    None
}

fn extract_workspace_from_blob(blob: &[u8]) -> Option<String> {
    // Recherche de l'URI "file:///" dans le blob
    let marker = b"file:///";
    let mut i = 0;
    while i + marker.len() <= blob.len() {
        if &blob[i..i + marker.len()] == marker {
            let start = i + marker.len();
            let mut end = start;
            while end < blob.len()
                && blob[end] >= 0x20
                && blob[end] <= 0x7e
                && blob[end] != b'"'
                && blob[end] != b'\''
            {
                end += 1;
            }
            if end > start {
                let raw_str = String::from_utf8_lossy(&blob[start..end]).to_string();
                let decoded = urlencoding_decode(&raw_str);
                let cleaned = decoded.replace('/', "\\");
                return Some(cleaned);
            }
        }
        i += 1;
    }
    None
}

fn urlencoding_decode(s: &str) -> String {
    let mut result = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex_val) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                result.push(hex_val);
                i += 3;
                continue;
            }
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

/// Décodeur Protobuf optimisé pour extraire le modèle et les tokens réels de `gen_metadata`
struct AntigravityTokenUsage {
    model: String,
    input_tokens: i64,
    output_tokens: i64,
    cache_read_tokens: i64,
}

fn parse_gen_metadata_step(blob: &[u8]) -> Option<AntigravityTokenUsage> {
    let fields = decode_proto(blob);
    for (fnum, fval) in fields {
        if fnum == 1 {
            // Sous-message du résultat de génération
            if let ProtoValue::Bytes(sub_bytes) = fval {
                let sub_fields = decode_proto(&sub_bytes);
                let mut model = None;
                let mut in_tokens: i64 = 0;
                let mut out_tokens: i64 = 0;
                let mut cache_read_tokens: i64 = 0;

                for (sfnum, sfval) in sub_fields {
                    if sfnum == 19 {
                        // Nom du modèle utilisé
                        if let ProtoValue::Bytes(name_bytes) = sfval {
                            let name = String::from_utf8_lossy(&name_bytes).trim().to_string();
                            if !name.is_empty() {
                                model = Some(name);
                            }
                        }
                    } else if sfnum == 4 {
                        // Statistiques de tokens
                        if let ProtoValue::Bytes(stat_bytes) = sfval {
                            let stat_fields = decode_proto(&stat_bytes);
                            for (tfnum, tfval) in stat_fields {
                                if tfnum == 2 {
                                    if let ProtoValue::Varint(v) = tfval {
                                        in_tokens = v as i64;
                                    }
                                } else if tfnum == 3 {
                                    if let ProtoValue::Varint(v) = tfval {
                                        out_tokens = v as i64;
                                    }
                                } else if tfnum == 5 {
                                    if let ProtoValue::Varint(v) = tfval {
                                        cache_read_tokens = v as i64;
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(m) = model {
                    return Some(AntigravityTokenUsage {
                        model: m,
                        // Antigravity reports uncached input and cache-read tokens
                        // as separate counters. The cache counter can legitimately
                        // be larger than input, so subtracting it creates nonsense.
                        input_tokens: in_tokens,
                        output_tokens: out_tokens,
                        cache_read_tokens,
                    });
                }
            }
        }
    }
    None
}

enum ProtoValue {
    Varint(u64),
    Bytes(Vec<u8>),
}

fn decode_proto(b: &[u8]) -> Vec<(u32, ProtoValue)> {
    let mut pos = 0;
    let mut fields = Vec::new();
    while pos < b.len() {
        let (key, new_pos) = read_varint(b, pos);
        if new_pos == pos {
            break;
        }
        pos = new_pos;

        let field_num = (key >> 3) as u32;
        let wire_type = key & 7;

        match wire_type {
            0 => {
                let (val, new_pos) = read_varint(b, pos);
                if new_pos == pos {
                    break;
                }
                pos = new_pos;
                fields.push((field_num, ProtoValue::Varint(val)));
            }
            2 => {
                let (len, new_pos) = read_varint(b, pos);
                if new_pos == pos {
                    break;
                }
                pos = new_pos;
                let len = len as usize;
                if pos + len <= b.len() {
                    fields.push((field_num, ProtoValue::Bytes(b[pos..pos + len].to_vec())));
                    pos += len;
                } else {
                    break;
                }
            }
            1 => {
                if pos + 8 <= b.len() {
                    pos += 8;
                } else {
                    break;
                }
            }
            5 => {
                if pos + 4 <= b.len() {
                    pos += 4;
                } else {
                    break;
                }
            }
            _ => {
                // Autres types non utilisés pour nos champs (skip)
                break;
            }
        }
    }
    fields
}

fn read_varint(b: &[u8], mut pos: usize) -> (u64, usize) {
    let mut val: u64 = 0;
    let mut shift: u32 = 0;
    while pos < b.len() {
        let byte = b[pos];
        pos += 1;
        val |= ((byte & 0x7f) as u64) << shift;
        if (byte & 0x80) == 0 {
            return (val, pos);
        }
        shift += 7;
        if shift >= 64 {
            break;
        }
    }
    (val, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                return bytes;
            }
        }
    }

    fn varint_field(number: u8, value: u64) -> Vec<u8> {
        let mut bytes = varint(u64::from(number) << 3);
        bytes.extend(varint(value));
        bytes
    }

    fn bytes_field(number: u8, value: &[u8]) -> Vec<u8> {
        let mut bytes = varint((u64::from(number) << 3) | 2);
        bytes.extend(varint(value.len() as u64));
        bytes.extend(value);
        bytes
    }

    #[test]
    fn percent_decoding_preserves_utf8_characters() {
        assert_eq!(urlencoding_decode("Caf%C3%A9%20M%C3%A9nage"), "Café Ménage");
    }

    #[test]
    fn protobuf_decoder_continues_after_fixed64_fields() {
        let mut blob = varint((2_u64 << 3) | 1);
        blob.extend([0; 8]);
        blob.extend(varint_field(3, 42));

        let fields = decode_proto(&blob);

        assert!(matches!(fields.as_slice(), [(3, ProtoValue::Varint(42))]));
    }

    #[test]
    fn protobuf_decoder_continues_after_fixed32_fields() {
        let mut blob = varint((2_u64 << 3) | 5);
        blob.extend([0; 4]);
        blob.extend(bytes_field(3, b"gemini-test"));

        let fields = decode_proto(&blob);

        assert!(matches!(
            fields.as_slice(),
            [(3, ProtoValue::Bytes(value))] if value == b"gemini-test"
        ));
    }

    #[test]
    fn generation_metadata_separates_cache_read_from_uncached_input() {
        let mut stats = varint_field(2, 20_000);
        stats.extend(varint_field(3, 500));
        stats.extend(varint_field(5, 12_000));
        let mut generation = bytes_field(4, &stats);
        generation.extend(bytes_field(19, b"gemini-3.8-flash"));
        let blob = bytes_field(1, &generation);

        let usage = parse_gen_metadata_step(&blob).expect("valid generation metadata");

        assert_eq!(usage.input_tokens, 20_000);
        assert_eq!(usage.cache_read_tokens, 12_000);
        assert_eq!(usage.output_tokens, 500);
    }

    #[test]
    fn live_status_uses_real_plan_and_model_quota_fields() {
        let status = serde_json::json!({"userStatus": {
            "name": "Real User",
            "email": "real@example.test",
            "planStatus": {"planInfo": {"planName": "Pro"}},
            "cascadeModelConfigData": {"clientModelConfigs": [{
                "label": "Gemini Pro",
                "modelId": "gemini-pro-agent",
                "quotaInfo": {"remainingFraction": 0.54, "resetTime": "2026-09-15T20:00:00Z"}
            }]}
        }});

        let plan = parse_live_status(&status).expect("real plan");

        assert_eq!(plan.plan, "Pro");
        assert_eq!(plan.name.as_deref(), Some("Real User"));
        assert_eq!(plan.quotas[0].remaining, 54.0);
        assert_eq!(plan.quotas[0].model_id.as_deref(), Some("gemini-pro-agent"));
    }

    #[test]
    fn live_status_extracts_weekly_and_session_quota_from_quota_summary() {
        let status = serde_json::json!({
            "userStatus": {
                "name": "Real User",
                "planStatus": {"planInfo": {"planName": "Pro"}},
                "cascadeModelConfigData": {"clientModelConfigs": []}
            },
            "quotaSummary": {
                "groups": [{
                    "displayName": "Gemini Models",
                    "buckets": [
                        {
                            "bucketId": "gemini-weekly",
                            "window": "weekly",
                            "remainingFraction": 0.9734,
                            "resetTime": "2026-09-23T04:34:22Z"
                        },
                        {
                            "bucketId": "gemini-5h",
                            "window": "5h",
                            "remainingFraction": 0.8409,
                            "resetTime": "2026-09-16T09:34:22Z"
                        }
                    ]
                }]
            }
        });

        let plan = parse_live_status(&status).expect("real plan with quota summary");

        assert_eq!(plan.plan, "Pro");
        assert!((plan.weekly_remaining.unwrap() - 97.34).abs() < 0.01);
        assert_eq!(plan.weekly_reset_time.as_deref(), Some("2026-09-23T04:34:22Z"));
        assert!((plan.session_remaining.unwrap() - 84.09).abs() < 0.01);
        assert_eq!(plan.session_reset_time.as_deref(), Some("2026-09-16T09:34:22Z"));
    }

    #[test]
    fn pro_plan_has_known_monthly_price() {
        assert_eq!(antigravity_plan_price("Pro", None), 20.0);
    }

    #[test]
    fn ultra_plan_requires_the_users_tier_choice() {
        assert_eq!(antigravity_plan_price("Ultra", None), 0.0);
        assert_eq!(antigravity_plan_price("Ultra", Some(100.0)), 100.0);
        assert_eq!(antigravity_plan_price("Ultra", Some(200.0)), 200.0);
    }

    #[test]
    fn unavailable_live_plan_does_not_fabricate_account_or_quotas() {
        assert!(unavailable_live_plan().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn live_status_process_is_hidden_on_windows() {
        assert_eq!(powershell_creation_flags(), 0x08000000);
    }

    #[test]
    #[ignore = "requires a running local Antigravity language server"]
    fn running_antigravity_exposes_its_real_plan() {
        let plan = get_live_antigravity_plan().expect("live Antigravity plan");
        assert!(!plan.plan.is_empty());
        assert!(!plan.quotas.is_empty());
    }

    #[test]
    #[ignore = "local installation smoke test; deterministic protobuf coverage is above"]
    fn test_antigravity_scan_exact_tokens_and_models() {
        let result = get_antigravity_data(Some("all"));
        assert!(result.is_ok());
        let data = result.unwrap();
        println!("Antigravity total sessions: {}", data.session_count);
        println!("Antigravity total tokens: {}", data.total_tokens);
        println!("Antigravity total cost: ${:.2}", data.total_cost);
        println!("JSON_OUTPUT: {}", serde_json::to_string(&data).unwrap());
        assert!(data.session_count > 0);
        for m in &data.top_models {
            println!(
                "  Model: {:<30} Tokens: {:>10} Cost: ${:.4}",
                m.model, m.tokens, m.cost
            );
            assert!(m.cost >= 0.0);
        }
    }
}
