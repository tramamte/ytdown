# 01 Spikes (Task 0) - verified 2026-10-10

Verified on macOS arm64 by direct download/run unless marked UNVERIFIED.

## 1. macOS ffmpeg

| | evermeet.cx | osxexperts.net |
|---|---|---|
| Arch | **Intel x86_64 only** (author states he will not build ARM; Intel binary runs on ARM via Rosetta - UNVERIFIED here) | arm64 (ffmpeg 9.0) + Intel (ffmpeg 8.0) |
| URL | `https://evermeet.cx/ffmpeg/ffmpeg-9.0.2.zip` (also `.7z`); stable redirect `https://evermeet.cx/ffmpeg/getrelease/zip` -> 302 -> **a third-party mirror** (`e.deolaha.ca:4242`) | arm64 `https://www.osxexperts.net/ffmpeg9arm.zip`; Intel `https://www.osxexperts.net/ffmpeg80intel.zip` |
| Archive | zip / 7z, contains single `ffmpeg` | zip, contains `ffmpeg` + junk `__MACOSX/._ffmpeg` |
| Integrity | GPG detached `.sig` per archive (key 0x476C4B611A660874, fpr `20F6 EA3E 0CFD 6B4C 5344 7A73 476C 4B61 1A66 0874`). No sha256 file. Info API: `https://evermeet.cx/ffmpeg/info/ffmpeg/release` (JSON: version, size, urls) | SHA256 printed on page of the **extracted binary** (not the zip). arm64 `591260c945d0eef150e3bf82b0ef988bd36a9cecc18ff05d6679617159f0a95e` (verified: matches), Intel `df3f1e3facdc1ae0ad0bd898cdfb072fbc9641bf47b11f172844525a05db8d11` (not downloaded). Hashes are on an HTML page, not a machine-readable file; they change per release |
| License | `--enable-gpl --enable-version3` (GPL v3) | `--enable-gpl` (GPL) |
| Sources | https://evermeet.cx/ffmpeg/ , https://evermeet.cx/ffmpeg/apple-silicon-arm | https://www.osxexperts.net/ |

Observed: osxexperts arm64 binary after curl download has only `com.apple.provenance` (no `com.apple.quarantine`), runs (`ffmpeg version 9.0`), is ad-hoc signed Mach-O thin arm64 (`codesign -dv` OK). Site still advises `xattr -dr com.apple.quarantine` (browser downloads). Neither site is an official FFmpeg project; both are single-maintainer builds.
Binary in zip is an extracted-hash scenario: verify sha256 AFTER unzip. The hash is hard-coded per version, so the app must pin URL+hash (or fetch the page - brittle). No stable "latest" URL for osxexperts (file name is version-specific: `ffmpeg9arm`, `ffmpeg80intel`) - UNVERIFIED whether they overwrite the same name on new releases.

## 2. Windows / Linux ffmpeg (BtbN/FFmpeg-Builds, release tag `latest`, published 2026-10-09)

Base: `https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/`

| OS | Asset | Format | Binary path inside |
|---|---|---|---|
| Windows x64 | `ffmpeg-master-latest-win64-gpl.zip` | zip | `ffmpeg-master-latest-win64-gpl/bin/ffmpeg.exe` (verified) |
| Linux x64 | `ffmpeg-master-latest-linux64-gpl.tar.xz` | tar.xz | `ffmpeg-master-latest-linux64-gpl/bin/ffmpeg` (verified) |

Also exist: `-lgpl` variants (`...win64-lgpl.zip`, `...linux64-lgpl.tar.xz`), `-gpl-shared` (needs DLLs/.so - avoid), `linuxarm64`, `winarm64`, and versioned `ffmpeg-n8.1-latest-*` / `ffmpeg-n9.0-latest-*` (those exist only as `-shared` in the asset list).
Checksums: single file `checksums.sha256` in the same release, format `<sha256>  <filename>` (sha256sum style, two spaces). Verified lines (2026-10-10; changes daily):
```
dafe64a1c4ece573b8e5f076bf8914ea997be9533be4bb52e6c5a33adfdd8d1b  ffmpeg-master-latest-win64-gpl.zip
8be42254704c7dcaffb68db720ff5b92d3e62f09f92d19543fd2db8ee383ac39  ffmpeg-master-latest-linux64-gpl.tar.xz
```
Both matched my own `shasum -a 256` of the downloads. Archives are ~170 MB binaries (win ffmpeg.exe 169 MB). Source: GitHub API `https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/tags/latest`. License: GPL (LICENSE.txt in archive).
Note: `latest` is a rolling tag, so hash must be fetched from `checksums.sha256` at install time (not hard-coded).

## 3. yt-dlp (latest stable 2026.08.19)

Base: `https://github.com/yt-dlp/yt-dlp/releases/latest/download/`

| OS | Asset (raw single-file) | Notes |
|---|---|---|
| macOS | `yt-dlp_macos` | universal2 Mach-O (x86_64+arm64) (verified via `file`); `yt-dlp_macos.zip` also exists (onedir) |
| Windows x64 | `yt-dlp.exe` | also `yt-dlp_x86.exe`, `yt-dlp_arm64.exe`; `*_win.zip` = onedir |
| Linux x64 | `yt-dlp_linux` | also `_linux_aarch64`, `_musllinux`; `yt-dlp` = python zipapp (needs python) |

Checksums: `SHA2-256SUMS` (+ `.sig`, `SHA2-512SUMS`). Format `<sha256>  <filename>`, one per asset, e.g.
`0f192b7ec147ab6288885d6351d9ab67367640029b4377576ef46dd79cf7b202  yt-dlp_macos` (my download matched).
`yt-dlp -U` on the standalone macOS build: works (`Latest version: stable@2026.08.19 ... up to date`). Not run on Win/Linux (UNVERIFIED there; expected same, the update mechanism is built-in for the release binaries). Self-update needs write access to the binary's folder (app-data is fine).
`yt-dlp_macos` downloaded via curl: xattr = only `com.apple.provenance`, NO `com.apple.quarantine`; `chmod +x` then runs fine (`--version` -> 2026.08.19). Same expected with reqwest (no quarantine is set by non-browser downloaders - observed for curl only). Keep the xattr removal as a harmless no-op.

### JS runtime (EJS) - source: https://github.com/yt-dlp/yt-dlp/wiki/EJS
- **Required in practice.** Observed without any runtime: `WARNING: [youtube] No supported JavaScript runtime could be found. Only deno is enabled by default ... YouTube extraction without a JS runtime has been deprecated, and some formats may be missing.` For the 19 s test video the download still succeeded (formats f251 audio, f395 video), but missing formats / failures are expected on other videos. Treat as required.
- Runtimes/min versions: **Deno >= 2.3.0** (default, enabled automatically), Node >= 22.0.0 (`--js-runtimes node`), QuickJS / QuickJS-NG (`--js-runtimes quickjs`, binary must be named `qjs`), Bun 1.2.11..1.3.14 (deprecated).
- Discovery: on `PATH`, or (Windows) same folder as `yt-dlp.exe`, or explicit `--js-runtimes "deno:/path/to/deno"` (path to exe or its folder). Verified both: `--js-runtimes "deno:<abs path>/deno"` works; deno on PATH with no flag works.
- Standalone binaries bundle `yt-dlp-ejs` scripts (wiki: "No additional action required") - no `--remote-components` needed for our binaries.
- Deno latest v2.9.7. Assets at `https://github.com/denoland/deno/releases/latest/download/`:

| OS | Asset (zip containing `deno`/`deno.exe`) | Checksum file |
|---|---|---|
| macOS arm64 | `deno-aarch64-apple-darwin.zip` | `deno-aarch64-apple-darwin.zip.sha256sum` |
| macOS x64 | `deno-x86_64-apple-darwin.zip` | `deno-x86_64-apple-darwin.zip.sha256sum` |
| Windows x64 | `deno-x86_64-pc-windows-msvc.zip` | `deno-x86_64-pc-windows-msvc.zip.sha256sum` |
| Linux x64 | `deno-x86_64-unknown-linux-gnu.zip` | `deno-x86_64-unknown-linux-gnu.zip.sha256sum` |

  (Linux/Win aarch64 variants also exist.) The `.zip.sha256sum` for macOS arm64 contained just `5cd46d6268f6f78f5d88bdc7159d20bd44cdaa4b3303474839f87ec6fe7ae25c` (hash only, no filename as printed - parse the first whitespace token). My download matched. `deno --version` ran: `deno 2.9.7 (stable, release, aarch64-apple-darwin)`. Also `deno-*.sha256sum` (without `.zip`) exists = hash of the extracted exe (not inspected).
- **Decision for Task 5:** add a Deno install step (download zip, verify sha256, unzip into app-data bin dir next to yt-dlp). Pass `--js-runtimes "deno:<appdata>/bin/deno"` explicitly rather than relying on PATH (GUI apps on macOS have minimal PATH).

## 4. yt-dlp output format (real run, yt-dlp 2026.08.19, video jNQXAC9IVRw "Me at the zoo")

Flag sets tried (all with `--newline --progress-template "download:ytdown:%(progress._percent_str)s" --print "before_dl:ytdown-title:%(title)s" --no-simulate`):

| Extra flags | Title line | Progress lines |
|---|---|---|
| none (as in brief) | yes | **NO** (`--print` implies `--quiet`) |
| `--progress` | yes | **yes** |

**Working flag set** (prints both): add `--progress`:
```
yt-dlp --js-runtimes "deno:<path>" --progress --newline \
  --progress-template "download:ytdown:%(progress._percent_str)s" \
  --print "before_dl:ytdown-title:%(title)s" --no-simulate \
  -P "temp:<dir>" -o "<dir>/%(title)s.%(ext)s" "<url>"
```
Real output (stdout+stderr merged; trimmed):
```
WARNING: --paths is ignored since an absolute path is given in output template
ytdown-title:Me at the zoo
WARNING: You have requested merging of multiple formats but ffmpeg is not installed. The formats won't be merged
ytdown:  0.5%
ytdown:  1.4%
ytdown:  3.2%
ytdown:  6.9%
ytdown: 14.2%
ytdown: 28.8%
ytdown: 58.1%
ytdown:100.0%
ytdown:100.0%
ytdown:  0.4%
...
ytdown:100.0%
ytdown:100.0%
```
Findings for Task 3/4:
- Title line: exactly `ytdown-title:<title>` (1 line; the `before_dl:` prefix is stripped). Goes to stdout.
- Progress lines: `ytdown:` + `%(progress._percent_str)s`, which is **left-padded to 6 chars** (`"  0.5%"`, `" 14.2%"`, `"100.0%"`), so the line is `ytdown:  0.5%` (spaces after the colon vary). Parser must `trim()` and strip `%`. Test input should include padded forms, not `ytdown: 12.3%` only.
- Percent restarts at 0 for **each stream** (video then audio, separate downloads) and `100.0%` is printed twice per stream. Overall progress therefore needs mapping (e.g. stream index/2) or the UI must tolerate resets; ignoring is also an option.
- Without ffmpeg, yt-dlp leaves un-merged `Me at the zoo.f251.webm` + `Me at the zoo.f395.mp4` (ffmpeg is required for merge/audio extraction; pass `--ffmpeg-location`).
- Passing an absolute path in `-o` makes `-P` ignored (`WARNING: --paths is ignored`) - use a relative `-o` template with `-P home:` / `-P temp:`, or drop `-P`. (My first run with relative `-o out/...` plus `-P temp:out` produced `out/out/` doubling, a test artefact.)
- Warnings go to stderr, prefixed `WARNING:`; the app should read both stdout and stderr.
- Not tested: audio-only (`-x`), final-filename print (`--print after_move:filepath`), ffmpeg merge.
