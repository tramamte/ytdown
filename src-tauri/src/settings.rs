use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub quality: Option<u32>, // None = best
    pub container: String,    // mp4 | mkv | webm
    pub audio_format: String, // mp3 | m4a | opus | ...
    pub shortcut: String,
}

impl Settings {
    pub fn new(download_dir: PathBuf) -> Self {
        Self { download_dir, quality: None, container: "mp4".into(),
               audio_format: "mp3".into(), shortcut: "CommandOrControl+Shift+Y".into() }
    }
}

fn path(app: &AppHandle) -> PathBuf {
    app.path().app_config_dir().expect("config dir").join("settings.json")
}

pub fn load(app: &AppHandle) -> Settings {
    std::fs::read(path(app)).ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| Settings::new(app.path().download_dir().unwrap_or_default()))
}

pub fn save(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let p = path(app);
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(p, serde_json::to_vec_pretty(s).unwrap()).map_err(|e| e.to_string())
}
