use axum::{
    body::{to_bytes, Body},
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post, put},
    Extension, Json, Router,
};
use parking_lot::Mutex;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use tracing::info;

use crate::auth;
use crate::models::{
    AuthSession, CreateFolderRequest, InitUploadRequest, LoginRequest, ReadingProgress,
    SeriesFilter, UploadStatus,
};
use crate::pdf;
use crate::AppState;

const MAX_UPLOAD_BYTES: u64 = 8 * 1024 * 1024 * 1024;
const MAX_CHUNK_BYTES: u64 = 8 * 1024 * 1024;
const CSP: &str = "default-src 'self'; img-src 'self' blob: data:; media-src 'self' blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; font-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'";
const CSP_HTTPS: &str = "default-src 'self'; img-src 'self' blob: data:; media-src 'self' blob:; style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; font-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'; upgrade-insecure-requests";

#[derive(Clone)]
pub struct HttpState {
    pub app: Arc<AppState>,
}

pub struct CloudServer {
    shutdown: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    stopped: Mutex<Option<tokio::sync::oneshot::Receiver<()>>>,
    running: AtomicBool,
    lan_bind: AtomicBool,
}

impl CloudServer {
    pub fn new() -> Self {
        Self {
            shutdown: Mutex::new(None),
            stopped: Mutex::new(None),
            running: AtomicBool::new(false),
            lan_bind: AtomicBool::new(false),
        }
    }

    pub fn lan_bind(&self) -> bool {
        self.lan_bind.load(Ordering::Acquire)
    }

    pub async fn start(&self, app: Arc<AppState>, port: u16, lan_bind: bool) -> Result<String, String> {
        if self.running() && self.lan_bind() == lan_bind {
            return Ok(format!("http://127.0.0.1:{port}"));
        }
        self.stop().await;

        let state = HttpState {
            app: Arc::clone(&app),
        };

        let public_api = Router::new()
            .route("/health", get(health))
            .route("/auth/login", post(login));

        let protected = Router::new()
            .route("/auth/session", get(current_session))
            .route("/auth/logout", post(logout))
            .route("/library/series", get(list_series))
            .route("/library/series/{id}", get(get_series))
            .route("/library/series/{id}/chapters", get(list_chapters))
            .route("/library/series/{id}/files", post(init_series_upload))
            .route("/library/folders", post(create_folder))
            .route("/library/continue", get(continue_reading))
            .route("/library/recent", get(recently_added))
            .route(
                "/library/history",
                get(reading_history).delete(clear_reading_history),
            )
            .route("/library/collections", get(list_collections))
            .route("/uploads/{id}", put(put_upload_chunk).get(get_upload_status))
            .route("/uploads/{id}/complete", post(complete_upload))
            .route("/covers/{id}", get(get_cover))
            .route("/chapters/{id}", get(get_chapter))
            .route("/chapters/{id}/page-count", get(get_page_count))
            .route("/chapters/{id}/page/{page}", get(get_page))
            .route("/chapters/{id}/tiles", get(get_chapter_tiles))
            .route("/chapters/{id}/tile/{page}/{tile}", get(get_tile))
            .route("/chapters/{id}/file", get(get_chapter_file))
            .route(
                "/chapters/{id}/progress",
                get(get_progress).post(save_progress),
            )
            .route("/chapters/{id}/adjacent", get(get_adjacent))
            .route("/chapters/{id}/translation/{page}", get(get_translation))
            .route_layer(middleware::from_fn_with_state(
                state.clone(),
                session_guard,
            ));

        let router = Router::new()
            .nest("/api", public_api.merge(protected))
            .fallback(spa_fallback)
            .layer(middleware::from_fn_with_state(state.clone(), host_guard))
            .layer(middleware::from_fn_with_state(state.clone(), https_redirect))
            .layer(middleware::from_fn(security_headers))
            .with_state(state);

        let addr = if lan_bind {
            SocketAddr::from(([0, 0, 0, 0], port))
        } else {
            SocketAddr::from(([127, 0, 0, 1], port))
        };
        let listener = bind_with_retry(addr).await?;
        let url = format!("http://127.0.0.1:{port}");

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        *self.shutdown.lock() = Some(shutdown_tx);
        *self.stopped.lock() = Some(done_rx);
        self.running.store(true, Ordering::Release);
        self.lan_bind.store(lan_bind, Ordering::Release);

        tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .ok();
            let _ = done_tx.send(());
        });

        info!(lan_bind, "Library origin listening at {url}");
        Ok(url)
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::Release);
        self.lan_bind.store(false, Ordering::Release);
        if let Some(tx) = self.shutdown.lock().take() {
            let _ = tx.send(());
        }
        let stopped = self.stopped.lock().take();
        if let Some(rx) = stopped {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), rx).await;
        }
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

pub struct UploadManager {
    inner: Mutex<HashMap<String, UploadState>>,
}

struct UploadState {
    user_id: i64,
    series_id: i64,
    file_name: String,
    size: u64,
    received: u64,
    path: PathBuf,
}

impl UploadManager {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    for part in cookie.split(';') {
        let part = part.trim();
        if let Some(token) = part
            .strip_prefix("shelf_session=")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            return Some(token);
        }
    }
    None
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

fn request_host(headers: &HeaderMap) -> String {
    headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn request_is_https(headers: &HeaderMap) -> bool {
    if headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| {
            s.split(',')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("https")
        })
    {
        return true;
    }
    headers
        .get("cf-visitor")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.to_ascii_lowercase().contains("\"scheme\":\"https\""))
}

fn configured_public_host(state: &HttpState) -> Option<String> {
    let stored = state.app.db.get_setting("remote_hostname").ok().flatten();
    crate::tunnel::resolve_hostname(stored.as_deref())
}

fn public_cookie_domain(headers: &HeaderMap, configured: &str) -> Option<String> {
    let host = request_host(headers);
    if configured.is_empty() {
        return None;
    }
    if crate::tunnel::host_matches_configured(&host, configured) {
        Some(configured.to_string())
    } else {
        None
    }
}

fn session_cookie_header(token: &str, headers: &HeaderMap, state: &HttpState) -> String {
    let configured = configured_public_host(state).unwrap_or_default();
    auth::session_cookie(
        token,
        request_is_https(headers),
        public_cookie_domain(headers, &configured).as_deref(),
    )
}

fn clear_session_cookie_header(headers: &HeaderMap, state: &HttpState) -> String {
    let configured = configured_public_host(state).unwrap_or_default();
    auth::clear_session_cookie(
        request_is_https(headers),
        public_cookie_domain(headers, &configured).as_deref(),
    )
}

fn client_ip(headers: &HeaderMap, addr: Option<SocketAddr>) -> String {
    if let Some(ip) = headers
        .get("cf-connecting-ip")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return ip.to_string();
    }
    addr.map(|a| a.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn connect_addr(req: &Request) -> Option<SocketAddr> {
    req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0)
}

async fn security_headers(req: Request, next: Next) -> Response {
    let https = request_is_https(req.headers());
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    headers.insert(
        header::HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::HeaderName::from_static("x-frame-options"),
        HeaderValue::from_static("DENY"),
    );
    headers.insert(
        header::HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(if https { CSP_HTTPS } else { CSP }),
    );
    if https {
        headers.insert(
            header::HeaderName::from_static("strict-transport-security"),
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        );
    }
    res
}

async fn https_redirect(State(state): State<HttpState>, req: Request, next: Next) -> Response {
    let Some(configured) = configured_public_host(&state) else {
        return next.run(req).await;
    };
    let host = request_host(req.headers());
    let public = crate::tunnel::host_matches_configured(&host, &configured);
    if !public || request_is_https(req.headers()) {
        return next.run(req).await;
    }
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return next.run(req).await;
    }
    let path = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/");
    let location = format!("https://{}{path}", configured);
    (
        StatusCode::MOVED_PERMANENTLY,
        [(header::LOCATION, location)],
    )
        .into_response()
}

async fn host_guard(State(state): State<HttpState>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let configured = configured_public_host(&state);
    let lan = current_lan_identity(&state);
    if crate::lan::host_allowed(host, configured.as_deref(), lan.as_ref()) {
        next.run(req).await
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "Unknown host" })),
        )
            .into_response()
    }
}

fn current_lan_identity(state: &HttpState) -> Option<crate::lan::LanIdentity> {
    if crate::lan::enabled(&state.app.db) {
        Some(state.app.lan.identity())
    } else {
        None
    }
}

async fn session_guard(State(state): State<HttpState>, mut req: Request, next: Next) -> Response {
    let headers = req.headers().clone();
    let token = bearer_token(&headers)
        .or_else(|| cookie_token(&headers))
        .unwrap_or_default();
    match auth::lookup_session(&state.app.db, &token) {
        Ok(Some(session)) => {
            let rotated = auth::touch_session(&state.app.db, &session).ok().flatten();
            req.extensions_mut().insert(session);
            let mut response = next.run(req).await;
            if let Some(new_token) = rotated {
                if let Ok(value) = HeaderValue::from_str(&session_cookie_header(&new_token, &headers, &state)) {
                    response.headers_mut().append(header::SET_COOKIE, value);
                }
            }
            response
        }
        Ok(None) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Unauthorized" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e })),
        )
            .into_response(),
    }
}

async fn health(State(state): State<HttpState>, headers: HeaderMap) -> impl IntoResponse {
    let login_ready = state.app.db.owner_password_set().unwrap_or(false);
    let lan_on = crate::lan::enabled(&state.app.db);
    let host = request_host(&headers);
    let identity = current_lan_identity(&state);
    let on_lan = identity
        .as_ref()
        .is_some_and(|id| id.matches_host(&host));
    Json(serde_json::json!({
        "status": "ok",
        "loginReady": login_ready,
        "lan": lan_on,
        "localUrl": if on_lan { identity.map(|id| id.url) } else { None::<String> },
    }))
}

async fn login(State(state): State<HttpState>, req: Request) -> Result<Response, ApiError> {
    let headers = req.headers().clone();
    let ip = client_ip(&headers, connect_addr(&req));
    let bytes = to_bytes(req.into_body(), 64 * 1024)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::BAD_REQUEST,
            error: e.to_string(),
        })?;
    let body: LoginRequest = serde_json::from_slice(&bytes).map_err(|e| ApiError {
        status: StatusCode::BAD_REQUEST,
        error: e.to_string(),
    })?;
    match auth::login(
        &state.app.db,
        &body,
        &state.app.login_limiter,
        &ip,
    ) {
        Ok(issued) => {
            let cookie: HeaderValue = session_cookie_header(&issued.token, &headers, &state)
                .parse()
                .map_err(|e: axum::http::header::InvalidHeaderValue| ApiError {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    error: e.to_string(),
                })?;
            let mut payload = serde_json::to_value(&issued.info).map_err(|e| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                error: e.to_string(),
            })?;
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("token".into(), serde_json::json!(issued.token));
            }
            let mut response = Json(payload).into_response();
            response.headers_mut().append(header::SET_COOKIE, cookie);
            Ok(response)
        }
        Err(e) => {
            let status = if e.contains("Too many") {
                StatusCode::TOO_MANY_REQUESTS
            } else if e.contains("not configured") {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNAUTHORIZED
            };
            Err(ApiError { status, error: e })
        }
    }
}

async fn logout(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    headers: HeaderMap,
) -> Response {
    let _ = state.app.db.delete_auth_session(session.id);
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&clear_session_cookie_header(&headers, &state)) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
    response
}

async fn current_session(Extension(session): Extension<AuthSession>) -> impl IntoResponse {
    Json(serde_json::json!({
        "userId": session.viewer.user_id,
        "username": session.username,
        "displayName": session.display_name,
        "role": session.role,
        "accessAll": session.viewer.access_all,
        "expiresAt": session.idle_expires_at,
    }))
}

#[derive(Deserialize)]
struct SeriesQuery {
    search: Option<String>,
    favorites: Option<bool>,
    sort: Option<String>,
    unread: Option<bool>,
    collection_id: Option<i64>,
    reading_mode: Option<String>,
    content_type: Option<String>,
}

async fn list_series(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Query(q): Query<SeriesQuery>,
) -> Result<Response, ApiError> {
    state
        .app
        .db
        .list_series_filtered(
            &session.viewer,
            &SeriesFilter {
                search: q.search,
                favorites_only: q.favorites.unwrap_or(false),
                unread_only: q.unread.unwrap_or(false),
                collection_id: q.collection_id,
                reading_mode: q.reading_mode,
                content_type: q.content_type,
                sort: q.sort.unwrap_or_else(|| "title".into()),
            },
        )
        .map(|v| Json(v).into_response())
        .map_err(|e| ApiError::from(e.to_string()))
}

async fn get_series(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    state
        .app
        .db
        .get_series(&session.viewer, id)
        .map(|v| Json(v).into_response())
        .map_err(|e| not_found(e.to_string()))
}

async fn list_chapters(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    state
        .app
        .db
        .list_chapters(&session.viewer, id)
        .map(|v| Json(v).into_response())
        .map_err(|e| ApiError::from(e.to_string()))
}

async fn continue_reading(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
) -> Result<Response, ApiError> {
    state
        .app
        .db
        .continue_reading(&session.viewer, 10)
        .map(|v| Json(v).into_response())
        .map_err(|e| ApiError::from(e.to_string()))
}

async fn recently_added(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
) -> Result<Response, ApiError> {
    state
        .app
        .db
        .recently_added(&session.viewer, 24)
        .map(|v| Json(v).into_response())
        .map_err(|e| ApiError::from(e.to_string()))
}

async fn reading_history(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
) -> Result<Response, ApiError> {
    state
        .app
        .db
        .reading_history(&session.viewer, 20)
        .map(|v| Json(v).into_response())
        .map_err(|e| ApiError::from(e.to_string()))
}

async fn clear_reading_history(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
) -> Result<StatusCode, ApiError> {
    state
        .app
        .db
        .clear_reading_history(&session.viewer)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|e| ApiError::from(e.to_string()))
}

async fn list_collections(State(state): State<HttpState>) -> Result<Response, ApiError> {
    state
        .app
        .db
        .list_collections()
        .map(|v| Json(v).into_response())
        .map_err(|e| ApiError::from(e.to_string()))
}

async fn get_chapter(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    state
        .app
        .db
        .get_chapter(&session.viewer, id)
        .map(|v| Json(v).into_response())
        .map_err(|e| not_found(e.to_string()))
}

async fn get_page_count(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    let chapter = state
        .app
        .db
        .get_chapter(&session.viewer, id)
        .map_err(|e| not_found(e.to_string()))?;
    if !crate::media::is_pdf_path(&chapter.file_path) {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Page count is only available for PDF files".into(),
        });
    }
    pdf::get_page_count_urgent(&chapter.file_path)
        .map(|count| Json(count).into_response())
        .map_err(|e| ApiError::from(e.to_string()))
}

#[derive(Deserialize)]
struct FileQuery {
    download: Option<bool>,
}

async fn get_chapter_file(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
    Query(q): Query<FileQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let path = crate::media::resolve_chapter_file(&state.app.db, &session.viewer, id).map_err(
        |error| ApiError {
            status: StatusCode::NOT_FOUND,
            error,
        },
    )?;
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let plan = crate::media::plan_file_range(&path, range).map_err(ApiError::from)?;
    let mut response_headers = crate::media::range_headers(&plan);
    if q.download.unwrap_or(false) {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("download");
        if let Ok(value) = HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")) {
            response_headers.insert(header::CONTENT_DISPOSITION, value);
        }
    }
    if plan.length == 0 {
        return Ok((plan.status, response_headers, Body::empty()).into_response());
    }
    let mut file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| ApiError::from(e.to_string()))?;
    file.seek(std::io::SeekFrom::Start(plan.start))
        .await
        .map_err(|e| ApiError::from(e.to_string()))?;
    let stream = ReaderStream::new(file.take(plan.length));
    Ok((plan.status, response_headers, Body::from_stream(stream)).into_response())
}

#[derive(Deserialize)]
struct PageQuery {
    width: Option<u32>,
}

async fn get_page(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path((id, page)): Path<(i64, i32)>,
    Query(q): Query<PageQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let chapter = state
        .app
        .db
        .get_chapter(&session.viewer, id)
        .map_err(|e| not_found(e.to_string()))?;
    if chapter.missing {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Chapter file is missing".into(),
        });
    }
    if !crate::media::is_pdf_path(&chapter.file_path) {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Page rendering is only available for PDF files".into(),
        });
    }
    let width = q.width.unwrap_or(1200).min(2560);
    let (bytes, w, h, _cached) = pdf::render_page_bytes(
        &state.app.cache,
        id,
        &chapter.file_path,
        page,
        width,
    )
    .map_err(|e| ApiError::from(e.to_string()))?;

    let etag = format!("\"{id}-{page}-{width}-{}\"", bytes.len());
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(etag.as_str())
    {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }
    Ok((
        [
            (header::CONTENT_TYPE, crate::pdf::image_mime(&bytes).to_string()),
            (header::CACHE_CONTROL, "private, max-age=86400".to_string()),
            (header::ETAG, etag),
            (header::HeaderName::from_static("x-image-width"), w.to_string()),
            (header::HeaderName::from_static("x-image-height"), h.to_string()),
        ],
        bytes,
    )
        .into_response())
}

async fn get_chapter_tiles(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
    Query(q): Query<PageQuery>,
) -> Result<Response, ApiError> {
    let chapter = state
        .app
        .db
        .get_chapter(&session.viewer, id)
        .map_err(|e| not_found(e.to_string()))?;
    if !crate::media::is_pdf_path(&chapter.file_path) {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Page rendering is only available for PDF files".into(),
        });
    }
    let width = q.width.unwrap_or(1200).min(2560);
    let tiles = pdf::chapter_tiles(&chapter.file_path, width).map_err(|e| ApiError::from(e.to_string()))?;
    let tiles: Vec<crate::models::PageTile> = tiles
        .into_iter()
        .map(|t| crate::models::PageTile {
            page_index: t.page_index,
            tile_index: t.tile_index,
            width: t.width,
            height: t.height,
        })
        .collect();
    Ok(Json(tiles).into_response())
}

async fn get_tile(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path((id, page, tile)): Path<(i64, i32, u32)>,
    Query(q): Query<PageQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let chapter = state
        .app
        .db
        .get_chapter(&session.viewer, id)
        .map_err(|e| not_found(e.to_string()))?;
    if chapter.missing {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Chapter file is missing".into(),
        });
    }
    if !crate::media::is_pdf_path(&chapter.file_path) {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Page rendering is only available for PDF files".into(),
        });
    }
    let width = q.width.unwrap_or(1200).min(2560);
    let (bytes, w, h, _cached) = pdf::render_tile_bytes(
        &state.app.cache,
        id,
        &chapter.file_path,
        page,
        tile,
        width,
        true,
    )
    .map_err(|e| ApiError::from(e.to_string()))?;

    let etag = format!("\"{id}-{page}-t{tile}-{width}-{}\"", bytes.len());
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(etag.as_str())
    {
        return Ok(StatusCode::NOT_MODIFIED.into_response());
    }
    Ok((
        [
            (header::CONTENT_TYPE, crate::pdf::image_mime(&bytes).to_string()),
            (header::CACHE_CONTROL, "private, max-age=86400".to_string()),
            (header::ETAG, etag),
            (header::HeaderName::from_static("x-image-width"), w.to_string()),
            (header::HeaderName::from_static("x-image-height"), h.to_string()),
        ],
        bytes,
    )
        .into_response())
}

async fn get_cover(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    if !state
        .app
        .db
        .can_view_series(&session.viewer, id)
        .map_err(|e| ApiError::from(e.to_string()))?
    {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "No cover".into(),
        });
    }
    let Some(bytes) = state.app.cache.get_cover(id) else {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "No cover".into(),
        });
    };
    Ok((
        [
            (header::CONTENT_TYPE, crate::pdf::image_mime(&bytes)),
            (header::CACHE_CONTROL, "private, max-age=86400"),
        ],
        bytes,
    )
        .into_response())
}

async fn get_progress(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
) -> Result<Response, ApiError> {
    state
        .app
        .db
        .get_progress(&session.viewer, id)
        .map(|v| Json(v).into_response())
        .map_err(|e| ApiError::from(e.to_string()))
}

async fn save_progress(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
    Json(mut progress): Json<ReadingProgress>,
) -> Result<StatusCode, ApiError> {
    progress.chapter_id = id;
    state
        .app
        .db
        .save_progress(&session.viewer, &progress)
        .map_err(|e| ApiError::from(e.to_string()))?;
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct AdjacentQuery {
    next: Option<bool>,
}

async fn get_adjacent(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<i64>,
    Query(q): Query<AdjacentQuery>,
) -> Result<Response, ApiError> {
    state
        .app
        .db
        .get_adjacent_chapter(&session.viewer, id, q.next.unwrap_or(true))
        .map(|v| Json(v).into_response())
        .map_err(|e| ApiError::from(e.to_string()))
}

async fn get_translation(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path((id, page)): Path<(i64, i32)>,
) -> Result<Response, ApiError> {
    let _chapter = state
        .app
        .db
        .get_chapter(&session.viewer, id)
        .map_err(|e| not_found(e.to_string()))?;
    let engine = state
        .app
        .db
        .get_setting("ocr_engine_id")
        .ok()
        .flatten()
        .unwrap_or_else(|| "apple_vision".into());
    let translator = state
        .app
        .db
        .get_setting("translator_id")
        .ok()
        .flatten()
        .unwrap_or_else(|| "apple".into());
    let app = Arc::clone(&state.app);
    let engine_c = engine.clone();
    let translator_c = translator.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::run_ocr_pipeline(&app, id, page, &engine_c, &translator_c)
    })
    .await
    .map_err(|e| ApiError::from(e.to_string()))?;
    match result {
        Ok(regions) => Ok(Json(regions).into_response()),
        Err(e) => Err(ApiError::from(e)),
    }
}

async fn create_folder(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Json(body): Json<CreateFolderRequest>,
) -> Result<Response, ApiError> {
    if !session.viewer.access_all {
        return Err(ApiError {
            status: StatusCode::FORBIDDEN,
            error: "Only full-library accounts can create folders".into(),
        });
    }
    let name = sanitize_folder_name(&body.name)?;
    let root = state
        .app
        .db
        .get_root(body.root_id)
        .map_err(|e| ApiError::from(e.to_string()))?;
    let dest = FsPath::new(&root.path).join(&name);
    let root_canon = FsPath::new(&root.path)
        .canonicalize()
        .map_err(|e| ApiError::from(e.to_string()))?;
    if dest.exists() {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            error: "A folder with that name already exists".into(),
        });
    }
    std::fs::create_dir(&dest).map_err(|e| ApiError::from(e.to_string()))?;
    let dest_canon = dest.canonicalize().map_err(|e| ApiError::from(e.to_string()))?;
    if !dest_canon.starts_with(&root_canon) {
        let _ = std::fs::remove_dir(&dest_canon);
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Folder is not inside a library root".into(),
        });
    }
    state
        .app
        .indexer
        .scan_root(root.id, &root.path)
        .map_err(ApiError::from)?;
    Ok(StatusCode::CREATED.into_response())
}

async fn init_series_upload(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(series_id): Path<i64>,
    Json(mut body): Json<InitUploadRequest>,
) -> Result<Response, ApiError> {
    body.series_id = series_id;
    init_upload_inner(&state, &session, body).await
}

async fn init_upload_inner(
    state: &HttpState,
    session: &AuthSession,
    body: InitUploadRequest,
) -> Result<Response, ApiError> {
    if !state
        .app
        .db
        .can_view_series(&session.viewer, body.series_id)
        .map_err(|e| ApiError::from(e.to_string()))?
    {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Series not found".into(),
        });
    }
    if body.size == 0 || body.size > MAX_UPLOAD_BYTES {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "File size is not allowed".into(),
        });
    }
    let file_name = sanitize_filename(&body.file_name)?;
    let upload_id = auth::random_token();
    std::fs::create_dir_all(&state.app.upload_dir).map_err(|e| ApiError::from(e.to_string()))?;
    let path = state.app.upload_dir.join(&upload_id);
    std::fs::File::create(&path).map_err(|e| ApiError::from(e.to_string()))?;
    state.app.uploads.inner.lock().insert(
        upload_id.clone(),
        UploadState {
            user_id: session.viewer.user_id,
            series_id: body.series_id,
            file_name,
            size: body.size,
            received: 0,
            path,
        },
    );
    Ok(Json(UploadStatus {
        upload_id,
        offset: 0,
        size: body.size,
    })
    .into_response())
}

async fn get_upload_status(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let uploads = state.app.uploads.inner.lock();
    let Some(item) = uploads.get(&id) else {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Upload not found".into(),
        });
    };
    if item.user_id != session.viewer.user_id {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Upload not found".into(),
        });
    }
    Ok(Json(UploadStatus {
        upload_id: id,
        offset: item.received,
        size: item.size,
    })
    .into_response())
}

async fn put_upload_chunk(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let bytes = to_bytes(body, MAX_CHUNK_BYTES as usize + 1)
        .await
        .map_err(|e| ApiError {
            status: StatusCode::BAD_REQUEST,
            error: e.to_string(),
        })?;
    if bytes.len() as u64 > MAX_CHUNK_BYTES {
        return Err(ApiError {
            status: StatusCode::PAYLOAD_TOO_LARGE,
            error: "Chunk is too large".into(),
        });
    }
    let mut uploads = state.app.uploads.inner.lock();
    let Some(item) = uploads.get_mut(&id) else {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Upload not found".into(),
        });
    };
    if item.user_id != session.viewer.user_id {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Upload not found".into(),
        });
    }
    let offset = parse_upload_offset(headers.get(header::CONTENT_RANGE).and_then(|v| v.to_str().ok()))
        .unwrap_or(item.received);
    if offset != item.received {
        return Ok(Json(UploadStatus {
            upload_id: id,
            offset: item.received,
            size: item.size,
        })
        .into_response());
    }
    if item.received + bytes.len() as u64 > item.size {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Upload exceeds declared size".into(),
        });
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&item.path)
        .map_err(|e| ApiError::from(e.to_string()))?;
    file.write_all(&bytes)
        .map_err(|e| ApiError::from(e.to_string()))?;
    item.received += bytes.len() as u64;
    Ok(Json(UploadStatus {
        upload_id: id,
        offset: item.received,
        size: item.size,
    })
    .into_response())
}

async fn complete_upload(
    State(state): State<HttpState>,
    Extension(session): Extension<AuthSession>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let item = {
        let mut uploads = state.app.uploads.inner.lock();
        uploads.remove(&id)
    };
    let Some(item) = item else {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Upload not found".into(),
        });
    };
    if item.user_id != session.viewer.user_id {
        return Err(ApiError {
            status: StatusCode::NOT_FOUND,
            error: "Upload not found".into(),
        });
    }
    if item.received != item.size {
        let _ = std::fs::remove_file(&item.path);
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Upload is incomplete".into(),
        });
    }
    let series = state
        .app
        .db
        .get_series(&session.viewer, item.series_id)
        .map_err(|e| not_found(e.to_string()))?;
    let dest_dir = PathBuf::from(&series.folder_path);
    let dest = unique_dest(&dest_dir, &item.file_name);
    let dest_canon_parent = dest_dir
        .canonicalize()
        .map_err(|e| ApiError::from(e.to_string()))?;
    std::fs::rename(&item.path, &dest).or_else(|_| {
        std::fs::copy(&item.path, &dest).map(|_| {
            let _ = std::fs::remove_file(&item.path);
        })
    }).map_err(|e| ApiError::from(e.to_string()))?;
    let dest_canon = dest.canonicalize().map_err(|e| ApiError::from(e.to_string()))?;
    if !dest_canon.starts_with(&dest_canon_parent) {
        let _ = std::fs::remove_file(&dest_canon);
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Upload escaped the series folder".into(),
        });
    }
    let _ = state
        .app
        .indexer
        .handle_file_added(&dest_canon.to_string_lossy());
    Ok(StatusCode::CREATED.into_response())
}

fn parse_upload_offset(header: Option<&str>) -> Option<u64> {
    let header = header?.trim();
    let spec = header.strip_prefix("bytes ")?;
    let (range, _) = spec.split_once('/')?;
    let (start, _) = range.split_once('-')?;
    start.parse().ok()
}

fn unique_dest(dir: &FsPath, file_name: &str) -> PathBuf {
    let candidate = dir.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = FsPath::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = FsPath::new(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    for i in 1..1000 {
        let name = if ext.is_empty() {
            format!("{stem}-{i}")
        } else {
            format!("{stem}-{i}.{ext}")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
    }
    dir.join(format!("{file_name}.new"))
}

fn sanitize_filename(name: &str) -> Result<String, ApiError> {
    let base = FsPath::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .replace(['/', '\\', '\0'], "");
    if base.is_empty() || base == "." || base == ".." || base.starts_with('.') {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Invalid file name".into(),
        });
    }
    if base.len() > 180 {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "File name is too long".into(),
        });
    }
    let lower = base.to_ascii_lowercase();
    if !(lower.ends_with(".pdf") || lower.ends_with(".mp4")) {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Only PDF and MP4 files can be uploaded".into(),
        });
    }
    Ok(base)
}

fn sanitize_folder_name(name: &str) -> Result<String, ApiError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 80 {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Folder name must be 1–80 characters".into(),
        });
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') || trimmed == "." || trimmed == ".." {
        return Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: "Invalid folder name".into(),
        });
    }
    Ok(trimmed.to_string())
}

async fn spa_fallback(State(state): State<HttpState>, uri: Uri) -> Response {
    let dist = &state.app.web_root;
    if let Some((mime, bytes)) = safe_static_file(dist, uri.path()) {
        return ([(header::CONTENT_TYPE, mime)], bytes).into_response();
    }
    let index = dist.join("index.html");
    match std::fs::read_to_string(index) {
        Ok(html) => Html(html).into_response(),
        Err(_) => (
            StatusCode::NOT_FOUND,
            "Shelf web client is not built yet. Run npm run build.",
        )
            .into_response(),
    }
}

pub fn safe_static_file(dist: &FsPath, uri_path: &str) -> Option<(&'static str, Vec<u8>)> {
    let rel = uri_path.trim_start_matches('/');
    if rel.is_empty() || rel.starts_with("api/") {
        return None;
    }
    if rel.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
        return None;
    }
    let file = dist.join(rel);
    let dist_canon = dist.canonicalize().ok()?;
    let canon = file.canonicalize().ok()?;
    if !canon.starts_with(&dist_canon) || !canon.is_file() {
        return None;
    }
    let bytes = std::fs::read(&canon).ok()?;
    Some((mime_guess(&canon), bytes))
}

fn mime_guess(path: &FsPath) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("webp") => "image/webp",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("html") => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[derive(Debug, serde::Serialize)]
struct ApiError {
    #[serde(skip)]
    status: StatusCode,
    error: String,
}

impl ApiError {
    fn from(error: String) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            error,
        }
    }
}

fn not_found(error: String) -> ApiError {
    ApiError {
        status: StatusCode::NOT_FOUND,
        error,
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(serde_json::json!({ "error": self.error }))).into_response()
    }
}

async fn bind_with_retry(addr: SocketAddr) -> Result<tokio::net::TcpListener, String> {
    let mut last = "bind failed".to_string();
    for attempt in 0..15 {
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(80 * (attempt + 1))).await;
            }
        }
    }
    Err(format!("Could not listen on {addr}: {last}"))
}

#[cfg(test)]
mod tests {
    use super::{cookie_token, request_is_https, safe_static_file, sanitize_filename, sanitize_folder_name};
    use axum::http::{header, HeaderMap, HeaderValue};
    use std::fs;

    #[test]
    fn https_from_forwarded_proto_not_cf_ip() {
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", HeaderValue::from_static("1.2.3.4"));
        assert!(!request_is_https(&headers));
        headers.insert("x-forwarded-proto", HeaderValue::from_static("https"));
        assert!(request_is_https(&headers));
    }

    #[test]
    fn cookie_token_reads_shelf_session() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("x=1; shelf_session=new-token"),
        );
        assert_eq!(cookie_token(&headers).as_deref(), Some("new-token"));
    }

    #[test]
    fn spa_rejects_path_traversal() {
        let dir = std::env::temp_dir().join(format!("shelf-spa-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ok.js"), "1").unwrap();
        assert!(safe_static_file(&dir, "/ok.js").is_some());
        assert!(safe_static_file(&dir, "/../ok.js").is_none());
        assert!(safe_static_file(&dir, "/foo/../../etc/passwd").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn spa_serves_webmanifest_with_manifest_content_type() {
        let dir = std::env::temp_dir().join(format!("shelf-manifest-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.webmanifest"), r#"{"name":"Shelf"}"#).unwrap();
        let (mime, bytes) = safe_static_file(&dir, "/manifest.webmanifest").expect("manifest");
        assert_eq!(mime, "application/manifest+json");
        assert_eq!(bytes, br#"{"name":"Shelf"}"#);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn filename_rules() {
        assert!(sanitize_filename("ch01.pdf").is_ok());
        assert!(sanitize_filename("../etc/passwd.pdf").is_ok()); // basename only
        assert_eq!(sanitize_filename("../etc/passwd.pdf").unwrap(), "passwd.pdf");
        assert!(sanitize_filename("nope.txt").is_err());
        assert!(sanitize_folder_name("New Series").is_ok());
        assert!(sanitize_folder_name("../escape").is_err());
    }
}
