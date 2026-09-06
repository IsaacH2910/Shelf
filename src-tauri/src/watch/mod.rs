use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

pub struct FolderWatcher {
    _watcher: RecommendedWatcher,
}

impl FolderWatcher {
    pub fn watch_roots<F>(roots: Vec<String>, on_event: F) -> Result<Self, String>
    where
        F: Fn(WatchEvent) + Send + Sync + 'static,
    {
        let handler = Arc::new(on_event);
        let (tx, rx) = mpsc::channel();

        let mut watcher = RecommendedWatcher::new(
            move |res| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )
        .map_err(|e| e.to_string())?;

        for root in &roots {
            watcher
                .watch(Path::new(root), RecursiveMode::Recursive)
                .map_err(|e| e.to_string())?;
            info!("Watching folder: {root}");
        }

        let handler_clone = Arc::clone(&handler);
        std::thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                match event.kind {
                    EventKind::Create(_) => {
                        for path in event.paths {
                            let supported = path.extension().and_then(|e| e.to_str()).map(|e| {
                                e.eq_ignore_ascii_case("pdf") || e.eq_ignore_ascii_case("mp4")
                            }).unwrap_or(false);
                            if supported {
                                handler_clone(WatchEvent::Added(path.to_string_lossy().to_string()));
                            }
                        }
                    }
                    EventKind::Modify(_) => {
                        for path in event.paths {
                            let supported = path.extension().and_then(|e| e.to_str()).map(|e| {
                                e.eq_ignore_ascii_case("pdf") || e.eq_ignore_ascii_case("mp4")
                            }).unwrap_or(false);
                            if supported {
                                handler_clone(WatchEvent::Modified(path.to_string_lossy().to_string()));
                            }
                        }
                    }
                    EventKind::Remove(_) => {
                        for path in event.paths {
                            handler_clone(WatchEvent::Removed(path.to_string_lossy().to_string()));
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(Self { _watcher: watcher })
    }
}

#[derive(Debug, Clone)]
pub enum WatchEvent {
    Added(String),
    Modified(String),
    Removed(String),
}

pub fn restart_watcher(
    roots: Vec<String>,
    indexer: Arc<crate::index::Indexer>,
) -> Result<FolderWatcher, String> {
    FolderWatcher::watch_roots(roots, move |event| {
        let result = match event {
            WatchEvent::Added(path) | WatchEvent::Modified(path) => indexer.handle_file_added(&path),
            WatchEvent::Removed(path) => indexer.handle_file_removed(&path),
        };
        if let Err(e) = result {
            error!("Watch handler error: {e}");
        }
    })
}
