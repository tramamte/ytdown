# Task 23 report: playlist parent row + per-item child rows

Status: DONE_WITH_CONCERNS (concerns are minor; see the end)
Branch `feat/playlist-group`, commits (not pushed):
- `3ad5eb9` feat: group playlist downloads into a parent row with per-item child rows
- `a6dabf2` docs: playlist grouping in PRD and PO decision (no per-item cancel)

## Verified yt-dlp facts (installed 2026.08.19, onedir, live runs)
- `%(playlist_index)s|%(playlist_count)s|%(playlist_title)s` works in `before_dl` prints. The index can be zero-padded (`001/120` in flat lists). The parser handles that and treats NA as 0 or "".
- Multiple `--print before_dl:` prints run in the order given, so the item marker comes before the title and formats prints (checked in the capture).
- Error continuation: `--help` says `--no-abort-on-error` is the **default** ("Continue with next video on download errors; e.g. to skip unavailable videos in a playlist"). Live run on `PLW7B4PyySHApSUCjSpXzBd6LXT8IhjvH9` (7 items, #5 unavailable): items 1–4 downloaded, then `ERROR: [youtube] Eb8yXfxT1hw: Video unavailable` on stderr, then items 6 and 7 downloaded. **Exit code 1.** No flag added because the default already continues (the args comment says so).
- **An unavailable item prints no `ytdown-item` line.** Extraction fails before `before_dl`, so the only trace is the stderr `ERROR:` line, which arrives between item 4's `ytdown-file` line and item 6's `ytdown-item` line (merged capture).
- An item whose file already exists prints Item → Title → Formats → File with no progress lines. This happened live because macOS APFS is case-insensitive: item 5 ("…Timer With Soundeffect") collided with item 3 ("…timer with Soundeffect").
- On a successful playlist run, yt-dlp exits 0 and prints one `ytdown-file` line per item.

## Design
- args.rs: for playlist jobs only, adds `--print before_dl:ytdown-item:%(playlist_index)s|%(playlist_count)s|%(playlist_title)s` before the title/formats prints.
- progress.rs: adds `Line::Item { index, total, playlist_title }`. NA becomes 0 or "", and a `|` inside the title is kept (`splitn(3)`).
- download.rs:
  - New `Tracker` struct, a pure state machine with no I/O. It holds the files, formats and per-stream max that used to be local variables in `run()`, plus `items`.
  - `stdout()`, `stderr()`, `settle()`, `outcome()`, `update_percent()`.
  - For single videos, `stdout()` does exactly what the old match did: Title/Progress update the parent and emit it, Formats resets the per-stream max, File is collected, and `stderr()` is a no-op.
  - Playlist rules:
    - An Item line starts a child row and sets the parent title (once known) and `items_total`.
    - Title, progress and stage go to the running child.
    - A `ytdown-file` line marks the child done with its file size.
    - An `ERROR:` line fails the running child. If no child is running, it creates a failed child at last index + 1; this covers the "unavailable" case. The message goes through the existing `failure_message`, so the 429 mapping applies.
    - Cancel moves the running child to `canceled`.
    - At process end, any child still running settles to done or error.
  - Parent percent = (done + failed + current item's share) / total × 100, where the share is stage-aware (stage 1/2 at 50% → 0.25). It never decreases.
  - Parent outcome:
    - exit 0 with no failures → `done`
    - no item succeeded (or failures without any failed row) → `error` with the yt-dlp message
    - otherwise → `done` with message `"n of N failed"`
    - Parent `size` = sum of all item files.
  - `Update` has new fields `playlist`, `items_done`, `items_failed`, `items_total`. The child event is `download://item` with `{job_id, index, title, kind, state, percent, stage, stages, stream, size, message}`.
  - Draining, cancel, kill_tree, broken-binary detection and retry-once are unchanged:
    - The `never_started` arm now comes before the success arm. This is equivalent because it is false for exit 0.
    - A comment and a `debug_assert!(t.items.is_empty())` document that a retry cannot duplicate child rows: rows only come from stdout lines or yt-dlp's own stderr lines, and either one rules out `never_started`.
- UI (main.js/style.css):
  - Parent row:
    - Kind label is "Playlist".
    - Running: `Playlist · done/total · pct%`. Done: `Playlist · N items · size`.
    - The red `.msg` now also shows for `done` (the partial-failure note).
  - Children:
    - A nested `<ul class="items">` is appended to the parent `<li>` after the progress bar and message.
    - Each child shows title (ellipsis, falling back to `Item N`) and `Video|Audio · stage/pct | size | Failed | Canceled`, plus a 3 px bar only while running.
    - State shows as a left bar only, with no background tint.
  - Text goes in via `textContent`. `innerHTML` is used only for static templates.
  - Single-video rendering produces the same strings as before; it now goes through the shared `runDetail()`/`status()` helpers.
  - "Clear finished" is unchanged. Children live inside the parent `<li>`, so they leave with it, and a running parent keeps them.

## TDD evidence
- args: RED. `item_print_only_for_playlists` and `video_capped_height_playlist` failed: `assertion failed: has_pair(&a, ["--print", ITEM])` and `unwrap() on None`. GREEN: 7/7 args tests.
- progress: RED. `parses_playlist_item` failed to compile (`no variant named Item`). GREEN: 4/4.
- download: RED. 21 compile errors (no `Tracker`, `Item` or new `Update` fields). GREEN after implementation. New tests (captured lines):
  - `playlist_items_done_in_order`
  - `playlist_item_progress_and_parent_percent`
  - `playlist_failed_item_does_not_stop_the_rest` (the real unavailable-item sequence)
  - `playlist_error_while_running_and_none_succeeded` (429 mapping, error outcome)
  - `playlist_cancel_marks_running_item`
  - `single_video_has_no_items` (old parent behaviour preserved)
- Final: `cargo test` 50 passed, 0 failed. `cargo clippy --all-targets` produced 0 warnings. `node --check src/main.js` OK. `git diff --check` is clean.

## Live check
- Exact `args::build(.., playlist=true, ..)` output was dumped by a temporary `#[ignore]` test (removed before commit). It ran with default settings into scratch `live/`.
  1. `PLr-mgdpse3TpWFoJm99Y9RkXcL6r1RaWM` (6 × ~7 s, video args): exit 0.
  2. `PLW7B4PyySHApSUCjSpXzBd6LXT8IhjvH9` (7 items, #5 unavailable, audio args): exit 1.
- Replaying both captures through `Tracker` (temporary `#[ignore]` test, removed):
  - Run 1: started order [1..6], all `done` with their real file sizes (122275, 345823, 104544, 364528, 104544, 85259). Parent: title = playlist title, done=6 failed=0 total=6, percent 100 → `("done", "")`.
  - Run 2: started order [1..7]. Items 1–4 and 6–7 are `done` with real sizes. #5 is `error`, msg `ERROR: [youtube] Eb8yXfxT1hw: Video unavailable`. Parent: done=6 failed=1 total=7 → `("done", "1 of 7 failed")`, size 1248373, which equals the files on disk.
- The failure path was verified live with a real unavailable video, not only by unit test.
- Scratch downloads were deleted. No processes left running, and the app was not touched.

## Files changed
- /Users/tramamte/src/rust/ytdown/src-tauri/src/args.rs
- /Users/tramamte/src/rust/ytdown/src-tauri/src/progress.rs
- /Users/tramamte/src/rust/ytdown/src-tauri/src/download.rs
- /Users/tramamte/src/rust/ytdown/src/main.js
- /Users/tramamte/src/rust/ytdown/src/style.css
- /Users/tramamte/src/rust/ytdown/docs/00_PRD.md
- /Users/tramamte/src/rust/ytdown/docs/02_decisions.md

## PO GUI checklist
1. Whole playlist (e.g. the 6-item countdown playlist):
   - The parent row title becomes the playlist name.
   - Meta reads `Playlist · k/6 · p%`, and the bar rises steadily.
   - Child rows appear one by one, compact and indented, with a blue left bar while running and green when done (with size).
   - At the end the parent shows `Playlist · 6 items · size`.
2. Playlist with an unavailable item (`PLW7B4PyySHApSUCjSpXzBd6LXT8IhjvH9`): an `Item 5` child shows red `Failed` plus the yt-dlp message, the rest continue, and the parent ends green with the red line `1 of 7 failed`.
3. Cancel (×) on the parent mid-download: the running child shows `Canceled` and the parent shows `Canceled`.
4. "Clear finished" removes finished playlists with their children and keeps a running playlist's finished children.
5. Single video and "This video only": looks exactly as before.
6. Light and dark mode for the child rows' colors and spacing.

## Concerns
- Stderr/stdout ordering: an extraction failure is attributed to "last index + 1" when no item is running. If the stderr read lagged behind the next item's stdout lines, the error would mark the next item instead. This is unlikely, because yt-dlp writes the ERROR before extracting the next item over the network. It is marked with a `ponytail:` comment.
- An item that failed before starting has no title. The child shows `Item N` plus the error message, which contains the video id.
- The parent size can double-count when two items map to the same file. This happened live on case-insensitive APFS: item 5 reported item 3's file. The same thing happened before for multi-file jobs.
- `docs/02_decisions.md` still lists the known limitation "재생목록 다음 항목으로 넘어갈 때 단계·퍼센트 표시가 잠깐 이전 값으로 남음". With per-item rows (each starts at 0) and an aggregate parent percent, this should no longer apply. I left it for the PO to confirm or remove.
- The app does not pass `--ignore-config`, so a user yt-dlp config containing `--abort-on-error` would stop at the first failed item. This was already true before this change; I added no flag.

## Follow-up fix (R35), commit after a6dabf2: `fix: ignore user yt-dlp config; dedupe playlist size`
- args.rs: `--ignore-config` is now the first base arg for all jobs. `video_best_mp4_single` asserts `a[0] == "--ignore-config"`.
  - Verified on yt-dlp 2026.08.19: `--help` lists it. A scratch `XDG_CONFIG_HOME` holding a user config with `--abort-on-error` was loaded without the flag (`[debug] User config ...: ['--abort-on-error']`) and not loaded with it (only `Command-line config` in the debug log).
- download.rs: `total_size` counts each final path once. Paths are compared lowercased except on Linux, so two items landing on one file are not double-counted. `sums_sizes_ignoring_missing` adds a repeated exact path and a case variant (`A`); the expected total stays 15.
  - RED: `assertion left == right failed` in both tests. GREEN after the change.
- docs/02_decisions.md: removed the obsolete stale stage/percent limitation bullet and added R35.
- `cargo test`: 50 passed, 0 failed. `cargo clippy --all-targets`: 0 warnings. `node --check src/main.js`: OK.

## Review fixes (R36), commit after 01f9efc: `fix: playlist child styling and final counts`
- style.css: the top-level state rules are now scoped to `#list > li[...]`. The done meta, progress and title rules are scoped to direct children with `>`. A parent's done, error or canceled state therefore no longer leaks into its child rows.
  - Each child is styled by its own state: left-bar colour, green meta when done, and a faint title when done or canceled.
  - A canceled child gets no opacity of its own, so it doesn't stack with a canceled parent's .6.
  - Top-level rows are unchanged: they are all direct children and keep the same specificity.
- download.rs `outcome`: `items_failed = max(items_failed, total - items_done)` when the total is known. Items that never ran because yt-dlp died early (or exited 0 with items missing) count as failed. For example, 1 done of 5 then a crash gives "4 of 5 failed".
- download.rs `Tracker::stdout`: if an Item line arrives while the previous child is still running (no file, no ERROR), that child becomes `error` with message "Did not finish" and is counted as failed. `stdout` now returns `Vec<Item>` so both rows are emitted, and `run()` emits each one.
- Tests:
  - New: `playlist_early_end_counts_never_run_items_as_failed` (crash case "4 of 5 failed"; exit 0 with items missing gives "2 of 3 failed"). `playlist_unfinished_item_fails_when_next_starts` (emits item 1 as error "Did not finish", then item 2 running; failed=1).
  - RED: the new tests did not compile until `stdout` returned `Vec<Item>`.
  - Adjusted: `playlist_items_done_in_order`'s trimmed capture count was edited from 6 to 2 so it represents a complete run. `playlist_failed_item_does_not_stop_the_rest` replays a trimmed capture (items 4–6 of 7), so it now expects "5 of 7 failed". The full live replay is still "1 of 7 failed": done=6, failed=1, and max(1, 7-6) = 1.
- `cargo test`: 52 passed, 0 failed. `cargo clippy --all-targets`: 0 warnings. `node --check src/main.js`: OK.
