# Library

Shelf indexes files that already exist on disk. It never moves, renames, or rewrites them.

## Add a folder

**Settings → Add folder** registers a library root. Shelf walks that tree, creates a series for each immediate subfolder that contains supported files, and watches for later changes.

**Rescan** rebuilds the index from the current roots without deleting reading progress for files that are still there.

Removing a root drops that folder from the index. The files on disk are left alone.

## Supported files

| Extension | Role |
| --- | --- |
| `.pdf` | Manga, manhwa, comics, books, and documents |
| `.mp4` | Local video |

Other extensions are ignored. Classification uses the path and filename (chapter/volume markers, “episode”, “movie”, and similar hints). The file extension still decides the viewer: a PDF never opens in the video player, and an MP4 never goes through PDFium.

## Folder layout

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

Rules:

- Each series is a folder under a library root.
- Supported files are discovered recursively, so season, volume, and extras subfolders are fine.
- A single movie can be one `.mp4` inside its own folder.

## Chapter names and order

Filenames are parsed for chapter and volume numbers. Recognized patterns include:

- `Ch 12`, `Chapter 12`, `Ep 3`, `Episode 03`, `第12話`
- `S1E03` / `S1 E03`
- A leading number (`01 - title`, `1. title`)
- `Vol 3` / `v03` (volume, used when there is no chapter number)

Unnumbered files sort after numbered ones. Alternate versions of the same chapter stay grouped together.

## Chapter variants

When several files represent the same chapter (same chapter or volume number), Shelf treats extras as variants of the primary file.

| Preference | Typical filename hints |
| --- | --- |
| Primary | The first file in sort order |
| Uncensored | 无修正 / 無修正 / 无码 / 無碼 / 去码 / 去碼 |
| Revised | 修正 / 修復 / revised / fix |
| Bonus | extra / bonus / 番外 / 補圖 / 外傳 |
| Alternate | Any other extra in the group |

Set the series default in the series page. In the reader, use **Prefer current**, or press `V` / `Shift+V` to cycle.

## Reading modes

| Mode | Behavior |
| --- | --- |
| Auto | Webtoon when the series is LTR; paged RTL otherwise |
| Webtoon | Continuous vertical scroll (tall pages are split into stacked segments) |
| Paged (LTR) | One page at a time, left-to-right |
| Paged (RTL) | One page at a time, right-to-left |

Video always uses the full-window player. Progress stores a timestamp instead of a page index.

## Reader shortcuts

| Keys | Action |
| --- | --- |
| Arrows, Page Up/Down, Space | Page / scroll |
| Home / End | Jump to start or end |
| Esc | Close the reader |
| F | Fullscreen |
| T | Translation overlay |
| V / Shift+V | Next / previous chapter variant |

Shortcuts are ignored while a text field is focused.

## Translation

Press `T` to run OCR on the current page and show an overlay. On macOS the default engine is Apple Vision. Simplified Chinese can be converted to Traditional with OpenCC. Apple Translation is used when the OS provides it.

Results are cached in `shelf.db`. The source PDF is not modified. Overlay edits stay in the app database.

## Cache

Rendered pages are stored under the app data directory. **Settings → Reader** sets the budget (64–8192 MB) and can clear the cache. Eviction prefers pages outside the current read window.
