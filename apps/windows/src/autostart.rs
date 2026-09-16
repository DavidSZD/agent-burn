use std::env;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const REG_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const REG_VALUE_NAME: &str = "AgentBurn";

pub fn is_autostart_enabled() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(run_key) = hkcu.open_subkey_with_flags(REG_KEY_PATH, KEY_READ) {
        if let Ok(val) = run_key.get_value::<String, _>(REG_VALUE_NAME) {
            if let Ok(current_exe) = env::current_exe() {
                let current_str = current_exe.to_string_lossy().to_string();
                return val.contains(&current_str);
            }
            return !val.is_empty();
        }
    }
    false
}

pub fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    if enabled {
        let current_exe = env::current_exe()
            .map_err(|e| format!("Impossible d'obtenir le chemin de l'exécutable: {e}"))?;
        let exe_path_str = format!("\"{}\"", current_exe.to_string_lossy());

        let (run_key, _) = hkcu
            .create_subkey(REG_KEY_PATH)
            .map_err(|e| format!("Impossible d'ouvrir la clé de registre Run: {e}"))?;

        run_key
            .set_value(REG_VALUE_NAME, &exe_path_str)
            .map_err(|e| format!("Impossible d'écrire dans le registre Windows: {e}"))?;
    } else {
        if let Ok(run_key) = hkcu.open_subkey_with_flags(REG_KEY_PATH, KEY_WRITE) {
            let _ = run_key.delete_value(REG_VALUE_NAME);
        }
    }

    Ok(())
}
