# Changelog

All notable changes to Shelf are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versioning follows [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Installable web reader: Home Screen manifest, Apple meta tags, and PNG icons so iPhone and iPad can Add to Home Screen from the Cloudflare URL
- Collections in the phone bottom navigation
- Safe-area padding for the mobile tab bar, login, and reader chrome

### Changed

- Remote series page hides Favorite / reading-mode / version-preference controls that only work on the Mac
- Remote sidebar no longer shows stubbed “0 series · 0 chapters” counts

## [0.1.0] — 2026-09-05

First public release.

### Added

- Local-first macOS library for PDFs and local MP4 video, indexed in place
- Home, Library, Search, Activity, collections, tags, and favorites
- Content-aware readers: continuous vertical (webtoon), paged LTR/RTL, and a full-window video player
- Per-series chapter variant preference with in-reader “Prefer current” and `V` / `Shift+V` cycling
- Resume at page, scroll offset, or video timestamp
- Folder watcher and background job queue (interactive render, prefetch, indexing, covers, OCR, cache eviction)
- Optional HTTPS cloud access through a Cloudflare named tunnel to a loopback origin; hostname and token are entered in Settings and stored locally (SQLite + keychain), not in source
- Household accounts with Argon2id passwords, HttpOnly session cookies, and per-user library grants
- Device session list with revoke
- Apple Vision OCR, OpenCC Simplified → Traditional conversion, and Apple Translation when available
- Translation overlay that never writes back to source files
- In-app “What’s new” dialog, shown once per version and reopenable from Settings
- Web and desktop icons generated from `public/shelf.svg`

### Notes

- The bundle identifier stays `com.isaach.shelf` so existing local libraries keep working.
