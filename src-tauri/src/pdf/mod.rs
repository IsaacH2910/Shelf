use parking_lot::Mutex;
use pdfium_render::prelude::*;
use image::ImageEncoder;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use thiserror::Error;
use tracing::warn;

static PDFIUM_SEARCH_PATHS: OnceLock<Vec<PathBuf>> = OnceLock::new();

pub fn set_search_paths(paths: Vec<PathBuf>) {
    let _ = PDFIUM_SEARCH_PATHS.set(paths);
}

#[derive(Error, Debug)]
pub enum PdfError {
    #[error("PDFium error: {0}")]
    Pdfium(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("{0}")]
    Other(String),
}

pub struct RenderResult {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

struct PdfiumChannels {
    urgent: crossbeam_channel::Sender<PdfRequest>,
    normal: crossbeam_channel::Sender<PdfRequest>,
}

static PDFIUM_THREAD: OnceLock<PdfiumChannels> = OnceLock::new();

enum PdfRequest {
    PageCount {
        path: String,
        reply: crossbeam_channel::Sender<Result<i32, PdfError>>,
    },
    RenderPage {
        path: String,
        page_index: i32,
        target_width: u32,
        reply: crossbeam_channel::Sender<Result<RenderResult, PdfError>>,
    },
    PageDimensions {
        path: String,
        page_index: i32,
        reply: crossbeam_channel::Sender<Result<(f64, f64), PdfError>>,
    },
    ChapterTiles {
        path: String,
        target_width: u32,
        reply: crossbeam_channel::Sender<Result<Vec<TileGeometry>, PdfError>>,
    },
    RenderPageTiles {
        path: String,
        page_index: i32,
        target_width: u32,
        reply: crossbeam_channel::Sender<Result<Vec<RenderResult>, PdfError>>,
    },
    RenderPageOcr {
        path: String,
        page_index: i32,
        reply: crossbeam_channel::Sender<Result<Vec<RenderResult>, PdfError>>,
    },
}

impl PdfRequest {
    fn reply_unavailable(self) {
        let err = || PdfError::Pdfium("PDFium not installed".into());
        match self {
            PdfRequest::PageCount { reply, .. } => {
                let _ = reply.send(Err(err()));
            }
            PdfRequest::RenderPage { reply, .. } => {
                let _ = reply.send(Err(err()));
            }
            PdfRequest::PageDimensions { reply, .. } => {
                let _ = reply.send(Err(err()));
            }
            PdfRequest::ChapterTiles { reply, .. } => {
                let _ = reply.send(Err(err()));
            }
            PdfRequest::RenderPageTiles { reply, .. } => {
                let _ = reply.send(Err(err()));
            }
            PdfRequest::RenderPageOcr { reply, .. } => {
                let _ = reply.send(Err(err()));
            }
        }
    }
}

fn handle_pdf_request(pdfium: &Pdfium, req: PdfRequest) {
    match req {
        PdfRequest::PageCount { path, reply } => {
            let _ = reply.send(page_count_inner(pdfium, &path));
        }
        PdfRequest::RenderPage {
            path,
            page_index,
            target_width,
            reply,
        } => {
            let _ = reply.send(render_page_inner(pdfium, &path, page_index, target_width));
        }
        PdfRequest::PageDimensions {
            path,
            page_index,
            reply,
        } => {
            let _ = reply.send(page_dimensions_inner(pdfium, &path, page_index));
        }
        PdfRequest::ChapterTiles {
            path,
            target_width,
            reply,
        } => {
            let _ = reply.send(chapter_tiles_inner(pdfium, &path, target_width));
        }
        PdfRequest::RenderPageTiles {
            path,
            page_index,
            target_width,
            reply,
        } => {
            let _ = reply.send(render_page_tiles_inner(pdfium, &path, page_index, target_width));
        }
        PdfRequest::RenderPageOcr {
            path,
            page_index,
            reply,
        } => {
            let _ = reply.send(render_page_ocr_inner(pdfium, &path, page_index));
        }
    }
}

fn pdfium_lib_names() -> &'static [&'static str] {
    &["libpdfium.dylib", "libpdfium.so", "pdfium.dll"]
}

fn pdfium_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(extra) = PDFIUM_SEARCH_PATHS.get() {
        dirs.extend(extra.iter().cloned());
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dirs.push(manifest_dir.join("resources/pdfium"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("resources/pdfium"));
            dirs.push(parent.join("pdfium"));
            dirs.push(parent.join("../Resources/resources/pdfium"));
            dirs.push(parent.join("../Resources/pdfium"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("src-tauri/resources/pdfium"));
        dirs.push(cwd.join("resources/pdfium"));
    }
    dirs
}

pub fn pdfium_lib_path() -> PathBuf {
    let names = pdfium_lib_names();
    for dir in pdfium_search_dirs() {
        for name in names {
            let candidate = dir.join(name);
            if candidate.exists() {
                return candidate;
            }
        }
        if dir.is_file() && is_pdfium_lib(&dir) {
            return dir;
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/pdfium/libpdfium.dylib")
}

fn is_pdfium_lib(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|name| pdfium_lib_names().iter().any(|expected| name.eq_ignore_ascii_case(expected)))
        .unwrap_or(false)
}

pub fn pdfium_available() -> bool {
    pdfium_lib_path().exists()
}

fn init_pdfium_thread() -> PdfiumChannels {
    let (urgent_tx, urgent_rx) = crossbeam_channel::unbounded::<PdfRequest>();
    let (normal_tx, normal_rx) = crossbeam_channel::unbounded::<PdfRequest>();

    std::thread::Builder::new()
        .name("pdfium-worker".into())
        .spawn(move || {
            let lib_path = pdfium_lib_path();
            let pdfium = match Pdfium::bind_to_library(&lib_path) {
                Ok(bindings) => Pdfium::new(bindings),
                Err(e) => {
                    warn!(
                        "PDFium not available at {:?}: {e}. PDF page rendering is disabled until a matching libpdfium is installed.",
                        lib_path
                    );
                    loop {
                        crossbeam_channel::select! {
                            recv(urgent_rx) -> req => { if let Ok(req) = req { req.reply_unavailable(); } }
                            recv(normal_rx) -> req => { if let Ok(req) = req { req.reply_unavailable(); } }
                        }
                    }
                }
            };

            loop {
                // Always drain reader (urgent) requests before background work.
                while let Ok(req) = urgent_rx.try_recv() {
                    handle_pdf_request(&pdfium, req);
                }
                match normal_rx.try_recv() {
                    Ok(req) => handle_pdf_request(&pdfium, req),
                    Err(_) => {
                        crossbeam_channel::select! {
                            recv(urgent_rx) -> req => { if let Ok(req) = req { handle_pdf_request(&pdfium, req); } }
                            recv(normal_rx) -> req => { if let Ok(req) = req { handle_pdf_request(&pdfium, req); } }
                        }
                    }
                }
            }
        })
        .expect("Failed to spawn PDFium thread");

    PdfiumChannels {
        urgent: urgent_tx,
        normal: normal_tx,
    }
}

fn pdfium_channels() -> &'static PdfiumChannels {
    PDFIUM_THREAD.get_or_init(init_pdfium_thread)
}

fn page_count_inner(pdfium: &Pdfium, path: &str) -> Result<i32, PdfError> {
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    Ok(doc.pages().len() as i32)
}

fn page_dimensions_inner(pdfium: &Pdfium, path: &str, page_index: i32) -> Result<(f64, f64), PdfError> {
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    let page = doc
        .pages()
        .get(page_index as u16)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    let w = page.width().value as f64;
    let h = page.height().value as f64;
    Ok((w, h))
}

/// Tallest thumbnail we produce, as a multiple of its width.
const COVER_MAX_ASPECT: f64 = 1.6;
/// Output height of a single stacked strip segment. Webtoon PDFs store one page as a
/// single image tens of thousands of pixels tall, which no browser will decode in one
/// piece, so pages are sliced into segments of this height and stacked flush.
const MAX_TILE_HEIGHT: u32 = 2400;
/// Ceiling for a whole-page PDFium rasterization before slicing. Keeps the intermediate
/// bitmap allocation bounded for pathological vector pages.
const MAX_RASTER_HEIGHT: u32 = 12000;
const MAX_PAGE_OUTPUT_HEIGHT: u32 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileGeometry {
    pub page_index: i32,
    pub tile_index: u32,
    pub width: u32,
    pub height: u32,
}

pub fn clamp_target_width(target_width: u32) -> u32 {
    target_width.clamp(100, 2560)
}

/// Full output size of a page at the requested width, preserving the page aspect ratio.
fn page_output_size(page_w_pts: f32, page_h_pts: f32, target_width: u32) -> (u32, u32) {
    let width = clamp_target_width(target_width);
    let ratio = (page_h_pts.max(1.0) / page_w_pts.max(1.0)) as f64;
    let height = ((width as f64 * ratio).round().max(1.0) as u32).min(MAX_PAGE_OUTPUT_HEIGHT);
    (width, height)
}

/// Splits a page's output height into stacked segments. Returns (y_offset, height) pairs.
fn tile_layout(output_height: u32) -> Vec<(u32, u32)> {
    let mut tiles = Vec::new();
    if output_height == 0 {
        return tiles;
    }
    let mut y = 0;
    while y < output_height {
        let height = MAX_TILE_HEIGHT.min(output_height - y);
        tiles.push((y, height));
        y += height;
    }
    tiles
}

fn bounded_raster_size(output_width: u32, output_height: u32) -> (u32, u32) {
    if output_height <= MAX_RASTER_HEIGHT {
        return (output_width, output_height);
    }
    let scale = MAX_RASTER_HEIGHT as f64 / output_height as f64;
    let width = ((output_width as f64 * scale).round().max(1.0) as u32).max(1);
    (width, MAX_RASTER_HEIGHT)
}

pub fn image_mime(bytes: &[u8]) -> &'static str {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        "image/jpeg"
    } else if bytes.len() >= 8 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else {
        "image/jpeg"
    }
}

fn peek_image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Raw bytes of the single image that covers a page, when the page is just one picture.
/// Manga and webtoon PDFs are almost always built this way, which lets us decode the
/// stored JPEG directly instead of asking PDFium to rasterize a giant page.
fn full_page_image_raw(page: &PdfPage) -> Option<Vec<u8>> {
    let page_area = page.width().value * page.height().value;
    if page_area <= 0.0 {
        return None;
    }

    let mut raw_images = Vec::new();
    for obj in page.objects().iter() {
        let Some(img_obj) = obj.as_image_object() else {
            if !matches!(obj.object_type(), PdfPageObjectType::Image) {
                return None;
            }
            continue;
        };
        let bounds = obj.bounds().ok()?;
        let area = bounds.width().value * bounds.height().value;
        if area / page_area < 0.8 {
            return None;
        }
        raw_images.push(img_obj.get_raw_image_data().ok()?);
    }

    if raw_images.len() != 1 {
        return None;
    }
    let raw = raw_images.pop()?;
    if raw.is_empty() {
        return None;
    }
    Some(raw)
}

/// Crops `source` into the requested stacked segments, scaling each to the output width.
/// Cropping before scaling keeps peak memory close to one segment instead of one page.
fn slice_into_tiles(
    source: &image::DynamicImage,
    output_width: u32,
    output_height: u32,
    layout: &[(u32, u32)],
    lossless: bool,
) -> Result<Vec<RenderResult>, PdfError> {
    let src_w = source.width();
    let src_h = source.height();
    if src_w == 0 || src_h == 0 || output_height == 0 {
        return Err(PdfError::Other("Rendered page has empty dimensions".into()));
    }

    let mut tiles = Vec::with_capacity(layout.len());
    for (y, height) in layout {
        let src_top = ((*y as f64) * src_h as f64 / output_height as f64).floor() as u32;
        let src_bottom = (((*y + *height) as f64) * src_h as f64 / output_height as f64).ceil() as u32;
        let src_top = src_top.min(src_h.saturating_sub(1));
        let src_bottom = src_bottom.clamp(src_top + 1, src_h);
        let crop = source.crop_imm(0, src_top, src_w, src_bottom - src_top);
        let scaled = if crop.width() == output_width && crop.height() == *height {
            crop
        } else {
            crop.resize_exact(output_width, *height, image::imageops::FilterType::Triangle)
        };
        tiles.push(if lossless {
            encode_png(&scaled)?
        } else {
            encode_jpeg(&scaled)?
        });
    }
    Ok(tiles)
}

fn encode_jpeg(image: &image::DynamicImage) -> Result<RenderResult, PdfError> {
    let rgb = image.to_rgb8();
    let width = rgb.width();
    let height = rgb.height();
    if width == 0 || height == 0 {
        return Err(PdfError::Other("Rendered page has empty dimensions".into()));
    }
    let mut buf = Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 82)
        .encode(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .map_err(PdfError::Image)?;
    Ok(RenderResult {
        bytes: buf.into_inner(),
        width,
        height,
    })
}

fn encode_png(image: &image::DynamicImage) -> Result<RenderResult, PdfError> {
    let rgb = image.to_rgb8();
    let width = rgb.width();
    let height = rgb.height();
    if width == 0 || height == 0 {
        return Err(PdfError::Other("Rendered page has empty dimensions".into()));
    }
    let mut buf = Cursor::new(Vec::new());
    image::codecs::png::PngEncoder::new(&mut buf)
        .write_image(rgb.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .map_err(PdfError::Image)?;
    Ok(RenderResult {
        bytes: buf.into_inner(),
        width,
        height,
    })
}

fn chapter_tiles_inner(
    pdfium: &Pdfium,
    path: &str,
    target_width: u32,
) -> Result<Vec<TileGeometry>, PdfError> {
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    let mut geometry = Vec::new();
    for (page_index, page) in doc.pages().iter().enumerate() {
        let (width, height) =
            page_output_size(page.width().value, page.height().value, target_width);
        for (tile_index, (_, tile_height)) in tile_layout(height).into_iter().enumerate() {
            geometry.push(TileGeometry {
                page_index: page_index as i32,
                tile_index: tile_index as u32,
                width,
                height: tile_height,
            });
        }
    }
    if geometry.is_empty() {
        return Err(PdfError::Other("This PDF has no readable pages".into()));
    }
    Ok(geometry)
}

fn render_page_tiles_inner(
    pdfium: &Pdfium,
    path: &str,
    page_index: i32,
    target_width: u32,
) -> Result<Vec<RenderResult>, PdfError> {
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    let page = doc
        .pages()
        .get(page_index as u16)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;

    let (out_w, out_h) = page_output_size(page.width().value, page.height().value, target_width);
    let layout = tile_layout(out_h);
    if layout.is_empty() {
        return Err(PdfError::Other("This page has no readable content".into()));
    }

    if let Some(raw) = full_page_image_raw(&page) {
        // Raw stream bytes are only decodable for image filters such as DCTDecode; anything
        // else (for example Flate-compressed samples) falls through to rasterization.
        if let Ok(decoded) = image::load_from_memory(&raw) {
            if decoded.width() > 0 && decoded.height() > 0 {
                return slice_into_tiles(&decoded, out_w, out_h, &layout, false);
            }
        }
    }

    let (raster_w, raster_h) = bounded_raster_size(out_w, out_h);
    let config = PdfRenderConfig::new()
        .set_target_width(raster_w as i32)
        .set_maximum_width(raster_w as i32)
        .set_maximum_height(raster_h as i32)
        .rotate_if_landscape(PdfPageRenderRotation::None, false);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    let image = bitmap.as_image();
    if image.width() == 0 || image.height() == 0 {
        return Err(PdfError::Other("Rendered page has empty dimensions".into()));
    }
    slice_into_tiles(&image, out_w, out_h, &layout, false)
}

/// Native-resolution lossless slices for OCR. Display tiles are JPEG at 82, which is
/// enough to blur neighboring Chinese characters into each other.
fn render_page_ocr_inner(
    pdfium: &Pdfium,
    path: &str,
    page_index: i32,
) -> Result<Vec<RenderResult>, PdfError> {
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    let page = doc
        .pages()
        .get(page_index as u16)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;

    if let Some(raw) = full_page_image_raw(&page) {
        if let Ok(decoded) = image::load_from_memory(&raw) {
            if decoded.width() >= 600 && decoded.width() <= 2200 && decoded.height() > 0 {
                let width = decoded.width();
                let height = decoded.height().min(MAX_PAGE_OUTPUT_HEIGHT);
                let layout = tile_layout(height);
                if !layout.is_empty() {
                    return slice_into_tiles(&decoded, width, height, &layout, true);
                }
            }
        }
    }

    let (out_w, out_h) = page_output_size(page.width().value, page.height().value, 1400);
    let layout = tile_layout(out_h);
    if layout.is_empty() {
        return Err(PdfError::Other("This page has no readable content".into()));
    }
    let (raster_w, raster_h) = bounded_raster_size(out_w, out_h);
    let config = PdfRenderConfig::new()
        .set_target_width(raster_w as i32)
        .set_maximum_width(raster_w as i32)
        .set_maximum_height(raster_h as i32)
        .rotate_if_landscape(PdfPageRenderRotation::None, false);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    let image = bitmap.as_image();
    if image.width() == 0 || image.height() == 0 {
        return Err(PdfError::Other("Rendered page has empty dimensions".into()));
    }
    slice_into_tiles(&image, out_w, out_h, &layout, true)
}

fn render_page_inner(
    pdfium: &Pdfium,
    path: &str,
    page_index: i32,
    target_width: u32,
) -> Result<RenderResult, PdfError> {
    let doc = pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    let page = doc
        .pages()
        .get(page_index as u16)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;

    let (out_w, full_h) = page_output_size(page.width().value, page.height().value, target_width);
    // A webtoon page can be forty thousand pixels tall; squashing all of it into one
    // thumbnail yields an unreadable sliver, so tall pages are cropped to their top.
    let out_h = full_h
        .min((out_w as f64 * COVER_MAX_ASPECT).round() as u32)
        .max(1);
    let crop = [(0u32, out_h)];

    if let Some(raw) = full_page_image_raw(&page) {
        if let Ok(decoded) = image::load_from_memory(&raw) {
            if decoded.width() > 0 && decoded.height() > 0 {
                return slice_into_tiles(&decoded, out_w, full_h, &crop, false)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| PdfError::Other("Rendered page has empty dimensions".into()));
            }
        }
    }

    let (raster_w, raster_h) = bounded_raster_size(out_w, full_h);
    let config = PdfRenderConfig::new()
        .set_target_width(raster_w as i32)
        .set_maximum_width(raster_w as i32)
        .set_maximum_height(raster_h as i32)
        .rotate_if_landscape(PdfPageRenderRotation::None, false);

    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| PdfError::Pdfium(e.to_string()))?;
    let image = bitmap.as_image();
    if image.width() == 0 || image.height() == 0 {
        return Err(PdfError::Other("Rendered page has empty dimensions".into()));
    }
    slice_into_tiles(&image, out_w, full_h, &crop, false)?
        .into_iter()
        .next()
        .ok_or_else(|| PdfError::Other("Rendered page has empty dimensions".into()))
}

pub fn get_page_count(path: &str) -> Result<i32, PdfError> {
    get_page_count_prioritized(path, false)
}

pub fn get_page_count_urgent(path: &str) -> Result<i32, PdfError> {
    get_page_count_prioritized(path, true)
}

fn get_page_count_prioritized(path: &str, urgent: bool) -> Result<i32, PdfError> {
    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    let channels = pdfium_channels();
    let tx = if urgent { &channels.urgent } else { &channels.normal };
    tx.send(PdfRequest::PageCount {
        path: path.to_string(),
        reply: reply_tx,
    })
    .map_err(|e| PdfError::Other(e.to_string()))?;
    reply_rx.recv().map_err(|e| PdfError::Other(e.to_string()))?
}

pub fn render_page(path: &str, page_index: i32, target_width: u32) -> Result<RenderResult, PdfError> {
    render_page_prioritized(path, page_index, target_width, false)
}

pub fn render_page_urgent(path: &str, page_index: i32, target_width: u32) -> Result<RenderResult, PdfError> {
    render_page_prioritized(path, page_index, target_width, true)
}

fn render_page_prioritized(
    path: &str,
    page_index: i32,
    target_width: u32,
    urgent: bool,
) -> Result<RenderResult, PdfError> {
    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    let channels = pdfium_channels();
    let tx = if urgent { &channels.urgent } else { &channels.normal };
    tx.send(PdfRequest::RenderPage {
        path: path.to_string(),
        page_index,
        target_width,
        reply: reply_tx,
    })
    .map_err(|e| PdfError::Other(e.to_string()))?;
    reply_rx.recv().map_err(|e| PdfError::Other(e.to_string()))?
}

pub fn get_page_dimensions(path: &str, page_index: i32) -> Result<(f64, f64), PdfError> {
    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    pdfium_channels()
        .normal
        .send(PdfRequest::PageDimensions {
            path: path.to_string(),
            page_index,
            reply: reply_tx,
        })
        .map_err(|e| PdfError::Other(e.to_string()))?;
    reply_rx.recv().map_err(|e| PdfError::Other(e.to_string()))?
}

pub struct PageCache {
    cache_dir: PathBuf,
    max_bytes: Mutex<u64>,
    current_bytes: Mutex<u64>,
}

impl PageCache {
    pub fn new(cache_dir: PathBuf, max_mb: u32) -> Self {
        let _ = std::fs::create_dir_all(&cache_dir);
        let covers = cache_dir.join("covers");
        let pages = cache_dir.join("pages");
        let _ = std::fs::create_dir_all(&covers);
        let _ = std::fs::create_dir_all(&pages);
        let cache = Self {
            cache_dir,
            max_bytes: Mutex::new(max_mb as u64 * 1024 * 1024),
            current_bytes: Mutex::new(0),
        };
        cache
    }

    fn dir_bytes(dir: &PathBuf) -> u64 {
        let mut total = 0u64;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        total += meta.len();
                    }
                }
            }
        }
        total
    }

    pub fn scan_existing(&self) {
        let pages = self.cache_dir.join("pages");
        let covers = self.cache_dir.join("covers");
        *self.current_bytes.lock() = Self::dir_bytes(&pages) + Self::dir_bytes(&covers);
    }

    pub fn used_bytes(&self) -> u64 {
        *self.current_bytes.lock()
    }

    pub fn clear(&self) {
        for sub in ["pages", "covers"] {
            let dir = self.cache_dir.join(sub);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        *self.current_bytes.lock() = 0;
    }

    fn page_path(&self, chapter_id: i64, page_index: i32, width: u32) -> PathBuf {
        self.cache_dir
            .join("pages")
            .join(format!("{chapter_id}_{page_index}_{width}.jpg"))
    }

    fn cover_path(&self, series_id: i64) -> PathBuf {
        self.cache_dir.join("covers").join(format!("{series_id}.webp"))
    }

    pub fn get_page(&self, chapter_id: i64, page_index: i32, width: u32) -> Option<Vec<u8>> {
        let path = self.page_path(chapter_id, page_index, width);
        std::fs::read(path).ok()
    }

    pub fn put_page(&self, chapter_id: i64, page_index: i32, width: u32, data: &[u8]) {
        let path = self.page_path(chapter_id, page_index, width);
        if std::fs::write(&path, data).is_ok() {
            *self.current_bytes.lock() += data.len() as u64;
            self.maybe_evict();
        }
    }

    fn tile_path(&self, chapter_id: i64, page_index: i32, tile_index: u32, width: u32) -> PathBuf {
        self.cache_dir
            .join("pages")
            .join(format!("{chapter_id}_{page_index}_t{tile_index}_{width}.jpg"))
    }

    pub fn get_tile(
        &self,
        chapter_id: i64,
        page_index: i32,
        tile_index: u32,
        width: u32,
    ) -> Option<Vec<u8>> {
        std::fs::read(self.tile_path(chapter_id, page_index, tile_index, width)).ok()
    }

    pub fn put_tile(
        &self,
        chapter_id: i64,
        page_index: i32,
        tile_index: u32,
        width: u32,
        data: &[u8],
    ) {
        let path = self.tile_path(chapter_id, page_index, tile_index, width);
        if std::fs::write(&path, data).is_ok() {
            *self.current_bytes.lock() += data.len() as u64;
        }
    }

    pub fn get_cover(&self, series_id: i64) -> Option<Vec<u8>> {
        let path = self.cover_path(series_id);
        std::fs::read(path).ok()
    }

    pub fn put_cover(&self, series_id: i64, data: &[u8]) -> String {
        let path = self.cover_path(series_id);
        let _ = std::fs::write(&path, data);
        path.to_string_lossy().to_string()
    }

    pub fn evict_distant_pages(&self, chapter_id: i64, current_page: i32, window: i32) {
        let pages_dir = self.cache_dir.join("pages");
        if let Ok(entries) = std::fs::read_dir(pages_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(rest) = name.strip_prefix(&format!("{chapter_id}_")) {
                    if let Some(page_str) = rest.split('_').next() {
                        if let Ok(page) = page_str.parse::<i32>() {
                            if (page - current_page).abs() > window {
                                let _ = std::fs::remove_file(entry.path());
                            }
                        }
                    }
                }
            }
        }
    }

    fn maybe_evict(&self) {
        let max = *self.max_bytes.lock();
        let mut current = *self.current_bytes.lock();
        if current <= max {
            return;
        }
        let pages_dir = self.cache_dir.join("pages");
        let mut files: Vec<(PathBuf, std::time::SystemTime, u64)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&pages_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(modified) = meta.modified() {
                        files.push((entry.path(), modified, meta.len()));
                    }
                }
            }
        }
        files.sort_by_key(|(_, t, _)| *t);
        for (path, _, size) in files {
            if current <= max * 8 / 10 {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                current = current.saturating_sub(size);
            }
        }
        *self.current_bytes.lock() = current;
    }

    pub fn page_bytes(&self, chapter_id: i64, page_index: i32, width: u32) -> Option<Vec<u8>> {
        self.get_page(chapter_id, page_index, width)
    }

    pub fn cover_file_path(&self, series_id: i64) -> PathBuf {
        self.cover_path(series_id)
    }

    pub fn set_max_mb(&self, mb: u32) {
        *self.max_bytes.lock() = mb as u64 * 1024 * 1024;
    }
}

pub fn render_page_bytes(
    cache: &PageCache,
    chapter_id: i64,
    file_path: &str,
    page_index: i32,
    target_width: u32,
) -> Result<(Vec<u8>, u32, u32, bool), PdfError> {
    if let Some(cached) = cache.get_page(chapter_id, page_index, target_width) {
        let dims = image_dimensions_from_webp(&cached)?;
        return Ok((cached, dims.0, dims.1, true));
    }
    let result = render_page_urgent(file_path, page_index, target_width)?;
    cache.put_page(chapter_id, page_index, target_width, &result.bytes);
    Ok((result.bytes, result.width, result.height, false))
}

pub fn render_page_data_url(
    cache: &PageCache,
    chapter_id: i64,
    file_path: &str,
    page_index: i32,
    target_width: u32,
) -> Result<(String, u32, u32, bool), PdfError> {
    if let Some(cached) = cache.get_page(chapter_id, page_index, target_width) {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &cached);
        let dims = image_dimensions_from_webp(&cached)?;
        return Ok((format!("data:{};base64,{b64}", image_mime(&cached)), dims.0, dims.1, true));
    }

    let result = render_page(file_path, page_index, target_width)?;
    cache.put_page(chapter_id, page_index, target_width, &result.bytes);
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &result.bytes);
    Ok((
        format!("data:{};base64,{b64}", image_mime(&result.bytes)),
        result.width,
        result.height,
        false,
    ))
}

pub fn chapter_tiles(path: &str, target_width: u32) -> Result<Vec<TileGeometry>, PdfError> {
    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    pdfium_channels()
        .urgent
        .send(PdfRequest::ChapterTiles {
            path: path.to_string(),
            target_width,
            reply: reply_tx,
        })
        .map_err(|e| PdfError::Other(e.to_string()))?;
    reply_rx.recv().map_err(|e| PdfError::Other(e.to_string()))?
}

pub fn page_tiles(
    path: &str,
    page_index: i32,
    target_width: u32,
) -> Result<Vec<TileGeometry>, PdfError> {
    let tiles: Vec<TileGeometry> = chapter_tiles(path, target_width)?
        .into_iter()
        .filter(|tile| tile.page_index == page_index)
        .collect();
    if tiles.is_empty() {
        return Err(PdfError::Other(format!("Page {page_index} is out of range")));
    }
    Ok(tiles)
}

pub fn render_page_ocr_images(path: &str, page_index: i32) -> Result<Vec<RenderResult>, PdfError> {
    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    pdfium_channels()
        .urgent
        .send(PdfRequest::RenderPageOcr {
            path: path.to_string(),
            page_index,
            reply: reply_tx,
        })
        .map_err(|e| PdfError::Other(e.to_string()))?;
    reply_rx.recv().map_err(|e| PdfError::Other(e.to_string()))?
}

fn render_page_tiles(
    path: &str,
    page_index: i32,
    target_width: u32,
    urgent: bool,
) -> Result<Vec<RenderResult>, PdfError> {
    let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
    let channels = pdfium_channels();
    let tx = if urgent { &channels.urgent } else { &channels.normal };
    tx.send(PdfRequest::RenderPageTiles {
        path: path.to_string(),
        page_index,
        target_width,
        reply: reply_tx,
    })
    .map_err(|e| PdfError::Other(e.to_string()))?;
    reply_rx.recv().map_err(|e| PdfError::Other(e.to_string()))?
}

/// Serializes per-page work so that the several segments a reader requests at once
/// decode the underlying page image only the first time.
fn page_render_lock(chapter_id: i64, page_index: i32, width: u32) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<(i64, i32, u32), Arc<Mutex<()>>>>> = OnceLock::new();
    let map = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock();
    Arc::clone(
        guard
            .entry((chapter_id, page_index, width))
            .or_insert_with(|| Arc::new(Mutex::new(()))),
    )
}

/// Renders every segment of a page in one pass and caches them, returning the requested one.
pub fn render_tile_bytes(
    cache: &PageCache,
    chapter_id: i64,
    file_path: &str,
    page_index: i32,
    tile_index: u32,
    target_width: u32,
    urgent: bool,
) -> Result<(Vec<u8>, u32, u32, bool), PdfError> {
    let width = clamp_target_width(target_width);
    if let Some(cached) = cache.get_tile(chapter_id, page_index, tile_index, width) {
        let (w, h) = peek_image_size(&cached)
            .ok_or_else(|| PdfError::Other("Cached segment is unreadable".into()))?;
        return Ok((cached, w, h, true));
    }

    let lock = page_render_lock(chapter_id, page_index, width);
    let _held = lock.lock();
    if let Some(cached) = cache.get_tile(chapter_id, page_index, tile_index, width) {
        let (w, h) = peek_image_size(&cached)
            .ok_or_else(|| PdfError::Other("Cached segment is unreadable".into()))?;
        return Ok((cached, w, h, true));
    }

    let tiles = render_page_tiles(file_path, page_index, width, urgent)?;
    for (index, tile) in tiles.iter().enumerate() {
        cache.put_tile(chapter_id, page_index, index as u32, width, &tile.bytes);
    }
    let tile = tiles
        .into_iter()
        .nth(tile_index as usize)
        .ok_or_else(|| PdfError::Other(format!("Segment {tile_index} is out of range")))?;
    Ok((tile.bytes, tile.width, tile.height, false))
}

pub fn render_tile_data_url(
    cache: &PageCache,
    chapter_id: i64,
    file_path: &str,
    page_index: i32,
    tile_index: u32,
    target_width: u32,
) -> Result<(String, u32, u32, bool), PdfError> {
    let (bytes, width, height, from_cache) = render_tile_bytes(
        cache,
        chapter_id,
        file_path,
        page_index,
        tile_index,
        target_width,
        true,
    )?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
    Ok((
        format!("data:{};base64,{b64}", image_mime(&bytes)),
        width,
        height,
        from_cache,
    ))
}

fn image_dimensions_from_webp(data: &[u8]) -> Result<(u32, u32), PdfError> {
    let img = image::load_from_memory(data).map_err(PdfError::Image)?;
    Ok((img.width(), img.height()))
}

pub fn generate_cover(
    cache: &PageCache,
    series_id: i64,
    file_path: &str,
) -> Result<String, PdfError> {
    if cache.get_cover(series_id).is_some() {
        return Ok(cache.cover_path(series_id).to_string_lossy().to_string());
    }
    let result = render_page(file_path, 0, 400)?;
    let path = cache.put_cover(series_id, &result.bytes);
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_PDF: &[u8] = b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>\nendobj\nxref\n0 4\n0000000000 65535 f \n0000000015 00000 n \n0000000064 00000 n \n0000000121 00000 n \ntrailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n192\n%%EOF\n";

    #[test]
    fn pdfium_library_is_present() {
        let path = pdfium_lib_path();
        assert!(
            path.exists(),
            "PDFium binary missing at {:?}. Run `npm run pdfium`.",
            path
        );
    }

    #[test]
    fn pdfium_counts_pages_for_a_file_with_spaces() {
        let dir = std::env::temp_dir().join("shelf-pdfium-smoke");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("Chapter 001.pdf");
        std::fs::write(&path, MINIMAL_PDF).expect("write pdf");
        let count = get_page_count(path.to_str().expect("utf8 path"))
            .expect("PDFium should bind chromium/7543 and open this PDF");
        assert_eq!(count, 1);

        let rendered = render_page(path.to_str().expect("utf8 path"), 0, 200)
            .expect("PDFium should rasterize page 0");
        assert!(rendered.width > 0 && rendered.height > 0);
        assert!(!rendered.bytes.is_empty());
        assert_eq!(&rendered.bytes[0..2], &[0xFF, 0xD8]);
    }

    #[test]
    fn tall_strip_is_split_into_full_width_segments() {
        let (width, height) = page_output_size(921.6, 33177.6, 1200);
        assert_eq!(width, 1200);
        assert_eq!(height, 43200);

        let layout = tile_layout(height);
        assert_eq!(layout.len(), 18);
        assert_eq!(layout[0], (0, MAX_TILE_HEIGHT));
        // Segments must tile the page exactly, with no gap or overlap.
        assert_eq!(
            layout.iter().map(|(_, h)| *h).sum::<u32>(),
            height,
            "segments must cover the page"
        );
        for pair in layout.windows(2) {
            assert_eq!(pair[0].0 + pair[0].1, pair[1].0);
        }
    }

    #[test]
    fn short_page_is_a_single_segment() {
        let (width, height) = page_output_size(612.0, 792.0, 1000);
        assert_eq!(width, 1000);
        let layout = tile_layout(height);
        assert_eq!(layout.len(), 1);
        assert_eq!(layout[0], (0, height));
    }

    #[test]
    fn slicing_keeps_full_width_and_stacks_to_full_height() {
        let source = image::DynamicImage::ImageRgb8(image::RgbImage::new(120, 6000));
        let layout = tile_layout(6000);
        let tiles = slice_into_tiles(&source, 120, 6000, &layout, false).expect("slice");

        assert_eq!(tiles.len(), 3);
        for tile in &tiles {
            assert_eq!(tile.width, 120, "segments keep the requested width");
            assert_eq!(&tile.bytes[0..2], &[0xFF, 0xD8]);
            assert_eq!(image_mime(&tile.bytes), "image/jpeg");
        }
        assert_eq!(tiles.iter().map(|t| t.height).sum::<u32>(), 6000);
    }

    #[test]
    fn raster_size_is_bounded_for_very_tall_pages() {
        let (width, height) = bounded_raster_size(1200, 43200);
        assert_eq!(height, MAX_RASTER_HEIGHT);
        assert!(width > 0 && width < 1200);
        assert_eq!(bounded_raster_size(1200, 1600), (1200, 1600));
    }
}
