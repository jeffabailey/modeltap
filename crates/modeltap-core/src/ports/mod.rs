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
// FsProbe (US-10 unify, US-19 cross-fs fallback, ADR-008)
//
// The trait was inlined here in step 03-02; step 03-03 extracted it into the
// `fs_probe` sub-module and added `same_filesystem` / `device_id` / `inode`
// helpers on top of `dev_and_inode` so the cross-fs choice dialog (US-19) can
// probe per-target without copying the device-comparison logic. We re-export
// the trait at the parent path so existing callers (`build_plan`,
// `actions::unify`) keep their import line stable.
// ---------------------------------------------------------------------------

pub mod fs_probe;

pub use fs_probe::{FsProbe, ProbeError, RunningProcess};
