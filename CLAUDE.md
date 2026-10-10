# ytdown — notes for Claude

- Spec: `docs/00_PRD.md`; decisions and known limitations: `docs/02_decisions.md`; dependency facts: `docs/01_spikes.md`.
- Session records (task briefs + implementer reports): `docs/superpowers/sessions/`.

## Working rules learned with the PO

- The PO often edits values directly (e.g. window size in `src-tauri/tauri.conf.json`) between turns. Before committing or documenting a PO-edited value, re-read the file in the same step and quote that value — never one seen earlier. (Once "380" was written in docs while the file held the PO's 420.)
- The PO may be using the dev app: don't kill or relaunch it without saying so; live checks that need the app must be announced first.
- UI changes: render `src/index.html` with headless Chrome (`--headless=new --screenshot`) and look at it before reporting; this Mac has no screen-recording permission for the app window.
