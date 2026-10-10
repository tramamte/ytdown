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
const ITEM_EVENT: &str = "download://item";
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
    playlist: bool,
    items_done: u32,
    items_failed: u32,
    items_total: u32, // 0 if unknown
}

/// Child row of a playlist job (`download://item`).
#[derive(Serialize, Clone, Debug)]
struct Item {
    job_id: u64,
    index: u32, // playlist_index; 0 if unknown
    title: String,
    kind: Kind,
    state: &'static str, // "running" | "done" | "error" | "canceled"
    percent: f32,
    stage: u32,
    stages: u32,
    stream: &'static str,
    size: u64,
    message: String,
}

/// State of one yt-dlp run, fed line by line. No I/O, so captured runs can be replayed in tests.
/// Playlist jobs: yt-dlp downloads the items one after another, so at most the last row is running.
#[derive(Default)]
struct Tracker {
    files: Vec<String>,
    formats: Vec<String>,
    max: (String, f32),
    items: Vec<Item>,
}

impl Tracker {
    fn running(&mut self) -> Option<&mut Item> {
        self.items.last_mut().filter(|i| i.state == "running")
    }

    fn push(&mut self, ev: &Update, index: u32) {
        self.items.push(Item { job_id: ev.id, index, title: String::new(), kind: ev.kind, state: "running",
            percent: 0.0, stage: 0, stages: 0, stream: "", size: 0, message: String::new() });
    }

    /// Applies a stdout line. Returns (parent changed, child row to emit).
    /// Single videos update the parent as before; playlist jobs update the running child.
    fn stdout(&mut self, ev: &mut Update, l: Line) -> (bool, Option<Item>) {
        match l {
            Line::Item { index, total, playlist_title } => {
                if !playlist_title.is_empty() { ev.title = playlist_title; }
                ev.items_total = total;
                self.push(ev, index);
            }
            Line::Title(t) => {
                if !ev.playlist { ev.title = t; return (true, None) }
                let Some(it) = self.running() else { return (false, None) };
                it.title = t;
            }
            Line::Formats(f) => { self.formats = f; self.max = Default::default(); return (false, None) }
            Line::Progress { percent, format_id, is_video } => {
                let p = (
                    monotonic(&mut self.max, &format_id, percent),
                    self.formats.iter().position(|f| *f == format_id).map_or(0, |i| i as u32 + 1),
                    self.formats.len() as u32,
                    if is_video { "video" } else { "audio" },
                );
                if !ev.playlist { (ev.percent, ev.stage, ev.stages, ev.stream) = p; return (true, None) }
                let Some(it) = self.running() else { return (false, None) };
                (it.percent, it.stage, it.stages, it.stream) = p;
            }
            Line::File(f) => {
                let size = total_size(std::slice::from_ref(&f));
                self.files.push(f);
                let Some(it) = self.running() else { return (false, None) };
                (it.state, it.size, it.percent) = ("done", size, 100.0);
                ev.items_done += 1;
            }
        }
        self.update_percent(ev);
        (true, self.items.last().cloned())
    }

    /// An `ERROR:` line of a playlist job fails the running item, or else an item that failed
    /// before it started (e.g. unavailable: yt-dlp prints no ytdown-item line for it) and
    /// yt-dlp moves on to the next one.
    // ponytail: assumes the ERROR line is read before the next item's stdout lines (yt-dlp writes it
    // before extracting the next item); a pipe-read race would blame the next item instead
    fn stderr(&mut self, ev: &mut Update, l: &str) -> Option<Item> {
        if !ev.playlist || !l.starts_with("ERROR:") { return None }
        if self.running().is_none() {
            let next = self.items.last().map_or(1, |i| i.index + 1);
            self.push(ev, next);
        }
        let it = self.items.last_mut()?;
        (it.state, it.message) = ("error", failure_message(&[l.to_string()], None));
        ev.items_failed += 1;
        self.update_percent(ev);
        self.items.last().cloned()
    }

    /// Ends the running item (canceled, or the run ended without its file/ERROR line).
    fn settle(&mut self, ev: &mut Update, state: &'static str) -> Option<Item> {
        let it = self.running()?;
        it.state = state;
        let it = it.clone();
        match state { "done" => ev.items_done += 1, "error" => ev.items_failed += 1, _ => {} }
        Some(it)
    }

    /// Parent percent of a playlist job: finished items plus the running item's share; never decreases.
    fn update_percent(&self, ev: &mut Update) {
        if ev.items_total == 0 { return }
        let cur = self.items.last().filter(|i| i.state == "running").map_or(0.0, |i| match i.stage {
            0 => i.percent / 100.0,
            s => ((s - 1) as f32 + i.percent / 100.0) / i.stages as f32,
        });
        let p = ((ev.items_done + ev.items_failed) as f32 + cur) / ev.items_total as f32 * 100.0;
        ev.percent = ev.percent.max(p.min(100.0));
    }

    /// Final state of a playlist job: some items failed → done with a note; none succeeded → error.
    fn outcome(&self, ev: &mut Update, ok: bool, failure: String) -> (&'static str, String) {
        ev.size = total_size(&self.files);
        let total = if ev.items_total > 0 { ev.items_total } else { self.items.len() as u32 };
        if ok && ev.items_failed == 0 {
            ("done", String::new())
        } else if ev.items_done == 0 || ev.items_failed == 0 {
            ("error", failure)
        } else {
            ("done", format!("{} of {total} failed", ev.items_failed))
        }
    }
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
    let ev = Update { id, title: url.clone(), kind, state: "queued", percent: 0.0, message: String::new(), size: 0, stage: 0, stages: 0, stream: "",
                     playlist, items_done: 0, items_failed: 0, items_total: 0 };
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

fn emit_item(app: &AppHandle, it: &Item) {
    let _ = app.emit(ITEM_EVENT, it);
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
    let mut t = Tracker::default();

    // drain both pipes concurrently (a full pipe would block yt-dlp), then reap
    let status = loop {
        tokio::select! {
            r = out.read_until(b'\n', &mut ob), if !out_done => match r {
                Ok(n) if n > 0 => {
                    out_lines += 1;
                    if let Some(l) = progress::parse(&decode(&ob)) {
                        let (parent, item) = t.stdout(ev, l);
                        if parent { emit(app, ev) }
                        if let Some(it) = item { emit_item(app, &it) }
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
                    if let Some(it) = t.stderr(ev, &l) { emit(app, ev); emit_item(app, &it) }
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
                if let Some(it) = t.settle(ev, "canceled") { emit_item(app, &it) }
                return Some(CANCELED);
            }
        }
    };
    Some(match status {
        // the retry-once after None cannot duplicate child rows: rows only come from stdout
        // lines or own stderr lines, both of which rule out never_started
        Ok(s) if never_started(true, s.code(), out_lines, spoke) => {
            debug_assert!(t.items.is_empty());
            log::warn!("yt-dlp did not start: {}", failure_message(&tail, s.code()));
            return None;
        }
        Ok(s) if ev.playlist => {
            if let Some(it) = t.settle(ev, if s.success() { "done" } else { "error" }) { emit_item(app, &it) }
            t.outcome(ev, s.success(), failure_message(&tail, s.code()))
        }
        Ok(s) if s.success() => {
            ev.size = total_size(&t.files);
            ("done", String::new())
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

    fn update(playlist: bool) -> Update {
        Update { id: 7, title: "url".into(), kind: Kind::Audio, state: "running", percent: 0.0, message: String::new(),
                 size: 0, stage: 0, stages: 0, stream: "", playlist, items_done: 0, items_failed: 0, items_total: 0 }
    }
    /// Feeds captured lines ("E:" = stderr); returns the child rows in emit order.
    fn replay(t: &mut Tracker, ev: &mut Update, lines: &[String]) -> Vec<Item> {
        let mut out = Vec::new();
        for l in lines {
            out.extend(match l.strip_prefix("E:") {
                Some(e) => t.stderr(ev, e),
                None => progress::parse(l).and_then(|x| t.stdout(ev, x).1),
            });
        }
        out
    }
    fn lines(a: &[&str]) -> Vec<String> { a.iter().map(|s| s.to_string()).collect() }
    fn temp_files(name: &str, sizes: &[usize]) -> (std::path::PathBuf, Vec<String>) {
        let d = std::env::temp_dir().join(format!("ytdown-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = sizes.iter().enumerate().map(|(i, n)| {
            let f = d.join(format!("{i}.mkv"));
            std::fs::write(&f, vec![0u8; *n]).unwrap();
            f.display().to_string()
        }).collect();
        (d, p)
    }

    #[test]
    fn playlist_items_done_in_order() {
        let (d, f) = temp_files("pl-ok", &[10, 5]);
        // captured from yt-dlp 2026.08.19 (PLr-mgdpse3TpWFoJm99Y9RkXcL6r1RaWM, default video settings), trimmed
        let l = lines(&["ytdown-item:1|6|5 Seconds Countdown Timer with Soundeffect", "ytdown-title:Five A",
            "ytdown-formats:136+251", "ytdown: 60.1%|136|avc1.64001F|NA|NA", "ytdown:100.0%|136|avc1.64001F|NA|NA",
            "ytdown:  1.0%|251|none|NA|NA", "ytdown:100.0%|251|none|NA|NA", &format!("ytdown-file:{}", f[0]),
            "ytdown-item:2|6|5 Seconds Countdown Timer with Soundeffect", "ytdown-title:Five B",
            "ytdown-formats:136+251", "ytdown: 86.1%|136|avc1.64001F|NA|NA", &format!("ytdown-file:{}", f[1])]);
        let (mut t, mut ev) = (Tracker::default(), update(true));
        let rows = replay(&mut t, &mut ev, &l);
        assert_eq!(ev.title, "5 Seconds Countdown Timer with Soundeffect");
        assert_eq!((ev.items_done, ev.items_failed, ev.items_total), (2, 0, 6));
        assert!((ev.percent - 200.0 / 6.0).abs() < 0.01, "{}", ev.percent);
        assert_eq!(rows.first().map(|r| (r.index, r.state, r.title.as_str())), Some((1, "running", "")));
        let last: Vec<_> = t.items.iter().map(|r| (r.job_id, r.index, r.title.as_str(), r.state, r.size)).collect();
        assert_eq!(last, [(7, 1, "Five A", "done", 10), (7, 2, "Five B", "done", 5)]);
        assert_eq!(t.outcome(&mut ev, true, "x".into()), ("done", String::new()));
        assert_eq!(ev.size, 15);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn playlist_item_progress_and_parent_percent() {
        let (mut t, mut ev) = (Tracker::default(), update(true));
        fn feed(t: &mut Tracker, ev: &mut Update, l: &str) -> (bool, Option<(f32, u32, u32, &'static str)>) {
            let r = t.stdout(ev, progress::parse(l).unwrap());
            (r.0, r.1.map(|i| (i.percent, i.stage, i.stages, i.stream)))
        }
        assert_eq!(feed(&mut t, &mut ev, "ytdown-item:1|2|P").1, Some((0.0, 0, 0, "")));
        feed(&mut t, &mut ev, "ytdown-formats:136+251");
        assert_eq!(feed(&mut t, &mut ev, "ytdown: 50.0%|136|avc1|NA|NA"), (true, Some((50.0, 1, 2, "video"))));
        assert_eq!(ev.percent, 12.5); // (0 finished + 0.5 of stage 1/2 → 0.25) / 2
        feed(&mut t, &mut ev, "ytdown: 40.0%|136|avc1|NA|NA"); // backward step held
        assert_eq!(ev.percent, 12.5);
        feed(&mut t, &mut ev, "ytdown:  0.0%|251|none|NA|NA");
        assert_eq!(ev.percent, 25.0);
        assert_eq!((ev.stage, ev.stages, ev.stream), (0, 0, "")); // parent shows items, not streams
    }

    #[test]
    fn playlist_failed_item_does_not_stop_the_rest() {
        let (d, f) = temp_files("pl-fail", &[3, 4]);
        // captured from yt-dlp 2026.08.19 (PLW7B4PyySHApSUCjSpXzBd6LXT8IhjvH9, audio): item 5 is unavailable,
        // fails before before_dl (no ytdown-item line), yt-dlp continues and exits 1
        let l = lines(&["ytdown-item:4|7|gallery 23:5 seconds countdown", "ytdown-title:23 Second Countdown",
            "ytdown-formats:140", "ytdown:  0.3%|140|none|NA|NA", &format!("ytdown-file:{}", f[0]),
            "E:ERROR: [youtube] Eb8yXfxT1hw: Video unavailable",
            "ytdown-item:6|7|gallery 23:5 seconds countdown", "ytdown-title:23 Second Countdown Timer / iPhone Timer Style",
            "ytdown-formats:251", &format!("ytdown-file:{}", f[1])]);
        let (mut t, mut ev) = (Tracker::default(), update(true));
        replay(&mut t, &mut ev, &l);
        let rows: Vec<_> = t.items.iter().map(|r| (r.index, r.state, r.message.as_str())).collect();
        assert_eq!(rows, [(4, "done", ""), (5, "error", "ERROR: [youtube] Eb8yXfxT1hw: Video unavailable"), (6, "done", "")]);
        assert_eq!((ev.items_done, ev.items_failed, ev.items_total), (2, 1, 7));
        assert_eq!(t.outcome(&mut ev, false, "x".into()), ("done", "1 of 7 failed".into()));
        assert_eq!(ev.size, 7);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn playlist_error_while_running_and_none_succeeded() {
        let (mut t, mut ev) = (Tracker::default(), update(true));
        let rows = replay(&mut t, &mut ev, &lines(&["ytdown-item:1|1|P", "ytdown-title:A", "ytdown-formats:251",
            "ytdown: 10.0%|251|none|NA|NA", "E:WARNING: x", "E:ERROR: unable to download video data: HTTP Error 429: Too Many Requests"]));
        let r = rows.last().unwrap();
        assert_eq!((r.index, r.title.as_str(), r.state, r.message.as_str()), (1, "A", "error", "Rate limited by YouTube. Retry later."));
        assert_eq!(t.items.len(), 1);
        assert_eq!(t.outcome(&mut ev, false, "ERROR: boom".into()), ("error", "ERROR: boom".into()));
    }

    #[test]
    fn playlist_cancel_marks_running_item() {
        let (mut t, mut ev) = (Tracker::default(), update(true));
        replay(&mut t, &mut ev, &lines(&["ytdown-item:2|3|P", "ytdown: 10.0%|251|none|NA|NA"]));
        assert_eq!(t.settle(&mut ev, "canceled").map(|r| (r.index, r.state)), Some((2, "canceled")));
        assert!(t.settle(&mut ev, "canceled").is_none());
    }

    #[test]
    fn single_video_has_no_items() {
        let (mut t, mut ev) = (Tracker::default(), update(false));
        let rows = replay(&mut t, &mut ev, &lines(&["ytdown-title:My Video", "ytdown-formats:401+251",
            "ytdown: 40.0%|401|av01|NA|NA", "ytdown: 30.0%|401|av01|NA|NA", "ytdown:  5.0%|251|none|NA|NA",
            "E:ERROR: something", "ytdown-file:/nonexistent/x.mp4"]));
        assert!(rows.is_empty() && t.items.is_empty());
        assert_eq!((ev.title.as_str(), ev.percent, ev.stage, ev.stages, ev.stream), ("My Video", 5.0, 2, 2, "audio"));
        assert_eq!(t.files, ["/nonexistent/x.mp4"]);
        assert_eq!((ev.items_done, ev.items_failed, ev.items_total), (0, 0, 0));
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
