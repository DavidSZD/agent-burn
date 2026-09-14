use std::{
    env,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::process::Command;

pub struct AppState {
    pub cli_path: Option<PathBuf>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            cli_path: resolve_cli_path(),
        }
    }
}

pub fn resolve_cli_path() -> Option<PathBuf> {
    if let Ok(current_exe) = env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let bundled = parent.join("agent-burn.exe");
            if bundled.is_file() {
                return Some(bundled);
            }
            let in_resources = parent.join("resources").join("agent-burn.exe");
            if in_resources.is_file() {
                return Some(in_resources);
            }
        }
    }

    // Recherche dans les dossiers du projet
    for candidate in [
        Path::new("agent-burn.exe"),
        Path::new("resources/agent-burn.exe"),
        Path::new("../../rust/target/release/agent-burn.exe"),
        Path::new("../../../rust/target/release/agent-burn.exe"),
    ] {
        if candidate.is_file() {
            if let Ok(canon) = candidate.canonicalize() {
                return Some(canon);
            }
        }
    }

    if let Ok(output) = std::process::Command::new("where.exe")
        .arg("agent-burn.exe")
        .output()
    {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout);
            if let Some(first_line) = path_str.lines().next() {
                let p = PathBuf::from(first_line.trim());
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }

    None
}

pub async fn execute_cli_json(
    cli_path: Option<&Path>,
    args: &[&str],
) -> Result<serde_json::Value, String> {
    let mut cmd = if let Some(path) = cli_path {
        Command::new(path)
    } else {
        let mut c = Command::new("cmd.exe");
        c.args(&["/C", "npx", "agent-burn@latest"]);
        c
    };

    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW (masque tout terminal)

    cmd.args(args);
    cmd.arg("--json");
    cmd.arg("--no-color");
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Impossible d'exécuter la CLI: {e}"))?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Erreur CLI (code {}): {}", output.status, err_msg));
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&json_str)
        .map_err(|e| format!("Format JSON invalide retourné par la CLI: {e}"))
}
