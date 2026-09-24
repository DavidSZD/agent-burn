use crate::antigravity::{AntigravityPlan, AntigravityQuotaInfo};
use serde_json::{json, Value};
use std::{env, fs, path::PathBuf, sync::OnceLock, time::Duration};

const KEYRING_TARGET: &str = "gemini:antigravity";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const API_HOST: &str = "https://daily-cloudcode-pa.googleapis.com";
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
struct OAuthCredential {
    access_token: Option<String>,
    refresh_token: String,
    expiry_ms: Option<i64>,
}

/// Parse the cloud responses into the same plan shape used by the local
/// language-server probe. This is deliberately pure so response-shape changes
/// can be covered by deterministic fixtures instead of requiring a live account.
pub(crate) fn build_plan_from_responses(
    load_code_assist: &Value,
    quota_summary: Option<&Value>,
    available_models: Option<&Value>,
    quota_buckets: Option<&Value>,
) -> Option<AntigravityPlan> {
    let plan = cloud_plan_name(load_code_assist)?;
    let email = subscription_email(load_code_assist);

    // `retrieveUserQuota` is the authoritative per-account Gemini meter used
    // by agy-quota. Prefer those request buckets for the live limits shown in
    // Agent Burn; `fetchAvailableModels` is only a pooled provider view.
    let mut quotas = bucket_quotas(quota_buckets);
    if quotas.is_empty() {
        quotas = model_quotas(available_models);
    }
    if quotas.is_empty() {
        quotas = summary_quotas(quota_summary);
    }
    let mut weekly_remaining = None;
    let mut weekly_reset_time = None;
    let mut session_remaining = None;
    let mut session_reset_time = None;
    let mut session_preferred = false;
    let mut weekly_preferred = false;

    // The quota summary explicitly labels both windows and separates Gemini
    // from Claude/GPT. Use that classification before considering the
    // per-model retrieveUserQuota buckets, which may omit their window type.
    if let Some(summary) = quota_summary {
        let summary = summary.get("response").unwrap_or(summary);
        if let Some(groups) = summary.get("groups").and_then(Value::as_array) {
            for group in groups.iter().filter(|group| is_gemini_group(group)) {
                let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
                    continue;
                };
                for bucket in buckets {
                    if !is_gemini_quota_bucket(bucket) {
                        continue;
                    }
                    let Some(fraction) = bucket.get("remainingFraction").and_then(Value::as_f64)
                    else {
                        continue;
                    };
                    let remaining = (fraction * 100.0).clamp(0.0, 100.0);
                    let reset = bucket
                        .get("resetTime")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    let bucket_id = bucket
                        .get("bucketId")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    match quota_window_kind(bucket) {
                        Some(QuotaWindowKind::Weekly) => update_window(
                            &mut weekly_remaining,
                            &mut weekly_reset_time,
                            remaining,
                            reset,
                            bucket_id.eq_ignore_ascii_case("gemini-weekly"),
                            &mut weekly_preferred,
                        ),
                        Some(QuotaWindowKind::FiveHour) => update_window(
                            &mut session_remaining,
                            &mut session_reset_time,
                            remaining,
                            reset,
                            bucket_id.eq_ignore_ascii_case("gemini-5h"),
                            &mut session_preferred,
                        ),
                        None => {}
                    }
                }
            }
        }
    }

    if weekly_remaining.is_none() {
        if let Some((remaining, reset_time)) = authoritative_quota_window(quota_buckets) {
            weekly_remaining = Some(remaining);
            weekly_reset_time = reset_time;
        }
    }

    Some(AntigravityPlan {
        plan,
        price_per_month: 0.0,
        email,
        name: None,
        quotas,
        weekly_remaining,
        weekly_reset_time,
        session_remaining,
        session_reset_time,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuotaWindowKind {
    Weekly,
    FiveHour,
}

fn quota_window_kind(bucket: &Value) -> Option<QuotaWindowKind> {
    let window = bucket
        .get("window")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let bucket_id = bucket
        .get("bucketId")
        .or_else(|| bucket.get("modelId"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    if matches!(
        window.as_str(),
        "5h" | "five_hour" | "five-hour" | "session"
    ) || bucket_id.contains("5h")
        || bucket_id.contains("five-hour")
    {
        return Some(QuotaWindowKind::FiveHour);
    }
    if window == "weekly" || bucket_id.contains("weekly") {
        return Some(QuotaWindowKind::Weekly);
    }
    None
}

fn is_gemini_quota_bucket(bucket: &Value) -> bool {
    ["modelId", "bucketId"].iter().any(|field| {
        bucket
            .get(*field)
            .and_then(Value::as_str)
            .is_some_and(|id| id.to_ascii_lowercase().contains("gemini"))
    })
}

fn is_gemini_group(group: &Value) -> bool {
    if let Some(name) = group.get("displayName").and_then(Value::as_str) {
        return name.to_ascii_lowercase().contains("gemini");
    }
    group
        .get("buckets")
        .and_then(Value::as_array)
        .is_some_and(|buckets| buckets.iter().any(is_gemini_quota_bucket))
}

fn authoritative_quota_window(quota: Option<&Value>) -> Option<(f64, Option<String>)> {
    let buckets = quota
        .and_then(|value| value.get("response").unwrap_or(value).get("buckets"))
        .and_then(Value::as_array)?;

    // retrieveUserQuota also contains provider pools for Claude/GPT. Only
    // Gemini model buckets may feed Antigravity's Gemini weekly meter.
    let gemini_buckets = buckets
        .iter()
        .filter(|bucket| is_gemini_quota_bucket(bucket))
        .collect::<Vec<_>>();
    if gemini_buckets.is_empty() {
        return None;
    }

    // A generic REQUESTS bucket is not enough to distinguish weekly from a
    // five-hour model pool. Only use the direct endpoint when the payload
    // explicitly identifies a weekly bucket.
    let candidates = gemini_buckets
        .iter()
        .copied()
        .filter(|bucket| quota_window_kind(bucket) == Some(QuotaWindowKind::Weekly))
        .collect::<Vec<_>>();

    let tightest = candidates
        .iter()
        .filter_map(|bucket| {
            let remaining = bucket
                .get("remainingFraction")
                .and_then(Value::as_f64)
                .map(|fraction| (fraction * 100.0).clamp(0.0, 100.0))?;
            Some((remaining, bucket))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))?;
    let reset_time = tightest
        .1
        .get("resetTime")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some((tightest.0, reset_time))
}

fn update_window(
    remaining: &mut Option<f64>,
    reset_time: &mut Option<String>,
    candidate: f64,
    candidate_reset: Option<String>,
    preferred: bool,
    selected_preferred: &mut bool,
) {
    let should_replace = if preferred {
        !*selected_preferred || remaining.is_none_or(|current| candidate < current)
    } else {
        !*selected_preferred && remaining.is_none_or(|current| candidate < current)
    };
    if should_replace {
        *remaining = Some(candidate);
        *reset_time = candidate_reset;
        *selected_preferred = preferred;
    }
}

fn cloud_plan_name(load_code_assist: &Value) -> Option<String> {
    let paid_name = load_code_assist
        .pointer("/paidTier/name")
        .and_then(Value::as_str);
    let paid_id = load_code_assist
        .pointer("/paidTier/id")
        .and_then(Value::as_str);
    let current_name = load_code_assist
        .pointer("/currentTier/name")
        .and_then(Value::as_str);
    let current_id = load_code_assist
        .pointer("/currentTier/id")
        .and_then(Value::as_str);

    paid_name
        .or(paid_id)
        .or(current_id)
        .or(current_name)
        .map(normalize_plan_name)
}

fn normalize_plan_name(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    if lower.contains("ultra") {
        "Ultra".to_string()
    } else if lower.contains("pro") {
        "Pro".to_string()
    } else if lower.contains("free") {
        "Free".to_string()
    } else if lower.contains("standard") {
        "Standard".to_string()
    } else if lower.contains("legacy") {
        "Legacy".to_string()
    } else {
        raw.trim().to_string()
    }
}

fn subscription_email(load_code_assist: &Value) -> Option<String> {
    let uri = load_code_assist
        .get("manageSubscriptionUri")
        .or_else(|| load_code_assist.get("upgradeSubscriptionUri"))
        .and_then(Value::as_str)?;
    let encoded = uri.split_once("Email=")?.1.split('&').next()?;
    Some(percent_decode(encoded))
}

fn percent_decode(value: &str) -> String {
    let mut output = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(decoded) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                output.push(decoded);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn model_quotas(available_models: Option<&Value>) -> Vec<AntigravityQuotaInfo> {
    let Some(models) = available_models
        .and_then(|value| value.get("models"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    let mut quotas = models
        .iter()
        .filter_map(|(model_id, model)| {
            if !model_id.to_ascii_lowercase().contains("gemini") {
                return None;
            }
            let quota = model.get("quotaInfo")?;
            let remaining = quota.get("remainingFraction")?.as_f64()?;
            Some(AntigravityQuotaInfo {
                label: model
                    .get("displayName")
                    .or_else(|| model.get("label"))
                    .and_then(Value::as_str)
                    .unwrap_or(model_id)
                    .to_string(),
                remaining: (remaining * 100.0).clamp(0.0, 100.0),
                reset_time: quota
                    .get("resetTime")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                model_id: Some(model_id.clone()),
            })
        })
        .collect::<Vec<_>>();
    quotas.sort_by(|left, right| left.label.cmp(&right.label));
    quotas
}

fn summary_quotas(summary: Option<&Value>) -> Vec<AntigravityQuotaInfo> {
    let Some(groups) = summary
        .and_then(|value| value.get("response").unwrap_or(value).get("groups"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    groups
        .iter()
        .filter(|group| is_gemini_group(group))
        .flat_map(|group| {
            let group_label = group
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or("Antigravity");
            group
                .get("buckets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|bucket| is_gemini_quota_bucket(bucket))
                .filter_map(move |bucket| {
                    let remaining = bucket.get("remainingFraction")?.as_f64()?;
                    let bucket_id = bucket.get("bucketId")?.as_str()?;
                    let bucket_label = bucket
                        .get("displayName")
                        .and_then(Value::as_str)
                        .unwrap_or(bucket_id);
                    Some(AntigravityQuotaInfo {
                        label: format!("{group_label} · {bucket_label}"),
                        remaining: (remaining * 100.0).clamp(0.0, 100.0),
                        reset_time: bucket
                            .get("resetTime")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        model_id: Some(bucket_id.to_string()),
                    })
                })
        })
        .collect()
}

fn bucket_quotas(quota: Option<&Value>) -> Vec<AntigravityQuotaInfo> {
    quota
        .and_then(|value| value.get("response").unwrap_or(value).get("buckets"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|bucket| {
            if !is_gemini_quota_bucket(bucket) {
                return None;
            }
            let remaining = bucket.get("remainingFraction")?.as_f64()?;
            let model_id = bucket
                .get("modelId")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let token_type = bucket
                .get("tokenType")
                .and_then(Value::as_str)
                .unwrap_or("quota");
            Some(AntigravityQuotaInfo {
                label: format!("{token_type} · {model_id}"),
                remaining: (remaining * 100.0).clamp(0.0, 100.0),
                reset_time: bucket
                    .get("resetTime")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                model_id: Some(model_id.to_string()),
            })
        })
        .collect()
}

/// Fetch the authenticated Antigravity quota without starting the CLI or a
/// terminal process. On non-Windows platforms this provider remains disabled;
/// the native desktop target is Windows-only.
#[cfg(windows)]
pub(crate) fn fetch_plan() -> Option<AntigravityPlan> {
    let credential = read_credential()?;
    let agent = http_agent();

    // Prefer the cached access token, but retry once with a freshly exchanged
    // token if the provider rejects it (credentials can outlive the expiry
    // metadata written by the Antigravity client).
    if let Some(access_token) = valid_access_token(&credential) {
        if let Some(plan) = fetch_plan_with_token(&agent, &access_token) {
            return Some(plan);
        }
    }
    let access_token = refresh_access_token(&credential.refresh_token)?;
    fetch_plan_with_token(&agent, &access_token)
}

#[cfg(windows)]
fn fetch_plan_with_token(agent: &ureq::Agent, access_token: &str) -> Option<AntigravityPlan> {
    let load = post_json(
        agent,
        &format!("{API_HOST}/v1internal:loadCodeAssist"),
        &access_token,
        &json!({}),
    )?;
    let project = project_id(&load);
    let project_body = project
        .as_ref()
        .map(|project| json!({"project": project}))
        .unwrap_or_else(|| json!({}));

    // Match agy-quota's headless path: retrieveUserQuota is the authoritative
    // Gemini REQUESTS meter and does not require the IDE or loopback server.
    let buckets = post_json(
        agent,
        &format!("{API_HOST}/v1internal:retrieveUserQuota"),
        &access_token,
        &json!({}),
    );
    // Keep the summary endpoint only as an optional source for the short
    // session window. It must never replace the direct account quota above.
    let summary = post_json(
        agent,
        &format!("{API_HOST}/v1internal:retrieveUserQuotaSummary"),
        &access_token,
        &json!({}),
    );
    let models = post_json(
        agent,
        &format!("{API_HOST}/v1internal:fetchAvailableModels"),
        &access_token,
        &project_body,
    );
    build_plan_from_responses(&load, summary.as_ref(), models.as_ref(), buckets.as_ref())
}

#[cfg(not(windows))]
pub(crate) fn fetch_plan() -> Option<AntigravityPlan> {
    None
}

#[cfg(windows)]
fn read_credential() -> Option<OAuthCredential> {
    let bytes = keyring::read(KEYRING_TARGET)?;
    parse_credential(&bytes)
}

#[cfg(not(windows))]
fn read_credential() -> Option<OAuthCredential> {
    None
}

fn parse_credential(blob: &[u8]) -> Option<OAuthCredential> {
    let value = serde_json::from_slice::<Value>(blob).ok()?;
    let token = value.get("token").unwrap_or(&value);
    let refresh_token = token.get("refresh_token")?.as_str()?.trim();
    if refresh_token.is_empty() {
        return None;
    }
    Some(OAuthCredential {
        access_token: token
            .get("access_token")
            .and_then(Value::as_str)
            .map(str::to_owned),
        refresh_token: refresh_token.to_string(),
        expiry_ms: token
            .get("expiry")
            .or_else(|| token.get("expiry_date"))
            .and_then(parse_expiry_ms),
    })
}

fn parse_expiry_ms(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(if number < 10_000_000_000 {
            number.saturating_mul(1000)
        } else {
            number
        });
    }
    let text = value.as_str()?;
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|date| date.timestamp_millis())
}

fn valid_access_token(credential: &OAuthCredential) -> Option<String> {
    let token = credential.access_token.as_ref()?.trim();
    if token.is_empty() {
        return None;
    }
    let now = chrono::Utc::now().timestamp_millis();
    if credential
        .expiry_ms
        .is_none_or(|expiry| expiry > now.saturating_add(60_000))
    {
        Some(token.to_string())
    } else {
        None
    }
}

#[cfg(windows)]
fn refresh_access_token(refresh_token: &str) -> Option<String> {
    let agent = http_agent();
    for (client_id, client_secret) in oauth_client_candidates() {
        let form = format!(
            "client_id={}&client_secret={}&refresh_token={}&grant_type=refresh_token",
            form_encode(&client_id),
            form_encode(&client_secret),
            form_encode(refresh_token),
        );
        let Ok(mut response) = agent
            .post(TOKEN_ENDPOINT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(form.as_bytes())
        else {
            continue;
        };
        if response.status().as_u16() != 200 {
            continue;
        }
        let Ok(body) = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_to_string()
        else {
            continue;
        };
        let Ok(json) = serde_json::from_str::<Value>(&body) else {
            continue;
        };
        let Some(access_token) = json.get("access_token").and_then(Value::as_str) else {
            continue;
        };
        return Some(access_token.to_string());
    }
    None
}

#[cfg(not(windows))]
fn refresh_access_token(_refresh_token: &str) -> Option<String> {
    None
}

fn form_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{byte:02X}"));
        }
    }
    encoded
}

#[cfg(windows)]
fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .new_agent()
}

#[cfg(not(windows))]
fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder().build().new_agent()
}

#[cfg(windows)]
fn post_json(
    agent: &ureq::Agent,
    endpoint: &str,
    access_token: &str,
    body: &Value,
) -> Option<Value> {
    let payload = serde_json::to_vec(body).ok()?;
    let mut response = agent
        .post(endpoint)
        .header("Authorization", &format!("Bearer {access_token}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        // These are the same product headers used by agy-quota. They are
        // harmless for Gemini and allow fetchAvailableModels to expose the
        // Anthropic/OpenAI provider pools as well.
        .header("User-Agent", "antigravity")
        .header(
            "X-Goog-Api-Client",
            "google-cloud-sdk vscode_cloudshelleditor/0.1",
        )
        .header(
            "Client-Metadata",
            r#"{"ideType":"ANTIGRAVITY","platform":"WINDOWS","pluginType":"GEMINI"}"#,
        )
        .send(payload)
        .ok()?;
    if response.status().as_u16() != 200 {
        return None;
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .ok()?;
    serde_json::from_str(&body).ok()
}

#[cfg(windows)]
fn project_id(load_code_assist: &Value) -> Option<String> {
    let project = load_code_assist.get("cloudaicompanionProject")?;
    project
        .as_str()
        .map(str::to_owned)
        .or_else(|| project.get("id").and_then(Value::as_str).map(str::to_owned))
        .or_else(|| {
            project
                .get("projectId")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

#[cfg(windows)]
fn agy_binary_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("AGY_BIN").map(PathBuf::from) {
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let path = PathBuf::from(local_app_data)
            .join("agy")
            .join("bin")
            .join("agy.exe");
        if path.is_file() {
            return Some(path);
        }
    }
    env::var_os("PATH")?
        .to_string_lossy()
        .split(';')
        .find_map(|dir| {
            let path = PathBuf::from(dir).join("agy.exe");
            path.is_file().then_some(path)
        })
}

#[cfg(windows)]
fn oauth_client_candidates() -> Vec<(String, String)> {
    static CANDIDATES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    CANDIDATES
        .get_or_init(|| {
            let Some(path) = agy_binary_path() else {
                return Vec::new();
            };
            let Ok(binary) = fs::read(path) else {
                return Vec::new();
            };
            let ids = extract_ids(&binary);
            let secrets = extract_secrets(&binary);
            ids.into_iter()
                .flat_map(|id| {
                    secrets
                        .iter()
                        .cloned()
                        .map(move |secret| (id.clone(), secret))
                })
                .take(64)
                .collect()
        })
        .clone()
}

#[cfg(windows)]
fn extract_ids(binary: &[u8]) -> Vec<String> {
    let suffix = b".apps.googleusercontent.com";
    let mut ids = Vec::new();
    for (end, window) in binary.windows(suffix.len()).enumerate() {
        if window != suffix {
            continue;
        }
        let mut start = end;
        while start > 0
            && (binary[start - 1].is_ascii_lowercase()
                || binary[start - 1].is_ascii_digit()
                || binary[start - 1] == b'-')
        {
            start -= 1;
        }
        if end.saturating_sub(start) < 10 || !binary[start..end].iter().any(u8::is_ascii_digit) {
            continue;
        }
        let id = String::from_utf8_lossy(&binary[start..end + suffix.len()]).to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

#[cfg(windows)]
fn extract_secrets(binary: &[u8]) -> Vec<String> {
    let prefix = b"GOCSPX-";
    let mut secrets = Vec::new();
    for (start, window) in binary.windows(prefix.len()).enumerate() {
        if window != prefix {
            continue;
        }
        let mut end = start + prefix.len();
        while end < binary.len()
            && (binary[end].is_ascii_alphanumeric() || matches!(binary[end], b'_' | b'-'))
        {
            end += 1;
        }
        if end.saturating_sub(start + prefix.len()) != 28 {
            continue;
        }
        let secret = String::from_utf8_lossy(&binary[start..end]).to_string();
        if !secrets.contains(&secret) {
            secrets.push(secret);
        }
    }
    secrets
}

#[cfg(windows)]
mod keyring {
    use std::{ffi::c_void, ptr, slice};

    #[repr(C)]
    struct Credential {
        flags: u32,
        type_: u32,
        target_name: *mut u16,
        comment: *mut u16,
        last_written: i64,
        credential_blob_size: u32,
        credential_blob: *mut u8,
        persist: u32,
        attribute_count: u32,
        attributes: *mut c_void,
        target_alias: *mut u16,
        user_name: *mut u16,
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn CredReadW(
            target_name: *const u16,
            credential_type: u32,
            flags: u32,
            credential: *mut *mut Credential,
        ) -> i32;
        fn CredFree(buffer: *mut c_void);
    }

    pub(super) fn read(target: &str) -> Option<Vec<u8>> {
        let mut target_wide = target.encode_utf16().collect::<Vec<_>>();
        target_wide.push(0);
        let mut credential = ptr::null_mut();
        let ok = unsafe { CredReadW(target_wide.as_ptr(), 1, 0, &mut credential) };
        if ok == 0 || credential.is_null() {
            return None;
        }
        let result = unsafe {
            let value = &*credential;
            if value.credential_blob.is_null() || value.credential_blob_size == 0 {
                None
            } else {
                Some(
                    slice::from_raw_parts(
                        value.credential_blob,
                        value.credential_blob_size as usize,
                    )
                    .to_vec(),
                )
            }
        };
        unsafe { CredFree(credential.cast()) };
        result
    }
}

#[cfg(test)]
mod tests {
    use super::{build_plan_from_responses, form_encode, normalize_plan_name, parse_credential};
    use serde_json::json;

    #[test]
    fn cloud_plan_uses_paid_tier_and_authoritative_quota_windows() {
        let load = json!({
            "currentTier": {"id": "free-tier", "name": "Antigravity"},
            "paidTier": {"id": "g1-pro-tier", "name": "Google AI Pro"},
            "cloudaicompanionProject": "aicode-consumers"
        });
        let summary = json!({
            "groups": [{
                "displayName": "Gemini Models",
                "buckets": [
                    {"bucketId": "other-weekly", "window": "weekly", "remainingFraction": 0.43, "resetTime": "2026-09-22T04:34:22Z"},
                    {"bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.73, "resetTime": "2026-09-23T04:34:22Z"},
                    {"bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.88, "resetTime": "2026-09-17T09:34:22Z"}
                ]
            }]
        });
        let models = json!({"models": {
            "gemini-3-flash": {"displayName": "Gemini 3 Flash", "quotaInfo": {"remainingFraction": 0.91, "resetTime": "2026-09-17T09:34:22Z"}}
        }});
        let quota = json!({"buckets": [
            {"tokenType": "REQUESTS", "modelId": "gemini-3-flash", "remainingFraction": 0.86, "resetTime": "2026-09-22T04:34:22Z"}
        ]});

        let plan = build_plan_from_responses(&load, Some(&summary), Some(&models), Some(&quota))
            .expect("cloud response should produce a plan");

        assert_eq!(plan.plan, "Pro");
        assert_eq!(plan.quotas[0].model_id.as_deref(), Some("gemini-3-flash"));
        assert_eq!(plan.quotas[0].remaining, 86.0);
        assert_eq!(plan.weekly_remaining, Some(73.0));
        assert_eq!(
            plan.weekly_reset_time.as_deref(),
            Some("2026-09-23T04:34:22Z")
        );
        assert_eq!(plan.session_remaining, Some(88.0));
    }

    #[test]
    fn cloud_plan_ignores_non_gemini_buckets_when_summary_is_unavailable() {
        let load = json!({"currentTier": {"id": "free-tier", "name": "Antigravity"}});
        let quota = json!({"buckets": [
            {"tokenType": "WTUS", "modelId": "chat_20706", "remainingFraction": 0.42, "resetTime": "2026-09-17T09:34:22Z"}
        ]});

        let plan = build_plan_from_responses(&load, None, None, Some(&quota))
            .expect("quota buckets should produce a plan");

        assert_eq!(plan.plan, "Free");
        assert!(plan.quotas.is_empty());
        assert_eq!(plan.weekly_remaining, None);
        assert_eq!(plan.session_remaining, None);
    }

    #[test]
    fn untyped_gemini_request_buckets_are_not_misreported_as_weekly() {
        let load = json!({"currentTier": {"id": "pro-tier", "name": "Google AI Pro"}});
        let quota = json!({"buckets": [
            {"tokenType": "REQUESTS", "modelId": "gemini-3.8-flash", "remainingFraction": 0.92, "resetTime": "2026-09-25T02:00:00Z"}
        ]});

        let plan = build_plan_from_responses(&load, None, None, Some(&quota))
            .expect("account plan is available");

        assert_eq!(plan.weekly_remaining, None);
        assert_eq!(plan.weekly_reset_time, None);
    }

    #[test]
    fn cloud_plan_never_uses_the_five_hour_bucket_as_weekly_quota() {
        let load = json!({"currentTier": {"id": "pro-tier", "name": "Google AI Pro"}});
        let summary = json!({"groups": [{"buckets": [
            {"bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.86, "resetTime": "2026-09-23T04:34:22Z"},
            {"bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.43, "resetTime": "2026-09-18T09:34:22Z"}
        ]}]});
        let quota = json!({"buckets": [
            {"tokenType": "REQUESTS", "modelId": "gemini-3-pro", "remainingFraction": 0.86, "resetTime": "2026-09-23T04:34:22Z"},
            {"tokenType": "REQUESTS", "modelId": "gemini-3-pro-5h", "window": "5h", "remainingFraction": 0.43, "resetTime": "2026-09-18T09:34:22Z"}
        ]});

        let plan = build_plan_from_responses(&load, Some(&summary), None, Some(&quota))
            .expect("cloud response should produce a plan");

        assert_eq!(plan.weekly_remaining, Some(86.0));
        assert_eq!(
            plan.weekly_reset_time.as_deref(),
            Some("2026-09-23T04:34:22Z")
        );
        assert_eq!(plan.session_remaining, Some(43.0));
        assert_eq!(
            plan.session_reset_time.as_deref(),
            Some("2026-09-18T09:34:22Z")
        );
    }

    #[test]
    fn cloud_plan_uses_only_gemini_models_for_both_quota_windows() {
        let load = json!({"currentTier": {"id": "pro-tier", "name": "Google AI Pro"}});
        let summary = json!({"groups": [
            {"displayName": "Gemini Models", "buckets": [
                {"bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.76, "resetTime": "2026-10-01T00:00:00Z"},
                {"bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.92, "resetTime": "2026-09-25T02:00:00Z"}
            ]},
            {"displayName": "Claude and GPT models", "buckets": [
                {"bucketId": "claude-weekly", "window": "weekly", "remainingFraction": 0.80, "resetTime": "2026-10-01T01:00:00Z"},
                {"bucketId": "claude-5h", "window": "5h", "remainingFraction": 0.40, "resetTime": "2026-09-24T21:00:00Z"}
            ]}
        ]});
        let quota = json!({"buckets": [
            {"tokenType": "REQUESTS", "modelId": "gemini-3.8-flash", "remainingFraction": 0.92, "resetTime": "2026-09-25T02:00:00Z"},
            {"tokenType": "REQUESTS", "modelId": "claude-sonnet-4-6", "remainingFraction": 0.396, "resetTime": "2026-09-24T19:54:31Z"}
        ]});

        let plan = build_plan_from_responses(&load, Some(&summary), None, Some(&quota))
            .expect("cloud response should produce a plan");

        assert_eq!(plan.weekly_remaining, Some(76.0));
        assert_eq!(
            plan.weekly_reset_time.as_deref(),
            Some("2026-10-01T00:00:00Z")
        );
        assert_eq!(plan.session_remaining, Some(92.0));
        assert_eq!(
            plan.session_reset_time.as_deref(),
            Some("2026-09-25T02:00:00Z")
        );
        assert!(plan.quotas.iter().all(|quota| quota
            .model_id
            .as_deref()
            .is_some_and(|id| id.starts_with("gemini"))));
    }

    #[test]
    fn cloud_summary_fallback_ignores_non_gemini_groups() {
        let load = json!({"currentTier": {"id": "pro-tier", "name": "Google AI Pro"}});
        let summary = json!({"groups": [
            {"displayName": "Gemini Models", "buckets": [
                {"bucketId": "gemini-weekly", "window": "weekly", "remainingFraction": 0.76, "resetTime": "2026-10-01T00:00:00Z"},
                {"bucketId": "gemini-5h", "window": "5h", "remainingFraction": 0.92, "resetTime": "2026-09-25T02:00:00Z"}
            ]},
            {"displayName": "Claude and GPT models", "buckets": [
                {"bucketId": "claude-weekly", "window": "weekly", "remainingFraction": 0.80, "resetTime": "2026-10-01T01:00:00Z"},
                {"bucketId": "claude-5h", "window": "5h", "remainingFraction": 0.40, "resetTime": "2026-09-24T21:00:00Z"}
            ]}
        ]});

        let plan = build_plan_from_responses(&load, Some(&summary), None, None)
            .expect("plan name should be retained without the optional APIs");

        assert_eq!(plan.weekly_remaining, Some(76.0));
        assert_eq!(
            plan.weekly_reset_time.as_deref(),
            Some("2026-10-01T00:00:00Z")
        );
        assert_eq!(plan.session_remaining, Some(92.0));
        assert_eq!(
            plan.session_reset_time.as_deref(),
            Some("2026-09-25T02:00:00Z")
        );
        assert!(plan
            .quotas
            .iter()
            .all(|quota| quota.label.starts_with("Gemini Models · ")));
    }

    #[test]
    fn cloud_summary_rejects_non_gemini_buckets_inside_gemini_group() {
        let load = json!({"currentTier": {"id": "pro-tier", "name": "Google AI Pro"}});
        let summary = json!({"groups": [{
            "displayName": "Gemini Models",
            "buckets": [
                {"bucketId": "claude-weekly", "window": "weekly", "remainingFraction": 0.396, "resetTime": "2026-09-24T19:54:31Z"},
                {"bucketId": "claude-5h", "window": "5h", "remainingFraction": 0.40, "resetTime": "2026-09-24T19:54:31Z"}
            ]
        }]});

        let plan = build_plan_from_responses(&load, Some(&summary), None, None)
            .expect("account plan is available");

        assert_eq!(plan.weekly_remaining, None);
        assert_eq!(plan.session_remaining, None);
        assert!(plan.quotas.is_empty());
    }

    #[test]
    fn weekly_reset_time_belongs_to_the_tightest_weekly_bucket() {
        let load = json!({"currentTier": {"id": "pro-tier", "name": "Google AI Pro"}});
        let quota = json!({"buckets": [
            {"modelId": "gemini-weekly-a", "window": "weekly", "remainingFraction": 0.86, "resetTime": "2026-09-23T04:34:22Z"},
            {"modelId": "gemini-weekly-b", "window": "weekly", "remainingFraction": 0.94, "resetTime": "2026-09-22T04:34:22Z"}
        ]});

        let plan = build_plan_from_responses(&load, None, None, Some(&quota))
            .expect("cloud response should produce a plan");

        assert_eq!(plan.weekly_remaining, Some(86.0));
        assert_eq!(
            plan.weekly_reset_time.as_deref(),
            Some("2026-09-23T04:34:22Z")
        );
    }

    #[test]
    fn cloud_credentials_accept_nested_and_expiry_date_values() {
        let credentials = parse_credential(
            br#"{"token":{"access_token":"access","refresh_token":"refresh","expiry_date":1789671252000}}"#,
        )
        .expect("valid credential blob");

        assert_eq!(credentials.access_token.as_deref(), Some("access"));
        assert_eq!(credentials.refresh_token, "refresh");
        assert!(credentials.expiry_ms.is_some());
    }

    #[test]
    fn plan_names_are_reduced_to_pricing_tiers() {
        assert_eq!(normalize_plan_name("Google AI Ultra"), "Ultra");
        assert_eq!(normalize_plan_name("g1-pro-tier"), "Pro");
        assert_eq!(normalize_plan_name("free-tier"), "Free");
    }

    #[test]
    fn oauth_form_encoding_preserves_safe_characters() {
        assert_eq!(form_encode("a+b/c"), "a%2Bb%2Fc");
        assert_eq!(form_encode("client-id._~"), "client-id._~");
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a local authenticated Antigravity credential"]
    fn authenticated_cloud_fetch_returns_plan_and_quota() {
        let plan = super::fetch_plan().expect("cloud quota should be available");
        assert!(!plan.plan.is_empty());
        assert!(plan.weekly_remaining.is_some() || !plan.quotas.is_empty());
    }
}
