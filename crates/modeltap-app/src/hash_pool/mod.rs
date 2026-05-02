//! Background SHA256 hash pool — composition root facing module (ADR-013).
//!
//! ## Design (ADR-013)
//!
//! After first paint completes, the composition root constructs a `Vec<HashJob>`
//! from the `discover()` output and calls [`spawn`]. Spawn returns a
//! [`HashPoolHandle`] containing:
//!
//! - shared atomic counters ([`HashPoolProgress`]) for the summary bar,
//! - a [`tokio_util::sync::CancellationToken`] that the composition root
//!   triggers on `Msg::Quit`,
//! - a `JoinSet<()>` holding the worker tasks + the throttle task, awaited by
//!   [`HashPoolHandle::shutdown`] within the 200 ms ADR-013 budget.
//!
//! Workers consume `HashJob`s from a bounded `tokio::sync::mpsc` queue. Each
//! worker, on a [`tokio::task::spawn_blocking`] call (CPU-bound SHA256 is the
//! correct primitive — see ADR-013 alternative B rejection), uses the existing
//! [`Sha256Cache::get_or_compute`] with the supplied [`Hasher`]. After a hash
//! lands the worker also reads the file's `(device, inode)` so the pure
//! `update::handle_hash_computed` can populate the inode map; this is the only
//! NEW system call the pool makes beyond what `Sha2Hasher` already does.
//!
//! ## Pool sizing
//!
//! Defaults to `min(num_cpus, 4)`. The undocumented `MODELTAP_HASH_WORKERS`
//! env var (ADR-013 §"Cons" mitigation) overrides this for users on very
//! slow HDDs or single-core VMs.
//!
//! ## Cancellation
//!
//! The composition root holds the `CancellationToken`. On `Msg::Quit` it calls
//! [`HashPoolHandle::shutdown`] which cancels and awaits join with a 200 ms
//! timeout — meeting AC-U1.5 (clean quit <500 ms; the rest of the budget is
//! TUI teardown).

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use modeltap_core::ports::Hasher;
use modeltap_core::ToolId;
use modeltap_tui::msg::Msg;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::sha256_cache::Sha256Cache;

mod handle;
mod queue;
mod throttle;
mod worker;

pub use handle::HashPoolHandle;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One unit of work for the pool. Captures the cache key fields
/// (`mtime`, `size`) AT JOB-START time so a file changed mid-hash will not
/// poison the next launch's cache (per ADR-013 §"Negative consequences").
#[derive(Debug, Clone)]
pub struct HashJob {
    pub tool: ToolId,
    pub model_id: String,
    pub path: PathBuf,
    pub mtime: u64,
    pub size: u64,
}

/// Lock-free atomic counters surfaced in the summary bar (`(N/M)`). All three
/// fields are `Arc<AtomicU64>` so the renderer can read without contending
/// with the workers.
#[derive(Debug, Clone, Default)]
pub struct HashPoolProgress {
    pub total: Arc<AtomicU64>,
    pub completed: Arc<AtomicU64>,
    pub failed: Arc<AtomicU64>,
}

// ---------------------------------------------------------------------------
// Pool sizing
// ---------------------------------------------------------------------------

/// Worker count: `MODELTAP_HASH_WORKERS` env var when set & parseable;
/// otherwise `num_cpus.min(4).max(1)` (ADR-013 default).
fn worker_count() -> usize {
    std::env::var("MODELTAP_HASH_WORKERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .unwrap_or_else(|| num_cpus::get().clamp(1, 4))
}

// ---------------------------------------------------------------------------
// Spawn entry point
// ---------------------------------------------------------------------------

/// Spawn the background hash pool. Returns a [`HashPoolHandle`] the
/// composition root holds for the lifetime of the launch.
///
/// `runtime` is the ambient tokio runtime handle; we use
/// [`tokio::runtime::Handle::spawn`] explicitly so this function works
/// whether called from inside or outside an async context (the composition
/// root, after first paint, is sync — it has just returned from the
/// initial render pass).
pub fn spawn(
    jobs: Vec<HashJob>,
    cache: Sha256Cache,
    hasher: Arc<dyn Hasher + Send + Sync>,
    msg_tx: UnboundedSender<Msg>,
    cancel: CancellationToken,
    runtime: &tokio::runtime::Handle,
) -> HashPoolHandle {
    let progress = HashPoolProgress::default();
    progress
        .total
        .store(jobs.len() as u64, std::sync::atomic::Ordering::SeqCst);

    let workers = worker_count();
    let (queue_rx, queue_handle) = queue::build(jobs, workers);

    let mut join: JoinSet<()> = JoinSet::new();

    // Workers all share the same Receiver via a Mutex (mpsc has a single
    // consumer; we serialize the pop to fan-out via spawn_blocking).
    let queue_rx = Arc::new(tokio::sync::Mutex::new(queue_rx));

    for _ in 0..workers {
        let rx = queue_rx.clone();
        let cache = cache.clone();
        let hasher = hasher.clone();
        let tx = msg_tx.clone();
        let progress = progress.clone();
        let cancel = cancel.clone();
        let rt = runtime.clone();
        join.spawn_on(
            async move {
                worker::worker_loop(rx, cache, hasher, tx, progress, cancel, rt).await;
            },
            runtime,
        );
    }

    // Throttle task posts HashProgressTick at 250ms cadence.
    {
        let tx = msg_tx.clone();
        let cancel = cancel.clone();
        join.spawn_on(
            async move {
                throttle::throttle_loop(tx, cancel).await;
            },
            runtime,
        );
    }

    // Drop the producer so workers see end-of-stream once the queue drains.
    drop(queue_handle);

    HashPoolHandle {
        progress,
        cancel,
        join,
    }
}
