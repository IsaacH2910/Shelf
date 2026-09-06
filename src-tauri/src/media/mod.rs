use crate::db::Database;
use crate::models::{ContentType, SourceFormat, Viewer};
use crate::pdf::{self, PageCache};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tauri::http::{header, HeaderMap, HeaderValue, StatusCode};

const MAX_UNRANGED_BYTES: u64 = 32 * 1024 * 1024;
const DEFAULT_RANGE_WINDOW: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaKind {
    pub content_type: ContentType,
    pub source_format: SourceFormat,
    pub supported: bool,
}

pub fn is_pdf_path(path: &str) -> bool {
    path.to_lowercase().ends_with(".pdf")
}

pub fn is_mp4_path(path: &str) -> bool {
    path.to_lowercase().ends_with(".mp4")
}

pub fn is_supported_media_path(path: &str) -> bool {
    is_pdf_path(path) || is_mp4_path(path)
}

pub fn mime_for_path(path: &str) -> &'static str {
    if is_mp4_path(path) {
        "video/mp4"
    } else if is_pdf_path(path) {
        "application/pdf"
    } else {
        "application/octet-stream"
    }
}

/// Filename-based classification used by both indexing and the reader.
pub fn detect_media_kind(path: &str) -> MediaKind {
    let lower = path.to_lowercase();
    if lower.ends_with(".mp4") {
        return MediaKind {
            content_type: ContentType::Video,
            source_format: SourceFormat::Mp4,
            supported: true,
        };
    }
    if lower.ends_with(".pdf") {
        return MediaKind {
            content_type: if pdf_looks_like_manga(path) {
                ContentType::Manga
            } else {
                ContentType::Document
            },
            source_format: SourceFormat::Pdf,
            supported: true,
        };
    }
    MediaKind {
        content_type: ContentType::Document,
        source_format: SourceFormat::Pdf,
        supported: false,
    }
}

fn pdf_looks_like_manga(path: &str) -> bool {
    let file_name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_lowercase();
    let label = file_name.trim_start_matches('.');
    regex_is_match(
        r"(^|[/\-_ ])(ch|chapter|chap|vol|volume|episode|ep)[\-_ ]?\d",
        label,
    ) || regex_is_match(r"\b[0-9]{1,3}\b", label)
        || regex_is_match(r"第\s*\d", label)
        || (regex_is_match(r"[話话回]", label) && regex_is_match(r"\d", label))
}

fn regex_is_match(pattern: &str, haystack: &str) -> bool {
    regex::Regex::new(pattern)
        .map(|re| re.is_match(haystack))
        .unwrap_or(false)
}

pub fn infer_series_media(files: &[PathBuf]) -> Option<(ContentType, SourceFormat)> {
    let mut pdf_kinds = Vec::new();
    let mut mp4_count = 0usize;

    for path in files {
        let kind = detect_media_kind(&path.to_string_lossy());
        if !kind.supported {
            continue;
        }
        match kind.source_format {
            SourceFormat::Mp4 => mp4_count += 1,
            SourceFormat::Pdf => pdf_kinds.push(kind.content_type),
        }
    }

    let pdf_count = pdf_kinds.len();
    if pdf_count == 0 && mp4_count == 0 {
        return None;
    }
    if mp4_count > pdf_count {
        return Some((ContentType::Video, SourceFormat::Mp4));
    }
    if pdf_count > mp4_count {
        let mangaish = pdf_kinds.iter().any(|kind| {
            matches!(
                kind,
                ContentType::Manga | ContentType::Manhwa | ContentType::Comic
            )
        });
        let content_type = if mangaish {
            ContentType::Manga
        } else {
            ContentType::Document
        };
        return Some((content_type, SourceFormat::Pdf));
    }
    None
}

pub fn resolve_chapter_file(db: &Database, viewer: &Viewer, chapter_id: i64) -> Result<PathBuf, String> {
    let chapter = db.get_chapter(viewer, chapter_id).map_err(|e| e.to_string())?;
    if chapter.missing {
        return Err("Chapter file is missing".into());
    }
    let path = PathBuf::from(&chapter.file_path);
    if !path.is_file() {
        return Err("Chapter file is missing".into());
    }
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    let roots = db.list_roots().map_err(|e| e.to_string())?;
    let allowed = roots.iter().any(|root| {
        let root_path = PathBuf::from(&root.path);
        let root_canon = root_path.canonicalize().unwrap_or(root_path);
        canonical.starts_with(&root_canon)
    });
    if !allowed {
        return Err("File is not inside a library folder".into());
    }
    Ok(canonical)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

pub fn parse_byte_range(header: Option<&str>, file_size: u64) -> Option<ByteRange> {
    if file_size == 0 {
        return None;
    }
    let header = header?.trim();
    let spec = header.strip_prefix("bytes=")?.trim();
    let spec = spec.split(',').next()?.trim();
    if spec.is_empty() {
        return None;
    }
    let last = file_size - 1;
    if let Some(suffix) = spec.strip_prefix('-') {
        let n: u64 = suffix.parse().ok()?;
        if n == 0 {
            return None;
        }
        let start = file_size.saturating_sub(n);
        return Some(ByteRange { start, end: last });
    }
    let (start_raw, end_raw) = spec.split_once('-')?;
    let start: u64 = start_raw.parse().ok()?;
    if start > last {
        return None;
    }
    let end = if end_raw.is_empty() {
        last
    } else {
        end_raw.parse::<u64>().ok()?.min(last)
    };
    if end < start {
        return None;
    }
    Some(ByteRange { start, end })
}

pub struct FileRangePlan {
    pub status: StatusCode,
    pub start: u64,
    pub end: u64,
    pub length: u64,
    pub file_size: u64,
    pub mime: &'static str,
}

pub fn plan_file_range(path: &Path, range_header: Option<&str>) -> Result<FileRangePlan, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let file_size = meta.len();
    let mime = mime_for_path(&path.to_string_lossy());
    let requested = parse_byte_range(range_header, file_size);
    let last_byte = file_size.saturating_sub(1);
    let (status, start, end) = match requested {
        Some(range) => (StatusCode::PARTIAL_CONTENT, range.start, range.end),
        None if file_size > MAX_UNRANGED_BYTES => (
            StatusCode::PARTIAL_CONTENT,
            0,
            DEFAULT_RANGE_WINDOW.saturating_sub(1).min(last_byte),
        ),
        None => (StatusCode::OK, 0, last_byte),
    };
    let length = if file_size == 0 {
        0
    } else {
        end.saturating_sub(start) + 1
    };
    Ok(FileRangePlan {
        status,
        start,
        end,
        length,
        file_size,
        mime,
    })
}

pub fn range_headers(plan: &FileRangePlan) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(plan.mime));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&plan.length.to_string()).unwrap_or(HeaderValue::from_static("0")),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=0, must-revalidate"),
    );
    if plan.status == StatusCode::PARTIAL_CONTENT {
        let content_range = format!("bytes {}-{}/{}", plan.start, plan.end, plan.file_size);
        if let Ok(value) = HeaderValue::from_str(&content_range) {
            headers.insert(header::CONTENT_RANGE, value);
        }
    }
    headers
}

pub struct FileBody {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Vec<u8>,
}

pub fn read_file_range(path: &Path, range_header: Option<&str>) -> Result<FileBody, String> {
    let plan = plan_file_range(path, range_header)?;
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut body = vec![0u8; plan.length as usize];
    if plan.length > 0 {
        file.seek(SeekFrom::Start(plan.start)).map_err(|e| e.to_string())?;
        file.read_exact(&mut body).map_err(|e| e.to_string())?;
    }
    Ok(FileBody {
        status: plan.status,
        headers: range_headers(&plan),
        body,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolTarget {
    File { chapter_id: i64 },
    Page {
        chapter_id: i64,
        page_index: i32,
        width: u32,
    },
    Tile {
        chapter_id: i64,
        page_index: i32,
        tile_index: u32,
        width: u32,
    },
}

pub fn protocol_response(
    db: &Database,
    cache: &PageCache,
    request: &tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    if request.method() == tauri::http::Method::OPTIONS {
        return tauri::http::Response::builder()
            .status(StatusCode::NO_CONTENT)
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "range")
            .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "content-range, accept-ranges, content-length, content-type")
            .body(Vec::new())
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()));
    }

    let path = request.uri().path();
    let query = request.uri().query();
    let Some(target) = parse_protocol_target(path, query) else {
        tracing::warn!(uri = %request.uri(), "Invalid shelf-media URL");
        return error_response(StatusCode::BAD_REQUEST, "Invalid media URL");
    };

    match target {
        ProtocolTarget::Page {
            chapter_id,
            page_index,
            width,
        } => page_protocol_response(db, cache, chapter_id, page_index, width),
        ProtocolTarget::Tile {
            chapter_id,
            page_index,
            tile_index,
            width,
        } => tile_protocol_response(db, cache, chapter_id, page_index, tile_index, width),
        ProtocolTarget::File { chapter_id } => file_protocol_response(db, request, chapter_id),
    }
}

fn file_protocol_response(
    db: &Database,
    request: &tauri::http::Request<Vec<u8>>,
    chapter_id: i64,
) -> tauri::http::Response<Vec<u8>> {
    let file_path = match resolve_chapter_file(db, &crate::models::Viewer::SYSTEM, chapter_id) {
        Ok(path) => path,
        Err(error) => return error_response(StatusCode::NOT_FOUND, &error),
    };
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok());
    match read_file_range(&file_path, range) {
        Ok(body) => {
            let mut builder = tauri::http::Response::builder().status(body.status);
            for (name, value) in body.headers.iter() {
                builder = builder.header(name, value);
            }
            builder = builder.header(
                header::ACCESS_CONTROL_EXPOSE_HEADERS,
                "content-range, accept-ranges, content-length, content-type",
            );
            builder
                .body(body.body)
                .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "Response failed"))
        }
        Err(error) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &error),
    }
}

fn page_protocol_response(
    db: &Database,
    cache: &PageCache,
    chapter_id: i64,
    page_index: i32,
    width: u32,
) -> tauri::http::Response<Vec<u8>> {
    let file_path = match resolve_chapter_file(db, &crate::models::Viewer::SYSTEM, chapter_id) {
        Ok(path) => path,
        Err(error) => return error_response(StatusCode::NOT_FOUND, &error),
    };
    if !is_pdf_path(&file_path.to_string_lossy()) {
        return error_response(StatusCode::BAD_REQUEST, "Page rendering is only available for PDF files");
    }
    let width = width.clamp(100, 2560);
    match pdf::render_page_bytes(cache, chapter_id, &file_path.to_string_lossy(), page_index, width) {
        Ok((bytes, w, h, _)) => {
            tauri::http::Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, pdf::image_mime(&bytes))
                .header(header::CONTENT_LENGTH, bytes.len().to_string())
                .header(header::CACHE_CONTROL, "private, max-age=86400")
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .header("x-image-width", w.to_string())
                .header("x-image-height", h.to_string())
                .body(bytes)
                .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "Response failed"))
        }
        Err(error) => {
            tracing::warn!(chapter_id, page_index, width, "PDF page render failed: {error}");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
        }
    }
}

fn tile_protocol_response(
    db: &Database,
    cache: &PageCache,
    chapter_id: i64,
    page_index: i32,
    tile_index: u32,
    width: u32,
) -> tauri::http::Response<Vec<u8>> {
    let file_path = match resolve_chapter_file(db, &crate::models::Viewer::SYSTEM, chapter_id) {
        Ok(path) => path,
        Err(error) => return error_response(StatusCode::NOT_FOUND, &error),
    };
    if !is_pdf_path(&file_path.to_string_lossy()) {
        return error_response(StatusCode::BAD_REQUEST, "Page rendering is only available for PDF files");
    }
    match pdf::render_tile_bytes(
        cache,
        chapter_id,
        &file_path.to_string_lossy(),
        page_index,
        tile_index,
        width,
        true,
    ) {
        Ok((bytes, w, h, _)) => tauri::http::Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, pdf::image_mime(&bytes))
            .header(header::CONTENT_LENGTH, bytes.len().to_string())
            .header(header::CACHE_CONTROL, "private, max-age=86400")
            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "x-image-width, x-image-height")
            .header("x-image-width", w.to_string())
            .header("x-image-height", h.to_string())
            .body(bytes)
            .unwrap_or_else(|_| error_response(StatusCode::INTERNAL_SERVER_ERROR, "Response failed")),
        Err(error) => {
            tracing::warn!(
                chapter_id,
                page_index,
                tile_index,
                width,
                "PDF segment render failed: {error}"
            );
            error_response(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
        }
    }
}

pub fn parse_protocol_target(path: &str, query: Option<&str>) -> Option<ProtocolTarget> {
    let decoded = percent_decode_path(path);
    let trimmed = decoded.trim_matches('/');
    let query_width = query.and_then(width_from_query);

    for (prefix, separator) in [("tile-", '-'), ("tile/", '/')] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let mut parts = rest.split(separator);
            let chapter_id = parts.next()?.parse().ok()?;
            let page_index = parts.next()?.parse().ok()?;
            let tile_index = parts.next()?.parse().ok()?;
            let width = parts
                .next()
                .and_then(|value| value.parse().ok())
                .or(query_width)
                .unwrap_or(1200);
            return Some(ProtocolTarget::Tile {
                chapter_id,
                page_index,
                tile_index,
                width: width.clamp(100, 2560),
            });
        }
    }

    if let Some(rest) = trimmed.strip_prefix("page-") {
        let mut parts = rest.split('-');
        let chapter_id = parts.next()?.parse().ok()?;
        let page_index = parts.next()?.parse().ok()?;
        let width = parts
            .next()
            .and_then(|value| value.parse().ok())
            .or(query_width)
            .unwrap_or(1400);
        return Some(ProtocolTarget::Page {
            chapter_id,
            page_index,
            width: width.clamp(100, 2560),
        });
    }

    if let Some(rest) = trimmed.strip_prefix("page/") {
        let mut parts = rest.split('/');
        let chapter_id = parts.next()?.parse().ok()?;
        let page_index = parts.next()?.parse().ok()?;
        let width = parts
            .next()
            .and_then(|value| value.parse().ok())
            .or(query_width)
            .unwrap_or(1400);
        return Some(ProtocolTarget::Page {
            chapter_id,
            page_index,
            width: width.clamp(100, 2560),
        });
    }

    let chapter_id = parse_chapter_id(trimmed)?;
    Some(ProtocolTarget::File { chapter_id })
}

fn width_from_query(query: &str) -> Option<u32> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        if key == "width" || key == "w" {
            value.parse().ok()
        } else {
            None
        }
    })
}

fn percent_decode_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_chapter_id(path: &str) -> Option<i64> {
    let trimmed = path.trim_matches('/');
    let id = trimmed
        .strip_prefix("chapter/")
        .unwrap_or(trimmed)
        .split('/')
        .next()?;
    id.parse().ok()
}

fn error_response(status: StatusCode, message: &str) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(message.as_bytes().to_vec())
        .unwrap_or_else(|_| tauri::http::Response::new(message.as_bytes().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_pdf_and_mp4() {
        let pdf = detect_media_kind("/lib/Notes/report.pdf");
        assert_eq!(pdf.content_type, ContentType::Document);
        assert_eq!(pdf.source_format, SourceFormat::Pdf);
        assert!(pdf.supported);

        let manga = detect_media_kind("/lib/One Piece/Chapter 001.pdf");
        assert_eq!(manga.content_type, ContentType::Manga);

        let video = detect_media_kind("/lib/Show/Episode 01.mp4");
        assert_eq!(video.content_type, ContentType::Video);
        assert_eq!(video.source_format, SourceFormat::Mp4);
    }

    #[test]
    fn infers_series_from_files() {
        let videos = vec![
            PathBuf::from("S1/01.mp4"),
            PathBuf::from("S1/02.mp4"),
            PathBuf::from("bonus.pdf"),
        ];
        assert_eq!(
            infer_series_media(&videos),
            Some((ContentType::Video, SourceFormat::Mp4))
        );

        let docs = vec![PathBuf::from("manual.pdf"), PathBuf::from("appendix.pdf")];
        assert_eq!(
            infer_series_media(&docs),
            Some((ContentType::Document, SourceFormat::Pdf))
        );

        let manga = vec![PathBuf::from("Chapter 01.pdf"), PathBuf::from("Chapter 02.pdf")];
        assert_eq!(
            infer_series_media(&manga),
            Some((ContentType::Manga, SourceFormat::Pdf))
        );
    }

    #[test]
    fn parses_byte_ranges() {
        assert_eq!(
            parse_byte_range(Some("bytes=0-1"), 1000),
            Some(ByteRange { start: 0, end: 1 })
        );
        assert_eq!(
            parse_byte_range(Some("bytes=10-"), 100),
            Some(ByteRange { start: 10, end: 99 })
        );
        assert_eq!(
            parse_byte_range(Some("bytes=-10"), 100),
            Some(ByteRange { start: 90, end: 99 })
        );
        assert_eq!(parse_byte_range(Some("bytes=500-10"), 100), None);
    }

    #[test]
    fn parses_protocol_chapter_ids() {
        assert_eq!(parse_chapter_id("/12"), Some(12));
        assert_eq!(parse_chapter_id("chapter/12"), Some(12));
        assert_eq!(parse_chapter_id("/chapter/12/"), Some(12));
        assert_eq!(parse_chapter_id("nope"), None);
    }

    #[test]
    fn parses_protocol_page_and_file_targets() {
        assert_eq!(
            parse_protocol_target("/page-12-0-1400", None),
            Some(ProtocolTarget::Page {
                chapter_id: 12,
                page_index: 0,
                width: 1400,
            })
        );
        assert_eq!(
            parse_protocol_target("/page/12/2/800", None),
            Some(ProtocolTarget::Page {
                chapter_id: 12,
                page_index: 2,
                width: 800,
            })
        );
        assert_eq!(
            parse_protocol_target("/page%2F12%2F1%2F1200", None),
            Some(ProtocolTarget::Page {
                chapter_id: 12,
                page_index: 1,
                width: 1200,
            })
        );
        assert_eq!(
            parse_protocol_target("/page-9-0", Some("width=1600")),
            Some(ProtocolTarget::Page {
                chapter_id: 9,
                page_index: 0,
                width: 1600,
            })
        );
        assert_eq!(
            parse_protocol_target("/18", None),
            Some(ProtocolTarget::File { chapter_id: 18 })
        );
    }

    #[test]
    fn parses_protocol_tile_targets() {
        assert_eq!(
            parse_protocol_target("/tile-12-1-7-1200", None),
            Some(ProtocolTarget::Tile {
                chapter_id: 12,
                page_index: 1,
                tile_index: 7,
                width: 1200,
            })
        );
        assert_eq!(
            parse_protocol_target("/tile%2F12%2F0%2F3%2F900", None),
            Some(ProtocolTarget::Tile {
                chapter_id: 12,
                page_index: 0,
                tile_index: 3,
                width: 900,
            })
        );
        assert_eq!(
            parse_protocol_target("/tile-4-0-0", Some("width=1600")),
            Some(ProtocolTarget::Tile {
                chapter_id: 4,
                page_index: 0,
                tile_index: 0,
                width: 1600,
            })
        );
        // A tile URL without a segment index is not a valid tile request.
        assert_eq!(parse_protocol_target("/tile-4-0", None), None);
    }

    #[test]
    fn range_reads_a_file_with_spaces_and_sets_mp4_mime() {
        let dir = std::env::temp_dir().join("shelf-media-smoke");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Episode 01.mp4");
        std::fs::write(&path, b"0123456789abcdef").unwrap();
        let body = read_file_range(&path, Some("bytes=4-7")).unwrap();
        assert_eq!(body.status, StatusCode::PARTIAL_CONTENT);
        assert_eq!(body.body, b"4567");
        assert_eq!(
            body.headers.get(header::CONTENT_TYPE).unwrap(),
            "video/mp4"
        );
        assert_eq!(
            body.headers.get(header::CONTENT_RANGE).unwrap(),
            "bytes 4-7/16"
        );
    }
}
