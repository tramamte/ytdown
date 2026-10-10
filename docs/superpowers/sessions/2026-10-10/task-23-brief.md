# Task 23: Group playlist downloads — parent row + one child row per item (PO request 2026-10-10)

PO request: when "Whole playlist" is chosen, show a representative playlist row at the top of the list (in progress), and add the items actually being downloaded below it one by one.

Single-video jobs must behave exactly as today.

## Facts / constraints
- A playlist job is ONE yt-dlp process downloading items sequentially (one job counts once against the concurrency cap of 3). Cancel = cancel the whole playlist (parent ×). No per-item cancel.
- Existing per-item prints: `before_dl:ytdown-formats:%(format_id)s`, `before_dl:ytdown-title:%(title)s`, `after_move:ytdown-file:%(filepath)s`; progress template `download:ytdown:<pct>|<format_id>|<vcodec>|<frag_idx>|<frag_cnt>`; per-stream monotonic percent guard resets on Formats (download.rs). Read args.rs, progress.rs, download.rs, main.js (render/LABEL/clear), style.css (#list rules) first.
- yt-dlp templates available for playlist items: `%(playlist_index)s`, `%(playlist_count)s` (or `n_entries`), `%(playlist_title)s`. VERIFY on the installed yt-dlp (`~/Library/Application Support/com.tramamte.ytdown/bin/yt-dlp/yt-dlp_macos`, onedir; ffmpeg/deno in `bin/`).
- Item failure behaviour: VERIFY whether yt-dlp continues with the next item after one item fails (default vs `--ignore-errors`/`--no-abort-on-error`) and the final exit code. For playlists we want: continue with remaining items; failed items shown as failed. Add the needed flag only for playlist jobs.

## Backend
- args.rs (playlist jobs only): add `--print before_dl:ytdown-item:%(playlist_index)s|%(playlist_count)s|%(playlist_title)s` (+ the error-continuation flag if needed). Tests.
- progress.rs: `Line::Item { index: u32, total: u32, playlist_title: String }` (tolerate NA). Tests.
- download.rs:
  - Parent = the existing job event `download://update` with new fields: `playlist: bool`, `items_done: u32`, `items_failed: u32`, `items_total: u32` (0 when unknown); parent `title` becomes the playlist title once known; parent `percent` = (items finished + current item fraction) / items_total * 100 (monotonic); parent `size` = sum of finished item files at the end.
  - Children: new event `download://item` `{ job_id, index, title, kind, state: "running"|"done"|"error"|"canceled", percent, stage, stages, stream, size, message }`. An item starts at its `Item` line; its title from the following Title line; progress/stage like today; done on its `ytdown-file:` line (size = that file); an `ERROR:` stderr line while an item is running marks that item `error` (message = the line, 429 mapping as today) and yt-dlp moves on. When the job is canceled, the running item → `canceled`.
  - Parent final state: all items ok → `done`; some failed → `done` with message `"<n> of <total> failed"`; none succeeded → `error`; canceled → `canceled`.
  - Keep draining/cancel/broken-binary detection/retry-once unchanged. Careful: the retry-once for a broken yt-dlp must not duplicate child rows (it only happens when yt-dlp never started, so no items exist yet — assert that in a comment/test).
- Unit-test the item state machine as a pure function where practical (feed real captured lines: two ok items, one failing item).

## UI (src/main.js, src/style.css)
- Parent row: as today but meta `Playlist · <done>/<total> · <pct>%` while running; when done `Playlist · <total> items · <size>` (+ red message line if some failed). Kind label "Playlist" when `playlist` is true.
- Children: a nested `<ul class="items">` inside the parent `<li>` (after its progress/msg), each child `<li>` compact and indented: title (ellipsis), meta (`Video|Audio · <stage/pct>` while running, size when done, "Failed" + message when error), thin progress bar only while running. Children appended in index order as they start. Same per-state colors as top-level rows, but subtler (no background tint; left bar only).
- "Clear finished" removes finished parents together with their children (nested, so it happens naturally); a running parent's finished children stay.
- Text via textContent only; no innerHTML with data.

## Verification
`cargo test`, `cargo clippy --all-targets` warning-free, `node --check src/main.js`. Live: find a small PUBLIC playlist with 2–4 short videos (check with `--flat-playlist`), run yt-dlp with exactly the args `args::build` produces for a playlist job (print via a temporary #[ignore] test removed before commit) into a scratch dir under docs/superpowers/sessions/2026-10-10/, capture stdout+stderr, and feed the captured lines through your state machine in a test to show: items started in order, each item done with its size, parent counts correct. If you can find a playlist containing an unavailable/private video, use it to verify the failure path; otherwise state that it is verified by unit test only. Delete scratch downloads. Do NOT kill or launch the app (the PO may be using it); kill only processes you started; nothing that waits on stdin. The GUI rendering will be checked by the PO.

## Docs
Update docs/00_PRD.md (메인 화면: 재생목록 묶음 표시, 취소는 재생목록 단위) and add a row to docs/02_decisions.md (PO 결정: 재생목록 묶음 표시; 항목별 취소 미지원 이유).
