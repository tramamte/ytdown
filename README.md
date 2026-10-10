# ytdown

Desktop app (Tauri v2) for downloading YouTube video or audio by pasting a link.

## Install

Releases are on the GitHub Releases page (macOS, Windows, Ubuntu).

- macOS/Windows builds may be unsigned until signing certificates are configured.
  On macOS 15+, open System Settings → Privacy & Security and click "Open Anyway"
  after the first blocked launch of an unsigned build.
  On Windows, if SmartScreen blocks the installer, click "More info → Run anyway".
- On first run the app downloads yt-dlp, ffmpeg and deno into the app data folder
  (macOS `~/Library/Application Support/com.tramamte.ytdown/bin`). yt-dlp updates are
  checked in the background and applied on the next launch (Settings → Update applies now).
- On Ubuntu the global shortcut works on X11 only (not Wayland).

## Use

- Paste a YouTube link and press Enter. Turn on **Audio only** (⌘⇧A / Ctrl+Shift+A)
  to get audio, or press Shift+Enter to get audio just this once.
- Global shortcut to bring the window up: ⌃⌥Y on macOS, Ctrl+Alt+Y elsewhere
  (change it in Settings by pressing the new keys).
- Settings: Ctrl+, (⌘, on macOS); ⌘S / Ctrl+S saves, Esc closes.
- By default files keep YouTube's original streams (no re-encoding). For QuickTime-friendly
  MP4, choose Video codec H.264 and Audio track codec AAC.

## Build locally

```
npm ci
npx tauri dev
npx tauri build
```

## Icons

Three sources: `app-icon.png` (rounded, transparent margin — Linux and the in-app logo),
`app-icon-mac.png` (full-bleed square; macOS applies its own mask) and `app-icon-win.png`
(`app-icon-mac.png` with 22.5% rounded corners, no margin — matches other Windows taskbar icons). To regenerate:

```
npx tauri icon app-icon.png                       # all platforms
npx tauri icon app-icon-mac.png -o /tmp/mac-icons # macOS only
cp /tmp/mac-icons/icon.icns src-tauri/icons/icon.icns
npx tauri icon app-icon-win.png -o /tmp/win-icons # Windows only
cp /tmp/win-icons/icon.ico src-tauri/icons/icon.ico
cp src-tauri/icons/128x128.png src/logo.png
rm -rf src-tauri/icons/{android,ios,Square*Logo.png,StoreLogo.png,64x64.png}
```

## License and legal

- ffmpeg is licensed under the GPL; it is downloaded at runtime, not bundled.
- The UI font is Inter (SIL Open Font License 1.1), bundled in `src/fonts/` with its license.
- You are responsible for complying with YouTube's Terms of Service and with
  copyright law when downloading content.
