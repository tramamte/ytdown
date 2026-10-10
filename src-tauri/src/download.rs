use crate::args::{self, Kind};
use crate::progress::{self, Line};
use crate::url::{classify, UrlKind};
use crate::{deps, settings};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager as _, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

const EVENT: &str = "download://update";
const CANCELED: (&str, String) = ("canceled", String::new());

pub struct Manager {
    pub sem: Arc<Semaphore>,
    jobs: Mutex<HashMap<u64, CancellationToken>>,
    next: AtomicU64,
}

impl Manager {
    pub fn new() -> Self {
        Self { sem: Arc::new(Semaphore::new(3)), jobs: Default::default(), next: AtomicU64::new(1) }
    }

    fn active(&self) -> bool {
        !self.jobs.lock().unwrap().is_empty()
    }
}

/// Highest percent seen for the current stream; resets when the stream (format_id) changes.
fn monotonic(max: &mut (String, f32), format_id: &str, percent: f32) -> f32 {
    if max.0 != format_id { *max = (format_id.to_string(), 0.0); }
    max.1 = max.1.max(percent);
    max.1
}

/// Any download queued or running.
pub fn has_active_jobs(app: &AppHandle) -> bool {
    app.state::<Manager>().active()
}

#[derive(Serialize, Clone)]
struct Update {
    id: u64,
    title: String,
    kind: Kind,
    state: &'static str,
    percent: f32,
    message: String,
    size: u64, // bytes of the finished files; 0 until done or if unknown
    stage: u32, // 1-based index of the stream being downloaded; 0 if unknown
    stages: u32, // number of streams of the item; 0 if unknown
    stream: &'static str, // "video" | "audio" | "" if unknown
}

#[tauri::command]
pub fn classify_url(url: String) -> UrlKind {
    classify(&url)
}

#[tauri::command]
pub fn start_download(app: AppHandle, state: State<'_, Manager>, url: String, kind: Kind, playlist: bool) -> Result<u64, String> {
    if classify(&url) == UrlKind::Invalid {
        return Err("Invalid YouTube URL".into());
    }
    if !deps::ytdlp_path(&app).exists() {
        return Err("Dependencies are not ready".into());
    }
    let id = state.next.fetch_add(1, Ordering::Relaxed);
    let tok = CancellationToken::new();
    state.jobs.lock().unwrap().insert(id, tok.clone());
    let url = url.trim().to_string();
    let ev = Update { id, title: url.clone(), kind, state: "queued", percent: 0.0, message: String::new(), size: 0, stage: 0, stages: 0, stream: "" };
    tauri::async_runtime::spawn(run_job(app, state.sem.clone(), tok, ev, url, playlist));
    Ok(id)
}

#[tauri::command]
pub fn cancel_download(state: State<'_, Manager>, id: u64) {
    if let Some(t) = state.jobs.lock().unwrap().get(&id) {
        t.cancel();
    }
}

fn emit(app: &AppHandle, ev: &Update) {
    let _ = app.emit(EVENT, ev);
}

async fn run_job(app: AppHandle, sem: Arc<Semaphore>, tok: CancellationToken, mut ev: Update, url: String, playlist: bool) {
    let (state, message) = match app.path().app_cache_dir() {
        Ok(dir) => {
            let tmp = dir.join(format!("ytdown-{}", ev.id));
            let r = job(&app, &sem, &tok, &mut ev, &url, playlist, &tmp).await;
            // best effort: an orphaned ffmpeg on Windows may still hold files
            let _ = tokio::fs::remove_dir_all(&tmp).await;
            r
        }
        Err(e) => ("error", e.to_string()),
    };
    app.state::<Manager>().jobs.lock().unwrap().remove(&ev.id);
    ev.state = state;
    ev.message = message;
    emit(&app, &ev);
}

async fn job(app: &AppHandle, sem: &Arc<Semaphore>, tok: &CancellationToken, ev: &mut Update,
             url: &str, playlist: bool, tmp: &Path) -> (&'static str, String) {
    emit(app, ev);
    let _permit = tokio::select! {
        p = sem.clone().acquire_owned() => match p { Ok(p) => p, Err(e) => return ("error", e.to_string()) },
        _ = tok.cancelled() => return CANCELED,
    };
    ev.state = "running";
    emit(app, ev);

    if let Err(e) = tokio::fs::create_dir_all(tmp).await {
        return ("error", e.to_string());
    }
    // ffmpeg/deno: fast probe (reinstalled if broken). Cancellable only while waiting to install.
    if let Err(e) = deps::repair_fast_tools(app, tok).await {
        return ("error", e);
    }
    if tok.is_cancelled() {
        return CANCELED;
    }
    let s = settings::load(app);
    let a = args::build(ev.kind, url, playlist, &s, &deps::ffmpeg_path(app), &deps::deno_path(app), tmp);
    // yt-dlp has no cheap probe: judge it by the real run, repair and retry once.
    let seen = deps::stamp(&deps::ytdlp_path(app));
    match run(app, tok, ev, &a).await {
        Some(r) => r,
        None => {
            if let Err(e) = deps::repair_ytdlp_since(app, tok, seen).await {
                return ("error", format!("{BROKEN} ({e})"));
            }
            if tok.is_cancelled() {
                return CANCELED;
            }
            run(app, tok, ev, &a).await.unwrap_or_else(|| ("error", BROKEN.into()))
        }
    }
}

const BROKEN: &str = "yt-dlp could not be started. Use Settings → Update to reinstall.";

/// Runs yt-dlp once. None = it never started (see `never_started`).
async fn run(app: &AppHandle, tok: &CancellationToken, ev: &mut Update, a: &[String]) -> Option<(&'static str, String)> {
    let mut cmd = deps::command(&deps::ytdlp_path(app));
    // own process group so cancel reaches everything yt-dlp started (ffmpeg)
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = match cmd.args(a)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn() { // kill_on_drop: last-resort fallback (direct child only)
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to start yt-dlp: {e}");
            return None;
        }
    };
    let mut out = BufReader::new(child.stdout.take().expect("piped stdout"));
    let mut err = BufReader::new(child.stderr.take().expect("piped stderr"));
    let (mut out_done, mut err_done) = (false, false);
    // read raw bytes: yt-dlp may emit non-UTF-8 (locale code page on Windows) and a
    // decode error must never stop draining. Buffers persist across select! iterations
    // because read_until keeps partially read bytes in them when cancelled.
    let (mut ob, mut eb) = (Vec::new(), Vec::new());
    let mut tail: Vec<String> = Vec::new();
    let (mut out_lines, mut spoke) = (0, false);
    let mut files: Vec<String> = Vec::new();
    let mut formats: Vec<String> = Vec::new();
    let mut max = (String::new(), 0.0f32);

    // drain both pipes concurrently (a full pipe would block yt-dlp), then reap
    let status = loop {
        tokio::select! {
            r = out.read_until(b'\n', &mut ob), if !out_done => match r {
                Ok(n) if n > 0 => {
                    out_lines += 1;
                    match progress::parse(&decode(&ob)) {
                        Some(Line::Title(t)) => { ev.title = t; emit(app, ev) }
                        Some(Line::Formats(f)) => { formats = f; max = Default::default() }
                        Some(Line::Progress { percent, format_id, is_video }) => {
                            ev.percent = monotonic(&mut max, &format_id, percent);
                            ev.stage = formats.iter().position(|f| *f == format_id).map_or(0, |i| i as u32 + 1);
                            ev.stages = formats.len() as u32;
                            ev.stream = if is_video { "video" } else { "audio" };
                            emit(app, ev)
                        }
                        Some(Line::File(f)) => files.push(f),
                        None => {}
                    }
                    ob.clear();
                }
                _ => out_done = true, // EOF or real I/O error
            },
            r = err.read_until(b'\n', &mut eb), if !err_done => match r {
                Ok(n) if n > 0 => {
                    let l = decode(&eb);
                    eb.clear();
                    spoke |= own_line(&l);
                    if !l.trim().is_empty() {
                        if tail.len() == 5 { tail.remove(0); }
                        tail.push(l);
                    }
                }
                _ => err_done = true, // EOF or real I/O error
            },
            s = child.wait(), if out_done && err_done => break s,
            _ = tok.cancelled() => {
                kill_tree(&mut child).await;
                return Some(CANCELED);
            }
        }
    };
    Some(match status {
        Ok(s) if s.success() => {
            ev.size = total_size(&files);
            ("done", String::new())
        }
        Ok(s) if never_started(true, s.code(), out_lines, spoke) => {
            log::warn!("yt-dlp did not start: {}", failure_message(&tail, s.code()));
            return None;
        }
        Ok(s) => ("error", failure_message(&tail, s.code())),
        Err(e) => ("error", e.to_string()),
    })
}

/// yt-dlp failed before printing anything of its own (e.g. PyInstaller bootloader error):
/// treat the binary as broken. `code` None = killed by a signal; 2 = yt-dlp's usage error.
fn never_started(spawned: bool, code: Option<i32>, stdout_lines: usize, own_stderr: bool) -> bool {
    !spawned || (!matches!(code, Some(0 | 2)) && stdout_lines == 0 && !own_stderr)
}

/// A stderr line only a running yt-dlp prints (not the PyInstaller bootloader's `[PYI-…]`).
fn own_line(l: &str) -> bool {
    ["ERROR:", "WARNING:", "Usage:", "yt-dlp: error", "Traceback"].iter().any(|p| l.starts_with(p))
        || (l.starts_with('[') && !l.starts_with("[PYI"))
}

/// Kills `child` and all its descendants, then reaps it.
/// Unix: SIGTERM the group first so yt-dlp and ffmpeg can clean up.
async fn kill_tree(child: &mut tokio::process::Child) {
    let Some(pid) = child.id() else { return }; // already reaped
    #[cfg(unix)]
    {
        let group = |sig: &'static str| {
            let g = format!("-{pid}");
            async move { let _ = deps::command(Path::new("kill")).args([sig, "--", &g]).status().await; }
        };
        group("-TERM").await;
        if tokio::time::timeout(std::time::Duration::from_secs(3), child.wait()).await.is_ok() {
            return;
        }
        group("-KILL").await; // launcher still unreaped here, so the pgid cannot be reused yet
    }
    #[cfg(windows)]
    let _ = deps::command(Path::new("taskkill")).args(["/T", "/F", "/PID", &pid.to_string()]).status().await;
    let _ = child.start_kill(); // fails harmlessly if already dead
    let _ = child.wait().await;
}

/// Sum of file sizes; missing/unreadable paths count as 0.
fn total_size(paths: &[String]) -> u64 {
    paths.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum()
}

fn decode(line: &[u8]) -> String {
    String::from_utf8_lossy(line).trim_end_matches(['\r', '\n']).to_string()
}

fn failure_message(tail: &[String], code: Option<i32>) -> String {
    if tail.iter().any(|l| l.contains("HTTP Error 429")) {
        return "Rate limited by YouTube. Retry later.".into();
    }
    tail.iter().rev().find(|l| l.starts_with("ERROR")).or(tail.last()).cloned()
        .unwrap_or_else(|| match code {
            Some(c) => format!("yt-dlp exited with code {c}"),
            None => "yt-dlp was terminated".into(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn limits_to_three() {
        let m = Manager::new();
        let p: Vec<_> = futures_util::future::join_all((0..3).map(|_| m.sem.clone().acquire_owned())).await;
        assert_eq!(p.len(), 3);
        assert!(m.sem.clone().try_acquire_owned().is_err());
    }

    #[test]
    fn percent_never_decreases_within_stream() {
        let mut m = (String::new(), 0.0);
        assert_eq!(monotonic(&mut m, "616", 5.0), 5.0);
        assert_eq!(monotonic(&mut m, "616", 2.0), 5.0); // backward step held
        assert_eq!(monotonic(&mut m, "616", 7.0), 7.0);
        assert_eq!(monotonic(&mut m, "251", 1.0), 1.0); // next stream resets
        m = Default::default(); // new Formats line (next playlist item)
        assert_eq!(monotonic(&mut m, "251", 0.5), 0.5);
    }

    #[test]
    fn tracks_active_jobs() {
        let m = Manager::new();
        assert!(!m.active());
        m.jobs.lock().unwrap().insert(1, CancellationToken::new()); // queued or running
        assert!(m.active());
        m.jobs.lock().unwrap().remove(&1);
        assert!(!m.active());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn kill_tree_kills_grandchildren() {
        let mut cmd = deps::command(Path::new("sh"));
        cmd.process_group(0);
        let mut child = cmd.args(["-c", "sleep 30 & echo $!; wait"]).stdout(Stdio::piped()).spawn().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).await.unwrap();
        let alive = |p: &str| std::process::Command::new("kill").args(["-0", p]).status().unwrap().success();
        let gc = line.trim().to_string();
        assert!(alive(&gc));
        kill_tree(&mut child).await;
        assert!(child.try_wait().unwrap().is_some());
        // the orphaned grandchild is reaped by init/launchd; give it a moment
        for _ in 0..50 {
            if !alive(&gc) { return; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("grandchild {gc} survived");
    }

    #[test]
    fn sums_sizes_ignoring_missing() {
        let d = std::env::temp_dir().join(format!("ytdown-size-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let (a, b) = (d.join("a"), d.join("b"));
        std::fs::write(&a, [0u8; 10]).unwrap();
        std::fs::write(&b, [0u8; 5]).unwrap();
        let paths = vec![a.display().to_string(), b.display().to_string(), d.join("missing").display().to_string()];
        assert_eq!(total_size(&paths), 15);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn decodes_lossy_lines() {
        assert_eq!(decode(b"ytdown: 42.3%\r\n"), "ytdown: 42.3%");
        assert_eq!(decode(b"ytdown-title:\xc7\xd1 x\n"), "ytdown-title:\u{FFFD}\u{FFFD} x");
        assert_eq!(decode(b"no newline"), "no newline");
    }

    #[test]
    fn detects_ytdlp_that_never_started() {
        assert!(never_started(false, None, 0, false)); // spawn error
        assert!(never_started(true, Some(1), 0, false)); // exit != 0, no own output
        assert!(never_started(true, None, 0, false)); // killed, no own output
        assert!(!never_started(true, Some(1), 0, true)); // printed own stderr
        assert!(!never_started(true, Some(1), 3, false)); // printed to stdout
        assert!(!never_started(true, Some(0), 0, false)); // exit 0
        assert!(!never_started(true, Some(2), 0, false)); // argparse usage error
    }

    #[test]
    fn recognizes_ytdlp_own_stderr() {
        for l in ["ERROR: [youtube] abc: Video unavailable", "WARNING: x", "[youtube] abc: Downloading webpage",
                  "Usage: yt-dlp [OPTIONS] URL [URL...]", "yt-dlp: error: no such option: --foo",
                  "Traceback (most recent call last):"] {
            assert!(own_line(l), "{l}");
        }
        for l in ["[PYI-123:ERROR] Failed to extract x", "something bad", ""] {
            assert!(!own_line(l), "{l}");
        }
    }

    #[test]
    fn picks_failure_message() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(failure_message(&v(&["WARNING: x", "ERROR: [youtube] abc: Video unavailable", "trailing"]), Some(1)),
                   "ERROR: [youtube] abc: Video unavailable");
        assert_eq!(failure_message(&v(&["WARNING: a", "something bad"]), Some(1)), "something bad");
        assert_eq!(failure_message(&[], Some(2)), "yt-dlp exited with code 2");
        assert_eq!(failure_message(&[], None), "yt-dlp was terminated");
        assert_eq!(failure_message(&v(&["ERROR: unable to download webpage: HTTP Error 429: Too Many Requests", "x"]), Some(1)),
                   "Rate limited by YouTube. Retry later.");
    }
}
