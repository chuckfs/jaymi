//! Asynchronous embedding generation queue.
//!
//! Embeddings are generated off the request path. The Planner never interacts
//! with this queue — Understanding schedules work after content upserts, and
//! Search consumes indexed vectors.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jaymi_core::{HealthReport, JaymiError, JaymiResult, Lifecycle};
use jaymi_database::{content_embedding_hash, Database, EmbeddingRecord};
use jaymi_providers::EmbeddingProvider;
use jaymi_understanding::EmbeddingScheduler;

const NAME: &str = "embedding_queue";
const DEPENDENCIES: &[&str] = &["configuration", "logging", "database"];
const BATCH_SIZE: usize = 16;
const IDLE_WAIT: Duration = Duration::from_millis(100);

/// Diagnostics snapshot for the embedding queue.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmbeddingQueueDiagnostics {
    /// Whether the worker is running.
    pub running: bool,
    /// Embedding model currently used for generation.
    pub model_id: String,
    /// Indexed embedding count.
    pub indexed_embeddings: u64,
    /// Pending queue depth.
    pub queue_depth: u64,
    /// Successfully embedded since boot.
    pub processed: u64,
    /// Failed embeds since boot.
    pub failures: u64,
    /// Last processed source id, when any.
    pub last_source_id: Option<String>,
    /// Short detail for diagnostics UI.
    pub detail: String,
}

#[derive(Default)]
struct Runtime {
    processed: u64,
    failures: u64,
    last_source_id: Option<String>,
}

struct Shared {
    database: Arc<Database>,
    provider: Arc<dyn EmbeddingProvider>,
    runtime: Mutex<Runtime>,
    wake: Condvar,
    stop: AtomicBool,
    notify: Mutex<()>,
}

/// Background worker that turns normalized content into stored embeddings.
pub struct EmbeddingQueue {
    initialized: bool,
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl EmbeddingQueue {
    /// Create an uninitialized queue.
    pub fn new(database: Arc<Database>, provider: Arc<dyn EmbeddingProvider>) -> Self {
        Self {
            initialized: false,
            shared: Arc::new(Shared {
                database,
                provider,
                runtime: Mutex::new(Runtime::default()),
                wake: Condvar::new(),
                stop: AtomicBool::new(false),
                notify: Mutex::new(()),
            }),
            worker: None,
        }
    }

    /// Provider model id used for generation / retrieval.
    pub fn model_id(&self) -> String {
        self.shared.provider.model_id().to_string()
    }

    /// Wake the worker after enqueue.
    fn notify_worker(&self) {
        self.shared.wake.notify_one();
    }

    /// Process pending jobs synchronously (tests / flush).
    pub fn process_pending(&self) -> JaymiResult<usize> {
        process_batch(&self.shared, BATCH_SIZE.saturating_mul(8))
    }

    /// Diagnostics for indexed embeddings / model / queue depth.
    pub fn diagnostics(&self) -> JaymiResult<EmbeddingQueueDiagnostics> {
        let counts = self.shared.database.embedding_counts()?;
        let runtime = self
            .shared
            .runtime
            .lock()
            .map_err(|_| JaymiError::new("embedding queue lock poisoned"))?;
        let model_id = self.shared.provider.model_id().to_string();
        let running = self.worker.is_some() && !self.shared.stop.load(Ordering::SeqCst);
        Ok(EmbeddingQueueDiagnostics {
            running,
            detail: format!(
                "model={} · indexed={} · queue={} · processed={} · failures={}",
                model_id, counts.indexed, counts.queued, runtime.processed, runtime.failures
            ),
            model_id,
            indexed_embeddings: counts.indexed,
            queue_depth: counts.queued,
            processed: runtime.processed,
            failures: runtime.failures,
            last_source_id: runtime.last_source_id.clone(),
        })
    }
}

impl EmbeddingScheduler for EmbeddingQueue {
    fn schedule(&self, source_id: &str) -> JaymiResult<()> {
        if !self.initialized {
            return Err(JaymiError::new("embedding queue is not initialized"));
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0);
        self.shared.database.enqueue_embedding(source_id, now)?;
        self.notify_worker();
        Ok(())
    }
}

impl Lifecycle for EmbeddingQueue {
    fn name(&self) -> &'static str {
        NAME
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn dependencies(&self) -> &[&'static str] {
        DEPENDENCIES
    }

    fn initialize(&mut self) -> JaymiResult<()> {
        if self.initialized {
            return Ok(());
        }
        self.shared.stop.store(false, Ordering::SeqCst);
        let shared = Arc::clone(&self.shared);
        self.worker = Some(thread::spawn(move || worker_loop(shared)));
        self.initialized = true;
        // Kick once in case backlog exists.
        self.notify_worker();
        Ok(())
    }

    fn health_check(&self) -> HealthReport {
        let ok = self.initialized && self.shared.provider.embedding_status().available;
        HealthReport::new(NAME, self.initialized, ok, self.version(), DEPENDENCIES)
    }

    fn shutdown(&mut self) -> JaymiResult<()> {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.wake.notify_all();
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
        self.initialized = false;
        Ok(())
    }
}

fn worker_loop(shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::SeqCst) {
        match process_batch(&shared, BATCH_SIZE) {
            Ok(0) => {
                let guard = shared
                    .notify
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let (guard, _) = shared
                    .wake
                    .wait_timeout(guard, IDLE_WAIT)
                    .unwrap_or_else(|error| error.into_inner());
                drop(guard);
            }
            Ok(_) => {}
            Err(error) => {
                jaymi_logging::warn(
                    "embedding_queue",
                    format!("batch failed: {}", error.message()),
                );
                thread::sleep(IDLE_WAIT);
            }
        }
    }
}

fn process_batch(shared: &Shared, limit: usize) -> JaymiResult<usize> {
    let jobs = shared.database.claim_embedding_queue(limit)?;
    if jobs.is_empty() {
        return Ok(0);
    }
    let mut done = 0usize;
    for job in jobs {
        match embed_one(shared, &job.source_id) {
            Ok(()) => {
                shared.database.complete_embedding_queue(&job.source_id)?;
                if let Ok(mut runtime) = shared.runtime.lock() {
                    runtime.processed = runtime.processed.saturating_add(1);
                    runtime.last_source_id = Some(job.source_id.clone());
                }
                done = done.saturating_add(1);
            }
            Err(error) => {
                let _ = shared
                    .database
                    .fail_embedding_queue(&job.source_id, error.message());
                if let Ok(mut runtime) = shared.runtime.lock() {
                    runtime.failures = runtime.failures.saturating_add(1);
                }
                // Drop poisonously failing jobs after a few attempts.
                if job.attempts >= 3 {
                    let _ = shared.database.complete_embedding_queue(&job.source_id);
                }
            }
        }
    }
    Ok(done)
}

fn embed_one(shared: &Shared, source_id: &str) -> JaymiResult<()> {
    let content = shared
        .database
        .get_content_by_source_id(source_id)?
        .ok_or_else(|| JaymiError::new(format!("content missing for embedding: {source_id}")))?;
    let hash = content_embedding_hash(content.title.as_deref(), &content.plain_text);
    let model_id = shared.provider.model_id().to_string();
    if let Some(existing) = shared.database.get_embedding_by_source_id(source_id)? {
        if existing.content_hash == hash && existing.model_id == model_id {
            return Ok(());
        }
    }

    let text = match content.title.as_ref() {
        Some(title) if !title.trim().is_empty() => format!("{title}\n{}", content.plain_text),
        _ => content.plain_text.clone(),
    };
    let vectors = shared.provider.embed(&[text])?;
    let vector = vectors
        .into_iter()
        .next()
        .ok_or_else(|| JaymiError::new("embedding provider returned no vectors"))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    shared.database.upsert_embedding(&EmbeddingRecord {
        source_id: source_id.to_string(),
        model_id: vector.model_id,
        dims: vector.values.len() as u32,
        vector: vector.values,
        content_hash: hash,
        embedded_at: now,
    })?;
    Ok(())
}
