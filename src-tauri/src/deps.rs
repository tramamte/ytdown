use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::env::consts::{ARCH, EXE_SUFFIX, OS};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

const YTDLP_BASE: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/";
const YTDLP_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const DENO_BASE: &str = "https://github.com/denoland/deno/releases/latest/download/";
const BTBN_BASE: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/";
const UNSUPPORTED: &str = "Unsupported platform";

pub fn bin_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().expect("app data dir").join("bin")
}
pub fn ytdlp_path(app: &AppHandle) -> PathBuf { bin_dir(app).join(format!("yt-dlp{EXE_SUFFIX}")) }
pub fn ffmpeg_path(app: &AppHandle) -> PathBuf { bin_dir(app).join(format!("ffmpeg{EXE_SUFFIX}")) }
pub fn deno_path(app: &AppHandle) -> PathBuf { bin_dir(app).join(format!("deno{EXE_SUFFIX}")) }

/// Child process with no console window on Windows. Use for every spawned binary.
pub fn command(path: &Path) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(path);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    cmd
}

fn hex_eq(actual: &str, expected: &str) -> bool {
    actual.eq_ignore_ascii_case(expected.trim())
}

/// Hash for `asset` in a sha256sum-style body (`<hash>  <name>` per line).
fn find_hash(body: &str, asset: &str) -> Option<String> {
    body.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let hash = it.next()?;
        (it.next()?.trim_start_matches('*') == asset).then(|| hash.to_string())
    })
}

/// First 64-hex token: covers bare `<hash>`, `<hash>  <name>` and PowerShell `Hash : <HASH>`.
fn first_token(body: &str) -> Option<String> {
    body.split_whitespace()
        .find(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_string)
}

/// How to verify a download.
enum Check {
    /// Archive hash listed in a sums file under `asset`.
    Listed { sums_url: String, asset: String },
    /// Archive hash is the first token of this file.
    SumFile(String),
    /// Pinned hash of the extracted binary.
    Binary(&'static str),
}

struct Source {
    url: String,
    check: Check,
    /// Archive entry to extract; None = the download is the binary itself.
    entry: Option<String>,
}

/// `base` is a release download URL ending in `/`.
fn ytdlp_source(base: &str) -> Result<Source, String> {
    let asset = match (OS, ARCH) {
        ("macos", _) => "yt-dlp_macos",
        ("windows", "x86_64") => "yt-dlp.exe",
        ("linux", "x86_64") => "yt-dlp_linux",
        _ => return Err(UNSUPPORTED.into()),
    };
    Ok(Source {
        url: format!("{base}{asset}"),
        check: Check::Listed { sums_url: format!("{base}SHA2-256SUMS"), asset: asset.into() },
        entry: None,
    })
}

fn deno_source() -> Result<Source, String> {
    let triple = match (OS, ARCH) {
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        _ => return Err(UNSUPPORTED.into()),
    };
    let url = format!("{DENO_BASE}deno-{triple}.zip");
    Ok(Source { check: Check::SumFile(format!("{url}.sha256sum")), url, entry: Some(format!("deno{EXE_SUFFIX}")) })
}

fn ffmpeg_source() -> Result<Source, String> {
    // ponytail: pinned single-maintainer macOS build (osxexperts.net, no checksum file);
    // bump URL + hash when it changes.
    let mac = |url: &str, hash| Source { url: url.into(), check: Check::Binary(hash), entry: Some("ffmpeg".into()) };
    let btbn = |name: &str, ext: &str, bin: &str| Source {
        url: format!("{BTBN_BASE}{name}{ext}"),
        check: Check::Listed { sums_url: format!("{BTBN_BASE}checksums.sha256"), asset: format!("{name}{ext}") },
        entry: Some(format!("{name}/bin/{bin}")),
    };
    Ok(match (OS, ARCH) {
        ("macos", "aarch64") => mac("https://www.osxexperts.net/ffmpeg9arm.zip",
            "591260c945d0eef150e3bf82b0ef988bd36a9cecc18ff05d6679617159f0a95e"),
        ("macos", "x86_64") => mac("https://www.osxexperts.net/ffmpeg80intel.zip",
            "df3f1e3facdc1ae0ad0bd898cdfb072fbc9641bf47b11f172844525a05db8d11"),
        ("windows", "x86_64") => btbn("ffmpeg-master-latest-win64-gpl", ".zip", "ffmpeg.exe"),
        ("linux", "x86_64") => btbn("ffmpeg-master-latest-linux64-gpl", ".tar.xz", "ffmpeg"),
        _ => return Err(UNSUPPORTED.into()),
    })
}

/// Shared client. Per-read timeout (not total) so slow-but-alive large archives still finish.
fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    // same panic-on-TLS-init-failure behavior as reqwest::get
    C.get_or_init(|| reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .user_agent("ytdown") // required by the GitHub API
        .build().expect("http client"))
}

async fn fetch(url: &str) -> Result<reqwest::Response, String> {
    client().get(url).send().await.and_then(|r| r.error_for_status()).map_err(|e| format!("Download failed: {e}"))
}

async fn fetch_text(url: &str) -> Result<String, String> {
    fetch(url).await?.text().await.map_err(|e| format!("Download failed: {e}"))
}

/// Streams `url` to `dest`, returning the sha256 hex of the body.
async fn download(url: &str, dest: &Path) -> Result<String, String> {
    let mut stream = fetch(url).await?.bytes_stream();
    let mut file = tokio::fs::File::create(dest).await.map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download failed: {e}"))?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { return Ok(hex::encode(hasher.finalize())); }
        hasher.update(&buf[..n]);
    }
}

/// Extracts `entry` from a zip or tar.xz `archive` into `out`.
fn extract(archive: &Path, tar_xz: bool, entry: &str, out: &Path) -> Result<(), String> {
    let err = |e: &dyn std::fmt::Display| format!("Extract failed: {e}");
    let file = std::fs::File::open(archive).map_err(|e| err(&e))?;
    let mut dest = std::fs::File::create(out).map_err(|e| err(&e))?;
    if tar_xz {
        let mut tar = tar::Archive::new(xz2::read::XzDecoder::new(file));
        for e in tar.entries().map_err(|e| err(&e))? {
            let mut e = e.map_err(|e| err(&e))?;
            if e.path().map_err(|e| err(&e))? == Path::new(entry) {
                std::io::copy(&mut e, &mut dest).map_err(|e| err(&e))?;
                return Ok(());
            }
        }
        Err(format!("{entry} not found in archive"))
    } else {
        let mut zip = zip::ZipArchive::new(file).map_err(|e| err(&e))?;
        let mut e = zip.by_name(entry).map_err(|e| err(&e))?;
        std::io::copy(&mut e, &mut dest).map_err(|e| err(&e))?;
        Ok(())
    }
}

/// Downloads, verifies and installs `src` at `dest`. Temp files are removed on every path.
async fn install(name: &str, src: Source, dest: &Path) -> Result<(), String> {
    let archive = dest.with_file_name(format!("{name}.download"));
    let part = dest.with_file_name(format!("{name}.part"));
    let result = install_inner(name, &src, &archive, &part, dest).await;
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_file(&part);
    result
}

async fn install_inner(name: &str, src: &Source, archive: &Path, part: &Path, dest: &Path) -> Result<(), String> {
    let mismatch = || format!("Checksum mismatch for {name}");
    let expected = match &src.check {
        Check::Listed { sums_url, asset } => find_hash(&fetch_text(sums_url).await?, asset),
        Check::SumFile(url) => first_token(&fetch_text(url).await?),
        Check::Binary(_) => None,
    };
    if !matches!(src.check, Check::Binary(_)) && expected.is_none() {
        return Err(format!("No checksum published for {name}"));
    }
    let got = download(&src.url, archive).await?;
    if expected.is_some_and(|h| !hex_eq(&got, &h)) {
        return Err(mismatch());
    }
    match &src.entry {
        Some(entry) => {
            let (a, e, p) = (archive.to_owned(), entry.clone(), part.to_owned());
            let tar_xz = src.url.ends_with(".tar.xz");
            tokio::task::spawn_blocking(move || extract(&a, tar_xz, &e, &p)).await.map_err(|e| e.to_string())??;
        }
        None => std::fs::rename(archive, part).map_err(|e| e.to_string())?,
    }
    if let Check::Binary(hash) = src.check {
        let p = part.to_owned();
        let got = tokio::task::spawn_blocking(move || sha256_file(&p)).await.map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        if !hex_eq(&got, hash) { return Err(mismatch()); }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(part, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(part, dest).map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("xattr").args(["-d", "com.apple.quarantine"]).arg(dest).output();
    Ok(())
}

async fn runs_ok(path: &Path, arg: &str) -> bool {
    path.exists() && command(path).arg(arg).output().await.is_ok_and(|o| o.status.success())
}

/// Non-empty (unix: executable) regular file. The launch check: running the tools there would
/// delay startup; they are probed (and repaired) at download time instead.
fn looks_installed(path: &Path) -> bool {
    let Ok(m) = std::fs::metadata(path) else { return false };
    #[cfg(unix)]
    let exec = std::os::unix::fs::PermissionsExt::mode(&m.permissions()) & 0o111 != 0;
    #[cfg(not(unix))]
    let exec = true;
    m.is_file() && m.len() > 0 && exec
}

/// Trimmed non-empty `--version` stdout, if the binary runs.
async fn version(path: &Path) -> Option<String> {
    if !path.exists() { return None; }
    let o = command(path).arg("--version").output().await.ok()?;
    let v = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (o.status.success() && !v.is_empty()).then_some(v)
}

/// Dotted numeric versions (yt-dlp `2026.08.19`, hotfix `2022.06.22.1`); unparseable = not newer.
fn is_newer(installed: &str, latest: &str) -> bool {
    let parse = |v: &str| v.trim().split('.').map(str::parse::<u64>).collect::<Result<Vec<_>, _>>().ok();
    matches!((parse(installed), parse(latest)), (Some(a), Some(b)) if b > a)
}

fn parse_tag(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body).ok()?["tag_name"].as_str().map(str::to_string)
}

/// Where an update for `bin` is staged until the next launch.
fn pending_path(bin: &Path) -> PathBuf {
    bin.with_file_name("pending").join(bin.file_name().expect("binary file name"))
}

/// Moves a staged update over `bin`. On failure the staged file is dropped.
fn apply_pending(bin: &Path) {
    let pending = pending_path(bin);
    if !pending.exists() { return; }
    if let Err(e) = std::fs::rename(&pending, bin) {
        log::warn!("Could not apply staged update {}: {e}", pending.display());
        let _ = std::fs::remove_file(&pending);
    }
}

/// Moves a staged update over `bin` now. On failure (e.g. Windows exe locked) it stays staged.
fn promote_pending(bin: &Path) -> std::io::Result<()> {
    std::fs::rename(pending_path(bin), bin)
}

/// Stages a newer yt-dlp in `pending/`. Returns (latest version, whether it was staged).
async fn stage_update(app: &AppHandle, installed: &str) -> Result<(String, bool), String> {
    let ytdlp = ytdlp_path(app);
    let latest = parse_tag(&fetch_text(YTDLP_API).await?).ok_or("No tag_name in yt-dlp release info")?;
    if !is_newer(installed, &latest) { return Ok((latest, false)); }
    let pending = pending_path(&ytdlp);
    if version(&pending).await.as_deref() != Some(latest.as_str()) {
        std::fs::create_dir_all(pending.parent().expect("pending dir")).map_err(|e| e.to_string())?;
        let base = format!("https://github.com/yt-dlp/yt-dlp/releases/download/{latest}/");
        install("yt-dlp", ytdlp_source(&base)?, &pending).await?;
    }
    Ok((latest, true))
}

static CHECKING: AtomicBool = AtomicBool::new(false);

/// Background update check, which is also the yt-dlp health probe; a request while one runs
/// is a no-op. Blocks input only to reinstall a yt-dlp that does not run.
fn spawn_update_check(app: &AppHandle) {
    if CHECKING.swap(true, Ordering::SeqCst) { return; }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match version(&ytdlp_path(&app)).await {
            Some(installed) => match stage_update(&app, &installed).await {
                Ok((v, true)) => emit(&app, "update",
                    &format!("yt-dlp {v} downloaded; it will be applied the next time you start ytdown.")),
                Ok((_, false)) => {}
                Err(e) => {
                    log::warn!("yt-dlp update check failed: {e}");
                    emit(&app, "ready", "Could not check for yt-dlp updates; using installed version.");
                }
            },
            None => repair_ytdlp(&app).await,
        }
        CHECKING.store(false, Ordering::SeqCst);
    });
}

/// Reinstalls a yt-dlp that does not run. Skipped if an install/update holds RUNNING.
async fn repair_ytdlp(app: &AppHandle) {
    let Ok(_guard) = RUNNING.try_lock() else { return };
    log::warn!("yt-dlp does not run; reinstalling");
    emit(app, "working", "Downloading yt-dlp…");
    match async { install("yt-dlp", ytdlp_source(YTDLP_BASE)?, &ytdlp_path(app)).await }.await {
        Ok(()) => emit(app, "ready", ""),
        Err(e) => emit(app, "error", &e),
    }
}

#[derive(Serialize, Clone)]
struct Status<'a> {
    state: &'a str,
    message: &'a str,
}

fn emit(app: &AppHandle, state: &str, message: &str) {
    let _ = app.emit("deps://status", Status { state, message });
}

static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tauri::command]
pub async fn ensure_deps(app: AppHandle) -> Result<(), String> {
    // Concurrent callers wait, then re-run. Released before the check so it can take RUNNING to repair.
    let result = { let _guard = RUNNING.lock().await; ensure(&app, false).await };
    match result {
        Ok(()) => { emit(&app, "ready", ""); spawn_update_check(&app); Ok(()) }
        Err(e) => { emit(&app, "error", &e); Err(e) }
    }
}

/// Settings "Update" button: installs missing binaries, then applies a newer yt-dlp now.
#[tauri::command]
pub async fn update_deps(app: AppHandle) -> Result<(), String> {
    let _guard = RUNNING.lock().await;
    // Hold the background-check guard too, so a check can't stage concurrently; wait out a running one.
    while CHECKING.swap(true, Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let result = update(&app).await;
    CHECKING.store(false, Ordering::SeqCst);
    result
}

async fn update(app: &AppHandle) -> Result<(), String> {
    emit(app, "working", "Checking for updates…");
    if let Err(e) = ensure(app, true).await {
        emit(app, "error", &e);
        return Err(e);
    }
    let ytdlp = ytdlp_path(app);
    let staged = match version(&ytdlp).await {
        // stage_update reuses an already-staged latest build instead of re-downloading.
        Some(installed) => stage_update(app, &installed).await,
        None => Err("yt-dlp does not run".into()),
    };
    match staged {
        Ok((v, true)) => match promote_pending(&ytdlp) {
            Ok(()) => emit(app, "update", &format!("yt-dlp updated to {v}.")),
            Err(e) => {
                log::warn!("Could not replace yt-dlp now: {e}");
                emit(app, "update", &format!("yt-dlp {v} downloaded; it will be applied the next time you start ytdown."));
            }
        },
        Ok((v, false)) => {
            // A leftover staged build is not newer; drop it so next launch can't downgrade.
            let _ = std::fs::remove_file(pending_path(&ytdlp));
            emit(app, "update", &format!("yt-dlp is up to date ({v})."));
        }
        Err(e) => {
            log::warn!("yt-dlp update check failed: {e}");
            emit(app, "ready", "Could not check for yt-dlp updates; using installed version.");
        }
    }
    Ok(())
}

static APPLIED: AtomicBool = AtomicBool::new(false);

/// `probe`: run each binary to check it (yt-dlp is slow); otherwise only check the files are there.
async fn ensure(app: &AppHandle, probe: bool) -> Result<(), String> {
    std::fs::create_dir_all(bin_dir(app)).map_err(|e| e.to_string())?;
    // Only on the first call (launch): later calls must not swap a binary that may be running.
    if !APPLIED.swap(true, Ordering::SeqCst) {
        apply_pending(&ytdlp_path(app));
    }
    for (name, path, arg, source) in [
        ("yt-dlp", ytdlp_path(app), "--version", (|| ytdlp_source(YTDLP_BASE)) as fn() -> Result<Source, String>),
        ("ffmpeg", ffmpeg_path(app), "-version", ffmpeg_source),
        ("deno", deno_path(app), "--version", deno_source),
    ] {
        let ok = if probe { runs_ok(&path, arg).await } else { looks_installed(&path) };
        if !ok {
            emit(app, "working", &format!("Downloading {name}…"));
            install(name, source()?, &path).await?;
        }
    }
    Ok(())
}

/// Download time: probes ffmpeg and deno (fast) and reinstalls a broken one.
pub async fn repair_fast_tools(app: &AppHandle) -> Result<(), String> {
    for (name, path, arg, source) in [
        ("ffmpeg", ffmpeg_path(app), "-version", ffmpeg_source as fn() -> Result<Source, String>),
        ("deno", deno_path(app), "--version", deno_source),
    ] {
        if runs_ok(&path, arg).await { continue; }
        let _guard = RUNNING.lock().await;
        // a concurrent job may have repaired it while we waited
        if !runs_ok(&path, arg).await {
            reinstall(app, name, source()?, &path).await?;
        }
    }
    Ok(())
}

/// Identity of a binary file, to tell whether it was replaced since a job ran it.
pub fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

/// Reinstalls a yt-dlp that failed to start, unless it was replaced since `seen` (another job,
/// the background check or Update already did it).
pub async fn repair_ytdlp_since(app: &AppHandle, seen: Option<(SystemTime, u64)>) -> Result<(), String> {
    let _guard = RUNNING.lock().await;
    let path = ytdlp_path(app);
    if stamp(&path) != seen { return Ok(()); }
    reinstall(app, "yt-dlp", ytdlp_source(YTDLP_BASE)?, &path).await
}

/// Caller holds RUNNING. On failure input stays usable; the job reports the error.
async fn reinstall(app: &AppHandle, name: &str, src: Source, path: &Path) -> Result<(), String> {
    log::warn!("{name} does not run; reinstalling");
    emit(app, "working", &format!("Repairing {name}…"));
    let r = install(name, src, path).await.map_err(|e| format!("Could not repair {name}: {e}"));
    emit(app, "ready", r.as_ref().err().map_or("", String::as_str));
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify_sha256(bytes: &[u8], expected_hex: &str) -> bool {
        hex_eq(&hex::encode(Sha256::digest(bytes)), expected_hex)
    }

    #[test]
    fn sha256_verify() {
        // sha256("abc")
        let h = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify_sha256(b"abc", h));
        assert!(!verify_sha256(b"abd", h));
        assert!(verify_sha256(b"abc", &h.to_uppercase()));
    }

    #[test]
    fn finds_hash_in_sums_file() {
        let body = "aaa111  yt-dlp\n0f192b  yt-dlp_macos\nccc333  yt-dlp_macos.zip\n";
        assert_eq!(find_hash(body, "yt-dlp_macos").as_deref(), Some("0f192b"));
        assert_eq!(find_hash(body, "yt-dlp").as_deref(), Some("aaa111"));
        assert_eq!(find_hash(body, "yt-dlp_linux"), None);
    }

    #[test]
    fn first_token_of_sumfile() {
        let h = "5cd46d6268f6f78f5d88bdc7159d20bd44cdaa4b3303474839f87ec6fe7ae25c";
        assert_eq!(first_token(&format!("{h}  deno.zip\n")).as_deref(), Some(h));
        assert_eq!(first_token(&format!("{h}\n")).as_deref(), Some(h));
        assert_eq!(first_token("  \n"), None);
    }

    #[test]
    fn hash_token_of_powershell_sumfile() {
        let h = "5CD46D6268F6F78F5D88BDC7159D20BD44CDAA4B3303474839F87EC6FE7AE25C";
        let body = format!("\r\nAlgorithm : SHA256\r\nHash      : {h}\r\nPath      : C:\\a\\deno\\deno-x86_64-pc-windows-msvc.zip\r\n");
        assert_eq!(first_token(&body).as_deref(), Some(h));
    }

    #[test]
    fn newer_version() {
        assert!(is_newer("2026.07.04", "2026.08.19"));
        assert!(is_newer("2026.08.19", "2026.08.19.1"));
        assert!(is_newer("2025.12.08", "2026.01.29"));
        assert!(!is_newer("2026.08.19", "2026.08.19"));
        assert!(!is_newer("2026.08.19\n", "2026.08.19"));
        assert!(!is_newer("2026.08.19.1", "2026.08.19"));
        assert!(!is_newer("2026.08.19", "garbage"));
    }

    #[test]
    fn tag_from_release_json() {
        assert_eq!(parse_tag(r#"{"url":"x","tag_name":"2026.08.19","name":"yt-dlp"}"#).as_deref(), Some("2026.08.19"));
        assert_eq!(parse_tag(r#"{"message":"API rate limit exceeded"}"#), None);
        assert_eq!(parse_tag("not json"), None);
    }

    #[test]
    fn applies_pending_binary() {
        let dir = std::env::temp_dir().join(format!("ytdown-pending-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("pending")).unwrap();
        let bin = dir.join("yt-dlp");
        std::fs::write(&bin, "old").unwrap();
        apply_pending(&bin); // absent -> no-op
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "old");
        std::fs::write(pending_path(&bin), "new").unwrap();
        apply_pending(&bin);
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "new");
        assert!(!pending_path(&bin).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn promote_keeps_pending_on_failure() {
        let dir = std::env::temp_dir().join(format!("ytdown-promote-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("pending")).unwrap();
        let bin = dir.join("yt-dlp");
        std::fs::write(pending_path(&bin), "new").unwrap();
        // A non-empty directory at `bin` makes the rename fail (stands in for a locked exe).
        std::fs::create_dir_all(bin.join("x")).unwrap();
        assert!(promote_pending(&bin).is_err());
        assert_eq!(std::fs::read_to_string(pending_path(&bin)).unwrap(), "new");
        std::fs::remove_dir_all(&bin).unwrap();
        std::fs::write(&bin, "old").unwrap();
        promote_pending(&bin).unwrap();
        assert_eq!(std::fs::read_to_string(&bin).unwrap(), "new");
        assert!(!pending_path(&bin).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn present_without_spawning() {
        let dir = std::env::temp_dir().join(format!("ytdown-present-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("yt-dlp");
        assert!(!looks_installed(&bin)); // missing
        std::fs::write(&bin, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(!looks_installed(&bin)); // empty
            std::fs::write(&bin, "x").unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(!looks_installed(&bin)); // not executable
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        #[cfg(not(unix))]
        {
            assert!(!looks_installed(&bin)); // empty
            std::fs::write(&bin, "x").unwrap();
        }
        assert!(looks_installed(&bin));
        assert!(!looks_installed(&dir)); // directory
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sources_for_current_platform() {
        for src in [ytdlp_source(YTDLP_BASE), ffmpeg_source(), deno_source()] {
            assert!(src.unwrap().url.starts_with("https://"));
        }
    }
}
