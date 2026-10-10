# Task 24: Pure-playlist URLs + two small display fixes (PO request 2026-10-10)

## A. Pure playlist URL (known limitation #7)
Today `url::classify` returns `Playlist` for any URL with a `list` param. For a URL with NO video (`https://www.youtube.com/playlist?list=PL…`, or `music.youtube.com/playlist?list=…`), "This video only" passes `--no-playlist`, which yt-dlp ignores when there is no video → the whole playlist downloads anyway.
Controller ruling R37: keep asking (avoid surprise large downloads) but don't offer the meaningless option.
- src-tauri/src/url.rs: new `UrlKind::PlaylistOnly` (serializes as "PlaylistOnly") when the URL has `list` but no video: no `v` query param and the path is not a youtu.be video id / `/shorts/<id>` / `/live/<id>` / `/embed/<id>`. Keep `Playlist` for video+list URLs. Unit tests for each form (incl. existing cases unchanged).
- src-tauri/src/download.rs `start_download`: if the URL classifies as `PlaylistOnly`, force `playlist = true` regardless of the argument (backend must not trust the UI).
- src/main.js + src/index.html (playlist dialog): for `PlaylistOnly`, hide the "This video only" option and show the text "This link is a playlist." with options "Whole playlist" and Cancel; focus starts on "Whole playlist"; the existing ↑/↓/Tab/Esc handling keeps working (↑/↓ simply has one option). For `Playlist` nothing changes.

## B. Size label rounding edge
`formatSize` in src/main.js shows "1000.0 KB" for 999,950–999,999 B (unit chosen before rounding). Choose the unit after rounding (e.g. bump to the next unit when the rounded value reaches 1000). Add a tiny node assertion check (not committed as a framework; a one-off `node -e` in your report is enough) for 999, 999949, 999950, 999999, 1000000, 1.23e9.

## C. Shortcut recorder accepts keys the hotkey parser rejects
The settings shortcut recorder (src/main.js) stores `e.code`-based key names; codes like `IntlBackslash`, `IntlRo`, `Unidentified`, `Numpad*` (check) may not be parseable by global-hotkey 0.8 (`~/.cargo/registry/src/*/global-hotkey-0.8.0/src/hotkey.rs` `parse_key`), and only fail at Save. Make the recorder accept only key codes `parse_key` supports (derive the allowlist from that source — letters, digits, F-keys, arrows, Space, Enter, Tab?, Backspace, Delete, Home/End/PageUp/PageDown, punctuation it lists, numpad keys if listed); otherwise keep recording and show "Key not supported". Keep everything else about the recorder unchanged.

## Verification
`cargo test`, `cargo clippy --all-targets` warning-free, `node --check src/main.js`, node assertion checks for B and for C's key filter. No app launch/kill (the PO may be using it); nothing that waits on stdin.

## Docs
docs/02_decisions.md: add R37; remove the known-limitation bullets for the pure-playlist URL, the "1000.0 KB" display, and the unsupported recorder keys. docs/00_PRD.md: one line under 다운로드 for pure playlist URLs.
