# Contributing

Thanks for taking an interest in Shelf. This document is how to build, test, and send changes.

## Development setup

Prerequisites are listed in the [README](README.md#requirements).

```bash
npm install
npm run tauri dev
```

`npm install` runs `scripts/fetch-pdfium.mjs`, which downloads the PDFium build that matches `pdfium-render` 0.8 (`chromium/7543`) into `src-tauri/resources/pdfium/`. Re-run it with `npm run pdfium` if that library is missing.

The Vite dev server binds to `127.0.0.1:1420`. The desktop window loads that URL during `tauri dev`.

If you enable cloud access while running from source, build the web client first so the tunnel can serve it:

```bash
npm run build
```

## Project layout

```text
src/                     React 19 + TypeScript UI
src/lib/                 Frontend API client, library helpers, shortcuts
src/pages/               Routes (Home, Library, Reader, Settings, …)
src-tauri/src/           Rust / Tauri backend
src-tauri/src/db/        SQLite schema and queries
src-tauri/src/http/      Loopback Axum API for the web client
src-tauri/src/pdf/       PDFium worker and page cache
src-tauri/src/jobs/      Priority job scheduler
scripts/                 PDFium fetch and other tooling
```

## Tests

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
```

`npm test` runs the TypeScript tests under `src/` with Node’s built-in test runner. Rust tests live next to the modules they cover (`auth`, `tunnel`, OCR cleanup, chapter parsing, and others).

## Coding notes

- Keep originals untouched. Indexing, OCR, and translation must not rewrite library files.
- The file extension is the source of truth for the viewer: `.pdf` never goes to the video player, `.mp4` never goes to PDFium.
- Desktop commands go through Tauri `invoke`. The iPhone/iPad client talks to the loopback HTTP API over the tunnel and uses cookie sessions.
- Do not commit secrets, tunnel tokens, keychain material, or personal hostnames. Cloud credentials belong in the macOS keychain or your own local config, not the repository.
- Do not commit `node_modules/`, `dist/`, or `src-tauri/target/`.

## Pull requests

1. Keep the change focused. One concern per PR.
2. Update docs when behavior users rely on changes.
3. Add or adjust tests when you change parsing, shortcuts, auth, or library helpers.
4. Run the test commands above before you open the PR.

Describe the *why* in the PR body, and include a short test plan for UI changes.
