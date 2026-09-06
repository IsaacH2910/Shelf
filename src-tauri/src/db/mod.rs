use crate::models::{
    ActiveSession, AuthSession, Chapter, Collection, ContentType, ContinueReadingItem, LibraryRoot,
    PageDirection, ReadingMode, ReadingProgress, Series, SeriesFilter, SeriesUpdate, SourceFormat,
    Tag, User, UserRole, VariantPreference, Viewer,
};
use chrono::{Duration, Utc};
use rusqlite::{params, types::Value, Connection, OptionalExtension};
use std::path::PathBuf;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Other(String),
}

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(app_data_dir: PathBuf) -> Result<Self, DbError> {
        std::fs::create_dir_all(&app_data_dir).map_err(|e| DbError::Other(e.to_string()))?;
        let db_path = app_data_dir.join("shelf.db");
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS library_roots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                added_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS series (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                root_id INTEGER NOT NULL,
                folder_path TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                cover_path TEXT,
                content_type TEXT NOT NULL DEFAULT 'manga',
                source_format TEXT NOT NULL DEFAULT 'pdf',
                reading_mode TEXT NOT NULL DEFAULT 'auto',
                variant_preference TEXT NOT NULL DEFAULT 'primary',
                page_direction TEXT NOT NULL DEFAULT 'rtl',
                favorite INTEGER NOT NULL DEFAULT 0,
                description TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (root_id) REFERENCES library_roots(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS chapters (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                series_id INTEGER NOT NULL,
                file_path TEXT NOT NULL UNIQUE,
                title TEXT NOT NULL,
                chapter_number REAL,
                volume_number INTEGER,
                sort_key REAL NOT NULL,
                page_count INTEGER NOT NULL DEFAULT 0,
                file_size INTEGER,
                file_mtime INTEGER,
                file_hash TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (series_id) REFERENCES series(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS progress (
                chapter_id INTEGER PRIMARY KEY,
                page_index INTEGER NOT NULL DEFAULT 0,
                scroll_offset REAL NOT NULL DEFAULT 0,
                position_seconds REAL,
                duration_seconds REAL,
                percent REAL NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE
            );

            CREATE TABLE IF NOT EXISTS series_tags (
                series_id INTEGER NOT NULL,
                tag_id INTEGER NOT NULL,
                PRIMARY KEY (series_id, tag_id),
                FOREIGN KEY (series_id) REFERENCES series(id) ON DELETE CASCADE,
                FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS page_dimensions (
                chapter_id INTEGER NOT NULL,
                page_index INTEGER NOT NULL,
                width REAL NOT NULL,
                height REAL NOT NULL,
                PRIMARY KEY (chapter_id, page_index),
                FOREIGN KEY (chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS ocr_regions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chapter_id INTEGER NOT NULL,
                page_index INTEGER NOT NULL,
                engine_id TEXT NOT NULL,
                regions_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS translation_cache (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                chapter_id INTEGER NOT NULL,
                page_index INTEGER NOT NULL,
                ocr_engine_id TEXT NOT NULL,
                translator_id TEXT NOT NULL,
                target_lang TEXT NOT NULL,
                content_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(chapter_id, page_index, ocr_engine_id, translator_id, target_lang)
            );

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_chapters_series ON chapters(series_id);
            CREATE INDEX IF NOT EXISTS idx_progress_updated ON progress(updated_at);
            ",
        )?;

        let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 2 {
            let _ = conn.execute("ALTER TABLE chapters ADD COLUMN missing INTEGER NOT NULL DEFAULT 0", []);
            let _ = conn.execute("ALTER TABLE progress ADD COLUMN device_id TEXT", []);
            let _ = conn.execute("ALTER TABLE series ADD COLUMN content_type TEXT NOT NULL DEFAULT 'manga'", []);
            let _ = conn.execute("ALTER TABLE series ADD COLUMN source_format TEXT NOT NULL DEFAULT 'pdf'", []);
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS collections (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL UNIQUE,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS collection_items (
                    collection_id INTEGER NOT NULL,
                    series_id INTEGER NOT NULL,
                    PRIMARY KEY (collection_id, series_id),
                    FOREIGN KEY (collection_id) REFERENCES collections(id) ON DELETE CASCADE,
                    FOREIGN KEY (series_id) REFERENCES series(id) ON DELETE CASCADE
                );
                CREATE TABLE IF NOT EXISTS devices (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    public_id TEXT NOT NULL UNIQUE,
                    name TEXT NOT NULL,
                    token_hash TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    last_seen TEXT,
                    revoked INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    device_id INTEGER NOT NULL,
                    token_hash TEXT NOT NULL UNIQUE,
                    created_at TEXT NOT NULL,
                    expires_at TEXT NOT NULL,
                    FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_sessions_hash ON sessions(token_hash);
                CREATE INDEX IF NOT EXISTS idx_chapters_missing ON chapters(missing);
                PRAGMA user_version = 2;
                ",
            )?;
        }

        let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 3 {
            let _ = conn.execute(
                "ALTER TABLE series ADD COLUMN variant_preference TEXT NOT NULL DEFAULT 'primary'",
                [],
            );
            conn.execute_batch("PRAGMA user_version = 3;")?;
        }
        let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 4 {
            let _ = conn.execute("ALTER TABLE progress ADD COLUMN position_seconds REAL", []);
            let _ = conn.execute("ALTER TABLE progress ADD COLUMN duration_seconds REAL", []);
            conn.execute_batch("PRAGMA user_version = 4;")?;
        }
        let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 5 {
            let _ = conn.execute("ALTER TABLE series ADD COLUMN group_name TEXT", []);
            // Folder-per-series model changed: rebuild series/chapters on next scan.
            let _ = conn.execute("DELETE FROM series", []);
            conn.execute_batch("PRAGMA user_version = 5;")?;
        }

        // v6 introduces real accounts. Before this the only identity was "a device that
        // knew the PIN", so reading progress was global and every device saw everything.
        // Progress is re-keyed onto (user_id, chapter_id) and existing rows are adopted by
        // the owner account, which is the person who was using the Mac app.
        let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < 6 {
            let now = Utc::now().to_rfc3339();
            conn.execute_batch("PRAGMA foreign_keys=OFF;")?;
            conn.execute_batch(
                "
                BEGIN;

                CREATE TABLE IF NOT EXISTS users (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    username TEXT NOT NULL UNIQUE,
                    display_name TEXT NOT NULL,
                    password_hash TEXT NOT NULL DEFAULT '',
                    role TEXT NOT NULL DEFAULT 'member',
                    access_all INTEGER NOT NULL DEFAULT 0,
                    disabled INTEGER NOT NULL DEFAULT 0,
                    failed_attempts INTEGER NOT NULL DEFAULT 0,
                    locked_until TEXT,
                    created_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS library_grants (
                    user_id INTEGER NOT NULL,
                    series_id INTEGER NOT NULL,
                    PRIMARY KEY (user_id, series_id),
                    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
                    FOREIGN KEY (series_id) REFERENCES series(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS auth_sessions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    user_id INTEGER NOT NULL,
                    token_hash TEXT NOT NULL UNIQUE,
                    device_label TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    last_used_at TEXT NOT NULL,
                    rotated_at TEXT NOT NULL,
                    idle_expires_at TEXT NOT NULL,
                    absolute_expires_at TEXT NOT NULL,
                    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
                );
                CREATE INDEX IF NOT EXISTS idx_auth_sessions_hash ON auth_sessions(token_hash);
                CREATE INDEX IF NOT EXISTS idx_auth_sessions_user ON auth_sessions(user_id);
                CREATE INDEX IF NOT EXISTS idx_library_grants_user ON library_grants(user_id);

                COMMIT;
                ",
            )?;

            // An owner row must exist before progress can reference a user.
            conn.execute(
                "INSERT OR IGNORE INTO users (id, username, display_name, password_hash, role, access_all, created_at)
                 VALUES (1, 'owner', 'Owner', '', 'owner', 1, ?1)",
                params![now],
            )?;

            let has_legacy_progress = conn
                .query_row(
                    "SELECT 1 FROM pragma_table_info('progress') WHERE name = 'user_id'",
                    [],
                    |row| row.get::<_, i32>(0),
                )
                .optional()?
                .is_none();

            if has_legacy_progress {
                conn.execute_batch(
                    "
                    BEGIN;

                    CREATE TABLE progress_v6 (
                        user_id INTEGER NOT NULL,
                        chapter_id INTEGER NOT NULL,
                        page_index INTEGER NOT NULL DEFAULT 0,
                        scroll_offset REAL NOT NULL DEFAULT 0,
                        position_seconds REAL,
                        duration_seconds REAL,
                        percent REAL NOT NULL DEFAULT 0,
                        updated_at TEXT NOT NULL,
                        device_id TEXT,
                        PRIMARY KEY (user_id, chapter_id),
                        FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
                        FOREIGN KEY (chapter_id) REFERENCES chapters(id) ON DELETE CASCADE
                    );

                    INSERT OR IGNORE INTO progress_v6
                        (user_id, chapter_id, page_index, scroll_offset, position_seconds,
                         duration_seconds, percent, updated_at, device_id)
                    SELECT 1, chapter_id, page_index, scroll_offset, position_seconds,
                           duration_seconds, percent, updated_at, device_id
                    FROM progress;

                    DROP TABLE progress;
                    ALTER TABLE progress_v6 RENAME TO progress;

                    CREATE INDEX IF NOT EXISTS idx_progress_updated ON progress(updated_at);
                    CREATE INDEX IF NOT EXISTS idx_progress_user ON progress(user_id);

                    COMMIT;
                    ",
                )?;
            }

            conn.execute_batch("PRAGMA user_version = 6;")?;
            conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        }

        // Repair partially applied migrations even when user_version was advanced.
        for statement in [
            "ALTER TABLE chapters ADD COLUMN missing INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE progress ADD COLUMN device_id TEXT",
            "ALTER TABLE progress ADD COLUMN position_seconds REAL",
            "ALTER TABLE progress ADD COLUMN duration_seconds REAL",
            "ALTER TABLE series ADD COLUMN content_type TEXT NOT NULL DEFAULT 'manga'",
            "ALTER TABLE series ADD COLUMN source_format TEXT NOT NULL DEFAULT 'pdf'",
            "ALTER TABLE series ADD COLUMN variant_preference TEXT NOT NULL DEFAULT 'primary'",
            "ALTER TABLE series ADD COLUMN group_name TEXT",
        ] {
            let _ = conn.execute(statement, []);
        }
        Ok(())
    }

    pub fn add_root(&self, path: &str) -> Result<LibraryRoot, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO library_roots (path, added_at) VALUES (?1, ?2)",
            params![path, now],
        )?;
        let id = conn.last_insert_rowid();
        if id == 0 {
            let id: i64 = conn.query_row(
                "SELECT id FROM library_roots WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )?;
            Ok(LibraryRoot {
                id,
                path: path.to_string(),
                added_at: now,
            })
        } else {
            Ok(LibraryRoot {
                id,
                path: path.to_string(),
                added_at: now,
            })
        }
    }

    pub fn list_roots(&self) -> Result<Vec<LibraryRoot>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, path, added_at FROM library_roots ORDER BY added_at")?;
        let rows = stmt.query_map([], |row| {
            Ok(LibraryRoot {
                id: row.get(0)?,
                path: row.get(1)?,
                added_at: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn remove_root(&self, id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM library_roots WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn upsert_series(&self, root_id: i64, folder_path: &str, title: &str, group_name: Option<&str>) -> Result<i64, DbError> {
        let now = Utc::now().to_rfc3339();
        let (content_type, source_format) = detect_series_media(folder_path, title);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO series (root_id, folder_path, title, group_name, content_type, source_format, reading_mode, variant_preference, page_direction, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'auto', 'primary', 'rtl', ?7, ?7)
             ON CONFLICT(folder_path) DO UPDATE SET title = excluded.title, group_name = excluded.group_name, content_type = excluded.content_type, source_format = excluded.source_format, updated_at = excluded.updated_at",
            params![root_id, folder_path, title, group_name, content_type.as_str(), source_format.as_str(), now],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM series WHERE folder_path = ?1",
            params![folder_path],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn list_series_filtered(
        &self,
        viewer: &Viewer,
        filter: &SeriesFilter,
    ) -> Result<Vec<Series>, DbError> {
        let order = match filter.sort.as_str() {
            "last_read" => "COALESCE(p.last_read, s.updated_at) DESC",
            "added" => "s.created_at DESC",
            "progress" => "COALESCE(agg.progress_percent, 0) DESC",
            _ => "s.title COLLATE NOCASE ASC",
        };
        self.list_series_bind(viewer, filter, order)
    }

    /// The single chokepoint for series visibility. Every other series read — `get_series`,
    /// `recently_added`, search — routes through here, so a member who was never granted a
    /// series cannot observe it, its chapter counts, or its progress by any path.
    fn list_series_bind(
        &self,
        viewer: &Viewer,
        filter: &SeriesFilter,
        order: &str,
    ) -> Result<Vec<Series>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut sql = format!(
            "SELECT s.id, s.root_id, s.folder_path, s.title, s.cover_path,
                    s.content_type, s.source_format, s.reading_mode, s.variant_preference, s.page_direction, s.favorite, s.description,
                    s.created_at, s.updated_at,
                    COALESCE(agg.chapter_count, 0),
                    COALESCE(agg.unread_count, 0),
                    COALESCE(agg.progress_percent, 0),
                    p.last_read,
                    (SELECT GROUP_CONCAT(t.name, ', ') FROM tags t
                     JOIN series_tags st ON st.tag_id = t.id WHERE st.series_id = s.id),
                    s.group_name
             FROM series s
             LEFT JOIN (
                 SELECT c.series_id,
                        COUNT(*) as chapter_count,
                        SUM(CASE WHEN COALESCE(pr.percent, 0) < 99 THEN 1 ELSE 0 END) as unread_count,
                        AVG(COALESCE(pr.percent, 0)) as progress_percent
                 FROM chapters c
                 LEFT JOIN progress pr ON pr.chapter_id = c.id AND pr.user_id = ?
                 WHERE COALESCE(c.missing, 0) = 0
                 GROUP BY c.series_id
             ) agg ON agg.series_id = s.id
             LEFT JOIN (
                 SELECT c.series_id, MAX(pr.updated_at) as last_read
                 FROM progress pr
                 JOIN chapters c ON c.id = pr.chapter_id
                 WHERE pr.user_id = ?
                 GROUP BY c.series_id
             ) p ON p.series_id = s.id
             WHERE (? = 1 OR EXISTS (
                 SELECT 1 FROM library_grants g
                 WHERE g.series_id = s.id AND g.user_id = ?
             ))"
        );

        // Bound in order of `?` appearance: the two progress joins, then the visibility test.
        let mut params_owned: Vec<Value> = vec![
            Value::Integer(viewer.user_id),
            Value::Integer(viewer.user_id),
            Value::Integer(if viewer.access_all { 1 } else { 0 }),
            Value::Integer(viewer.user_id),
        ];
        if filter.favorites_only {
            sql.push_str(" AND s.favorite = 1");
        }
        if filter.unread_only {
            sql.push_str(" AND COALESCE(agg.unread_count, 0) > 0");
        }
        if let Some(cid) = filter.collection_id {
            sql.push_str(" AND EXISTS (SELECT 1 FROM collection_items ci WHERE ci.series_id = s.id AND ci.collection_id = ?)");
            params_owned.push(Value::Integer(cid));
        }
        if let Some(mode) = &filter.reading_mode {
            sql.push_str(" AND s.reading_mode = ?");
            params_owned.push(Value::Text(mode.clone()));
        }
        if let Some(content_type) = &filter.content_type {
            sql.push_str(" AND s.content_type = ?");
            params_owned.push(Value::Text(content_type.clone()));
        }
        if let Some(q) = &filter.search {
            sql.push_str(
                " AND (
                    s.title LIKE ? OR COALESCE(s.description, '') LIKE ?
                    OR EXISTS (SELECT 1 FROM tags t JOIN series_tags st ON st.tag_id = t.id
                               WHERE st.series_id = s.id AND t.name LIKE ?)
                    OR EXISTS (SELECT 1 FROM collections col JOIN collection_items ci ON ci.collection_id = col.id
                               WHERE ci.series_id = s.id AND col.name LIKE ?)
                    OR EXISTS (SELECT 1 FROM chapters ch WHERE ch.series_id = s.id AND ch.title LIKE ?)
                )",
            );
            let like = format!("%{q}%");
            for _ in 0..5 {
                params_owned.push(Value::Text(like.clone()));
            }
        }
        sql.push_str(&format!(" ORDER BY {order}"));

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params_owned
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(param_refs.as_slice(), map_series_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    /// Returns the series only if `viewer` may see it. A member without a grant gets the
    /// same "not found" as a genuinely missing id, so the API never confirms that a series
    /// they cannot read exists.
    pub fn get_series(&self, viewer: &Viewer, id: i64) -> Result<Series, DbError> {
        let all = self.list_series_filtered(
            viewer,
            &SeriesFilter {
                sort: "title".into(),
                ..SeriesFilter::default()
            },
        )?;
        all.into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| DbError::Other(format!("Series {id} not found")))
    }

    pub fn recently_added(&self, viewer: &Viewer, limit: i64) -> Result<Vec<Series>, DbError> {
        let mut items = self.list_series_filtered(
            viewer,
            &SeriesFilter {
                sort: "added".into(),
                ..SeriesFilter::default()
            },
        )?;
        items.truncate(limit as usize);
        Ok(items)
    }

    /// True when `viewer` may read the series that owns `chapter_id`. Used by every
    /// chapter, page, tile, cover, and blob route before it touches the filesystem.
    pub fn can_view_chapter(&self, viewer: &Viewer, chapter_id: i64) -> Result<bool, DbError> {
        if viewer.access_all {
            return Ok(true);
        }
        let conn = self.conn.lock().unwrap();
        let allowed: Option<i32> = conn
            .query_row(
                "SELECT 1 FROM chapters c
                 JOIN library_grants g ON g.series_id = c.series_id
                 WHERE c.id = ?1 AND g.user_id = ?2",
                params![chapter_id, viewer.user_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(allowed.is_some())
    }

    pub fn can_view_series(&self, viewer: &Viewer, series_id: i64) -> Result<bool, DbError> {
        if viewer.access_all {
            return Ok(true);
        }
        let conn = self.conn.lock().unwrap();
        let allowed: Option<i32> = conn
            .query_row(
                "SELECT 1 FROM library_grants WHERE series_id = ?1 AND user_id = ?2",
                params![series_id, viewer.user_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(allowed.is_some())
    }

    pub fn update_series(&self, id: i64, update: &SeriesUpdate) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        if let Some(title) = &update.title {
            conn.execute(
                "UPDATE series SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![title, now, id],
            )?;
        }
        if let Some(desc) = &update.description {
            conn.execute(
                "UPDATE series SET description = ?1, updated_at = ?2 WHERE id = ?3",
                params![desc, now, id],
            )?;
        }
        if let Some(mode) = update.reading_mode {
            conn.execute(
                "UPDATE series SET reading_mode = ?1, updated_at = ?2 WHERE id = ?3",
                params![mode.as_str(), now, id],
            )?;
        }
        if let Some(pref) = update.variant_preference {
            conn.execute(
                "UPDATE series SET variant_preference = ?1, updated_at = ?2 WHERE id = ?3",
                params![pref.as_str(), now, id],
            )?;
        }
        if let Some(dir) = update.page_direction {
            conn.execute(
                "UPDATE series SET page_direction = ?1, updated_at = ?2 WHERE id = ?3",
                params![dir.as_str(), now, id],
            )?;
        }
        if let Some(fav) = update.favorite {
            conn.execute(
                "UPDATE series SET favorite = ?1, updated_at = ?2 WHERE id = ?3",
                params![fav as i32, now, id],
            )?;
        }
        Ok(())
    }

    pub fn set_series_cover(&self, id: i64, cover_path: &str) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE series SET cover_path = ?1, updated_at = ?2 WHERE id = ?3",
            params![cover_path, now, id],
        )?;
        Ok(())
    }

    pub fn update_series_reading_mode(
        &self,
        id: i64,
        mode: ReadingMode,
        direction: PageDirection,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE series SET reading_mode = ?1, page_direction = ?2, updated_at = ?3 WHERE id = ?4",
            params![mode.as_str(), direction.as_str(), now, id],
        )?;
        Ok(())
    }

    pub fn set_series_media_kind(
        &self,
        id: i64,
        content_type: ContentType,
        source_format: SourceFormat,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE series SET content_type = ?1, source_format = ?2, updated_at = ?3 WHERE id = ?4",
            params![content_type.as_str(), source_format.as_str(), now, id],
        )?;
        Ok(())
    }

    pub fn upsert_chapter(
        &self,
        series_id: i64,
        file_path: &str,
        title: &str,
        chapter_number: Option<f64>,
        volume_number: Option<i32>,
        sort_key: f64,
        mtime: i64,
        size: i64,
    ) -> Result<i64, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chapters (series_id, file_path, title, chapter_number, volume_number,
                                   sort_key, file_mtime, file_size, missing, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?9)
             ON CONFLICT(file_path) DO UPDATE SET
                series_id = excluded.series_id,
                title = excluded.title,
                chapter_number = excluded.chapter_number,
                volume_number = excluded.volume_number,
                sort_key = excluded.sort_key,
                file_mtime = excluded.file_mtime,
                file_size = excluded.file_size,
                missing = 0,
                updated_at = excluded.updated_at",
            params![
                series_id,
                file_path,
                title,
                chapter_number,
                volume_number,
                sort_key,
                mtime,
                size,
                now
            ],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM chapters WHERE file_path = ?1",
            params![file_path],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn get_chapter_by_path(&self, file_path: &str) -> Result<Option<ChapterRow>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, series_id, file_path, title, chapter_number, volume_number, sort_key,
                    page_count, file_mtime, file_size, file_hash, created_at, updated_at,
                    COALESCE(missing, 0)
             FROM chapters WHERE file_path = ?1",
            params![file_path],
            |row| {
                Ok(ChapterRow {
                    id: row.get(0)?,
                    series_id: row.get(1)?,
                    file_path: row.get(2)?,
                    title: row.get(3)?,
                    chapter_number: row.get(4)?,
                    volume_number: row.get(5)?,
                    sort_key: row.get(6)?,
                    page_count: row.get(7)?,
                    file_mtime: row.get(8)?,
                    file_size: row.get(9)?,
                    file_hash: row.get(10)?,
                    created_at: row.get(11)?,
                    updated_at: row.get(12)?,
                    missing: row.get::<_, i32>(13)? != 0,
                })
            },
        )
        .optional()
        .map_err(DbError::from)
    }

    pub fn list_chapter_paths(&self, series_id: i64) -> Result<Vec<(i64, String)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, file_path FROM chapters WHERE series_id = ?1")?;
        let rows = stmt.query_map(params![series_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn mark_chapter_missing(&self, id: i64, missing: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chapters SET missing = ?1 WHERE id = ?2",
            params![missing as i32, id],
        )?;
        Ok(())
    }

    pub fn set_chapter_hash(&self, id: i64, hash: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        let prev: Option<String> = conn
            .query_row(
                "SELECT file_hash FROM chapters WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        conn.execute(
            "UPDATE chapters SET file_hash = ?1 WHERE id = ?2",
            params![hash, id],
        )?;
        Ok(prev)
    }

    pub fn invalidate_page_caches(&self, chapter_id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM ocr_regions WHERE chapter_id = ?1", params![chapter_id])?;
        conn.execute(
            "DELETE FROM translation_cache WHERE chapter_id = ?1",
            params![chapter_id],
        )?;
        Ok(())
    }

    pub fn list_chapters(&self, viewer: &Viewer, series_id: i64) -> Result<Vec<Chapter>, DbError> {
        if !self.can_view_series(viewer, series_id)? {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.series_id, c.file_path, c.title, c.chapter_number, c.volume_number,
                    c.sort_key, c.page_count, COALESCE(p.percent, 0), COALESCE(p.page_index, 0),
                    c.created_at, c.updated_at, COALESCE(c.missing, 0)
             FROM chapters c
             LEFT JOIN progress p ON p.chapter_id = c.id AND p.user_id = ?2
             WHERE c.series_id = ?1
             ORDER BY c.sort_key ASC, c.title ASC",
        )?;
        let rows = stmt.query_map(params![series_id, viewer.user_id], map_chapter_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn get_chapter(&self, viewer: &Viewer, id: i64) -> Result<Chapter, DbError> {
        if !self.can_view_chapter(viewer, id)? {
            return Err(DbError::Other(format!("Chapter {id} not found")));
        }
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT c.id, c.series_id, c.file_path, c.title, c.chapter_number, c.volume_number,
                    c.sort_key, c.page_count, COALESCE(p.percent, 0), COALESCE(p.page_index, 0),
                    c.created_at, c.updated_at, COALESCE(c.missing, 0)
             FROM chapters c
             LEFT JOIN progress p ON p.chapter_id = c.id AND p.user_id = ?2
             WHERE c.id = ?1",
            params![id, viewer.user_id],
            map_chapter_row,
        )
        .map_err(DbError::from)
    }

    pub fn get_adjacent_chapter(
        &self,
        viewer: &Viewer,
        chapter_id: i64,
        next: bool,
    ) -> Result<Option<Chapter>, DbError> {
        let chapter = self.get_chapter(viewer, chapter_id)?;
        let chapters: Vec<Chapter> = self
            .list_chapters(viewer, chapter.series_id)?
            .into_iter()
            .filter(|c| !c.missing)
            .collect();
        let idx = chapters.iter().position(|c| c.id == chapter_id);
        if let Some(i) = idx {
            let target = if next {
                i + 1
            } else if i > 0 {
                i - 1
            } else {
                return Ok(None);
            };
            return Ok(chapters.get(target).cloned());
        }
        Ok(None)
    }

    pub fn delete_chapter(&self, id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM chapters WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn update_chapter_page_count(&self, id: i64, count: i32) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chapters SET page_count = ?1, updated_at = ?2 WHERE id = ?3",
            params![count, now, id],
        )?;
        Ok(())
    }

    pub fn save_progress(
        &self,
        viewer: &Viewer,
        progress: &ReadingProgress,
    ) -> Result<(), DbError> {
        if !self.can_view_chapter(viewer, progress.chapter_id)? {
            return Err(DbError::Other("Chapter not found".into()));
        }
        let conn = self.conn.lock().unwrap();
        let existing: Option<String> = conn
            .query_row(
                "SELECT updated_at FROM progress WHERE chapter_id = ?1 AND user_id = ?2",
                params![progress.chapter_id, viewer.user_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(prev) = existing {
            if progress.updated_at < prev {
                return Ok(());
            }
        }
        conn.execute(
                "INSERT INTO progress (user_id, chapter_id, page_index, scroll_offset, position_seconds, duration_seconds, percent, updated_at, device_id)
                 VALUES (?9, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(user_id, chapter_id) DO UPDATE SET
                page_index = excluded.page_index,
                scroll_offset = excluded.scroll_offset,
                     position_seconds = excluded.position_seconds,
                     duration_seconds = excluded.duration_seconds,
                percent = excluded.percent,
                updated_at = excluded.updated_at,
                device_id = excluded.device_id
             WHERE excluded.updated_at >= progress.updated_at",
            params![
                progress.chapter_id,
                progress.page_index,
                progress.scroll_offset,
                progress.position_seconds,
                progress.duration_seconds,
                progress.percent,
                progress.updated_at,
                progress.device_id,
                viewer.user_id
            ],
        )?;
        Ok(())
    }

    pub fn get_progress(
        &self,
        viewer: &Viewer,
        chapter_id: i64,
    ) -> Result<Option<ReadingProgress>, DbError> {
        if !self.can_view_chapter(viewer, chapter_id)? {
            return Ok(None);
        }
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT chapter_id, page_index, scroll_offset, position_seconds, duration_seconds, percent, updated_at, device_id
             FROM progress WHERE chapter_id = ?1 AND user_id = ?2",
            params![chapter_id, viewer.user_id],
            |row| {
                Ok(ReadingProgress {
                    chapter_id: row.get(0)?,
                    page_index: row.get(1)?,
                    scroll_offset: row.get(2)?,
                    position_seconds: row.get(3)?,
                    duration_seconds: row.get(4)?,
                    percent: row.get(5)?,
                    updated_at: row.get(6)?,
                    device_id: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(DbError::from)
    }

    pub fn continue_reading(
        &self,
        viewer: &Viewer,
        limit: i64,
    ) -> Result<Vec<ContinueReadingItem>, DbError> {
        self.progress_list(
            viewer,
            "SELECT p.chapter_id, p.page_index, p.scroll_offset, p.position_seconds, p.duration_seconds, p.percent, p.updated_at, p.device_id, c.series_id
             FROM progress p
             JOIN chapters c ON c.id = p.chapter_id
             WHERE p.user_id = ?2 AND p.percent > 0 AND p.percent < 99 AND COALESCE(c.missing, 0) = 0
             ORDER BY p.updated_at DESC
             LIMIT ?1",
            limit,
        )
    }

    pub fn reading_history(
        &self,
        viewer: &Viewer,
        limit: i64,
    ) -> Result<Vec<ContinueReadingItem>, DbError> {
        self.progress_list(
            viewer,
            "SELECT p.chapter_id, p.page_index, p.scroll_offset, p.position_seconds, p.duration_seconds, p.percent, p.updated_at, p.device_id, c.series_id
             FROM progress p
             JOIN chapters c ON c.id = p.chapter_id
             WHERE p.user_id = ?2 AND COALESCE(c.missing, 0) = 0
             ORDER BY p.updated_at DESC
             LIMIT ?1",
            limit,
        )
    }

    /// Clears only the caller's own history. Previously this wiped the single global
    /// progress table for everyone.
    pub fn clear_reading_history(&self, viewer: &Viewer) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM progress WHERE user_id = ?1",
            params![viewer.user_id],
        )?;
        Ok(())
    }

    fn progress_list(
        &self,
        viewer: &Viewer,
        sql: &str,
        limit: i64,
    ) -> Result<Vec<ContinueReadingItem>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(sql)?;
        let rows: Vec<(i64, i32, f64, Option<f64>, Option<f64>, f64, String, Option<String>, i64)> = stmt
            .query_map(params![limit, viewer.user_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);
        drop(conn);

        let mut items = Vec::new();
        for (chapter_id, page_index, scroll_offset, position_seconds, duration_seconds, percent, updated_at, device_id, series_id) in rows {
            if let (Ok(series), Ok(chapter)) = (
                self.get_series(viewer, series_id),
                self.get_chapter(viewer, chapter_id),
            ) {
                items.push(ContinueReadingItem {
                    series,
                    chapter,
                    progress: ReadingProgress {
                        chapter_id,
                        page_index,
                        scroll_offset,
                        position_seconds,
                        duration_seconds,
                        percent,
                        updated_at,
                        device_id,
                    },
                });
            }
        }
        Ok(items)
    }

    pub fn store_page_dimensions(
        &self,
        chapter_id: i64,
        page_index: i32,
        width: f64,
        height: f64,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO page_dimensions (chapter_id, page_index, width, height)
             VALUES (?1, ?2, ?3, ?4)",
            params![chapter_id, page_index, width, height],
        )?;
        Ok(())
    }

    pub fn get_chapter_page_dimensions(
        &self,
        chapter_id: i64,
    ) -> Result<Vec<(i32, f64, f64)>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT page_index, width, height FROM page_dimensions WHERE chapter_id = ?1",
        )?;
        let rows = stmt.query_map(params![chapter_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(DbError::from)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn save_ocr_regions(
        &self,
        chapter_id: i64,
        page_index: i32,
        engine_id: &str,
        regions_json: &str,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO ocr_regions (chapter_id, page_index, engine_id, regions_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![chapter_id, page_index, engine_id, regions_json, now],
        )?;
        Ok(())
    }

    pub fn get_ocr_regions(
        &self,
        chapter_id: i64,
        page_index: i32,
        engine_id: &str,
    ) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT regions_json FROM ocr_regions
             WHERE chapter_id = ?1 AND page_index = ?2 AND engine_id = ?3
             ORDER BY id DESC LIMIT 1",
            params![chapter_id, page_index, engine_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(DbError::from)
    }

    pub fn save_translation(
        &self,
        chapter_id: i64,
        page_index: i32,
        ocr_engine_id: &str,
        translator_id: &str,
        target_lang: &str,
        content_json: &str,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO translation_cache
                (chapter_id, page_index, ocr_engine_id, translator_id, target_lang, content_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(chapter_id, page_index, ocr_engine_id, translator_id, target_lang)
             DO UPDATE SET content_json = excluded.content_json, created_at = excluded.created_at",
            params![
                chapter_id,
                page_index,
                ocr_engine_id,
                translator_id,
                target_lang,
                content_json,
                now
            ],
        )?;
        Ok(())
    }

    pub fn get_translation(
        &self,
        chapter_id: i64,
        page_index: i32,
        ocr_engine_id: &str,
        translator_id: &str,
        target_lang: &str,
    ) -> Result<Option<String>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT content_json FROM translation_cache
             WHERE chapter_id = ?1 AND page_index = ?2 AND ocr_engine_id = ?3
               AND translator_id = ?4 AND target_lang = ?5",
            params![chapter_id, page_index, ocr_engine_id, translator_id, target_lang],
            |row| row.get(0),
        )
        .optional()
        .map_err(DbError::from)
    }

    pub fn counts(&self) -> Result<(i64, i64), DbError> {
        let conn = self.conn.lock().unwrap();
        let series_count: i64 =
            conn.query_row("SELECT COUNT(*) FROM series", [], |row| row.get(0))?;
        let chapter_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM chapters WHERE COALESCE(missing, 0) = 0",
            [],
            |row| row.get(0),
        )?;
        Ok((series_count, chapter_count))
    }

    pub fn list_collections(&self) -> Result<Vec<Collection>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT c.id, c.name, c.created_at,
                    (SELECT COUNT(*) FROM collection_items ci WHERE ci.collection_id = c.id)
             FROM collections c ORDER BY c.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Collection {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
                series_count: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn create_collection(&self, name: &str) -> Result<Collection, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO collections (name, created_at) VALUES (?1, ?2)",
            params![name, now],
        )?;
        Ok(Collection {
            id: conn.last_insert_rowid(),
            name: name.to_string(),
            created_at: now,
            series_count: 0,
        })
    }

    pub fn delete_collection(&self, id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM collections WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn set_collection_item(&self, collection_id: i64, series_id: i64, add: bool) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        if add {
            conn.execute(
                "INSERT OR IGNORE INTO collection_items (collection_id, series_id) VALUES (?1, ?2)",
                params![collection_id, series_id],
            )?;
        } else {
            conn.execute(
                "DELETE FROM collection_items WHERE collection_id = ?1 AND series_id = ?2",
                params![collection_id, series_id],
            )?;
        }
        Ok(())
    }

    pub fn series_collections(&self, series_id: i64) -> Result<Vec<i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT collection_id FROM collection_items WHERE series_id = ?1")?;
        let rows = stmt.query_map(params![series_id], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn list_tags(&self) -> Result<Vec<Tag>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name FROM tags ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn set_series_tags(&self, series_id: i64, names: &[String]) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM series_tags WHERE series_id = ?1", params![series_id])?;
        for name in names {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                continue;
            }
            conn.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", params![trimmed])?;
            let tag_id: i64 = conn.query_row(
                "SELECT id FROM tags WHERE name = ?1",
                params![trimmed],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO series_tags (series_id, tag_id) VALUES (?1, ?2)",
                params![series_id, tag_id],
            )?;
        }
        Ok(())
    }

    pub fn get_root(&self, id: i64) -> Result<LibraryRoot, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, path, added_at FROM library_roots WHERE id = ?1",
            params![id],
            |row| {
                Ok(LibraryRoot {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    added_at: row.get(2)?,
                })
            },
        )
        .map_err(DbError::from)
    }

    pub fn owner_password_set(&self) -> Result<bool, DbError> {
        let conn = self.conn.lock().unwrap();
        let hash: String = conn.query_row(
            "SELECT password_hash FROM users WHERE id = 1",
            [],
            |row| row.get(0),
        )?;
        Ok(!hash.is_empty())
    }

    pub fn get_user_by_username(&self, username: &str) -> Result<Option<UserRecord>, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, username, display_name, password_hash, role, access_all, disabled,
                    failed_attempts, locked_until, created_at
             FROM users WHERE username = ?1",
            params![username],
            map_user_record,
        )
        .optional()
        .map_err(DbError::from)
    }

    pub fn get_user(&self, id: i64) -> Result<User, DbError> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, username, display_name, password_hash, role, access_all, disabled,
                    failed_attempts, locked_until, created_at
             FROM users WHERE id = ?1",
            params![id],
            map_user_record,
        )
        .map(|row| row.into_user())
        .map_err(DbError::from)
    }

    pub fn list_users(&self) -> Result<Vec<User>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, display_name, password_hash, role, access_all, disabled,
                    failed_attempts, locked_until, created_at
             FROM users ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], map_user_record)?;
        rows.map(|r| r.map(|row| row.into_user()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(DbError::from)
    }

    pub fn create_user(
        &self,
        username: &str,
        display_name: &str,
        password_hash: &str,
        access_all: bool,
    ) -> Result<User, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (username, display_name, password_hash, role, access_all, created_at)
             VALUES (?1, ?2, ?3, 'member', ?4, ?5)",
            params![username, display_name, password_hash, access_all as i32, now],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        self.get_user(id)
    }

    pub fn set_user_password(&self, id: i64, password_hash: &str) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET password_hash = ?1, failed_attempts = 0, locked_until = NULL WHERE id = ?2",
            params![password_hash, id],
        )?;
        Ok(())
    }

    pub fn update_user(
        &self,
        id: i64,
        display_name: Option<&str>,
        access_all: Option<bool>,
        disabled: Option<bool>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        if let Some(name) = display_name {
            conn.execute(
                "UPDATE users SET display_name = ?1 WHERE id = ?2",
                params![name, id],
            )?;
        }
        if let Some(all) = access_all {
            conn.execute(
                "UPDATE users SET access_all = ?1 WHERE id = ?2 AND role != 'owner'",
                params![all as i32, id],
            )?;
        }
        if let Some(off) = disabled {
            conn.execute(
                "UPDATE users SET disabled = ?1 WHERE id = ?2 AND role != 'owner'",
                params![off as i32, id],
            )?;
        }
        Ok(())
    }

    pub fn record_login_failure(
        &self,
        id: i64,
        lock_after: i64,
        lock_minutes: i64,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET failed_attempts = failed_attempts + 1 WHERE id = ?1",
            params![id],
        )?;
        let attempts: i64 = conn.query_row(
            "SELECT failed_attempts FROM users WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        if attempts >= lock_after {
            let until = (Utc::now() + Duration::minutes(lock_minutes)).to_rfc3339();
            conn.execute(
                "UPDATE users SET locked_until = ?1 WHERE id = ?2",
                params![until, id],
            )?;
        }
        Ok(())
    }

    pub fn clear_login_failures(&self, id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET failed_attempts = 0, locked_until = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn set_library_grants(&self, user_id: i64, series_ids: &[i64]) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM library_grants WHERE user_id = ?1",
            params![user_id],
        )?;
        for series_id in series_ids {
            conn.execute(
                "INSERT OR IGNORE INTO library_grants (user_id, series_id) VALUES (?1, ?2)",
                params![user_id, series_id],
            )?;
        }
        Ok(())
    }

    pub fn list_library_grants(&self, user_id: i64) -> Result<Vec<i64>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT series_id FROM library_grants WHERE user_id = ?1 ORDER BY series_id")?;
        let rows = stmt.query_map(params![user_id], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }

    pub fn create_auth_session(
        &self,
        user_id: i64,
        token_hash: &str,
        device_label: &str,
        idle_expires_at: &str,
        absolute_expires_at: &str,
    ) -> Result<i64, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO auth_sessions
                (user_id, token_hash, device_label, created_at, last_used_at, rotated_at,
                 idle_expires_at, absolute_expires_at)
             VALUES (?1, ?2, ?3, ?4, ?4, ?4, ?5, ?6)",
            params![
                user_id,
                token_hash,
                device_label,
                now,
                idle_expires_at,
                absolute_expires_at
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn lookup_auth_session(&self, token_hash: &str) -> Result<Option<AuthSession>, DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT s.id, u.id, u.username, u.display_name, u.role, u.access_all, u.disabled,
                    s.idle_expires_at, s.absolute_expires_at, s.rotated_at
             FROM auth_sessions s
             JOIN users u ON u.id = s.user_id
             WHERE s.token_hash = ?1 AND s.idle_expires_at > ?2 AND s.absolute_expires_at > ?2",
            params![token_hash, now],
            |row| {
                let disabled: i32 = row.get(6)?;
                if disabled != 0 {
                    return Err(rusqlite::Error::QueryReturnedNoRows);
                }
                let access_all: i32 = row.get(5)?;
                let role = UserRole::from_str(row.get::<_, String>(4)?.as_str());
                Ok(AuthSession {
                    id: row.get(0)?,
                    viewer: Viewer {
                        user_id: row.get(1)?,
                        access_all: role == UserRole::Owner || access_all != 0,
                    },
                    username: row.get(2)?,
                    display_name: row.get(3)?,
                    role,
                    idle_expires_at: row.get(7)?,
                    absolute_expires_at: row.get(8)?,
                    rotated_at: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(DbError::from)
    }

    pub fn touch_auth_session(&self, id: i64, idle_expires_at: &str) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE auth_sessions SET last_used_at = ?1, idle_expires_at = ?2 WHERE id = ?3",
            params![now, idle_expires_at, id],
        )?;
        Ok(())
    }

    pub fn rotate_auth_session(
        &self,
        id: i64,
        token_hash: &str,
        idle_expires_at: &str,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE auth_sessions
             SET token_hash = ?1, last_used_at = ?2, rotated_at = ?2, idle_expires_at = ?3
             WHERE id = ?4",
            params![token_hash, now, idle_expires_at, id],
        )?;
        Ok(())
    }

    pub fn delete_auth_session(&self, id: i64) -> Result<(), DbError> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM auth_sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_auth_sessions(&self) -> Result<Vec<ActiveSession>, DbError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.user_id, u.username, s.device_label, s.created_at, s.last_used_at,
                    s.idle_expires_at
             FROM auth_sessions s
             JOIN users u ON u.id = s.user_id
             ORDER BY s.last_used_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ActiveSession {
                id: row.get(0)?,
                user_id: row.get(1)?,
                username: row.get(2)?,
                device_label: row.get(3)?,
                created_at: row.get(4)?,
                last_used_at: row.get(5)?,
                idle_expires_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(DbError::from)
    }
}

pub struct UserRecord {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub password_hash: String,
    pub role: UserRole,
    pub access_all: bool,
    pub disabled: bool,
    pub failed_attempts: i64,
    pub locked_until: Option<String>,
    pub created_at: String,
}

impl UserRecord {
    fn into_user(self) -> User {
        User {
            id: self.id,
            username: self.username,
            display_name: self.display_name,
            role: self.role,
            access_all: self.access_all || self.role == UserRole::Owner,
            disabled: self.disabled,
            has_password: !self.password_hash.is_empty(),
            created_at: self.created_at,
            locked_until: self.locked_until,
        }
    }
}

fn map_user_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserRecord> {
    Ok(UserRecord {
        id: row.get(0)?,
        username: row.get(1)?,
        display_name: row.get(2)?,
        password_hash: row.get(3)?,
        role: UserRole::from_str(row.get::<_, String>(4)?.as_str()),
        access_all: row.get::<_, i32>(5)? != 0,
        disabled: row.get::<_, i32>(6)? != 0,
        failed_attempts: row.get(7)?,
        locked_until: row.get(8)?,
        created_at: row.get(9)?,
    })
}

fn map_series_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Series> {
    let tags_raw: Option<String> = row.get(18)?;
    Ok(Series {
        id: row.get(0)?,
        root_id: row.get(1)?,
        folder_path: row.get(2)?,
        title: row.get(3)?,
        cover_path: row.get(4)?,
        content_type: ContentType::from_str(row.get::<_, String>(5)?.as_str()),
        source_format: SourceFormat::from_str(row.get::<_, String>(6)?.as_str()),
        reading_mode: ReadingMode::from_str(row.get::<_, String>(7)?.as_str()),
        variant_preference: VariantPreference::from_str(row.get::<_, String>(8)?.as_str()),
        page_direction: PageDirection::from_str(row.get::<_, String>(9)?.as_str()),
        favorite: row.get::<_, i32>(10)? != 0,
        description: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
        chapter_count: row.get(14)?,
        unread_count: row.get(15)?,
        progress_percent: row.get(16)?,
        last_read_at: row.get(17)?,
        group_name: row.get(19)?,
        tags: tags_raw
            .unwrap_or_default()
            .split(", ")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect(),
    })
}

fn detect_series_media(folder_path: &str, title: &str) -> (ContentType, SourceFormat) {
    let combined = format!("{folder_path}/{title}");
    let from_path = crate::media::detect_media_kind(&combined);
    if from_path.supported && (combined.to_lowercase().ends_with(".pdf") || combined.to_lowercase().ends_with(".mp4")) {
        return (from_path.content_type, from_path.source_format);
    }
    let lower = combined.to_lowercase();
    if lower.contains("video") || lower.contains("movie") || lower.contains("/movies/") {
        return (ContentType::Video, SourceFormat::Mp4);
    }
    if lower.contains("document")
        || lower.contains("book")
        || lower.contains("novel")
        || lower.contains("notes")
        || lower.contains("reference")
    {
        return (ContentType::Document, SourceFormat::Pdf);
    }
    if lower.contains("manhwa") || lower.contains("webtoon") {
        return (ContentType::Manhwa, SourceFormat::Pdf);
    }
    (ContentType::Manga, SourceFormat::Pdf)
}

fn map_chapter_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Chapter> {
    Ok(Chapter {
        id: row.get(0)?,
        series_id: row.get(1)?,
        file_path: row.get(2)?,
        title: row.get(3)?,
        chapter_number: row.get(4)?,
        volume_number: row.get(5)?,
        sort_key: row.get(6)?,
        page_count: row.get(7)?,
        progress_percent: row.get(8)?,
        last_page_index: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        missing: row.get::<_, i32>(12)? != 0,
    })
}

#[derive(Debug, Clone)]
pub struct ChapterRow {
    pub id: i64,
    pub series_id: i64,
    pub file_path: String,
    pub title: String,
    pub chapter_number: Option<f64>,
    pub volume_number: Option<i32>,
    pub sort_key: f64,
    pub page_count: i32,
    pub file_mtime: Option<i64>,
    pub file_size: Option<i64>,
    pub file_hash: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub missing: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ReadingProgress;

    fn temp_db() -> Database {
        let dir = std::env::temp_dir().join(format!("preview-test-{}", uuid::Uuid::new_v4()));
        Database::open(dir).unwrap()
    }

    #[test]
    fn progress_last_write_wins() {
        let db = temp_db();
        let root = db.add_root("/tmp/root").unwrap();
        let series = db.upsert_series(root.id, "/tmp/root/s", "S", None).unwrap();
        let ch = db
            .upsert_chapter(series, "/tmp/root/s/1.pdf", "1", Some(1.0), None, 1.0, 0, 1)
            .unwrap();
        db.save_progress(
            &Viewer::SYSTEM,
            &ReadingProgress {
                chapter_id: ch,
                page_index: 5,
                scroll_offset: 10.0,
                position_seconds: None,
                duration_seconds: None,
                percent: 20.0,
                updated_at: "2026-01-02T00:00:00Z".into(),
                device_id: Some("a".into()),
            },
        )
        .unwrap();
        db.save_progress(
            &Viewer::SYSTEM,
            &ReadingProgress {
                chapter_id: ch,
                page_index: 1,
                scroll_offset: 0.0,
                position_seconds: None,
                duration_seconds: None,
                percent: 5.0,
                updated_at: "2026-01-01T00:00:00Z".into(),
                device_id: Some("b".into()),
            },
        )
        .unwrap();
        let p = db.get_progress(&Viewer::SYSTEM, ch).unwrap().unwrap();
        assert_eq!(p.page_index, 5);
        assert_eq!(p.device_id.as_deref(), Some("a"));
    }

    #[test]
    fn member_cannot_see_ungranted_series() {
        let db = temp_db();
        let root = db.add_root("/tmp/root").unwrap();
        let visible = db.upsert_series(root.id, "/tmp/root/shown", "Shown", None).unwrap();
        let hidden = db.upsert_series(root.id, "/tmp/root/hidden", "Hidden", None).unwrap();
        let member = db
            .create_user("alex", "Alex", "hash", false)
            .unwrap();
        db.set_library_grants(member.id, &[visible]).unwrap();
        let viewer = Viewer {
            user_id: member.id,
            access_all: false,
        };
        let listed = db
            .list_series_filtered(&viewer, &SeriesFilter::default())
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "Shown");
        assert!(db.get_series(&viewer, hidden).is_err());
        assert!(db.get_series(&Viewer::SYSTEM, hidden).is_ok());
    }

    #[test]
    fn progress_is_per_user() {
        let db = temp_db();
        let root = db.add_root("/tmp/root").unwrap();
        let series = db.upsert_series(root.id, "/tmp/root/s", "S", None).unwrap();
        let ch = db
            .upsert_chapter(series, "/tmp/root/s/1.pdf", "1", Some(1.0), None, 1.0, 0, 1)
            .unwrap();
        let member = db.create_user("sam", "Sam", "hash", true).unwrap();
        let member_viewer = Viewer {
            user_id: member.id,
            access_all: true,
        };
        db.save_progress(
            &Viewer::SYSTEM,
            &ReadingProgress {
                chapter_id: ch,
                page_index: 9,
                scroll_offset: 0.0,
                position_seconds: None,
                duration_seconds: None,
                percent: 40.0,
                updated_at: "2026-01-02T00:00:00Z".into(),
                device_id: Some("mac".into()),
            },
        )
        .unwrap();
        db.save_progress(
            &member_viewer,
            &ReadingProgress {
                chapter_id: ch,
                page_index: 1,
                scroll_offset: 0.0,
                position_seconds: None,
                duration_seconds: None,
                percent: 5.0,
                updated_at: "2026-01-02T00:00:00Z".into(),
                device_id: Some("phone".into()),
            },
        )
        .unwrap();
        assert_eq!(
            db.get_progress(&Viewer::SYSTEM, ch).unwrap().unwrap().page_index,
            9
        );
        assert_eq!(
            db.get_progress(&member_viewer, ch).unwrap().unwrap().page_index,
            1
        );
    }
}
