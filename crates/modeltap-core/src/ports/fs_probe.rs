//! `FsProbe` — driven port for filesystem-level inspection (US-10, US-19, ADR-008).
//!
//! Two consumers exist as of step 03-03:
//!
//! 1. The unify planner (`logic::plan::build_plan`) — needs `(device, inode)`
//!    pairs to decide which targets are already hardlinked into the canonical
//!    and which would cross filesystems (and thus need the per-target
//!    [s] skip / [c] copy / [x] cancel choice per ADR-008).
//! 2. The cross-fs choice dialog (US-19, step 03-03) — flags any cross-fs
//!    target before linking so the user can choose the fallback.
//!
//! `dev_and_inode` is the foundational primitive (one `stat()` call); the
//! `same_filesystem`, `device_id`, and `inode` helpers are pure derivations
//! provided as default methods so test fakes only need to implement
//! `dev_and_inode`. Real production uses `std::fs::metadata().dev()` /
//! `.ino()` via `MetadataExt`; tests inject synthetic device IDs through a
//! `FakeFsProbe` to exercise cross-fs paths without mounting a filesystem.
//!
//! ADR-008 cites: refuse-default fallback. The probe is the seam that drives
//! the whole user-choice flow.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// One running tool process holding an in-scope file open. Surfaced by
/// `FsProbe::detect_running_tools` to the running-tool dialog (US-17,
/// intake Q5). The adapter parses `lsof` output into one of these per match.
///
/// The dialog renders `tool_name` (the `COMMAND` column from lsof) so the
/// user knows which app to close. `pid` is shown in parentheses for
/// disambiguation when multiple instances of the same tool are running. The
/// `path` is the resolved path that triggered the match — used by tests to
/// assert which file was identified, but NOT shown in the user-facing dialog
/// (per the kpi-instrumentation §"Privacy" rule, paths can leak username).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    /// Process command name as reported by lsof's `COMMAND` column.
    pub tool_name: String,
    /// Process id as reported by lsof's `PID` column.
    pub pid: u32,
    /// Resolved path the process holds open (the lsof `NAME` column).
    pub path: PathBuf,
}

/// Errors produced by `FsProbe::detect_running_tools`. Per ADR-007 this is a
/// typed enum (`thiserror`) so the running-tool gate can pattern-match
/// `LsofUnavailable` to surface the explicit "detection unavailable on this
/// system" dialog (US-17 AC-3) without string matching.
#[derive(Debug, Error)]
pub enum ProbeError {
    /// `lsof` binary not available on this system (stripped container,
    /// native Windows, etc.). The running-tool gate surfaces an explicit
    /// dialog and lets the user proceed at their own risk per US-17 AC-3.
    #[error("lsof unavailable: {reason}")]
    LsofUnavailable { reason: String },

    /// `lsof` was invoked successfully but its output could not be parsed.
    /// Surfaced as a non-fatal warning; treated as "no running tools" so
    /// the user is not blocked by an unrelated parser bug.
    #[error("lsof output parse error: {reason}")]
    ParseError { reason: String },

    /// Underlying I/O error from spawning the subprocess (other than
    /// "binary not found", which maps to `LsofUnavailable`).
    #[error("lsof I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Driven port for filesystem-level inspection used by the unify planner and
/// the US-19 cross-fs choice dialog.
///
/// `canonical_selector::select_canonical` and `logic::plan::build_plan` need
/// to know whether two paths are already hardlinked (same inode) and whether
/// they reside on the same filesystem (so a future `hard_link` would not
/// fail with `EXDEV`). Real I/O lives behind this port so the pure logic in
/// `modeltap-core::logic` can be tested with synthetic probes.
pub trait FsProbe: Send + Sync {
    /// Returns the device id + inode pair for `path`. Two paths share an
    /// inode iff their `(dev, ino)` tuples are equal — that is the canonical
    /// "already hardlinked" check on POSIX.
    ///
    /// `None` if the path does not exist or cannot be statted (the planner
    /// treats this as "no information, proceed conservatively").
    fn dev_and_inode(&self, path: &Path) -> Option<(u64, u64)>;

    /// True iff `a` and `b` reside on the same filesystem (same `dev_t`).
    /// Returns `false` when either path cannot be statted — the conservative
    /// "treat unknown as cross-fs" stance that aligns with ADR-008's
    /// refuse-default policy. The orchestrator surfaces the per-target choice
    /// dialog rather than silently linking-or-skipping.
    fn same_filesystem(&self, a: &Path, b: &Path) -> bool {
        match (self.dev_and_inode(a), self.dev_and_inode(b)) {
            (Some((dev_a, _)), Some((dev_b, _))) => dev_a == dev_b,
            _ => false,
        }
    }

    /// The device id (`dev_t`) of `path`, or `None` if the path cannot be
    /// statted. Useful for callers that need just the device part without the
    /// inode (e.g., per-target cross-fs flagging in `build_plan`).
    fn device_id(&self, path: &Path) -> Option<u64> {
        self.dev_and_inode(path).map(|(d, _)| d)
    }

    /// The inode (`ino_t`) of `path`, or `None` if the path cannot be statted.
    /// Useful for callers that need just the inode part without the device
    /// (e.g., post-action invariant checks on inode equality).
    fn inode(&self, path: &Path) -> Option<u64> {
        self.dev_and_inode(path).map(|(_, i)| i)
    }

    /// Detect any running tool processes that hold one or more of
    /// `target_paths` open. Used by the unify and delete-one orchestrators
    /// (US-17, intake Q5) to refuse mutating actions while a registered
    /// tool's process has a file in scope. The action is gated:
    /// when this returns `Ok(non_empty)`, the orchestrator MUST raise the
    /// running-tool dialog and refuse the action until the user closes the
    /// tool and presses [r] retry.
    ///
    /// Real adapter (`modeltap_app::lsof_adapter::LsofAdapter`) shells out to
    /// `lsof` on macOS/Linux (gated by `cfg!(unix)`). On Windows native, the
    /// adapter returns `Err(ProbeError::LsofUnavailable)` so the user sees
    /// the explicit message and can proceed at own risk. A fake-output env
    /// var (`MODELTAP_FAKE_LSOF_OUTPUT`) lets tests inject synthetic lsof
    /// output without spawning a real subprocess.
    ///
    /// Default impl returns empty (no running tools detected) so existing
    /// fakes that only need `dev_and_inode` continue to compile. The real
    /// production path overrides this; tests that exercise the running-tool
    /// gate use a dedicated `FakeRunningToolProbe`.
    fn detect_running_tools(
        &self,
        _target_paths: &[PathBuf],
    ) -> Result<Vec<RunningProcess>, ProbeError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// In-memory fake. Tests register synthetic `(dev, inode)` pairs per path.
    /// Paths not registered return `None` from `dev_and_inode`.
    #[derive(Default)]
    struct FakeFsProbe {
        entries: HashMap<PathBuf, (u64, u64)>,
    }

    impl FakeFsProbe {
        fn with(mut self, path: &str, dev: u64, ino: u64) -> Self {
            self.entries.insert(PathBuf::from(path), (dev, ino));
            self
        }
    }

    impl FsProbe for FakeFsProbe {
        fn dev_and_inode(&self, path: &Path) -> Option<(u64, u64)> {
            self.entries.get(path).copied()
        }
    }

    #[test]
    fn same_filesystem_true_when_devices_match() {
        let probe = FakeFsProbe::default().with("/a", 1, 100).with("/b", 1, 200);
        assert!(probe.same_filesystem(Path::new("/a"), Path::new("/b")));
    }

    #[test]
    fn same_filesystem_false_when_devices_differ() {
        let probe = FakeFsProbe::default().with("/a", 1, 100).with("/b", 2, 200);
        assert!(!probe.same_filesystem(Path::new("/a"), Path::new("/b")));
    }

    #[test]
    fn same_filesystem_false_when_either_path_missing() {
        // ADR-008 conservative-when-uncertain: unstattable paths are treated
        // as cross-fs so the user-choice dialog is shown rather than the
        // probe silently making a decision.
        let probe = FakeFsProbe::default().with("/a", 1, 100);
        assert!(!probe.same_filesystem(Path::new("/a"), Path::new("/missing")));
        assert!(!probe.same_filesystem(Path::new("/missing"), Path::new("/a")));
    }

    #[test]
    fn device_id_returns_dev_when_present() {
        let probe = FakeFsProbe::default().with("/a", 7, 100);
        assert_eq!(probe.device_id(Path::new("/a")), Some(7));
        assert_eq!(probe.device_id(Path::new("/missing")), None);
    }

    #[test]
    fn inode_returns_ino_when_present() {
        let probe = FakeFsProbe::default().with("/a", 7, 42);
        assert_eq!(probe.inode(Path::new("/a")), Some(42));
        assert_eq!(probe.inode(Path::new("/missing")), None);
    }
}
