mod chapter_parser;

pub use chapter_parser::parse_chapter_from_filename;

use crate::db::Database;
use crate::jobs::{JobKind, JobScheduler};
use crate::media;
use crate::models::LibraryRoot;
use crate::models::{PageDirection, ReadingMode, Viewer};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tracing::info;

pub struct Indexer {
    db: Arc<Database>,
    scheduler: Arc<JobScheduler>,
}

impl Indexer {
    pub fn new(db: Arc<Database>, scheduler: Arc<JobScheduler>) -> Self {
        Self { db, scheduler }
    }

    pub fn scan_root(&self, root_id: i64, root_path: &str) -> Result<(), String> {
        let root = PathBuf::from(root_path);
        if !root.is_dir() {
            return Err(format!("Root path is not a directory: {root_path}"));
        }

        for folder in collect_series_folders(&root) {
            self.index_series_folder(root_id, &root, &folder)?;
        }
        Ok(())
    }

    fn index_series_folder(&self, root_id: i64, root: &Path, folder: &Path) -> Result<(), String> {
        let folder_path = folder.to_string_lossy().to_string();
        let title = folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .trim_start_matches('.')
            .trim()
            .to_string();
        let group_name = derive_group_name(root, folder);

        let series_id = self
            .db
            .upsert_series(root_id, &folder_path, &title, group_name.as_deref())
            .map_err(|e| e.to_string())?;

        let media_files = collect_media_files_shallow(folder);
        if let Some((content_type, source_format)) = crate::media::infer_series_media(&media_files) {
            self.db
                .set_series_media_kind(series_id, content_type, source_format)
                .map_err(|e| e.to_string())?;
        }

        let on_disk: std::collections::HashSet<String> = media_files
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();

        if let Ok(existing) = self.db.list_chapter_paths(series_id) {
            for (id, path) in existing {
                if !on_disk.contains(&path) {
                    let _ = self.db.mark_chapter_missing(id, true);
                }
            }
        }

        let mut queued_work = false;
        let has_pdf = media_files.iter().any(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("pdf"))
                .unwrap_or(false)
        });
        for media_path in media_files {
            let file_path = media_path.to_string_lossy().to_string();
            if let Ok(meta) = std::fs::metadata(&media_path) {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let size = meta.len() as i64;

                if let Ok(Some(existing)) = self.db.get_chapter_by_path(&file_path) {
                    if existing.file_mtime == Some(mtime)
                        && existing.file_size == Some(size)
                        && !existing.missing
                    {
                        if existing.file_hash.is_none() {
                            self.scheduler.enqueue(JobKind::FileHash {
                                chapter_id: existing.id,
                                file_path: file_path.clone(),
                            });
                        }
                        continue;
                    }
                }

                let parsed = parse_chapter_from_filename(&file_path);
                let chapter_id = self
                    .db
                    .upsert_chapter(
                        series_id,
                        &file_path,
                        &parsed.title,
                        parsed.chapter_number,
                        parsed.volume_number,
                        parsed.sort_key,
                        mtime,
                        size,
                    )
                    .map_err(|e| e.to_string())?;

                queued_work = true;
                self.scheduler.enqueue(JobKind::Indexing {
                    chapter_id,
                    file_path: file_path.clone(),
                });
                if media::is_pdf_path(&file_path) {
                    self.scheduler.enqueue(JobKind::CoverGeneration {
                        series_id,
                        chapter_id,
                        file_path: file_path.clone(),
                    });
                }
                self.scheduler.enqueue(JobKind::FileHash {
                    chapter_id,
                    file_path,
                });
            }
        }

        if !queued_work && has_pdf {
            self.scheduler
                .enqueue(JobKind::DimensionAnalysis { series_id });
        }
        Ok(())
    }

    pub fn handle_file_added(&self, path: &str) -> Result<(), String> {
        if !media::is_supported_media_path(path) {
            return Ok(());
        }
        let path_buf = PathBuf::from(path);
        let roots = self.db.list_roots().map_err(|e| e.to_string())?;
        let root = match_library_root(&roots, &path_buf)
            .ok_or_else(|| "File not under any watched root".to_string())?;

        let root_path = Path::new(&root.path);
        let Some(series_folder) = derive_series_folder(root_path, &path_buf) else {
            info!("Ignoring file without a valid series folder under root: {path}");
            return Ok(());
        };

        self.index_series_folder(root.id, root_path, &series_folder)?;
        info!("Indexed new file: {path}");
        Ok(())
    }

    pub fn handle_file_removed(&self, path: &str) -> Result<(), String> {
        if let Ok(Some(chapter)) = self.db.get_chapter_by_path(path) {
            self.db
                .mark_chapter_missing(chapter.id, true)
                .map_err(|e| e.to_string())?;
            info!("Marked chapter missing: {path}");
        }
        Ok(())
    }

    pub fn analyze_series_dimensions(&self, series_id: i64) -> Result<(), String> {
        let chapters = self
            .db
            .list_chapters(&Viewer::SYSTEM, series_id)
            .map_err(|e| e.to_string())?;

        let mut ratios: Vec<f64> = Vec::new();
        for chapter in chapters.iter().filter(|c| !c.missing).take(3) {
            if let Ok(dims) = self.db.get_chapter_page_dimensions(chapter.id) {
                for (_, w, h) in dims {
                    if w > 0.0 {
                        ratios.push(h / w);
                    }
                }
            }
        }

        if ratios.is_empty() {
            return Ok(());
        }

        ratios.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = ratios[ratios.len() / 2];

        let (mode, direction) = detect_reading_mode(median);

        if let Ok(series) = self.db.get_series(&Viewer::SYSTEM, series_id) {
            if series.reading_mode == ReadingMode::Auto {
                self.db
                    .update_series_reading_mode(series_id, mode, direction)
                    .map_err(|e| e.to_string())?;
            }
        }

        Ok(())
    }

    pub fn index_chapter_metadata(&self, chapter_id: i64, file_path: &str) -> Result<(), String> {
        if !media::is_pdf_path(file_path) {
            return Ok(());
        }
        match crate::pdf::get_page_count(file_path) {
            Ok(count) => {
                self.db
                    .update_chapter_page_count(chapter_id, count)
                    .map_err(|e| e.to_string())?;
            }
            Err(e) => return Err(format!("Failed to get page count for {file_path}: {e}")),
        }
        Ok(())
    }

    pub fn hash_chapter_file(&self, chapter_id: i64, file_path: &str) -> Result<(), String> {
        let hash = hash_file(file_path)?;
        let prev = self
            .db
            .set_chapter_hash(chapter_id, &hash)
            .map_err(|e| e.to_string())?;
        if prev.as_ref() != Some(&hash) && prev.is_some() {
            let _ = self.db.invalidate_page_caches(chapter_id);
        }
        Ok(())
    }
}

pub fn hash_file(path: &str) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

pub fn detect_reading_mode(median_ratio: f64) -> (ReadingMode, PageDirection) {
    if median_ratio > 2.0 {
        (ReadingMode::Webtoon, PageDirection::Ltr)
    } else {
        (ReadingMode::PagedRtl, PageDirection::Rtl)
    }
}

fn is_supported_media(path: &Path) -> bool {
    media::is_supported_media_path(&path.to_string_lossy())
}

fn collect_media_files_shallow(folder: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_file() && is_supported_media(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Every directory that directly holds a supported media file is its own series.
fn collect_series_folders(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut has_media = false;
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.file_type().is_symlink() {
                continue;
            }
            if meta.is_dir() {
                subdirs.push(path);
            } else if meta.is_file() && is_supported_media(&path) {
                has_media = true;
            }
        }
        if has_media && dir != root {
            out.push(dir.clone());
        }
        for sub in subdirs {
            stack.push(sub);
        }
    }

    out.sort();
    out
}

fn match_library_root<'a>(roots: &'a [LibraryRoot], file_path: &Path) -> Option<&'a LibraryRoot> {
    roots
        .iter()
        .filter_map(|root| {
            let root_path = Path::new(&root.path);
            file_path
                .strip_prefix(root_path)
                .ok()
                .map(|_| (root, root_path.components().count()))
        })
        .max_by_key(|(_, depth)| *depth)
        .map(|(root, _)| root)
}

/// The series folder is the deepest folder that directly holds the media file.
fn derive_series_folder(root_path: &Path, file_path: &Path) -> Option<PathBuf> {
    let parent = file_path.parent()?;
    let rel = parent.strip_prefix(root_path).ok()?;
    if rel.components().next().is_none() {
        return None;
    }
    if parent.is_dir() {
        Some(parent.to_path_buf())
    } else {
        None
    }
}

/// The topic/group is the top-level folder under the root when a series is nested.
fn derive_group_name(root: &Path, series_folder: &Path) -> Option<String> {
    let rel = series_folder.strip_prefix(root).ok()?;
    let names: Vec<&str> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(n) => n.to_str(),
            _ => None,
        })
        .collect();
    if names.len() >= 2 {
        let group = names[0].trim_start_matches('.').trim();
        if group.is_empty() {
            None
        } else {
            Some(group.to_string())
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::detect_reading_mode;
    use super::{collect_media_files_shallow, collect_series_folders, derive_group_name, derive_series_folder, match_library_root};
    use crate::media::infer_series_media;
    use crate::models::{PageDirection, ReadingMode};
    use crate::models::LibraryRoot;
    use std::path::{Path, PathBuf};

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "preview-indexer-{name}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn tall_pages_are_webtoon() {
        let (mode, dir) = detect_reading_mode(2.4);
        assert_eq!(mode, ReadingMode::Webtoon);
        assert_eq!(dir, PageDirection::Ltr);
    }

    #[test]
    fn normal_pages_are_paged_rtl() {
        let (mode, dir) = detect_reading_mode(1.4);
        assert_eq!(mode, ReadingMode::PagedRtl);
        assert_eq!(dir, PageDirection::Rtl);
    }

    #[test]
    fn shallow_collect_ignores_subfolders_and_non_media() {
        let root = temp_path("collect");
        let nested = root.join("S1");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("01.pdf"), b"x").unwrap();
        std::fs::write(nested.join("02.mp4"), b"x").unwrap();
        std::fs::write(root.join("note.txt"), b"x").unwrap();

        let files = collect_media_files_shallow(&root);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("01.pdf"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn each_media_folder_becomes_its_own_series() {
        let root = temp_path("folders");
        let show = root.join("Show");
        let s1 = show.join("S1");
        let s2 = show.join("S2");
        std::fs::create_dir_all(&s1).unwrap();
        std::fs::create_dir_all(&s2).unwrap();
        let standalone = root.join("Standalone");
        std::fs::create_dir_all(&standalone).unwrap();
        std::fs::write(s1.join("01.mp4"), b"x").unwrap();
        std::fs::write(s2.join("01.mp4"), b"x").unwrap();
        std::fs::write(standalone.join("01.mp4"), b"x").unwrap();

        let folders = collect_series_folders(&root);
        assert_eq!(folders.len(), 3);
        assert!(folders.iter().any(|p| p.ends_with("S1")));
        assert!(folders.iter().any(|p| p.ends_with("S2")));
        assert!(folders.iter().any(|p| p.ends_with("Standalone")));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn collects_hidden_dot_prefixed_media_files() {
        let root = temp_path("hidden");
        let hidden_series = root.join(".Novel");
        std::fs::create_dir_all(&hidden_series).unwrap();
        std::fs::write(hidden_series.join(".影帝的秘密.pdf"), b"x").unwrap();
        std::fs::write(hidden_series.join(".thumb.jpg"), b"x").unwrap();

        let files = collect_media_files_shallow(&hidden_series);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with(".影帝的秘密.pdf"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn nested_series_derives_top_level_group() {
        let root = temp_path("group");
        let series = root.join("槍彈辯駁").join("未來篇");
        assert_eq!(derive_group_name(&root, &series).as_deref(), Some("槍彈辯駁"));

        let standalone = root.join("藍色監獄");
        assert_eq!(derive_group_name(&root, &standalone), None);

        let hidden = root.join(".ALPHA TRAUMA").join("S1");
        assert_eq!(derive_group_name(&root, &hidden).as_deref(), Some("ALPHA TRAUMA"));
    }

    #[test]
    fn derives_media_folder_for_nested_file() {
        let root = temp_path("series-folder");
        let series = root.join("My Series");
        let season = series.join("S1");
        std::fs::create_dir_all(&season).unwrap();
        let file = season.join("Episode 01.mp4");
        std::fs::write(&file, b"x").unwrap();

        let resolved = derive_series_folder(Path::new(&root), &file).unwrap();
        assert_eq!(resolved, season);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chooses_most_specific_matching_root() {
        let roots = vec![
            LibraryRoot {
                id: 1,
                path: "/Library".into(),
                added_at: "".into(),
            },
            LibraryRoot {
                id: 2,
                path: "/Library/Anime".into(),
                added_at: "".into(),
            },
        ];

        let matched = match_library_root(&roots, Path::new("/Library/Anime/Series/S1/01.mp4"))
            .unwrap();
        assert_eq!(matched.id, 2);
    }

    #[test]
    fn derives_hidden_series_folder_for_nested_file() {
        let root = temp_path("hidden-series");
        let series = root.join(".ALPHA TRAUMA");
        let nested = series.join("temp_第02话");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("第02话.mp4");
        std::fs::write(&file, b"x").unwrap();

        let resolved = derive_series_folder(Path::new(&root), &file).unwrap();
        assert_eq!(resolved, nested);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ignores_file_directly_under_root_without_series_folder() {
        let root = temp_path("root-file");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("lonely.mp4");
        std::fs::write(&file, b"x").unwrap();

        let resolved = derive_series_folder(Path::new(&root), &file);
        assert!(resolved.is_none());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ignores_file_when_top_level_series_folder_was_removed() {
        let root = temp_path("removed-series");
        let series = root.join("My Series");
        let nested = series.join("S1");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("01.mp4");
        std::fs::write(&file, b"x").unwrap();

        std::fs::remove_dir_all(&series).unwrap();
        let resolved = derive_series_folder(Path::new(&root), &file);
        assert!(resolved.is_none());

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recursive_collection_skips_symlinked_directories_and_files() {
        let root = temp_path("symlink-skip");
        let nested = root.join("S1");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("01.pdf"), b"x").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(root.join("01.pdf"), nested.join("alias.pdf")).unwrap();
            symlink(root.join("S1"), root.join("loop")).unwrap();
        }

        let files = collect_media_files_shallow(&root);
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("01.pdf"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn infers_video_series_when_mp4_dominates() {
        let files = vec![
            PathBuf::from("S1/01.mp4"),
            PathBuf::from("S1/02.mp4"),
            PathBuf::from("chapter.pdf"),
        ];
        let kind = infer_series_media(&files).unwrap();
        assert_eq!(kind.0, crate::models::ContentType::Video);
        assert_eq!(kind.1, crate::models::SourceFormat::Mp4);
    }

    #[test]
    fn infers_pdf_series_when_pdf_dominates() {
        let files = vec![
            PathBuf::from("第01话.pdf"),
            PathBuf::from("第02话.pdf"),
            PathBuf::from("teaser.mp4"),
        ];
        let kind = infer_series_media(&files).unwrap();
        assert_eq!(kind.0, crate::models::ContentType::Manga);
        assert_eq!(kind.1, crate::models::SourceFormat::Pdf);
    }

    #[test]
    fn infers_document_series_for_plain_pdfs() {
        let files = vec![PathBuf::from("manual.pdf"), PathBuf::from("appendix.pdf")];
        let kind = infer_series_media(&files).unwrap();
        assert_eq!(kind.0, crate::models::ContentType::Document);
        assert_eq!(kind.1, crate::models::SourceFormat::Pdf);
    }
}
