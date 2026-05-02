//! Throttle task — emits `Msg::HashProgressTick` at 250 ms cadence (ADR-013).
//!
//! Isolates progress UI updates from completion events, preventing a redraw
//! storm when N workers simultaneously complete (ADR-013 §"Pros" point 5).
//! The renderer reads the atomic counters off `HashPoolProgress` directly;
//! the tick is only a "wake the event loop" signal.

use std::time::Duration;

use modeltap_tui::msg::Msg;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

const TICK_INTERVAL: Duration = Duration::from_millis(250);

pub(super) async fn throttle_loop(msg_tx: UnboundedSender<Msg>, cancel: CancellationToken) {
    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(TICK_INTERVAL) => {
                // Receiver-dropped robustness: ignore SendError.
                if msg_tx.send(Msg::HashProgressTick).is_err() {
                    // No-op — the renderer is gone (composition root tearing
                    // down). Keep looping until cancel actually fires; if
                    // the receiver is permanently dropped the cancel token
                    // will arrive momentarily.
                    return;
                }
            }
        }
    }
}
