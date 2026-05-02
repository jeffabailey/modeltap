//! `HashPoolHandle` — composition-root-facing pool handle (ADR-013).
//!
//! The composition root holds this handle for the lifetime of the launch
//! and calls [`HashPoolHandle::shutdown`] on `Msg::Quit`. Shutdown cancels
//! the pool's [`CancellationToken`] and awaits the worker `JoinSet` with a
//! 200 ms timeout — the rest of AC-U1.5's 500 ms quit budget belongs to
//! the TUI teardown.

use std::time::Duration;

use tokio::task::JoinSet;
use tokio::time::error::Elapsed;
use tokio_util::sync::CancellationToken;

use super::HashPoolProgress;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(200);

/// Handle returned by [`super::spawn`].
pub struct HashPoolHandle {
    pub progress: HashPoolProgress,
    pub cancel: CancellationToken,
    pub join: JoinSet<()>,
}

impl HashPoolHandle {
    /// Cancel the pool and await all worker + throttle tasks with the
    /// 200 ms ADR-013 budget. Returns `Err(Elapsed)` if the budget is
    /// exceeded — the composition root surfaces this as a debug log line
    /// (no user-visible error per ADR-013 §"Negative consequences").
    pub async fn shutdown(mut self) -> Result<(), Elapsed> {
        self.cancel.cancel();
        tokio::time::timeout(SHUTDOWN_TIMEOUT, async {
            // Drain the JoinSet — ignore individual task errors; the worker
            // surface them via Msg::HashFailed already (panics translate to
            // HashFailureReason::Other).
            while let Some(_res) = self.join.join_next().await {
                // Discard the result; per ADR-013 panics in workers are
                // isolated and the affected row stays Pending.
            }
        })
        .await
    }
}
