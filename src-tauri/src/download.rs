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
}

#[derive(Serialize, Clone)]
struct Update {
    id: u64,
    title: String,
    kind: Kind,
    state: &'static str,
    percent: f32,
    message: String,
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
    let ev = Update { id, title: url.clone(), kind, state: "queued", percent: 0.0, message: String::new() };
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
    let s = settings::load(app);
    let a = args::build(ev.kind, url, playlist, &s, &deps::ffmpeg_path(app), &deps::deno_path(app), tmp);
    let mut child = match deps::command(&deps::ytdlp_path(app)).args(a)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .kill_on_drop(true).spawn() {
        Ok(c) => c,
        Err(e) => return ("error", format!("Failed to start yt-dlp: {e}")),
    };
    let mut out = BufReader::new(child.stdout.take().expect("piped stdout")).lines();
    let mut err = BufReader::new(child.stderr.take().expect("piped stderr")).lines();
    let (mut out_done, mut err_done) = (false, false);
    let mut tail: Vec<String> = Vec::new();

    // drain both pipes concurrently (a full pipe would block yt-dlp), then reap
    let status = loop {
        tokio::select! {
            l = out.next_line(), if !out_done => match l {
                Ok(Some(l)) => match progress::parse(&l) {
                    Some(Line::Title(t)) => { ev.title = t; emit(app, ev) }
                    Some(Line::Percent(p)) => { ev.percent = p; emit(app, ev) }
                    None => {}
                },
                _ => out_done = true,
            },
            l = err.next_line(), if !err_done => match l {
                Ok(Some(l)) if !l.trim().is_empty() => {
                    if tail.len() == 5 { tail.remove(0); }
                    tail.push(l);
                }
                Ok(Some(_)) => {}
                _ => err_done = true,
            },
            s = child.wait(), if out_done && err_done => break s,
            _ = tok.cancelled() => {
                let _ = child.kill().await;
                return CANCELED;
            }
        }
    };
    match status {
        Ok(s) if s.success() => ("done", String::new()),
        Ok(s) => ("error", failure_message(&tail, s.code())),
        Err(e) => ("error", e.to_string()),
    }
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
