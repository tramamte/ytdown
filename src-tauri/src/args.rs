use crate::settings::Settings;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind { Video, Audio }

pub fn build(kind: Kind, url: &str, playlist: bool, s: &Settings, ffmpeg: &Path, deno: &Path, tmp: &Path) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--newline".into(), "--no-colors".into(),
        // --print implies --quiet, which would suppress progress lines
        "--progress".into(),
        "--progress-template".into(), "download:ytdown:%(progress._percent_str)s".into(),
        "--print".into(), "before_dl:ytdown-title:%(title)s".into(), "--no-simulate".into(),
        "--ffmpeg-location".into(), ffmpeg.display().to_string(),
        "--js-runtimes".into(), format!("deno:{}", deno.display()),
        "-P".into(), format!("temp:{}", tmp.display()),
        "-P".into(), s.download_dir.display().to_string(),
        // keep -o relative: an absolute -o makes yt-dlp ignore -P
        "-o".into(), "%(title)s.%(ext)s".into(),
        (if playlist { "--yes-playlist" } else { "--no-playlist" }).into(),
    ];
    match kind {
        Kind::Video => {
            let f = match s.quality {
                None => "bv*+ba/b".to_string(),
                Some(h) => format!("bv*[height<={h}]+ba/b[height<={h}]"),
            };
            a.extend(["-f".into(), f, "--merge-output-format".into(), s.container.clone()]);
        }
        Kind::Audio => a.extend(["-x".into(), "--audio-format".into(), s.audio_format.clone()]),
    }
    // "--" stops option parsing so a URL can never be read as a flag
    a.extend(["--".into(), url.into()]);
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    fn s() -> Settings { Settings::new(PathBuf::from("/dl")) }
    fn has_pair(a: &[String], p: [&str; 2]) -> bool { a.windows(2).any(|w| w == p) }

    #[test]
    fn video_best_mp4_single() {
        let a = build(Kind::Video, "https://youtu.be/x", false, &s(), Path::new("/ff"), Path::new("/deno"), Path::new("/tmp/1"));
        assert!(a.windows(2).any(|w| w == ["-f", "bv*+ba/b"]));
        assert!(a.windows(2).any(|w| w == ["--merge-output-format", "mp4"]));
        assert!(a.contains(&"--no-playlist".to_string()));
        assert!(a.contains(&"--progress".to_string()));
        assert!(has_pair(&a, ["--js-runtimes", "deno:/deno"]));
        assert!(has_pair(&a, ["-o", "%(title)s.%(ext)s"]));
        assert_eq!(&a[a.len() - 2..], ["--", "https://youtu.be/x"]);
    }
    #[test]
    fn video_capped_height_playlist() {
        let mut st = s(); st.quality = Some(720);
        let a = build(Kind::Video, "u", true, &st, Path::new("/ff"), Path::new("/deno"), Path::new("/t"));
        assert!(a.windows(2).any(|w| w == ["-f", "bv*[height<=720]+ba/b[height<=720]"]));
        assert!(a.contains(&"--yes-playlist".to_string()));
        assert!(a.contains(&"--progress".to_string()));
        assert!(has_pair(&a, ["--js-runtimes", "deno:/deno"]));
    }
    #[test]
    fn audio_mp3() {
        let a = build(Kind::Audio, "u", false, &s(), Path::new("/ff"), Path::new("/deno"), Path::new("/t"));
        assert!(a.contains(&"-x".to_string()));
        assert!(a.windows(2).any(|w| w == ["--audio-format", "mp3"]));
        assert!(a.contains(&"--progress".to_string()));
        assert!(has_pair(&a, ["--js-runtimes", "deno:/deno"]));
    }
}
