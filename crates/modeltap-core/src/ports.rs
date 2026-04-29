//! Driven ports for the modeltap hexagon.
//!
//! Per ADR-006 (hexagonal architecture), the domain layer defines the
//! interfaces (ports) and the adapters (in `modeltap-app` and the plugin
//! crates) implement them. This module holds the ports used by the
//! application/UI layer that sit OUTSIDE the plugin contract (`Tool`).
//!
//! ## Hasher (US-13, ADR-002)
//!
//! Lazy SHA-256 streaming. The detail screen invokes the hasher when the user
//! opens it. The hash is then cached IN-PROCESS (per ADR-003 — no disk
//! persistence) keyed by `(path, mtime, size)`. The progress callback is
//! invoked periodically so the screen can render "computing dedup key... N%"
//! while a multi-GB hash is in-flight.
//!
//! Test seam: `modeltap-app::sha256_cache` provides the real implementation
//! backed by the `sha2` crate; tests inject a fake `Hasher` that yields canned
//! progress values without actually reading bytes.

use std::path::Path;

use crate::types::ContentHash;

/// Progress event emitted by `Hasher::sha256_streaming`. The detail screen
/// renders the most recent `percent_complete` next to the dedup-key label
/// while the hash is still in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashProgress {
    /// 0..=100. The hasher MUST emit at least 0 at the start and 100 at the
    /// end; intermediate values are advisory.
    pub percent_complete: u8,
    /// Bytes consumed so far. Useful for rate calculations in the UI.
    pub bytes_hashed: u64,
}

/// Driven port for streaming SHA-256 with progress callbacks.
///
/// The port returns the standard `std::io::Error` because that's what the
/// real adapter naturally produces (file open + read errors). The TUI layer
/// surfaces the error as a non-fatal banner — the user can still see the
/// detail screen with the hash placeholder.
pub trait Hasher: Send + Sync {
    /// Stream-hash the file at `path`, invoking `progress` periodically with
    /// percent-complete updates. Returns the final `ContentHash` on success.
    fn sha256_streaming(
        &self,
        path: &Path,
        progress: &mut dyn FnMut(HashProgress),
    ) -> std::io::Result<ContentHash>;
}

// ---------------------------------------------------------------------------
// FsProbe (US-10 unify, ADR-008)
// ---------------------------------------------------------------------------

/// Driven port for filesystem-level inspection used by the unify planner.
///
/// `canonical_selector::select_canonical` needs to know whether two paths
/// are already hardlinked (same inode) and whether they reside on the same
/// filesystem (so a future hardlink would not fail with `EXDEV`). Real I/O
/// lives behind this port so the pure logic in `modeltap-core::logic` can
/// be tested with synthetic probes.
///
/// The real adapter is trivial (one `stat` call per path); a future home is
/// `modeltap-app::fs_probe`. For now the trait alone lives here so the
/// planner can compile and be unit-tested with a fake.
pub trait FsProbe: Send + Sync {
    /// Returns the device id + inode pair for `path`. Two paths share an
    /// inode iff their `(dev, ino)` tuples are equal — that is the canonical
    /// "already hardlinked" check on POSIX.
    ///
    /// `None` if the path does not exist or cannot be statted (the planner
    /// treats this as "no information, proceed conservatively").
    fn dev_and_inode(&self, path: &Path) -> Option<(u64, u64)>;
}
