# ytdown

Desktop app (Tauri v2) for downloading YouTube video or audio by pasting a link.

## Install

Releases are on the GitHub Releases page (macOS, Windows, Ubuntu).

- macOS/Windows builds may be unsigned until signing certificates are configured.
  On macOS 15+, open System Settings → Privacy & Security and click "Open Anyway"
  after the first blocked launch of an unsigned build.
  On Windows, if SmartScreen blocks the installer, click "More info → Run anyway".
- On first run the app downloads yt-dlp, ffmpeg and deno into the app data folder.
- On Ubuntu the global shortcut works on X11 only (not Wayland).

## Build locally

```
npm ci
npx tauri dev
npx tauri build
```

## License and legal

- ffmpeg is licensed under the GPL; it is downloaded at runtime, not bundled.
- You are responsible for complying with YouTube's Terms of Service and with
  copyright law when downloading content.
