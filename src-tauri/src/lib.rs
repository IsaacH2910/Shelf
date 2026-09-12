mod auth;
mod db;
mod http;
mod index;
mod jobs;
mod keep_awake;
mod media;
mod models;
mod ocr;
mod pdf;
mod translate;
mod tunnel;
mod watch;

use db::Database;
use http::{CloudServer, UploadManager};
use index::Indexer;
use jobs::{JobKind, JobPriority, JobScheduler};
use models::*;
use parking_lot::Mutex;
use pdf::PageCache;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Manager, State};
use tracing_subscriber::EnvFilter;

pub struct AppState {
    pub db: Arc<Database>,
    pub cache: PageCache,
    pub scheduler: Arc<JobScheduler>,
    pub indexer: Arc<Indexer>,
    pub cloud: CloudServer,
    pub service_port: u16,
    pub watcher: Mutex<Option<watch::FolderWatcher>>,
    pub tunnel: Arc<tunnel::TunnelManager>,
    pub keep_awake: keep_awake::KeepAwake,
    pub login_limiter: auth::LoginLimiter,
    pub uploads: UploadManager,
    pub web_root: PathBuf,
    pub upload_dir: PathBuf,
}

fn bytes_to_data_url(bytes: &[u8]) -> String {
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    format!("data:{};base64,{b64}", pdf::image_mime(bytes))
}

fn page_image_from_bytes(bytes: &[u8], page_index: i32, from_cache: bool) -> Result<PageImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    Ok(PageImage {
        data_url: bytes_to_data_url(bytes),
        width: img.width(),
        height: img.height(),
        page_index,
        from_cache,
    })
}

/// Bumped whenever OCR or translation output changes so stale cache rows are ignored.
const TEXT_PIPELINE: &str = "v3";

fn text_cache_engine(engine_id: &str) -> String {
    format!("{engine_id}.{TEXT_PIPELINE}")
}

/// Rescales a region recognized within one segment into whole-page coordinates.
///
/// Vision reports normalized coordinates relative to the image it was given, which is a
/// single segment. The overlay positions regions against the whole page, so the vertical
/// axis is compressed into the segment's share of the page and shifted down to it.
fn region_to_page_space(
    mut region: OcrRegion,
    segment_top: u32,
    segment_height: u32,
    page_height: u32,
) -> OcrRegion {
    if page_height == 0 {
        return region;
    }
    let span = segment_height as f64 / page_height as f64;
    let top = segment_top as f64 / page_height as f64;
    region.y = top + region.y * span;
    region.height *= span;
    region
}

/// Recognizes text across a whole page.
///
/// A webtoon page is tens of thousands of pixels tall, far beyond what Vision will accept
/// in one request, and the reader displays it as stacked segments anyway. So each segment
/// is recognized separately and the results are mapped back onto the full page, keeping
/// region coordinates page-relative for the overlay.
fn ocr_page_regions(
    chapter_id: i64,
    file_path: &str,
    page_index: i32,
    engine_id: &str,
) -> Result<Vec<OcrRegion>, String> {
    let tiles = pdf::render_page_ocr_images(file_path, page_index).map_err(|e| e.to_string())?;
    let page_height: u32 = tiles.iter().map(|tile| tile.height).sum();
    if page_height == 0 {
        return Err("This page has no readable content".into());
    }

    let mut regions = Vec::new();
    let mut offset_y = 0u32;
    for (position, tile) in tiles.iter().enumerate() {
        match ocr::run_ocr_sync(engine_id, &tile.bytes) {
            Ok(found) => regions.extend(
                found
                    .into_iter()
                    .map(|region| region_to_page_space(region, offset_y, tile.height, page_height)),
            ),
            Err(error) if position == 0 => return Err(error.to_string()),
            Err(error) => {
                tracing::warn!(
                    chapter_id,
                    page_index,
                    tile_index = position,
                    "OCR failed for segment: {error}"
                );
            }
        }
        offset_y += tile.height;
    }

    Ok(regions)
}

pub(crate) fn run_ocr_pipeline(
    state: &AppState,
    chapter_id: i64,
    page_index: i32,
    engine_id: &str,
    translator_id: &str,
) -> Result<Vec<OcrRegion>, String> {
    const TARGET: &str = "zh-Hant";
    let cache_engine = text_cache_engine(engine_id);
    if let Ok(Some(cached)) = state.db.get_translation(chapter_id, page_index, &cache_engine, translator_id, TARGET) {
        if let Ok(regions) = serde_json::from_str::<Vec<OcrRegion>>(&cached) {
            return Ok(regions);
        }
    }

    let chapter = state.db.get_chapter(&Viewer::SYSTEM, chapter_id).map_err(|e| e.to_string())?;
    let mut regions = ocr_page_regions(chapter_id, &chapter.file_path, page_index, engine_id)?;
    if let Ok(Some(prev)) = state.db.get_ocr_regions(chapter_id, page_index, &cache_engine) {
        if let Ok(edits) = serde_json::from_str::<Vec<OcrRegion>>(&prev) {
            for region in regions.iter_mut() {
                if let Some(edit) = edits.iter().find(|e| e.text == region.text) {
                    if edit.translated_text.is_some() {
                        region.translated_text = edit.translated_text.clone();
                    }
                }
            }
        }
    }

    let json = serde_json::to_string(&regions).unwrap_or_else(|_| "[]".into());
    let _ = state.db.save_ocr_regions(chapter_id, page_index, &cache_engine, &json);

    let translated = translate::run_translate_sync(translator_id, &regions).map_err(|e| e.to_string())?;
    let tjson = serde_json::to_string(&translated).unwrap_or_else(|_| "[]".into());
    let _ = state.db.save_translation(chapter_id, page_index, &cache_engine, translator_id, TARGET, &tjson);
    Ok(translated)
}

impl AppState {
    fn setup_job_handler(self: &Arc<Self>) {
        let state = Arc::clone(self);
        self.scheduler.set_handler(Arc::new(move |job| {
            match job {
                JobKind::ScanRoot { root_id, path } => {
                    if state.indexer.scan_root(root_id, &path).is_err() {
                        state.scheduler.record_index_failure();
                    }
                }
                JobKind::Indexing { chapter_id, file_path } => {
                    if state.indexer.index_chapter_metadata(chapter_id, &file_path).is_err() {
                        state.scheduler.record_index_failure();
                    }
                }
                JobKind::CoverGeneration {
                    series_id,
                    chapter_id,
                    file_path,
                } => {
                    if !crate::media::is_pdf_path(&file_path) {
                        return;
                    }
                    let cover_result = pdf::generate_cover(&state.cache, series_id, &file_path);
                    let cover_failed = cover_result.is_err();
                    if let Ok(path) = cover_result {
                        let _ = state.db.set_series_cover(series_id, &path);
                    }
                    let dimensions_result = pdf::get_page_dimensions(&file_path, 0);
                    let dimensions_failed = dimensions_result.is_err();
                    if let Ok((w, h)) = dimensions_result {
                        let _ = state.db.store_page_dimensions(chapter_id, 0, w, h);
                    }
                    if cover_failed || dimensions_failed {
                        state.scheduler.record_index_failure();
                    }
                    state.scheduler.enqueue(JobKind::DimensionAnalysis { series_id });
                }
                JobKind::FileHash {
                    chapter_id,
                    file_path,
                } => {
                    if state.indexer.hash_chapter_file(chapter_id, &file_path).is_err() {
                        state.scheduler.record_index_failure();
                    }
                }
                JobKind::DimensionAnalysis { series_id } => {
                    if state.indexer.analyze_series_dimensions(series_id).is_err() {
                        state.scheduler.record_index_failure();
                    }
                }
                JobKind::RenderPage {
                    chapter_id,
                    file_path,
                    page_index,
                    target_width,
                    cancel_token,
                    reply,
                    ..
                } => {
                    if cancel_token.load(std::sync::atomic::Ordering::Relaxed) {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err("cancelled".into()));
                        }
                        return;
                    }
                    // Rendering segment 0 decodes the page once and caches every segment,
                    // so this single call warms the whole page for the reader.
                    let result = pdf::render_tile_bytes(
                        &state.cache,
                        chapter_id,
                        &file_path,
                        page_index,
                        0,
                        target_width,
                        false,
                    )
                    .map(|(bytes, w, h, _)| (bytes, w, h))
                    .map_err(|e| e.to_string());
                    if let Err(error) = &result {
                        tracing::warn!(chapter_id, page_index, "page prefetch failed: {error}");
                    }
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    }
                }
                JobKind::CacheEviction {
                    chapter_id,
                    current_page,
                    window,
                } => {
                    state.cache.evict_distant_pages(chapter_id, current_page, window);
                }
                JobKind::OcrTranslation {
                    chapter_id,
                    page_index,
                    engine_id,
                    translator_id,
                    reply,
                } => {
                    let state = Arc::clone(&state);
                    std::thread::spawn(move || {
                        let result = run_ocr_pipeline(&state, chapter_id, page_index, &engine_id, &translator_id);
                        if let Some(reply) = reply {
                            let _ = reply.send(result);
                        }
                    });
                }
            }
        }));
    }
}

fn qr_data_url(content: &str) -> Option<String> {
    let code = qrcode::QrCode::new(content.as_bytes()).ok()?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(200, 200)
        .build();
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, svg.as_bytes());
    Some(format!("data:image/svg+xml;base64,{b64}"))
}

fn remote_enabled(db: &Database) -> bool {
    db.get_setting("remote_enabled")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false)
}

async fn start_cloud_service(state: &Arc<AppState>) -> Result<String, String> {
    state
        .cloud
        .start(Arc::clone(state), state.service_port)
        .await
}

fn remote_hostname(db: &Database) -> String {
    let stored = db.get_setting("remote_hostname").ok().flatten();
    tunnel::resolve_hostname(stored.as_deref()).unwrap_or_default()
}

fn start_tunnel(state: &AppState) -> Result<(), String> {
    if !state.db.owner_password_set().map_err(|e| e.to_string())? {
        return Err("Set an owner password before enabling remote access".into());
    }
    if remote_hostname(&state.db).is_empty() {
        return Err("Set a public hostname before enabling remote access".into());
    }
    if !tunnel::cloudflared_available() {
        return Err("Install cloudflared: brew install cloudflare/cloudflare/cloudflared".into());
    }
    let token = tunnel::load_token()?.ok_or_else(|| {
        "Set a Cloudflare tunnel token before enabling remote access".to_string()
    })?;
    state.tunnel.start_named(&token)
}

fn resolve_web_root(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(dir) = app.path().resource_dir() {
        if dir.join("index.html").is_file() {
            return dir;
        }
        let nested = dir.join("_up_");
        if nested.join("index.html").is_file() {
            return nested;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.join("index.html").is_file() {
                return parent.to_path_buf();
            }
            let resources = parent.join("../Resources");
            if resources.join("index.html").is_file() {
                return resources;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist")
}

fn app_data_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    directories::ProjectDirs::from("com", "isaach", "Shelf")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| app.path().app_data_dir().unwrap_or_default())
}

fn restart_all_watchers(state: &AppState) -> Result<(), String> {
    let roots: Vec<String> = state
        .db
        .list_roots()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|r| r.path)
        .collect();
    if roots.is_empty() {
        *state.watcher.lock() = None;
        return Ok(());
    }
    let watcher = watch::restart_watcher(roots, Arc::clone(&state.indexer))?;
    *state.watcher.lock() = Some(watcher);
    Ok(())
}

#[tauri::command(async)]
fn add_library_root(state: State<'_, Arc<AppState>>, path: String) -> Result<LibraryRoot, String> {
    let root = state.db.add_root(&path).map_err(|e| e.to_string())?;
    state.scheduler.enqueue(JobKind::ScanRoot {
        root_id: root.id,
        path,
    });
    restart_all_watchers(&state)?;
    Ok(root)
}

#[tauri::command(async)]
fn list_library_roots(state: State<'_, Arc<AppState>>) -> Result<Vec<LibraryRoot>, String> {
    state.db.list_roots().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn remove_library_root(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    state.db.remove_root(id).map_err(|e| e.to_string())?;
    restart_all_watchers(&state)
}

#[tauri::command(async)]
fn list_series(
    state: State<'_, Arc<AppState>>,
    search: Option<String>,
    favorites_only: Option<bool>,
    sort: Option<String>,
    unread_only: Option<bool>,
    collection_id: Option<i64>,
    reading_mode: Option<String>,
    content_type: Option<String>,
) -> Result<Vec<Series>, String> {
    state
        .db
        .list_series_filtered(
            &Viewer::SYSTEM,
            &SeriesFilter {
                search,
                favorites_only: favorites_only.unwrap_or(false),
                unread_only: unread_only.unwrap_or(false),
                collection_id,
                reading_mode,
                content_type,
                sort: sort.unwrap_or_else(|| "title".into()),
            },
        )
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_series(state: State<'_, Arc<AppState>>, id: i64) -> Result<Series, String> {
    state.db.get_series(&Viewer::SYSTEM, id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn update_series(
    state: State<'_, Arc<AppState>>,
    id: i64,
    update: SeriesUpdate,
) -> Result<(), String> {
    state.db.update_series(id, &update).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_chapters(state: State<'_, Arc<AppState>>, series_id: i64) -> Result<Vec<Chapter>, String> {
    state.db.list_chapters(&Viewer::SYSTEM, series_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_chapter(state: State<'_, Arc<AppState>>, id: i64) -> Result<Chapter, String> {
    state.db.get_chapter(&Viewer::SYSTEM, id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_page_count(state: State<'_, Arc<AppState>>, id: i64) -> Result<i32, String> {
    let chapter = state.db.get_chapter(&Viewer::SYSTEM, id).map_err(|e| e.to_string())?;
    if !media::is_pdf_path(&chapter.file_path) {
        return Err("Page count is only available for PDF files".into());
    }
    pdf::get_page_count_urgent(&chapter.file_path).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_adjacent_chapter(
    state: State<'_, Arc<AppState>>,
    chapter_id: i64,
    next: bool,
) -> Result<Option<Chapter>, String> {
    state
        .db
        .get_adjacent_chapter(&Viewer::SYSTEM, chapter_id, next)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_page_image(
    state: State<'_, Arc<AppState>>,
    chapter_id: i64,
    page_index: i32,
    target_width: u32,
    prefetch: Option<bool>,
) -> Result<PageImage, String> {
    let chapter = state.db.get_chapter(&Viewer::SYSTEM, chapter_id).map_err(|e| e.to_string())?;
    if chapter.missing {
        return Err("Chapter file is missing".into());
    }
    if !media::is_pdf_path(&chapter.file_path) {
        return Err("Page rendering is only available for PDF files".into());
    }
    let width = target_width.min(2560).max(100);

    if prefetch.unwrap_or(false) {
        if let Some(cached) = state.cache.get_page(chapter_id, page_index, width) {
            return page_image_from_bytes(&cached, page_index, true);
        }
        state.scheduler.enqueue(JobKind::RenderPage {
            chapter_id,
            file_path: chapter.file_path,
            page_index,
            target_width: width,
            priority: JobPriority::ReaderPrefetch,
            cancel_token: Arc::new(AtomicBool::new(false)),
            reply: None,
        });
        return Err("prefetch".into());
    }

    state.scheduler.cancel_prefetch(chapter_id, Some(page_index), 2);

    if let Some(cached) = state.cache.get_page(chapter_id, page_index, width) {
        state.scheduler.enqueue(JobKind::CacheEviction {
            chapter_id,
            current_page: page_index,
            window: 5,
        });
        return page_image_from_bytes(&cached, page_index, true);
    }

    let rendered = pdf::render_page_urgent(&chapter.file_path, page_index, width)
        .map_err(|e| e.to_string())?;
    let bytes = rendered.bytes;
    let w = rendered.width;
    let h = rendered.height;
    state.cache.put_page(chapter_id, page_index, width, &bytes);

    state.scheduler.enqueue(JobKind::CacheEviction {
        chapter_id,
        current_page: page_index,
        window: 5,
    });

    Ok(PageImage {
        data_url: bytes_to_data_url(&bytes),
        width: w,
        height: h,
        page_index,
        from_cache: false,
    })
}

/// Layout of every stacked segment in a chapter. Derived from page dimensions only, so it
/// returns immediately and lets the reader reserve correct space before pixels arrive.
#[tauri::command(async)]
fn get_chapter_tiles(
    state: State<'_, Arc<AppState>>,
    chapter_id: i64,
    target_width: u32,
) -> Result<Vec<PageTile>, String> {
    let chapter = state.db.get_chapter(&Viewer::SYSTEM, chapter_id).map_err(|e| e.to_string())?;
    if chapter.missing {
        return Err("Chapter file is missing".into());
    }
    if !media::is_pdf_path(&chapter.file_path) {
        return Err("Page rendering is only available for PDF files".into());
    }
    let tiles = pdf::chapter_tiles(&chapter.file_path, target_width).map_err(|e| e.to_string())?;
    Ok(tiles
        .into_iter()
        .map(|t| PageTile {
            page_index: t.page_index,
            tile_index: t.tile_index,
            width: t.width,
            height: t.height,
        })
        .collect())
}

#[tauri::command(async)]
fn get_chapter_tile(
    state: State<'_, Arc<AppState>>,
    chapter_id: i64,
    page_index: i32,
    tile_index: u32,
    target_width: u32,
) -> Result<PageImage, String> {
    let chapter = state.db.get_chapter(&Viewer::SYSTEM, chapter_id).map_err(|e| e.to_string())?;
    if chapter.missing {
        return Err("Chapter file is missing".into());
    }
    if !media::is_pdf_path(&chapter.file_path) {
        return Err("Page rendering is only available for PDF files".into());
    }

    let (data_url, width, height, from_cache) = pdf::render_tile_data_url(
        &state.cache,
        chapter_id,
        &chapter.file_path,
        page_index,
        tile_index,
        target_width,
    )
    .map_err(|e| e.to_string())?;

    Ok(PageImage {
        data_url,
        width,
        height,
        page_index,
        from_cache,
    })
}

#[tauri::command(async)]
fn prefetch_pages(
    state: State<'_, Arc<AppState>>,
    chapter_id: i64,
    pages: Vec<i32>,
    target_width: u32,
) -> Result<(), String> {
    let chapter = state.db.get_chapter(&Viewer::SYSTEM, chapter_id).map_err(|e| e.to_string())?;
    if !media::is_pdf_path(&chapter.file_path) {
        return Ok(());
    }
    let keep = pages.first().copied();
    state.scheduler.cancel_prefetch(chapter_id, keep, 4);
    let width = target_width.min(2560).max(100);

    for page_index in pages {
        if state.cache.get_tile(chapter_id, page_index, 0, width).is_some() {
            continue;
        }
        state.scheduler.enqueue(JobKind::RenderPage {
            chapter_id,
            file_path: chapter.file_path.clone(),
            page_index,
            target_width: width,
            priority: JobPriority::ReaderPrefetch,
            cancel_token: Arc::new(AtomicBool::new(false)),
            reply: None,
        });
    }
    Ok(())
}

#[tauri::command(async)]
fn save_progress(state: State<'_, Arc<AppState>>, progress: ReadingProgress) -> Result<(), String> {
    state.db.save_progress(&Viewer::SYSTEM, &progress).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_progress(
    state: State<'_, Arc<AppState>>,
    chapter_id: i64,
) -> Result<Option<ReadingProgress>, String> {
    state.db.get_progress(&Viewer::SYSTEM, chapter_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn continue_reading(state: State<'_, Arc<AppState>>) -> Result<Vec<ContinueReadingItem>, String> {
    state.db.continue_reading(&Viewer::SYSTEM, 10).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn reading_history(state: State<'_, Arc<AppState>>) -> Result<Vec<ContinueReadingItem>, String> {
    state.db.reading_history(&Viewer::SYSTEM, 20).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn clear_reading_history(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.db.clear_reading_history(&Viewer::SYSTEM).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn get_cover_image(state: State<'_, Arc<AppState>>, series_id: i64) -> Result<Option<String>, String> {
    if let Some(bytes) = state.cache.get_cover(series_id) {
        let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
        return Ok(Some(format!("data:{};base64,{b64}", pdf::image_mime(&bytes))));
    }
    Ok(None)
}

#[tauri::command(async)]
fn get_index_status(state: State<'_, Arc<AppState>>) -> Result<IndexStatus, String> {
    let (series_count, chapter_count) = state.db.counts().map_err(|e| e.to_string())?;
    Ok(IndexStatus {
        indexing: state.scheduler.index_pending_count() > 0,
        pending_jobs: state.scheduler.index_pending_count(),
        processed_jobs: state.scheduler.index_processed_count(),
        failed_jobs: state.scheduler.index_failed_count(),
        series_count,
        chapter_count,
    })
}

#[tauri::command(async)]
fn get_settings(state: State<'_, Arc<AppState>>) -> Result<AppSettings, String> {
    let cache_mb = state
        .db
        .get_setting("cache_size_mb")
        .map_err(|e| e.to_string())?
        .and_then(|v| v.parse().ok())
        .unwrap_or(512);

    let remote_on = remote_enabled(&state.db);
    let hostname = remote_hostname(&state.db);
    let public_url = tunnel::public_url_for_hostname(&hostname);
    let cloudflared_installed = tunnel::cloudflared_available();
    let mut status = if !cloudflared_installed {
        "cloudflared not installed".into()
    } else if remote_on && !state.tunnel.running() {
        let live = state.tunnel.status();
        if live == "stopped" {
            "stopped".into()
        } else {
            live
        }
    } else {
        state.tunnel.status()
    };
    if !cloudflared_installed && remote_on {
        status = "cloudflared not installed".into();
    }
    let remote_login_ready = state.db.owner_password_set().map_err(|e| e.to_string())?;

    Ok(AppSettings {
        cache_size_mb: cache_mb,
        cache_used_mb: (state.cache.used_bytes() / (1024 * 1024)) as u32,
        default_reading_mode: ReadingMode::Auto,
        remote: RemoteSettings {
            enabled: remote_on,
            mode: TunnelMode::Named,
            public_url: public_url.clone(),
            configured_hostname: if hostname.is_empty() { None } else { Some(hostname) },
            has_token: tunnel::has_token(),
            cloudflared_installed,
            running: state.tunnel.running(),
            status,
            qr_data_url: public_url.as_deref().and_then(qr_data_url),
            restart_count: state.tunnel.restart_count(),
        },
        ocr_engine_id: state.db.get_setting("ocr_engine_id").map_err(|e| e.to_string())?,
        translator_id: state.db.get_setting("translator_id").map_err(|e| e.to_string())?,
        remote_login_ready,
    })
}

#[tauri::command(async)]
fn list_ocr_engines() -> Vec<EngineInfo> {
    ocr::list_engines()
        .into_iter()
        .map(|(id, name, available)| EngineInfo {
            id: id.to_string(),
            name: name.to_string(),
            available,
        })
        .collect()
}

#[tauri::command(async)]
fn list_translators() -> Vec<EngineInfo> {
    translate::list_providers()
        .into_iter()
        .map(|(id, name, available)| EngineInfo {
            id: id.to_string(),
            name: name.to_string(),
            available,
        })
        .collect()
}

#[tauri::command(async)]
fn get_page_translation(
    state: State<'_, Arc<AppState>>,
    chapter_id: i64,
    page_index: i32,
    engine_id: Option<String>,
    translator_id: Option<String>,
) -> Result<Vec<OcrRegion>, String> {
    let engine = engine_id
        .or_else(|| state.db.get_setting("ocr_engine_id").ok().flatten())
        .unwrap_or_else(|| "apple_vision".into());
    let translator = translator_id
        .or_else(|| state.db.get_setting("translator_id").ok().flatten())
        .unwrap_or_else(|| "apple".into());

    let cache_engine = text_cache_engine(&engine);
    if let Ok(Some(cached)) = state.db.get_translation(
        chapter_id,
        page_index,
        &cache_engine,
        &translator,
        "zh-Hant",
    ) {
        if let Ok(regions) = serde_json::from_str(&cached) {
            return Ok(regions);
        }
    }

    let (tx, rx) = crossbeam_channel::bounded(1);
    state.scheduler.enqueue(JobKind::OcrTranslation {
        chapter_id,
        page_index,
        engine_id: engine,
        translator_id: translator,
        reply: Some(tx),
    });
    rx.recv_timeout(Duration::from_secs(60))
        .map_err(|e| e.to_string())?
}

#[tauri::command(async)]
fn save_translation_edits(
    state: State<'_, Arc<AppState>>,
    chapter_id: i64,
    page_index: i32,
    regions: Vec<OcrRegion>,
    engine_id: Option<String>,
    translator_id: Option<String>,
) -> Result<(), String> {
    let engine = engine_id.unwrap_or_else(|| "apple_vision".into());
    let translator = translator_id.unwrap_or_else(|| "apple".into());
    let json = serde_json::to_string(&regions).map_err(|e| e.to_string())?;
    state
        .db
        .save_ocr_regions(chapter_id, page_index, &engine, &json)
        .map_err(|e| e.to_string())?;
    state
        .db
        .save_translation(chapter_id, page_index, &engine, &translator, "zh-Hant", &json)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn update_app_settings(
    state: State<'_, Arc<AppState>>,
    cache_size_mb: Option<u32>,
    ocr_engine_id: Option<String>,
    translator_id: Option<String>,
) -> Result<(), String> {
    if let Some(mb) = cache_size_mb {
        state.cache.set_max_mb(mb);
        state
            .db
            .set_setting("cache_size_mb", &mb.to_string())
            .map_err(|e| e.to_string())?;
    }
    if let Some(id) = ocr_engine_id {
        state.db.set_setting("ocr_engine_id", &id).map_err(|e| e.to_string())?;
    }
    if let Some(id) = translator_id {
        state.db.set_setting("translator_id", &id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command(async)]
fn clear_cache(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.cache.clear();
    Ok(())
}

#[tauri::command(async)]
fn recently_added(state: State<'_, Arc<AppState>>) -> Result<Vec<Series>, String> {
    state.db.recently_added(&Viewer::SYSTEM, 24).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_collections(state: State<'_, Arc<AppState>>) -> Result<Vec<Collection>, String> {
    state.db.list_collections().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn create_collection(state: State<'_, Arc<AppState>>, name: String) -> Result<Collection, String> {
    state.db.create_collection(&name).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn delete_collection(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    state.db.delete_collection(id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn set_collection_item(
    state: State<'_, Arc<AppState>>,
    collection_id: i64,
    series_id: i64,
    add: bool,
) -> Result<(), String> {
    state
        .db
        .set_collection_item(collection_id, series_id, add)
        .map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn series_collections(state: State<'_, Arc<AppState>>, series_id: i64) -> Result<Vec<i64>, String> {
    state.db.series_collections(series_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_tags(state: State<'_, Arc<AppState>>) -> Result<Vec<Tag>, String> {
    state.db.list_tags().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn set_series_tags(
    state: State<'_, Arc<AppState>>,
    series_id: i64,
    tags: Vec<String>,
) -> Result<(), String> {
    state.db.set_series_tags(series_id, &tags).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn set_owner_password(state: State<'_, Arc<AppState>>, password: String) -> Result<(), String> {
    let hash = auth::hash_password(&password)?;
    state.db.set_user_password(1, &hash).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_users(state: State<'_, Arc<AppState>>) -> Result<Vec<User>, String> {
    state.db.list_users().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn create_user(state: State<'_, Arc<AppState>>, request: CreateUserRequest) -> Result<User, String> {
    let username = auth::validate_username(&request.username)?;
    let display = request.display_name.trim();
    if display.is_empty() {
        return Err("Display name is required".into());
    }
    let hash = auth::hash_password(&request.password)?;
    let user = state
        .db
        .create_user(&username, display, &hash, request.access_all)
        .map_err(|e| e.to_string())?;
    if !request.access_all {
        state
            .db
            .set_library_grants(user.id, &request.series_ids)
            .map_err(|e| e.to_string())?;
    }
    Ok(user)
}

#[tauri::command(async)]
fn update_user(
    state: State<'_, Arc<AppState>>,
    id: i64,
    request: UpdateUserRequest,
) -> Result<User, String> {
    if let Some(password) = request.password {
        if !password.is_empty() {
            let hash = auth::hash_password(&password)?;
            state.db.set_user_password(id, &hash).map_err(|e| e.to_string())?;
        }
    }
    state
        .db
        .update_user(
            id,
            request.display_name.as_deref(),
            request.access_all,
            request.disabled,
        )
        .map_err(|e| e.to_string())?;
    if let Some(ids) = request.series_ids {
        state
            .db
            .set_library_grants(id, &ids)
            .map_err(|e| e.to_string())?;
    }
    state.db.get_user(id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_user_grants(state: State<'_, Arc<AppState>>, user_id: i64) -> Result<Vec<i64>, String> {
    state.db.list_library_grants(user_id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn list_sessions(state: State<'_, Arc<AppState>>) -> Result<Vec<ActiveSession>, String> {
    state.db.list_auth_sessions().map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn revoke_session(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
    state.db.delete_auth_session(id).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn set_remote_credentials(
    state: State<'_, Arc<AppState>>,
    hostname: String,
    token: Option<String>,
) -> Result<RemoteSettings, String> {
    let host = tunnel::normalize_hostname(&hostname)?;
    state
        .db
        .set_setting("remote_hostname", &host)
        .map_err(|e| e.to_string())?;
    if let Some(token) = token {
        let token = token.trim();
        if !token.is_empty() {
            tunnel::save_token(token)?;
        }
    }
    get_settings(state).map(|s| s.remote)
}

#[tauri::command(async)]
async fn set_remote_config(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
) -> Result<RemoteSettings, String> {
    if enabled {
        state.tunnel.stop();
        if let Err(error) = start_cloud_service(&state).await {
            state.tunnel.mark_error(&error);
            return Err(error);
        }
        if let Err(error) = start_tunnel(&state) {
            state.keep_awake.stop();
            state.tunnel.stop();
            state.tunnel.mark_error(&error);
            state.cloud.stop().await;
            return Err(error);
        }
        let awake = Arc::clone(&state);
        tokio::task::spawn_blocking(move || awake.keep_awake.start())
            .await
            .ok();
        state
            .db
            .set_setting("remote_enabled", "true")
            .map_err(|e| e.to_string())?;
    } else {
        state
            .db
            .set_setting("remote_enabled", "false")
            .map_err(|e| e.to_string())?;
        let awake = Arc::clone(&state);
        tokio::task::spawn_blocking(move || awake.keep_awake.stop())
            .await
            .ok();
        state.tunnel.stop();
        state.cloud.stop().await;
    }

    get_settings(state).map(|s| s.remote)
}

#[tauri::command(async)]
fn rescan_library(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    let roots = state.db.list_roots().map_err(|e| e.to_string())?;
    for root in roots {
        state.scheduler.enqueue(JobKind::ScanRoot {
            root_id: root.id,
            path: root.path,
        });
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("shelf=info".parse().unwrap()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("shelf-media", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let state = app.state::<Arc<AppState>>();
                responder.respond(media::protocol_response(&state.db, &state.cache, &request));
            });
        })
        .setup(|app| {
            let data_dir = app_data_dir(&app.handle());
            let web_root = resolve_web_root(&app.handle());
            let upload_dir = data_dir.join("uploads");
            let cache_dir = data_dir.join("cache");
            let mut pdfium_dirs = Vec::new();
            if let Ok(resource_dir) = app.path().resource_dir() {
                pdfium_dirs.push(resource_dir.join("resources/pdfium"));
                pdfium_dirs.push(resource_dir.join("pdfium"));
            }
            pdf::set_search_paths(pdfium_dirs);
            let db = Arc::new(Database::open(data_dir.clone()).expect("Failed to open database"));
            let _ = db.set_setting("cf_tunnel_token", "");
            let cache_mb = db
                .get_setting("cache_size_mb")
                .ok()
                .flatten()
                .and_then(|v| v.parse().ok())
                .unwrap_or(512);
            let cache = PageCache::new(cache_dir, cache_mb);
            let (scheduler, notify_rx) = JobScheduler::new();
            let indexer = Arc::new(Indexer::new(Arc::clone(&db), Arc::clone(&scheduler)));
            let tunnel_manager = Arc::new(tunnel::TunnelManager::new());
            tunnel::start_supervisor(Arc::clone(&tunnel_manager));

            let state = Arc::new(AppState {
                db: Arc::clone(&db),
                cache,
                scheduler: Arc::clone(&scheduler),
                indexer: Arc::clone(&indexer),
                cloud: CloudServer::new(),
                service_port: 7834,
                watcher: Mutex::new(None),
                tunnel: tunnel_manager,
                keep_awake: keep_awake::KeepAwake::new(),
                login_limiter: auth::LoginLimiter::new(),
                uploads: UploadManager::new(),
                web_root,
                upload_dir,
            });

            state.setup_job_handler();
            scheduler.start_worker(notify_rx);
            app.manage(Arc::clone(&state));

            std::thread::Builder::new()
                .name("preview-init".into())
                .spawn(move || {
                    state.cache.scan_existing();
                    match state.db.list_roots() {
                        Ok(roots) => {
                            for root in roots {
                                if let Err(error) = state.indexer.scan_root(root.id, &root.path) {
                                    tracing::warn!(root = %root.path, "Could not scan library root on startup: {error}");
                                }
                            }
                        }
                        Err(error) => tracing::warn!("Could not load library roots on startup: {error}"),
                    }
                    if let Err(error) = restart_all_watchers(&state) {
                        tracing::warn!("Could not initialize library watchers: {error}");
                    }
                    let _ = db.set_setting("lan_enabled", "false");
                    let _ = db.set_setting("lan_url", "");
                    if remote_enabled(&db) {
                        let s = Arc::clone(&state);
                        tauri::async_runtime::spawn(async move {
                            if let Err(error) = start_cloud_service(&s).await {
                                tracing::warn!("Could not restore cloud service: {error}");
                            }
                            if let Err(e) = start_tunnel(&s) {
                                s.keep_awake.stop();
                                tracing::warn!("Could not restore Cloudflare tunnel: {e}");
                                return;
                            }
                            let awake = Arc::clone(&s);
                            tokio::task::spawn_blocking(move || awake.keep_awake.start())
                                .await
                                .ok();
                        });
                    }
                })
                .ok();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            add_library_root,
            list_library_roots,
            remove_library_root,
            list_series,
            get_series,
            update_series,
            list_chapters,
            get_chapter,
            get_page_count,
            get_adjacent_chapter,
            get_page_image,
            get_chapter_tiles,
            get_chapter_tile,
            prefetch_pages,
            save_progress,
            get_progress,
            continue_reading,
            reading_history,
            clear_reading_history,
            get_cover_image,
            get_index_status,
            get_settings,
            list_ocr_engines,
            list_translators,
            get_page_translation,
            save_translation_edits,
            update_app_settings,
            clear_cache,
            rescan_library,
            recently_added,
            list_collections,
            create_collection,
            delete_collection,
            set_collection_item,
            series_collections,
            list_tags,
            set_series_tags,
            set_owner_password,
            list_users,
            create_user,
            update_user,
            list_user_grants,
            list_sessions,
            revoke_session,
            set_remote_credentials,
            set_remote_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(y: f64, height: f64) -> OcrRegion {
        OcrRegion {
            id: "r".into(),
            x: 0.25,
            y,
            width: 0.5,
            height,
            text: "text".into(),
            translated_text: None,
            vertical: false,
            hidden: false,
        }
    }

    #[test]
    fn segment_regions_map_onto_the_whole_page() {
        // A page of 18 segments, 2400px each, as a webtoon strip produces.
        let page_height = 43_200;

        let first = region_to_page_space(region(0.0, 0.5), 0, 2400, page_height);
        assert!((first.y - 0.0).abs() < 1e-9);
        assert!((first.height - 0.5 * 2400.0 / 43_200.0).abs() < 1e-9);

        // The tenth segment starts exactly halfway down the page.
        let middle = region_to_page_space(region(0.0, 0.0), 9 * 2400, 2400, page_height);
        assert!((middle.y - 0.5).abs() < 1e-9);

        // Halfway into that segment stays inside its own band.
        let inside = region_to_page_space(region(0.5, 0.0), 9 * 2400, 2400, page_height);
        assert!((inside.y - (9.5 * 2400.0 / 43_200.0)).abs() < 1e-9);

        // The bottom of the last segment is the bottom of the page.
        let last = region_to_page_space(region(1.0, 0.0), 17 * 2400, 2400, page_height);
        assert!((last.y - 1.0).abs() < 1e-9);

        // Horizontal placement is unaffected.
        assert!((last.x - 0.25).abs() < 1e-9);
        assert!((last.width - 0.5).abs() < 1e-9);
    }

    #[test]
    fn single_segment_page_keeps_regions_unchanged() {
        let mapped = region_to_page_space(region(0.4, 0.2), 0, 1600, 1600);
        assert!((mapped.y - 0.4).abs() < 1e-9);
        assert!((mapped.height - 0.2).abs() < 1e-9);
    }

    #[test]
    fn empty_page_height_is_not_a_divide_by_zero() {
        let mapped = region_to_page_space(region(0.4, 0.2), 0, 0, 0);
        assert!((mapped.y - 0.4).abs() < 1e-9);
    }
}
