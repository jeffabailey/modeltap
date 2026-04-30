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

use std::path::Path;

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
