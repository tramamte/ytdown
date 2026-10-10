# Task 21: Switch yt-dlp from the onefile build to the onedir build (PO decision 2026-10-10)

## Measured (controller, macOS arm64, yt-dlp 2026.08.19)
- onefile `yt-dlp_macos`: every `--version` run 6.0–6.3 s (PyInstaller unpacks ~30 MB each start).
- onedir `yt-dlp_macos.zip`: first run after install 6.1 s (macOS scans the 134 files once), then 0.17–0.18 s.
- Release assets (latest): `yt-dlp_macos.zip` (53.9 MB; universal), `yt-dlp_win.zip` (18 MB, x64), `yt-dlp_linux.zip` (40 MB, x64); all listed in `SHA2-256SUMS` as `<sha256>  <name>`. sha256 of the downloaded macOS zip matched.
- macOS zip layout: `yt-dlp_macos` (the executable) at the zip root + `_internal/…`; 134 regular files, 28 dirs, NO symlinks. Downloaded via curl: no quarantine xattr. Windows/Linux layouts not inspected — inspect them (`unzip -l` after downloading into the scratch dir) and pin the executable names you find (expected `yt-dlp.exe` / `yt-dlp_linux` + `_internal/`).
- `-U` works on the onedir build, but we don't use it (we stage/apply ourselves).

## Required behaviour (all existing behaviour stays; only the yt-dlp packaging changes)
- yt-dlp lives in a DIRECTORY `bin/yt-dlp/` (executable inside, name per OS). `deps::ytdlp_path(app)` returns the executable path inside it; every caller keeps working.
- Install: download the zip (streaming + sha256 as today), extract ALL entries into a fresh `bin/yt-dlp.part/` directory using zip-slip-safe paths (`enclosed_name()`), restore unix permissions from the entries (at least make the executable 0o755), remove macOS quarantine recursively (best effort), then swap atomically: rename existing `bin/yt-dlp` → `bin/yt-dlp.old` (if present), rename `bin/yt-dlp.part` → `bin/yt-dlp`, delete `.old`. Clean up `.part`/`.old`/zip on every path (also leftovers from a previous crashed run).
- Staged updates: pending is `bin/pending/yt-dlp/` (a directory) applied at first launch before anything spawns (same rules as today); the manual Update button's immediate apply and its Windows-locked fallback (stage for next launch) keep working with directories (a running exe inside the dir makes the rename fail on Windows → stage).
- Migration: an existing onefile install has `bin/yt-dlp` as a FILE. Treat that as "missing": remove the file and install the onedir build (this happens once; input disabled while installing, as for any missing dependency). A stale `bin/pending/yt-dlp` FILE from the old scheme must be deleted, not applied.
- `looks_installed` for yt-dlp: the directory exists and the executable inside is a non-empty file (executable bit on unix).
- Broken-binary detection at download time (`never_started` etc.) and the retry-once flow stay unchanged.
- Version check/compare, staging decision, and SHA2-256SUMS lookup now use the zip asset names above.

## Tests
- Pure/temp-dir tests: asset name per OS; extraction of a small in-memory zip into a temp dir (files, nested dirs, exec bit set on the executable, a `../evil` entry rejected/ignored); swap with and without an existing dir (old content replaced, no `.part`/`.old` left); migration from a FILE `bin/yt-dlp`; pending dir applied; stale pending FILE removed.
- `cargo test`, `cargo clippy --all-targets` warning-free.

## Live check (this Mac)
Stop any running app first (`pkill -f "target/debug/ytdown"; pkill -f "tauri dev"`). The current install is the onefile build (FILE `~/Library/Application Support/com.tramamte.ytdown/bin/yt-dlp`). Launch `npm run tauri dev` → it migrates to the onedir dir; show the dir layout, that the executable runs (`--version` timing: first and second run), and that a real download (https://www.youtube.com/watch?v=jNQXAC9IVRw) starts noticeably faster (show the time from start_download to the first progress line, via temporary logging removed before commit, or via file mtimes). Delete test downloads; kill the app; no background processes; never run commands that wait on stdin; leave a working onedir install.

## Docs
Update `docs/00_PRD.md` (의존성 대상: yt-dlp onedir 빌드), `docs/02_decisions.md` (PO decision row "yt-dlp 빌드" → onedir, with the measurement; remove the onefile known-limitation bullet), and the README dependency note if it mentions the file.
