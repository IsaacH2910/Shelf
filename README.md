# Shelf

A local-first personal media library for macOS. Shelf indexes the PDFs and local video already on your Mac, without moving or modifying the originals, and lets you read or watch them on the desktop — or sign in from an iPhone or iPad over HTTPS.

The desktop app works fully offline. Cloud access is optional.

## Features

- Cover-based library with Home, Activity (Continue / Favorites), collections, tags, and search
- In-place indexing with a folder watcher — originals are never moved, renamed, or rewritten
- Mixed-format support for PDFs and local MP4 video, with content-aware reading behavior
- Continuous vertical reading for manga and manhwa, paged reading for documents, and a full-window player for video
- Per-series chapter variant preference (Primary / Uncensored / Revised / Bonus / Alternate), including “Prefer current” in the reader and keyboard cycling with `V` / `Shift+V`
- Resume at the exact page, scroll offset, or video timestamp, synced across the Mac and signed-in devices
- Optional personal cloud: sign in over HTTPS from any network (home Wi-Fi, another Wi-Fi, or cellular), then Add to Home Screen on iPhone and iPad for a standalone reader
- Household accounts with per-user library grants and separate reading progress
- On-device OCR via Apple Vision; Simplified → Traditional conversion with OpenCC; Apple Translation when the OS provides it
- Translation overlay (`T`) that never writes back to the source file

## Requirements

- macOS (Apple Vision, Apple Translation, and the current bundle are macOS-only)
- [Node.js](https://nodejs.org/) 18 or later
- [Rust](https://rustup.rs/) 1.77 or later
- [Tauri 2 macOS prerequisites](https://v2.tauri.app/start/prerequisites/)

Cloud access also needs [`cloudflared`](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) and a Cloudflare named tunnel. See [Cloud access](docs/cloud.md) for enablement, household accounts, and adding the reader to an iPhone or iPad Home Screen.

## Quick start

```bash
npm install          # also fetches PDFium via postinstall
npm run tauri dev    # desktop app
```

Add a library folder with **Settings → Add folder**. Shelf scans supported files recursively inside each series folder.

App data lives at:

```text
~/Library/Application Support/com.isaach.Shelf/
  shelf.db
  cache/
```

The bundle identifier is `com.isaach.shelf`. Keep it if you already have a library — changing it creates a new empty data directory.

## Library layout

```text
LibraryRoot/
  Series Name/
    Chapter 01.pdf
    Chapter 02.pdf
    v03.pdf
    Extras/
      Bonus 01.pdf
    S1/
      Episode 01.mp4
  Movie Title/
    movie.mp4
```

Supported extensions are `.pdf` and `.mp4`. Season, volume, and extras subfolders are fine. See [Library layout](docs/library.md) for naming, reading modes, and chapter variants.

## Documentation

| Guide | What it covers |
| --- | --- |
| [Library](docs/library.md) | Folder layout, formats, reading modes, variants, shortcuts |
| [Cloud access](docs/cloud.md) | HTTPS remote access, household accounts, devices |
| [Architecture](docs/architecture.md) | Frontend, Rust backend, jobs, HTTP API, data |
| [Contributing](CONTRIBUTING.md) | Development setup, tests, pull requests |
| [Security](SECURITY.md) | Threat model and vulnerability reporting |
| [Changelog](CHANGELOG.md) | Release notes |

## Brand assets

- Primary mark: `public/shelf.svg`
- Web favicon: `index.html` points at `/shelf.svg`
- PWA / Home Screen: `public/manifest.webmanifest`, `public/apple-touch-icon.png`, `public/pwa-*.png`
- After changing the SVG, regenerate desktop and Home Screen icons:

```bash
npm run tauri icon public/shelf.svg
python3 scripts/generate-pwa-icons.py
```

The Tauri command refreshes `src-tauri/icons/` (`icon.icns`, `icon.ico`, and platform variants). The Python script (Pillow) writes the 180 / 192 / 512 PNG icons used by Add to Home Screen.

## License

[MIT](LICENSE)

PDFium is fetched at install time to match the `pdfium-render` bindings and is subject to its own license.
