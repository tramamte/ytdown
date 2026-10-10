# Task 22: Monotonic progress for fragmented (HLS/DASH) streams (PO bug report 2026-10-10)

## Verified cause (controller, yt-dlp 2026.08.19, onedir)
URL https://www.youtube.com/watch?v=QWFaZf3MpKk (default settings, `-f bv*+ba/b`): selected `616+251`, protocols `m3u8_native+https`. Stream 616 has 43 fragments and NO total size (`total_bytes=NA`); `%(progress._percent_str)s` is computed from a moving `total_bytes_estimate`, so 49 of 405 progress lines went BACKWARD within the same stream. The very first line was `100.0%` (dl=712, est=712, fragment 0/43) and the next was `1.2%`. Real lines:
```
P 616 100.0% dl=712 tot=NA est=712 frag=0/43
P 616   1.2% dl=712 tot=NA est=61232.0 frag=1/43
P 616  11.3% dl=1736 tot=NA est=15308.0 frag=1/43
P 616  10.1% dl=3784 tot=NA est=37324.0 frag=1/43
P 616  14.0% dl=1081942 tot=NA est=7746579.0 frag=5/43
```
(For streams with a known size, e.g. stream 251 over https, `_percent_str` is already monotonic.)

## Required
1. src-tauri/src/args.rs: extend the progress template to carry fragment info, e.g. `download:ytdown:%(progress._percent_str)s|%(info.format_id)s|%(info.vcodec)s|%(progress.fragment_index)s|%(progress.fragment_count)s`. Update args tests.
2. src-tauri/src/progress.rs: parse the two new fields (tolerate `NA`, missing fields for older lines). When `fragment_count` is a positive number and `fragment_index` is a number, percent = `fragment_index / fragment_count * 100` (clamped 0..100); otherwise use `_percent_str` as today. Add a unit test feeding the real lines above (expect 0, ~2.3, ~2.3, ~2.3, ~11.6) plus a known-size https line unchanged.
3. src-tauri/src/download.rs: per stream (format_id) never let the emitted percent decrease — keep the max seen for the current format_id; reset when the format_id changes (next stream) or a new `Formats` line arrives (next playlist item). This guards any remaining estimate-based case. Don't change draining/cancel/broken-detection/retry/size logic.

## Verification
`cargo test`, `cargo clippy --all-targets` warning-free. Live: run yt-dlp with exactly the args `args::build` produces (print via a temporary #[ignore] test, removed before commit) on https://www.youtube.com/watch?v=QWFaZf3MpKk (add nothing else; the URL's list param is irrelevant because args use --no-playlist) into a scratch dir under docs/superpowers/sessions/2026-10-10/ for ~40 s, feed every captured stdout line through `progress::parse` + the monotonic guard (a temporary test or tiny harness, removed before commit) and show 0 backward steps per stream. yt-dlp binary: `~/Library/Application Support/com.tramamte.ytdown/bin/yt-dlp/yt-dlp_macos` (onedir); ffmpeg/deno beside it in `bin/`. Delete scratch outputs; kill only processes you started (`pkill -f <your scratch dir>`); do NOT kill or launch the app (the PO may be using it); nothing that waits on stdin.
