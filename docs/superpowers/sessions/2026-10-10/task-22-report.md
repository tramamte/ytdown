# Task 22 report
Changes: args.rs template adds fragment_index|fragment_count (test updated); progress.rs parses them (NA/missing tolerated), percent = idx/count*100 clamped when count>0; download.rs `monotonic()` guard per format_id, reset on stream change and on new Formats line.
RED: fragment_progress_replaces_estimate failed ("100 vs 0") with fragment branch disabled; monotonic test failed to compile (no fn). GREEN: 42 passed; clippy clean.
Live (QWFaZf3MpKk, 40 s, 405 progress lines): raw _percent_str backward steps 49; after parse 0; after guard 0.
Files: src-tauri/src/{args,progress,download}.rs. Scratch outputs deleted; no processes left.
Concern: RED for args test was not recorded separately (impl edited before the test run); parse RED recorded by temporarily disabling the branch.
