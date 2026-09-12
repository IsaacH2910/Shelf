# Architecture

Shelf is a Tauri 2 desktop app: a React UI in the main window, and a Rust backend that owns the library, renderer, jobs, and optional cloud origin.

```text
┌─────────────────────────────────────────────────────────────┐
│  macOS desktop (Tauri)                                      │
│    React 19 + TypeScript + Tailwind CSS v4                  │
│    invoke() ───────────────────────────────────────────┐    │
│                                                        ▼    │
│    Rust: SQLite · PDFium worker · job queue · watcher       │
│                        │                                    │
│                        ▼                                    │
│    Axum 127.0.0.1:7834  ←── cloudflared (optional)          │
│    or 0.0.0.0:7834 + Bonjour _shelf._tcp (nearby session)   │
└─────────────────────────────────────────────────────────────┘
         ▲
         │ 1 Bonjour  2 LAN HTTP  3 Cloudflare HTTPS
  iPhone / iPad native shell · browsers on the same Wi-Fi
```

## Frontend

| Piece | Choice |
| --- | --- |
| UI | React 19, TypeScript, Tailwind CSS v4 |
| Routing | `react-router-dom` with a hash router (works as a static SPA behind the tunnel) |
| Install | Web app manifest + Apple Home Screen meta (Cloudflare / LAN URL). Native iOS shell in `ios/Shelf` is the Bonjour client |
| Virtualization | `@tanstack/react-virtual` for long chapter lists and tall webtoon stacks |
| Desktop bridge | `@tauri-apps/api` plus the dialog and opener plugins |

Routes live under `src/pages/`. The same bundle is the remote reader: `src/lib/api.ts` calls Tauri `invoke` in the desktop window and `/api/*` over `fetch` in the browser.

## Backend

Rust crate: `src-tauri/`. Main modules:

| Module | Responsibility |
| --- | --- |
| `db` | SQLite (`shelf.db`), WAL, schema migrations, grants, progress |
| `index` | Recursive scan, series/chapter creation, filename parsing |
| `watch` | `notify` folder watcher; file changes enqueue index jobs |
| `pdf` | PDFium on a dedicated worker thread; covers, tiles, page cache |
| `media` | PDF vs MP4 classification, range-served video |
| `jobs` | Priority scheduler |
| `ocr` / `translate` | Apple Vision, OpenCC / zhconv, Apple Translation |
| `auth` | Argon2id, sessions, lockout |
| `http` | Axum API and static SPA (loopback, or all interfaces during a nearby session) |
| `lan` | Nearby session helpers, Bonjour advertise (`mdns-sd`), host allow-list |
| `tunnel` | Supervises `cloudflared tunnel run --token …` |
| `keep_awake` | Process-scoped `caffeinate -ims` while Cloud is on (no admin prompt) |

## Job priorities

Highest priority runs first:

1. Interactive (the page the reader is waiting on)
2. Prefetch
3. Indexing
4. Covers
5. OCR / translation
6. Cache eviction

Dev builds optimize image crates (`opt-level` 2–3) so JPEG decode and resize stay usable while the app crate itself remains debuggable.

## HTTP API

The origin is loopback-only unless **Connect iPhone / iPad** is on, in which case it also listens on the LAN and advertises `_shelf._tcp`. Public routes:

- `GET /api/health`
- `POST /api/auth/login`

Everything else under `/api` requires a valid session cookie. The rest of the URL space serves the built SPA (`dist/` or the bundled Resources folder), with a host allow-list and HTTPS redirect for non-loopback hosts.

Representative protected routes:

- Library: series, chapters, continue, history, collections
- Reader: page images, webtoon tiles, chapter file / range, progress, adjacent chapter, translation
- Uploads: init, chunk `PUT`, complete
- Auth: session, logout

Security headers include a locked-down CSP. Cookies are HttpOnly.

## Data on disk

```text
~/Library/Application Support/com.isaach.Shelf/
  shelf.db          library, users, sessions, grants, progress, OCR cache
  cache/            rendered page images
```

Library files stay in the folders you added. The database stores paths, not copies.

## Identity

- One owner account (`owner`), created with the password you set in Settings
- Household members with optional per-series grants
- Progress and history are scoped to the signed-in user
- The desktop window uses a privileged local viewer and does not go through the cookie gate

## Rendering

PDFium renders pages off the UI thread. Tall webtoon pages are split into stacked segments (`PageTile`) so the browser never has to display a single tens-of-thousands-of-pixels image. OCR runs per segment; region coordinates are mapped back to whole-page space for the overlay.

Video is streamed with HTTP range requests from the local file.

## Tests

- TypeScript: `src/**/*.test.ts` via `npm test`
- Rust: unit tests in `auth`, `tunnel`, `ocr`, `index/chapter_parser`, `lib`, and related modules via `cargo test --manifest-path src-tauri/Cargo.toml`
