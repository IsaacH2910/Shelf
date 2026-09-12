use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRoot {
    pub id: i64,
    pub path: String,
    pub added_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Manga,
    Manhwa,
    Comic,
    Book,
    Document,
    Video,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manga => "manga",
            Self::Manhwa => "manhwa",
            Self::Comic => "comic",
            Self::Book => "book",
            Self::Document => "document",
            Self::Video => "video",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "manhwa" => Self::Manhwa,
            "comic" => Self::Comic,
            "book" => Self::Book,
            "document" => Self::Document,
            "video" => Self::Video,
            _ => Self::Manga,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    Pdf,
    Mp4,
}

impl SourceFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Mp4 => "mp4",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "mp4" => Self::Mp4,
            _ => Self::Pdf,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub id: i64,
    pub root_id: i64,
    pub folder_path: String,
    pub title: String,
    pub cover_path: Option<String>,
    pub content_type: ContentType,
    pub source_format: SourceFormat,
    pub reading_mode: ReadingMode,
    pub variant_preference: VariantPreference,
    pub page_direction: PageDirection,
    pub favorite: bool,
    pub description: Option<String>,
    pub chapter_count: i64,
    pub unread_count: i64,
    pub progress_percent: f64,
    pub last_read_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub group_name: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VariantPreference {
    Primary,
    Uncensored,
    Revised,
    Bonus,
    Alternate,
}

impl VariantPreference {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Primary => "primary",
            Self::Uncensored => "uncensored",
            Self::Revised => "revised",
            Self::Bonus => "bonus",
            Self::Alternate => "alternate",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "uncensored" => Self::Uncensored,
            "revised" => Self::Revised,
            "bonus" => Self::Bonus,
            "alternate" => Self::Alternate,
            _ => Self::Primary,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReadingMode {
    Auto,
    Webtoon,
    PagedLtr,
    PagedRtl,
}

impl ReadingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Webtoon => "webtoon",
            Self::PagedLtr => "paged_ltr",
            Self::PagedRtl => "paged_rtl",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "webtoon" => Self::Webtoon,
            "paged_ltr" => Self::PagedLtr,
            "paged_rtl" => Self::PagedRtl,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PageDirection {
    Ltr,
    Rtl,
}

impl PageDirection {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "ltr" => Self::Ltr,
            _ => Self::Rtl,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: i64,
    pub series_id: i64,
    pub file_path: String,
    pub title: String,
    pub chapter_number: Option<f64>,
    pub volume_number: Option<i32>,
    pub sort_key: f64,
    pub page_count: i32,
    pub progress_percent: f64,
    pub last_page_index: i32,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadingProgress {
    pub chapter_id: i64,
    pub page_index: i32,
    pub scroll_offset: f64,
    #[serde(default)]
    pub position_seconds: Option<f64>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    pub percent: f64,
    pub updated_at: String,
    #[serde(default)]
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueReadingItem {
    pub series: Series,
    pub chapter: Chapter,
    pub progress: ReadingProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageImage {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub page_index: i32,
    pub from_cache: bool,
}

/// One stacked segment of a page. Webtoon pages are far too tall to display as a single
/// image, so a chapter is presented as a flat list of these.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTile {
    pub page_index: i32,
    pub tile_index: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub reading_mode: Option<ReadingMode>,
    pub variant_preference: Option<VariantPreference>,
    pub page_direction: Option<PageDirection>,
    pub favorite: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrRegion {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub text: String,
    pub translated_text: Option<String>,
    pub vertical: bool,
    #[serde(default)]
    pub hidden: bool,
}

/// Public HTTPS reverse tunnel used for internet access.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TunnelMode {
    Named,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteSettings {
    pub enabled: bool,
    pub mode: TunnelMode,
    /// `https://` plus the configured public hostname.
    pub public_url: Option<String>,
    pub configured_hostname: Option<String>,
    pub has_token: bool,
    pub cloudflared_installed: bool,
    pub running: bool,
    pub status: String,
    pub qr_data_url: Option<String>,
    pub restart_count: u32,
}

/// Nearby connect session: LAN bind + Bonjour advertise. Off unless the owner starts it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanSettings {
    pub enabled: bool,
    /// `until_off`, `15m`, or `60m`.
    pub duration: String,
    pub expires_at: Option<String>,
    /// Best-effort `http://<hostname>.local:<port>` or `http://<lan-ip>:<port>`.
    pub local_url: Option<String>,
    pub hostname: Option<String>,
    pub ip: Option<String>,
    pub port: u16,
    pub advertised: bool,
    pub qr_data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub cache_size_mb: u32,
    pub cache_used_mb: u32,
    pub default_reading_mode: ReadingMode,
    pub remote: RemoteSettings,
    pub lan: LanSettings,
    pub ocr_engine_id: Option<String>,
    pub translator_id: Option<String>,
    /// False until the owner sets a password. Remote sign-in is impossible until then,
    /// which is what keeps a freshly enabled tunnel from exposing an open library.
    pub remote_login_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexStatus {
    pub indexing: bool,
    pub pending_jobs: u32,
    pub processed_jobs: u32,
    pub failed_jobs: u32,
    pub series_count: i64,
    pub chapter_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub series_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Owner,
    Member,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Member => "member",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "owner" => Self::Owner,
            _ => Self::Member,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub role: UserRole,
    /// Owners always see the whole library. Members see it only if this is set; otherwise
    /// they see exactly the series listed in `library_grants`.
    pub access_all: bool,
    pub disabled: bool,
    pub has_password: bool,
    pub created_at: String,
    pub locked_until: Option<String>,
}

/// Identity of whoever is making a request, and the basis for every visibility decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewer {
    pub user_id: i64,
    pub access_all: bool,
}

impl Viewer {
    /// Indexing, cover generation, and the folder watcher operate on the whole library
    /// regardless of who is signed in. Never construct this from request data.
    pub const SYSTEM: Self = Self {
        user_id: 1,
        access_all: true,
    };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSession {
    pub id: i64,
    pub user_id: i64,
    pub username: String,
    pub device_label: String,
    pub created_at: String,
    pub last_used_at: String,
    pub idle_expires_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub role: UserRole,
    pub access_all: bool,
    pub expires_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct SeriesFilter {
    pub search: Option<String>,
    pub favorites_only: bool,
    pub unread_only: bool,
    pub collection_id: Option<i64>,
    pub reading_mode: Option<String>,
    pub content_type: Option<String>,
    pub sort: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateUserRequest {
    pub username: String,
    pub display_name: String,
    pub password: String,
    #[serde(default)]
    pub access_all: bool,
    #[serde(default)]
    pub series_ids: Vec<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateUserRequest {
    pub display_name: Option<String>,
    pub password: Option<String>,
    pub access_all: Option<bool>,
    pub disabled: Option<bool>,
    pub series_ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFolderRequest {
    pub root_id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitUploadRequest {
    pub series_id: i64,
    pub file_name: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadStatus {
    pub upload_id: String,
    pub offset: u64,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct AuthSession {
    pub id: i64,
    pub viewer: Viewer,
    pub username: String,
    pub display_name: String,
    pub role: UserRole,
    pub idle_expires_at: String,
    pub absolute_expires_at: String,
    pub rotated_at: String,
}
