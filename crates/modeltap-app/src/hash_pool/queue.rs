//! Bounded MPSC queue feeding the hash workers (ADR-013).
//!
//! Capacity is `4 * num_workers` (the suggestion in step 01-07). Sender side
//! is closed (dropped) immediately after we push every job — the queue runs
//! to drain and the workers exit naturally on `recv() -> None`.

use tokio::sync::mpsc;

use super::HashJob;

/// Sender side returned to the caller. We push every job up front (the queue
/// is bounded but we use `try_send` in a tight loop with a fallback to
/// `blocking_send` so we never block the caller for long). Caller drops the
/// returned handle when ready — workers see end-of-stream.
pub(super) struct QueueHandle(#[allow(dead_code)] mpsc::Sender<HashJob>);

/// Build the queue, push every job, return the receiver + a sender handle
/// that the caller drops to signal end-of-stream.
pub(super) fn build(
    jobs: Vec<HashJob>,
    workers: usize,
    runtime: &tokio::runtime::Handle,
) -> (mpsc::Receiver<HashJob>, QueueHandle) {
    let cap = (workers.saturating_mul(4)).max(1);
    let (tx, rx) = mpsc::channel::<HashJob>(cap);

    // Fire-and-forget pusher task. We avoid `blocking_send` because that
    // requires being outside the runtime; instead we use try_send in a loop
    // with a small async sleep when the channel is momentarily full. For a
    // bounded queue with `cap = 4*workers` this rarely contends.
    let tx_for_push = tx.clone();
    runtime.spawn(async move {
        for job in jobs {
            // Push respecting bound: send().await blocks the producer task
            // (NOT the runtime) until a worker pops. This is the standard
            // backpressure pattern.
            if tx_for_push.send(job).await.is_err() {
                // All receivers dropped — pool was torn down before we
                // finished pushing. Stop silently.
                return;
            }
        }
        // tx_for_push drops here; the caller's QueueHandle wraps the OTHER
        // sender clone, dropped by the caller after `spawn` returns.
    });

    (rx, QueueHandle(tx))
}
