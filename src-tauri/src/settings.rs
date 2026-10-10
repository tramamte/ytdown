use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub quality: Option<u32>, // None = best
    pub container: String,    // mp4 | mkv | webm | original
    pub audio_format: String, // mp3 | m4a | opus | ... | original
    pub shortcut: String,
    #[serde(default = "default_codec")]
    pub video_codec: String,  // auto | h264 | vp9 | av1
    #[serde(default = "default_audio_quality")]
    pub audio_quality: String, // best | 320k | 192k | 128k
}

fn default_codec() -> String { "auto".into() }
fn default_audio_quality() -> String { "best".into() }

impl Settings {
    pub fn new(download_dir: PathBuf) -> Self {
        Self { download_dir, quality: None, container: "original".into(),
               audio_format: "original".into(), shortcut: "Control+Alt+Y".into(),
               video_codec: default_codec(), audio_quality: default_audio_quality() }
    }
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map(|d| d.join("settings.json")).map_err(|e| e.to_string())
}

pub fn load(app: &AppHandle) -> Settings {
    path(app).ok().and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| Settings::new(app.path().download_dir().unwrap_or_default()))
}

pub fn save(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let p = path(app)?;
    std::fs::create_dir_all(p.parent().ok_or("config dir: no parent")?).map_err(|e| e.to_string())?;
    std::fs::write(p, serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub fn validate(s: &Settings) -> Result<(), String> {
    if !["mp4", "mkv", "webm", "original"].contains(&s.container.as_str()) {
        return Err(format!("container: unsupported value {:?}", s.container));
    }
    if !["mp3", "m4a", "opus", "flac", "wav", "original"].contains(&s.audio_format.as_str()) {
        return Err(format!("audio_format: unsupported value {:?}", s.audio_format));
    }
    if !["auto", "h264", "vp9", "av1"].contains(&s.video_codec.as_str()) {
        return Err(format!("video_codec: unsupported value {:?}", s.video_codec));
    }
    if !["best", "320k", "192k", "128k"].contains(&s.audio_quality.as_str()) {
        return Err(format!("audio_quality: unsupported value {:?}", s.audio_quality));
    }
    if !matches!(s.quality, None | Some(2160 | 1440 | 1080 | 720 | 480 | 360)) {
        return Err("quality: unsupported value".into());
    }
    if !s.download_dir.is_absolute() || !s.download_dir.is_dir() {
        return Err("download_dir: must be an absolute path to an existing directory".into());
    }
    if s.shortcut.is_empty() {
        return Err("shortcut: must not be empty".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok() -> Settings { Settings::new(std::env::temp_dir()) }
    fn err(s: Settings, field: &str) {
        let e = validate(&s).unwrap_err();
        assert!(e.starts_with(&format!("{field}: ")), "{e}");
    }

    #[test]
    fn valid_default() { assert!(validate(&ok()).is_ok()); }

    #[test]
    fn invalid_fields() {
        err(Settings { container: "avi".into(), ..ok() }, "container");
        err(Settings { audio_format: "ogg".into(), ..ok() }, "audio_format");
        err(Settings { quality: Some(999), ..ok() }, "quality");
        err(Settings { download_dir: "relative".into(), ..ok() }, "download_dir");
        err(Settings { download_dir: std::env::temp_dir().join("ytdown-nope-xyz"), ..ok() }, "download_dir");
        err(Settings { shortcut: "".into(), ..ok() }, "shortcut");
    }

    #[test]
    fn invalid_codec_and_audio_quality() {
        err(Settings { video_codec: "hevc".into(), ..ok() }, "video_codec");
        err(Settings { audio_quality: "64k".into(), ..ok() }, "audio_quality");
    }

    #[test]
    fn legacy_json_gets_defaults() {
        let s: Settings = serde_json::from_str(
            r#"{"download_dir":"/dl","quality":720,"container":"mkv","audio_format":"m4a","shortcut":"X"}"#).unwrap();
        assert_eq!((s.quality, s.container.as_str(), s.audio_format.as_str()), (Some(720), "mkv", "m4a"));
        assert_eq!((s.video_codec.as_str(), s.audio_quality.as_str()), ("auto", "best"));
    }

    #[test]
    fn valid_quality() {
        assert!(validate(&Settings { quality: Some(1080), ..ok() }).is_ok());
    }

    #[test]
    fn original_defaults_and_valid() {
        let d = ok();
        assert_eq!((d.container.as_str(), d.audio_format.as_str()), ("original", "original"));
        assert!(validate(&Settings { container: "original".into(), ..ok() }).is_ok());
        assert!(validate(&Settings { audio_format: "original".into(), ..ok() }).is_ok());
    }
}
