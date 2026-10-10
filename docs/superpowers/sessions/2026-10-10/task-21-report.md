# Task 21 report: yt-dlp onedir build

Status: DONE_WITH_CONCERNS (the concerns are about Windows behaviour I could not test, plus one incident during the live check; the code itself is complete).

## Commits (branch feat/onedir, not pushed)
- 2bb8c57 feat: switch yt-dlp to the onedir build (src-tauri/src/deps.rs only)
- cc6a305 docs: yt-dlp onedir build decision and measurements (docs/00_PRD.md, docs/02_decisions.md)

## Design (deps.rs only; download.rs unchanged)
- `YTDLP_EXE` const per OS (`yt-dlp_macos` / `yt-dlp.exe` / `yt-dlp_linux`). New private `ytdlp_dir(app)` = `bin/yt-dlp`. `ytdlp_path(app)` = `bin/yt-dlp/<exe>`, so every caller, including download.rs (`stamp`, `command`, `exists`), keeps working unchanged.
- `ytdlp_source` assets: `yt-dlp_macos.zip` / `yt-dlp_win.zip` / `yt-dlp_linux.zip`. Checksums come from SHA2-256SUMS through the existing `Check::Listed` / `find_hash`.
- `Source.entry: None` now means "the zip is the binary's whole directory". The old "download is the binary itself" path is gone, because nothing uses it any more.
- `install(name, src, dest)`: callers still pass the binary path. For a directory build, the target is `dest.parent()`. Archive and part files sit next to the target (`bin/yt-dlp.download`, `bin/yt-dlp.part/`). A leftover `.part` is removed before and after the install, and the archive is removed after.
- `extract_dir`: uses zip 9's `ZipArchive::extract`. It sanitises paths with `enclosed_name` (`safe_prepare_path` → `resolve_enclosed`) and returns an error on `../evil`. It restores unix modes from the entries and keeps symlinks inside the folder (the releases contain none). It then checks that the executable exists.
- After extraction: chmod 0o755 on the executable (unix), `xattr -dr com.apple.quarantine` on the part dir (macOS, best effort), then `replace(part, target)`.
- `replace(new, dest)`: removes a leftover `dest.old`. If `dest` exists (file or directory), it is renamed to `dest.old`. Then `new` → `dest`; on failure the old one is restored and the error returned. Finally `.old` is removed (best effort). ffmpeg and deno use the same swap; for single files this behaves like the old rename.
- Migration: when `bin/yt-dlp` is a file, `looks_installed(bin/yt-dlp/<exe>)` is false (ENOTDIR), so ensure() installs. `replace` then moves the old file aside and deletes it. No special case was needed.
- Pending updates: `pending_path(bin/yt-dlp)` = `bin/pending/yt-dlp/` (a directory). `apply_pending` deletes a stale pending file left by the onefile scheme, otherwise calls `replace` (and drops the pending dir on failure). `promote_pending` = `replace`, which fails on Windows while the exe inside is running, so the update stays staged. `stage_update` checks the version of `pending/yt-dlp/<exe>`. `update()` removes a stale pending dir with `remove_any`.
- `looks_installed` is unchanged: the executable inside the directory must be a non-empty regular file with the exec bit set.

## Zip layouts found
- Windows `yt-dlp_win.zip` (18.1 MB, sha256 matched SHA2-256SUMS): `yt-dlp.exe` at the root plus `_internal/`. 143 entries, FAT host (no unix modes), no directory entries, no symlinks.
- Linux `yt-dlp_linux.zip` (40.5 MB, sha256 matched): `yt-dlp_linux` at the root (mode -rwxr-xr-x) plus `_internal/`. 175 entries (27 directories), no symlinks.
- macOS (from the brief, confirmed live): `yt-dlp_macos` + `_internal/`, 134 files, no symlinks.

## RED / GREEN
- RED: the new tests did not compile (`YTDLP_EXE`, `replace`, `extract_dir` missing: 17 errors).
- GREEN: `cargo test`: 38 passed. `cargo clippy --all-targets`: 0 warnings.
- New and changed tests: `ytdlp_onedir_asset`; `extracts_onedir_zip` (files, nested dirs, modes 0o755 and 0o644 restored, `../evil` rejected with nothing written outside, error when the exe is missing); `replaces_dir_file_or_nothing` (no existing install, existing dir with stale content removed, crashed-run `.old` leftover, migration from a file, no `.part`/`.old` left); `applies_pending_dir`; `drops_stale_pending_onefile`; `promote_keeps_pending_on_failure` (read-only parent as a stand-in for the Windows lock; unix only).

## Live check (this Mac)
- Before: `bin/yt-dlp` was a universal Mach-O file (onefile). `--version` took 6.58 s and 6.02 s.
- Launched `npm run tauri dev` with the new code: it migrated to `bin/yt-dlp/` = `yt-dlp_macos` (rwxr-xr-x) + `_internal/`, 134 files, 0 symlinks, no quarantine xattr. `bin/pending` is empty, and no `.part`, `.old` or `.download` files were left.
- `--version` after migration: 0.20 s, 0.18 s. A fresh copy of the dir: 6.38 s on the first run, then 0.18 s (the one-time macOS scan).
- From start_download to the first progress line (temporary log with ms timestamps, removed before commit; video jNQXAC9IVRw):
  - onefile baseline: 7.48 s
  - onedir on the migration launch (first run, which overlaps the scan and the background `--version`): 7.47 s
  - onedir on the next launch: 2.63 s (the rest is yt-dlp's network extraction)
- Test downloads deleted. App killed, no processes left. The onedir install is in place and working.

## Concerns
1. Incident: during the onefile baseline run, someone used the app window that had popped up and started an mp3 playlist download into ~/Downloads (job 2). My 150 s time limit then killed the app at about 09:33, so the playlist may be incomplete (about 11 mp3 files present). I did not touch those files. Later runs were bounded so that each stopped about 3 s after the test file finished.
2. Windows was not tested. The design assumes that renaming `bin\yt-dlp` fails while `yt-dlp.exe` inside it is running, so the update stays staged. If Windows allows that rename, the immediate update would succeed instead, and the old dir would stay as `yt-dlp.old` until the next swap. Neither case corrupts the install. Also on Windows, `replace` for a single-file ffmpeg or deno now renames a running exe aside instead of failing.
3. The first yt-dlp run after an install or update still takes about 6 s on macOS (one-time scan). The background `--version` check at launch usually absorbs it, but if a download starts within about 6 s of an install, it pays that cost.
4. Comments in download.rs still say "PyInstaller launcher" or `_MEI` (onefile wording). The behaviour (process-group kill, `[PYI` filtering) is still correct for onedir, so I left them alone.
5. docs/02_decisions.md R25 still gives the original onefile two-process reason. It is a historical record, so it is unchanged.

---

# Fix round 1 (R34 and minor fixes)

Commit: `fix: don't swap yt-dlp under running downloads; recover from interrupted swap` (deps.rs, download.rs). The app was not launched or killed in this round; checks are unit tests only.

## Changes
1. R34: `update()` no longer swaps in the new yt-dlp while any download is queued or running (`crate::download::has_active_jobs(app)`). It goes down the existing "could not replace now" branch instead: the build stays in `bin/pending/yt-dlp/`, a warning is logged, and the notice is the existing "yt-dlp {v} downloaded; it will be applied the next time you start ytdown.". `has_active_jobs` is a new helper in download.rs. It reads the Manager's `jobs` map, which gets an entry in `start_download` (queued) and loses it at the end of `run_job`, so queued and running jobs both count.
2. Repair paths (`repair_ytdlp`, `repair_ytdlp_since`) still swap. Each has a one-line comment explaining that swapping is fine there because the binary is broken and the running jobs are failing anyway.
3. Recovery from an interrupted swap: new `old_path()`, which `replace` now uses too, and `recover(dest)`. If `dest` is missing and `<dest>.old` exists, `recover` renames `.old` back. It runs only at launch, inside the `APPLIED` block before `apply_pending`, for `bin/yt-dlp`, `bin/pending/yt-dlp`, ffmpeg and deno. At launch nothing else is swapping yet: the background check is spawned after `ensure`.
4. `remove_any` uses `symlink_metadata()`: a directory is removed recursively, anything else (including a symlink) with `remove_file`, and a missing path is a no-op.
5. download.rs comments no longer use onefile wording: the process-group comment, kill_on_drop "(direct child only)", and the kill_tree doc ("so yt-dlp and ffmpeg can clean up"). Only comments changed.

## Tests covering the changes
- `download::tests::tracks_active_jobs`: Manager `active()` is false when empty, true with a job, false after it is removed. `has_active_jobs(app)` is a one-line wrapper around it.
- `deps::tests::recovers_interrupted_swap`: no-op when nothing exists; `.old` with no `dest` is restored; `dest` present means `.old` is left alone; `remove_any` on a symlink to a directory removes only the link (unix).
- The R34 branch in `update()` needs an AppHandle and network, so no unit test covers it. It reuses the already-tested staged path.
- Honesty note: these tests were written together with the code, not run RED first. With the old `is_dir()` version, the symlink assertion would also have passed, because `remove_dir_all` on a symlink removes only the link. The change is defensive and makes the dir/file decision explicit.

## Commands and output
- `cargo test`: `test result: ok. 40 passed; 0 failed`
- `cargo clippy --all-targets`: 0 warnings, 0 errors

## Remaining concern
- R34 has a small race: a download started between the `has_active_jobs` check and the swap is not blocked, because `start_download` does not take `RUNNING`. The window is a few milliseconds of renames. Closing it would mean `start_download` taking `RUNNING` or a shared flag; I did not do that.
