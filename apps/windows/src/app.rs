use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::RwLock,
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
    pub summary_scan: tokio::sync::Mutex<()>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshSnapshot {
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
            summary_scan: tokio::sync::Mutex::new(()),
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
    cli_path: Option<&Path>,
    args: &[&str],
    settings: &AppSettings,
) -> Result<serde_json::Value, String> {
    execute_cli_json_inner(cli_path, args, settings).await
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
    cmd.arg("--json");
    cmd.arg("--no-color");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let output = tokio::time::timeout(std::time::Duration::from_secs(120), cmd.output())
        .await
        .map_err(|_| "La CLI a dépassé le délai maximal de 120 secondes.".to_string())?
        .map_err(|e| format!("Impossible d'exécuter la CLI: {e}"))?;

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let stderr_str = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        return Err(format!(
            "Erreur CLI (code {}): {}",
            output.status,
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
