use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{atomic::AtomicU64, RwLock},
};
use tokio::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt as _;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

pub struct AppState {
    pub cli_path: Option<PathBuf>,
    pub settings: RwLock<AppSettings>,
    pub latest_refresh: RwLock<Option<RefreshSnapshot>>,
    pub scan_lock: tokio::sync::Mutex<()>,
    pub refresh_schedule_changed: tokio::sync::Notify,
    pub refresh_generation: AtomicU64,
    pub scan_control: crate::scan_control::ScanControl,
}

#[derive(Default)]
pub(crate) struct GeminiResetConfirmation {
    candidate: Option<(String, i64, u32, String, u8)>,
}

impl GeminiResetConfirmation {
    pub(crate) fn observe(&mut self, source: &str, reset_time: Option<&str>) -> Option<String> {
        let Some(reset_time) = reset_time else {
            self.candidate = None;
            return None;
        };
        let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(reset_time) else {
            self.candidate = None;
            return None;
        };
        let instant = (parsed.timestamp(), parsed.timestamp_subsec_nanos());

        match self.candidate.as_mut() {
            Some((candidate_source, seconds, nanos, value, consecutive))
                if candidate_source == source && (*seconds, *nanos) == instant =>
            {
                *consecutive = consecutive.saturating_add(1);
                *value = reset_time.to_string();
                (*consecutive >= 2).then(|| reset_time.to_string())
            }
            _ => {
                self.candidate = Some((
                    source.to_string(),
                    instant.0,
                    instant.1,
                    reset_time.to_string(),
                    1,
                ));
                None
            }
        }
    }

    pub(crate) fn observe_preserving_confirmed(
        &mut self,
        source: &str,
        reset_time: Option<&str>,
        previously_confirmed: Option<&str>,
        now_timestamp: i64,
    ) -> Option<String> {
        let newly_confirmed = self.observe(source, reset_time);
        if newly_confirmed.is_some() {
            return newly_confirmed;
        }

        previously_confirmed.and_then(|value| {
            chrono::DateTime::parse_from_rfc3339(value)
                .ok()
                .filter(|parsed| parsed.timestamp() > now_timestamp)
                .map(|_| value.to_string())
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshSnapshot {
    #[serde(default)]
    pub generation: u64,
    pub refreshed_at_ms: i64,
    pub report: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AppSettings {
    pub custom_cli_path: Option<String>,
    pub codex_homes: String,
    pub offline: bool,
    pub refresh_minutes: u64,
    pub quota_source: String,
    pub antigravity_ultra_price: Option<f64>,
    pub hidden_agents: Vec<String>,
    pub auto_check_updates: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            custom_cli_path: None,
            codex_homes: String::new(),
            offline: false,
            refresh_minutes: 1,
            quota_source: "codex".to_string(),
            antigravity_ultra_price: None,
            hidden_agents: Vec::new(),
            auto_check_updates: true,
        }
    }
}

impl AppSettings {
    pub fn normalized(mut self) -> Self {
        self.refresh_minutes = self.refresh_minutes.max(1);
        self.custom_cli_path = self
            .custom_cli_path
            .take()
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty());
        self.antigravity_ultra_price = self
            .antigravity_ultra_price
            .filter(|price| matches!(*price, 100.0 | 200.0));
        self.hidden_agents = self
            .hidden_agents
            .drain(..)
            .map(|agent| agent.trim().to_lowercase())
            .filter(|agent| !agent.is_empty())
            .collect();
        self.hidden_agents.sort();
        self.hidden_agents.dedup();
        self
    }
}

impl AppState {
    pub fn new() -> Self {
        let settings = load_settings();
        let cli_path = resolve_cli_path_with_override(settings.custom_cli_path.as_deref());
        Self {
            cli_path,
            settings: RwLock::new(settings),
            latest_refresh: RwLock::new(None),
            scan_lock: tokio::sync::Mutex::new(()),
            refresh_schedule_changed: tokio::sync::Notify::new(),
            refresh_generation: AtomicU64::new(0),
            scan_control: crate::scan_control::ScanControl::default(),
        }
    }
}

fn settings_path() -> PathBuf {
    crate::archive::get_data_dir().join("settings.json")
}

fn load_settings() -> AppSettings {
    fs::read(settings_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<AppSettings>(&bytes).ok())
        .unwrap_or_default()
        .normalized()
}

pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(settings_path(), bytes)
        .map_err(|error| format!("Impossible d'enregistrer les paramètres: {error}"))
}

pub fn resolve_cli_path_with_override(custom_path: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = custom_path.map(str::trim).filter(|path| !path.is_empty()) {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return candidate.canonicalize().ok();
        }
    }

    let current_exe_canon = env::current_exe().ok().and_then(|p| p.canonicalize().ok());

    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            // 1. En priorité dans le dossier resources/
            let in_resources = parent.join("resources").join("agent-burn.exe");
            if in_resources.is_file() {
                if let Ok(canon) = in_resources.canonicalize() {
                    if Some(&canon) != current_exe_canon.as_ref() {
                        return Some(canon);
                    }
                }
            }

            // 2. À côté de l'exécutable
            let bundled = parent.join("agent-burn.exe");
            if bundled.is_file() {
                if let Ok(canon) = bundled.canonicalize() {
                    if Some(&canon) != current_exe_canon.as_ref() {
                        return Some(canon);
                    }
                }
            }
        }
    }

    // 3. Recherche dans les dossiers du projet
    for candidate in [
        Path::new("resources/agent-burn.exe"),
        Path::new("agent-burn.exe"),
        Path::new("../../rust/target/release/agent-burn.exe"),
        Path::new("../../../rust/target/release/agent-burn.exe"),
    ] {
        if candidate.is_file() {
            if let Ok(canon) = candidate.canonicalize() {
                if Some(&canon) != current_exe_canon.as_ref() {
                    return Some(canon);
                }
            }
        }
    }

    // 4. Recherche dans le PATH Windows
    let mut where_command = std::process::Command::new("where.exe");
    #[cfg(windows)]
    where_command.creation_flags(CREATE_NO_WINDOW);
    if let Ok(output) = where_command.arg("agent-burn.exe").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            for line in path_str.lines() {
                let p = PathBuf::from(line.trim());
                if p.is_file() {
                    if let Ok(canon) = p.canonicalize() {
                        if Some(&canon) != current_exe_canon.as_ref() {
                            return Some(canon);
                        }
                    }
                }
            }
        }
    }

    None
}

pub async fn execute_cli_json_with_settings(
    scan_control: &crate::scan_control::ScanControl,
    cli_path: Option<&Path>,
    args: &[&str],
    settings: &AppSettings,
) -> Result<serde_json::Value, String> {
    let (_permit, mut cancellation) = scan_control.begin_scan()?;
    execute_cli_json_inner(cli_path, args, settings, &mut cancellation).await
}

fn cli_command(cli_path: Option<&Path>) -> Result<Command, String> {
    cli_path
        .map(Command::new)
        .ok_or_else(|| "Agent Burn CLI introuvable. Réinstallez l'application ou choisissez agent-burn.exe dans Settings.".to_string())
}

async fn execute_cli_json_inner(
    cli_path: Option<&Path>,
    args: &[&str],
    settings: &AppSettings,
    cancellation: &mut crate::scan_control::ScanCancellation,
) -> Result<serde_json::Value, String> {
    let mut cmd = cli_command(cli_path)?;

    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW); // masque tout terminal

    cmd.args(args);
    if settings.offline {
        cmd.arg("--offline");
    }
    if !settings.codex_homes.trim().is_empty() {
        cmd.env("CODEX_HOME", settings.codex_homes.trim());
    }
    cmd.env("AGENT_BURN_TIMELINE_CACHE", "1");
    cmd.arg("--json");
    cmd.arg("--no-color");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Impossible d'exécuter la CLI: {e}"))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_reader = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = stdout {
            tokio::io::AsyncReadExt::read_to_end(&mut pipe, &mut bytes)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok::<_, String>(bytes)
    });
    let stderr_reader = tokio::spawn(async move {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = stderr {
            tokio::io::AsyncReadExt::read_to_end(&mut pipe, &mut bytes)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok::<_, String>(bytes)
    });

    let status = tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = stdout_reader.await;
            let _ = stderr_reader.await;
            return Err("Scan annulé pour préparer la mise à jour.".to_string());
        }
        result = tokio::time::timeout(std::time::Duration::from_secs(120), child.wait()) => {
            match result {
                Ok(Ok(status)) => status,
                Ok(Err(error)) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    let _ = stdout_reader.await;
                    let _ = stderr_reader.await;
                    return Err(format!("Impossible d'attendre la fin de la CLI: {error}"));
                }
                Err(_) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    let _ = stdout_reader.await;
                    let _ = stderr_reader.await;
                    return Err("La CLI a dépassé le délai maximal de 120 secondes.".to_string());
                }
            }
        }
    };
    let stdout = stdout_reader
        .await
        .map_err(|error| format!("Lecture stdout impossible: {error}"))?
        .map_err(|error| format!("Lecture stdout impossible: {error}"))?;
    let stderr = stderr_reader
        .await
        .map_err(|error| format!("Lecture stderr impossible: {error}"))?
        .map_err(|error| format!("Lecture stderr impossible: {error}"))?;
    let stdout_str = String::from_utf8_lossy(&stdout);
    let stderr_str = String::from_utf8_lossy(&stderr);

    if !status.success() {
        return Err(format!(
            "Erreur CLI (code {}): {}",
            status,
            stderr_str.trim()
        ));
    }

    let trimmed = stdout_str.trim();
    if trimmed.is_empty() {
        return Err(format!(
            "La CLI n'a retourné aucune donnée JSON. Détails stderr: {}",
            if stderr_str.trim().is_empty() {
                "aucun message stderr"
            } else {
                stderr_str.trim()
            }
        ));
    }

    // Tenter de désérialiser directement
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Ok(val);
    }

    // Si du texte entoure le JSON (ex: logs), extraire le bloc JSON { ... }
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            let json_slice = &trimmed[start..=end];
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_slice) {
                return Ok(val);
            }
        }
    }

    Err(format!(
        "Format JSON invalide retourné par la CLI: contenu reçu ({} caractères): {}",
        trimmed.len(),
        trimmed.chars().take(200).collect::<String>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_reject_refresh_intervals_shorter_than_one_minute() {
        let settings = AppSettings {
            refresh_minutes: 0,
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(settings.refresh_minutes, 1);
    }

    #[test]
    fn a_five_hour_reset_is_hidden_until_two_successive_samples_match() {
        let mut confirmation = GeminiResetConfirmation::default();

        assert_eq!(
            confirmation.observe("cloud", Some("2026-09-25T16:30:12Z")),
            None
        );
        assert_eq!(
            confirmation.observe("cloud", Some("2026-09-25T16:30:12Z")),
            Some("2026-09-25T16:30:12Z".to_string())
        );
    }

    #[test]
    fn previously_confirmed_reset_survives_restart_on_first_matching_sample() {
        let mut confirmation = GeminiResetConfirmation::default();

        assert_eq!(
            confirmation.observe_preserving_confirmed(
                "cloud",
                Some("2026-09-25T16:30:12Z"),
                Some("2026-09-25T16:30:12Z"),
                1_790_350_000,
            ),
            Some("2026-09-25T16:30:12Z".to_string())
        );
    }

    #[test]
    fn changed_reset_keeps_the_future_confirmed_reset_until_confirmed_again() {
        let mut confirmation = GeminiResetConfirmation::default();

        assert_eq!(
            confirmation.observe_preserving_confirmed(
                "cloud",
                Some("2026-09-25T16:35:12Z"),
                Some("2026-09-25T16:30:12Z"),
                1_790_350_000,
            ),
            Some("2026-09-25T16:30:12Z".to_string())
        );
        assert_eq!(
            confirmation.observe_preserving_confirmed(
                "cloud",
                Some("2026-09-25T16:35:12Z"),
                Some("2026-09-25T16:30:12Z"),
                1_790_350_000,
            ),
            Some("2026-09-25T16:35:12Z".to_string())
        );
    }

    #[test]
    fn expired_confirmed_reset_is_not_restored() {
        let mut confirmation = GeminiResetConfirmation::default();

        assert_eq!(
            confirmation.observe_preserving_confirmed(
                "cloud",
                Some("2026-09-25T16:35:12Z"),
                Some("2026-09-25T16:30:12Z"),
                1_790_354_000,
            ),
            None
        );
    }

    #[test]
    fn a_changed_five_hour_reset_requires_two_new_matching_samples() {
        let mut confirmation = GeminiResetConfirmation::default();
        confirmation.observe("cloud", Some("2026-09-25T16:30:12Z"));
        confirmation.observe("cloud", Some("2026-09-25T16:30:12Z"));

        assert_eq!(
            confirmation.observe("cloud", Some("2026-09-25T16:35:12Z")),
            None
        );
        assert_eq!(
            confirmation.observe("cloud", Some("2026-09-25T16:35:12Z")),
            Some("2026-09-25T16:35:12Z".to_string())
        );
    }

    #[test]
    fn a_missing_or_invalid_five_hour_reset_clears_confirmation() {
        let mut confirmation = GeminiResetConfirmation::default();
        confirmation.observe("cloud", Some("2026-09-25T16:30:12Z"));
        confirmation.observe("cloud", Some("2026-09-25T16:30:12Z"));

        assert_eq!(confirmation.observe("cloud", None), None);
        assert_eq!(confirmation.observe("cloud", Some("not-a-date")), None);
        assert_eq!(
            confirmation.observe("cloud", Some("2026-09-25T16:30:12Z")),
            None
        );
    }

    #[test]
    fn equivalent_five_hour_reset_instants_match_across_timezones() {
        let mut confirmation = GeminiResetConfirmation::default();
        confirmation.observe("cloud", Some("2026-09-25T16:30:12Z"));

        assert_eq!(
            confirmation.observe("cloud", Some("2026-09-25T18:30:12+02:00")),
            Some("2026-09-25T18:30:12+02:00".to_string())
        );
    }

    #[test]
    fn a_five_hour_reset_from_a_different_source_does_not_confirm_the_candidate() {
        let mut confirmation = GeminiResetConfirmation::default();
        confirmation.observe("cloud", Some("2026-09-25T16:30:12Z"));

        assert_eq!(
            confirmation.observe("local", Some("2026-09-25T16:30:12Z")),
            None
        );
        assert_eq!(
            confirmation.observe("local", Some("2026-09-25T16:30:12Z")),
            Some("2026-09-25T16:30:12Z".to_string())
        );
    }

    #[test]
    fn automatic_updates_default_to_enabled_and_old_settings_remain_compatible() {
        assert!(AppSettings::default().auto_check_updates);
        let old_settings: AppSettings = serde_json::from_str(
            r#"{"codexHomes":"","offline":false,"refreshMinutes":5,"quotaSource":"codex"}"#,
        )
        .unwrap();
        assert!(old_settings.auto_check_updates);
    }

    #[test]
    fn blank_custom_cli_path_is_treated_as_unconfigured() {
        let settings = AppSettings {
            custom_cli_path: Some("   ".to_string()),
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(settings.custom_cli_path, None);
    }

    #[test]
    fn missing_cli_never_falls_back_to_a_network_download() {
        assert!(cli_command(None).is_err());
    }

    #[test]
    fn antigravity_ultra_price_accepts_only_exact_supported_prices() {
        for price in [100.0, 200.0] {
            let settings = AppSettings {
                antigravity_ultra_price: Some(price),
                ..AppSettings::default()
            }
            .normalized();
            assert_eq!(settings.antigravity_ultra_price, Some(price));
        }

        let settings = AppSettings {
            antigravity_ultra_price: Some(100.99),
            ..AppSettings::default()
        }
        .normalized();
        assert_eq!(settings.antigravity_ultra_price, None);
    }

    #[test]
    fn hidden_agents_are_normalized_and_deduplicated() {
        let settings = AppSettings {
            hidden_agents: vec![" Codex ".into(), "codex".into(), "".into()],
            ..AppSettings::default()
        }
        .normalized();

        assert_eq!(settings.hidden_agents, vec!["codex"]);
    }
}
