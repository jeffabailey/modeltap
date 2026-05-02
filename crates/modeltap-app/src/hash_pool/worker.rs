//! Hash worker — one task per logical worker (ADR-013).
//!
//! Each worker pops a `HashJob`, calls `tokio::task::spawn_blocking` (the
//! correct primitive for CPU-bound SHA256, per ADR-013 alternative B
//! rejection), then dispatches `Msg::HashComputed` (with `(device, inode)`
//! captured immediately after the hash) or `Msg::HashFailed`.
//!
//! Cancellation: `select!` between `recv()` and `cancel.cancelled()`. When
//! cancellation fires mid-`spawn_blocking`, we let the in-flight blocking
//! task finish (cancel cannot abort a blocking thread; it would leak the
//! `JoinHandle` otherwise) but skip dispatching the result — the row stays
//! Pending and the next launch re-hashes.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use modeltap_core::ports::{HashProgress, Hasher};
use modeltap_tui::msg::{HashFailureReason, Msg};
use tokio::sync::mpsc::{Receiver, UnboundedSender};
use tokio_util::sync::CancellationToken;

use super::{HashJob, HashPoolProgress};
use crate::sha256_cache::{Sha256Cache, Sha256CacheKey};

pub(super) async fn worker_loop(
    rx: Arc<tokio::sync::Mutex<Receiver<HashJob>>>,
    cache: Sha256Cache,
    hasher: Arc<dyn Hasher + Send + Sync>,
    msg_tx: UnboundedSender<Msg>,
    progress: HashPoolProgress,
    cancel: CancellationToken,
    runtime: tokio::runtime::Handle,
) {
    loop {
        // Acquire the queue mutex briefly to pop one job. select! on the
        // cancellation token so a long-idle worker exits promptly on quit.
        let job = {
            let mut guard = match cancel_or_locked(&cancel, &rx).await {
                Some(g) => g,
                None => return,
            };
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return,
                next = guard.recv() => match next {
                    Some(j) => j,
                    None => return, // queue closed; clean exit
                },
            }
        };

        // Hash on the blocking pool. `spawn_blocking` returns a JoinHandle we
        // can `.await` cooperatively.
        let cache_inner = cache.clone();
        let hasher_inner = hasher.clone();
        let path = job.path.clone();
        let mtime = job.mtime;
        let size = job.size;

        let mut blocking = runtime.spawn_blocking(move || {
            let key = Sha256CacheKey {
                path: path.clone(),
                mtime,
                size,
            };
            let mut sink = |_p: HashProgress| {};
            let hash_result = cache_inner.get_or_compute(key, &*hasher_inner, &mut sink);
            // Capture (device, inode) only on success.
            let inode = hash_result
                .as_ref()
                .ok()
                .and_then(|_| read_inode(&path).ok());
            (hash_result, inode)
        });

        // Wait either for the blocking job to finish OR for cancel — but if
        // cancel fires we still must let the blocking task return (we can't
        // abort it). On cancel we deliver Cancelled and break.
        let result = tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                // Drain the in-flight blocking result so it doesn't leak,
                // but do not surface the row's outcome — exit the loop.
                let _ = (&mut blocking).await;
                return;
            }
            r = &mut blocking => r,
        };

        match result {
            Ok((Ok(hash), inode)) => {
                let (device, inode) = inode.unwrap_or((0, 0));
                progress.completed.fetch_add(1, Ordering::SeqCst);
                let _ = msg_tx.send(Msg::HashComputed {
                    tool: job.tool,
                    model_id: job.model_id,
                    hash,
                    device,
                    inode,
                });
            }
            Ok((Err(io_err), _)) => {
                progress.completed.fetch_add(1, Ordering::SeqCst);
                progress.failed.fetch_add(1, Ordering::SeqCst);
                let _ = msg_tx.send(Msg::HashFailed {
                    tool: job.tool,
                    model_id: job.model_id,
                    reason: HashFailureReason::Io(io_err.to_string()),
                });
            }
            Err(join_err) => {
                progress.completed.fetch_add(1, Ordering::SeqCst);
                progress.failed.fetch_add(1, Ordering::SeqCst);
                let reason = if join_err.is_cancelled() {
                    HashFailureReason::Cancelled
                } else {
                    HashFailureReason::Other(format!("worker panic: {join_err}"))
                };
                let _ = msg_tx.send(Msg::HashFailed {
                    tool: job.tool,
                    model_id: job.model_id,
                    reason,
                });
            }
        }
    }
}

/// Lock the receiver, but bail out early if cancellation fires before we
/// acquire the lock.
async fn cancel_or_locked<'a, T: Send>(
    cancel: &CancellationToken,
    mu: &'a Arc<tokio::sync::Mutex<T>>,
) -> Option<tokio::sync::MutexGuard<'a, T>> {
    tokio::select! {
        biased;
        _ = cancel.cancelled() => None,
        g = mu.lock() => Some(g),
    }
}

// ---------------------------------------------------------------------------
// (device, inode) lookup — Unix-only (per project constraint #6: WSL on Windows).
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn read_inode(path: &Path) -> std::io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(path)?;
    Ok((m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn read_inode(_path: &Path) -> std::io::Result<(u64, u64)> {
    // WSL-only on Windows per project constraint #6 — Linux paths via WSL.
    Ok((0, 0))
}
