use crossbeam_channel::{Receiver, Sender};
use parking_lot::Mutex;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering as AtomicOrdering};
use std::sync::Arc;
use tracing::{debug, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum JobPriority {
    CacheEviction = 0,
    OcrTranslation = 1,
    CoverGeneration = 2,
    Indexing = 3,
    ReaderPrefetch = 4,
    Interactive = 5,
}

pub type RenderReply = Sender<Result<(Vec<u8>, u32, u32), String>>;
pub type OcrReply = Sender<Result<Vec<crate::models::OcrRegion>, String>>;

pub enum JobKind {
    ScanRoot {
        root_id: i64,
        path: String,
    },
    Indexing {
        chapter_id: i64,
        file_path: String,
    },
    CoverGeneration {
        series_id: i64,
        chapter_id: i64,
        file_path: String,
    },
    DimensionAnalysis {
        series_id: i64,
    },
    RenderPage {
        chapter_id: i64,
        file_path: String,
        page_index: i32,
        target_width: u32,
        priority: JobPriority,
        cancel_token: Arc<AtomicBool>,
        reply: Option<RenderReply>,
    },
    CacheEviction {
        chapter_id: i64,
        current_page: i32,
        window: i32,
    },
    OcrTranslation {
        chapter_id: i64,
        page_index: i32,
        engine_id: String,
        translator_id: String,
        reply: Option<OcrReply>,
    },
    FileHash {
        chapter_id: i64,
        file_path: String,
    },
}

struct PrioritizedJob {
    priority: JobPriority,
    seq: u64,
    job: JobKind,
}

impl PartialEq for PrioritizedJob {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.seq == other.seq
    }
}

impl Eq for PrioritizedJob {}

impl PartialOrd for PrioritizedJob {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedJob {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => other.seq.cmp(&self.seq),
            ord => ord,
        }
    }
}

pub type JobHandler = Arc<dyn Fn(JobKind) + Send + Sync>;

pub struct JobScheduler {
    queue: Mutex<BinaryHeap<PrioritizedJob>>,
    notify: Sender<()>,
    seq: Mutex<u64>,
    pending: AtomicU32,
    index_pending: AtomicU32,
    index_processed: AtomicU32,
    index_failed: AtomicU32,
    handler: Mutex<Option<JobHandler>>,
    interactive_pending: AtomicU32,
}

impl JobScheduler {
    pub fn new() -> (Arc<Self>, Receiver<()>) {
        let (notify_tx, notify_rx) = crossbeam_channel::unbounded();
        let scheduler = Arc::new(Self {
            queue: Mutex::new(BinaryHeap::new()),
            notify: notify_tx,
            seq: Mutex::new(0),
            pending: AtomicU32::new(0),
            index_pending: AtomicU32::new(0),
            index_processed: AtomicU32::new(0),
            index_failed: AtomicU32::new(0),
            handler: Mutex::new(None),
            interactive_pending: AtomicU32::new(0),
        });
        (scheduler, notify_rx)
    }

    pub fn set_handler(&self, handler: JobHandler) {
        *self.handler.lock() = Some(handler);
    }

    pub fn enqueue(&self, job: JobKind) {
        if is_index_job(&job) {
            self.index_pending.fetch_add(1, AtomicOrdering::Relaxed);
        }
        let priority = match &job {
            JobKind::RenderPage { priority, .. } => *priority,
            JobKind::ScanRoot { .. } | JobKind::Indexing { .. } | JobKind::FileHash { .. } => {
                JobPriority::Indexing
            }
            JobKind::CoverGeneration { .. } | JobKind::DimensionAnalysis { .. } => {
                JobPriority::CoverGeneration
            }
            JobKind::CacheEviction { .. } => JobPriority::CacheEviction,
            JobKind::OcrTranslation { .. } => JobPriority::OcrTranslation,
        };

        if priority == JobPriority::Interactive {
            self.interactive_pending.fetch_add(1, AtomicOrdering::Relaxed);
        }

        let mut seq = self.seq.lock();
        *seq += 1;
        let item = PrioritizedJob {
            priority,
            seq: *seq,
            job,
        };
        drop(seq);

        self.queue.lock().push(item);
        self.pending.fetch_add(1, AtomicOrdering::Relaxed);
        let _ = self.notify.send(());
    }

    pub fn has_interactive_pending(&self) -> bool {
        self.interactive_pending.load(AtomicOrdering::Relaxed) > 0
    }

    pub fn pending_count(&self) -> u32 {
        self.pending.load(AtomicOrdering::Relaxed)
    }

    pub fn index_pending_count(&self) -> u32 {
        self.index_pending.load(AtomicOrdering::Relaxed)
    }

    pub fn index_processed_count(&self) -> u32 {
        self.index_processed.load(AtomicOrdering::Relaxed)
    }

    pub fn record_index_failure(&self) {
        self.index_failed.fetch_add(1, AtomicOrdering::Relaxed);
    }

    pub fn index_failed_count(&self) -> u32 {
        self.index_failed.load(AtomicOrdering::Relaxed)
    }

    pub fn process_one(&self) -> bool {
        loop {
            let item = self.queue.lock().pop();
            let Some(item) = item else {
                return false;
            };
            self.pending.fetch_sub(1, AtomicOrdering::Relaxed);
            if is_index_job(&item.job) {
                self.index_pending.fetch_sub(1, AtomicOrdering::Relaxed);
            }

            if item.priority == JobPriority::Interactive {
                self.interactive_pending.fetch_sub(1, AtomicOrdering::Relaxed);
            }

            if let JobKind::RenderPage { cancel_token, reply, .. } = &item.job {
                if cancel_token.load(AtomicOrdering::Relaxed) {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err("cancelled".into()));
                    }
                    continue;
                }
            }

            // Never start OCR/eviction while the reader is waiting on a page.
            if self.has_interactive_pending()
                && matches!(
                    item.priority,
                    JobPriority::OcrTranslation | JobPriority::CacheEviction | JobPriority::CoverGeneration
                )
            {
                self.requeue(item);
                return true;
            }

            let index_job = is_index_job(&item.job);
            if let Some(handler) = self.handler.lock().clone() {
                debug!("Processing job {:?}", item.priority);
                handler(item.job);
            }
            if index_job {
                self.index_processed.fetch_add(1, AtomicOrdering::Relaxed);
            }
            return true;
        }
    }

    fn requeue(&self, item: PrioritizedJob) {
        if item.priority == JobPriority::Interactive {
            self.interactive_pending.fetch_add(1, AtomicOrdering::Relaxed);
        }
        self.queue.lock().push(item);
        self.pending.fetch_add(1, AtomicOrdering::Relaxed);
    }

    pub fn cancel_prefetch(&self, chapter_id: i64, keep_page: Option<i32>, window: i32) {
        let mut queue = self.queue.lock();
        let drained: Vec<_> = queue.drain().collect();
        let mut dropped = 0u32;
        for item in drained {
            let drop = match &item.job {
                JobKind::RenderPage {
                    chapter_id: cid,
                    page_index,
                    priority,
                    cancel_token,
                    reply,
                    ..
                } if *cid == chapter_id && *priority == JobPriority::ReaderPrefetch => {
                    let outside = keep_page
                        .map(|p| (*page_index - p).abs() > window)
                        .unwrap_or(true);
                    if outside {
                        cancel_token.store(true, AtomicOrdering::Relaxed);
                        if let Some(reply) = reply {
                            let _ = reply.send(Err("cancelled".into()));
                        }
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            };
            if drop {
                dropped += 1;
            } else {
                queue.push(item);
            }
        }
        if dropped > 0 {
            self.pending.fetch_sub(dropped, AtomicOrdering::Relaxed);
        }
    }

    pub fn start_worker(self: &Arc<Self>, notify_rx: Receiver<()>) {
        let scheduler = Arc::clone(self);
        std::thread::Builder::new()
            .name("job-scheduler".into())
            .spawn(move || loop {
                while scheduler.process_one() {}
                if notify_rx.recv().is_err() {
                    warn!("Job scheduler channel closed");
                    break;
                }
            })
            .expect("Failed to spawn job scheduler thread");
    }
}

fn is_index_job(job: &JobKind) -> bool {
    matches!(
        job,
        JobKind::ScanRoot { .. }
            | JobKind::Indexing { .. }
            | JobKind::CoverGeneration { .. }
            | JobKind::DimensionAnalysis { .. }
            | JobKind::FileHash { .. }
    )
}
