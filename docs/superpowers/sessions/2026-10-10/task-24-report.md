# Task 24 report
Commit 298640d on feat/task24 (single commit, code + docs).
- RED: `cargo test url::` -> classifies failed (left Playlist, right PlaylistOnly). GREEN: cargo test 52 passed; cargo clippy --all-targets clean.
- A: UrlKind::PlaylistOnly (list present, no `v`, not youtu.be/<id>, /shorts|live|embed/<id>; /embed/videoseries counts as pure playlist). start_download forces playlist=true. Dialog: #opt-single hidden, text "This link is a playlist.", focus on Whole playlist; arrow handling filters hidden options.
- B: formatSize picks unit after rounding. node check: 999->"999 B", 999949->"999.9 KB", 999950->"1.0 MB", 999999->"1.0 MB", 1e6->"1.0 MB", 1.23e9->"1.2 GB".
- C: KEY_OK regex derived from global-hotkey-0.8.0 parse_key (letters, digits, F1-F24, Numpad*, arrows, punctuation, nav keys, locks, volume/media). Others (IntlBackslash, IntlRo, Unidentified, F25...) show "Key not supported" and keep recording. Asserted via node.
- node --check src/main.js ok. Docs: R37 added, 3 limitation bullets removed, PRD line added.
Concern: UI dialog/recorder not exercised in the running app (not launched per instructions).
