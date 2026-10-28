use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};

/// Persisted user settings (stored as JSON in the app config dir).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Accelerators use KeyboardEvent.code names, e.g. "Ctrl+PrintScreen", "Alt+KeyS".
    pub hotkey_capture: String,
    pub hotkey_text: String,
    /// Empty = default folder (Pictures\Screenshots).
    pub save_dir: String,
    /// How long history keeps unstarred shots, in days. 0 = forever.
    pub history_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey_capture: "Ctrl+PrintScreen".into(),
            // Not Shift+PrtSc: Lightshot owns it, so it fails to register while Lightshot runs.
            hotkey_text: "Ctrl+Shift+PrintScreen".into(),
            save_dir: String::new(),
            history_days: 30,
        }
    }
}

fn file(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .expect("no app config dir")
        .join("settings.json")
}

pub fn exists(app: &AppHandle) -> bool {
    file(app).exists()
}

pub fn load(app: &AppHandle) -> Settings {
    fs::read_to_string(file(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = file(app);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

/// Folder screenshots are saved to, resolving the default when unset.
pub fn save_dir(app: &AppHandle, settings: &Settings) -> PathBuf {
    if settings.save_dir.is_empty() {
        app.path()
            .picture_dir()
            .unwrap_or_else(|_| app.path().home_dir().unwrap_or_default())
            .join("Screenshots")
    } else {
        PathBuf::from(&settings.save_dir)
    }
}
