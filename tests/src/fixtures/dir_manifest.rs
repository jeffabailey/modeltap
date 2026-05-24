//! Recursive directory snapshot used by the cache-opt-out acceptance suite.
//!
//! tool-model-info-sqlite-cache step 04-02 — AC-23-8 demands that a launch
//! with `--no-cache` (or `cache.enabled = false`) writes ZERO bytes to the
//! cache directory. "Zero bytes" is an awkward property to assert directly
//! against `cache.sqlite` because rusqlite may have created the file at all
//! does NOT necessarily mean it's been mutated. A byte-precise (path, size,
//! mtime) snapshot taken before and after the launch is the cleanest way to
//! prove the directory tree is exactly the same on both sides.
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/step-definitions-skeleton.md`
//! §F, this fixture is the canonical DirManifest helper for the cache-opt-out
//! scenarios.
//!
//! API:
//!   * `DirManifest::snapshot(root)` — walk `root` recursively, capturing
//!     each entry's relative path, byte size, and modification time. An
//!     empty manifest is returned when `root` does not exist (this is the
//!     correct "before" state for a fresh fixture where the cache directory
//!     has yet to be created).
//!   * `DirManifest::assert_equal(other)` — deep compare. Panics with a
//!     human-readable diff when the manifests differ.

#![allow(dead_code)] // Helper is exposed to the acceptance suite; cargo's
                     // dead-code lint can't see across the crate boundary
                     // when only one binary in this crate consumes it yet.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use walkdir::WalkDir;

/// Recursive snapshot of a directory tree's content state.
///
/// Each entry is `(relative_path, (size_bytes, mtime))`. `BTreeMap` is used
/// (not `HashMap`) so the assertion's diff output is deterministically
/// ordered — flaky test-output ordering would make CI failures hard to
/// triage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirManifest {
    pub entries: BTreeMap<PathBuf, (u64, SystemTime)>,
}

impl DirManifest {
    /// Snapshot `root` recursively. Returns an empty manifest if `root`
    /// does not exist. Both regular files and empty directories are
    /// captured (an empty directory's `size_bytes` is reported as 0).
    pub fn snapshot(root: &Path) -> Self {
        let mut entries: BTreeMap<PathBuf, (u64, SystemTime)> = BTreeMap::new();
        if !root.exists() {
            return Self { entries };
        }
        for entry in WalkDir::new(root).follow_links(false).into_iter().flatten() {
            // Skip the root itself; we only care about its descendants
            // (otherwise the snapshot's set always contains `root` which
            // is uninteresting to the equality check).
            if entry.path() == root {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let rel = match entry.path().strip_prefix(root) {
                Ok(r) => r.to_path_buf(),
                Err(_) => continue,
            };
            let size = if metadata.is_file() {
                metadata.len()
            } else {
                0
            };
            let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            entries.insert(rel, (size, mtime));
        }
        Self { entries }
    }

    /// Deep compare `self` to `other`. Panics with a diff describing the
    /// first three mismatched entries when they differ. The diff lists
    /// added / removed / mutated keys so the failure message points at
    /// the exact file that changed.
    pub fn assert_equal(&self, other: &Self) {
        if self == other {
            return;
        }
        let mut diff_lines: Vec<String> = Vec::new();
        // Added in `other`.
        for (path, after) in &other.entries {
            if !self.entries.contains_key(path) {
                diff_lines.push(format!("+ {}: size={}", path.display(), after.0));
            }
        }
        // Removed from `other`.
        for (path, before) in &self.entries {
            if !other.entries.contains_key(path) {
                diff_lines.push(format!("- {}: size={}", path.display(), before.0));
            }
        }
        // Mutated (same key, different value).
        for (path, before) in &self.entries {
            if let Some(after) = other.entries.get(path) {
                if before != after {
                    diff_lines.push(format!(
                        "~ {}: before size={}, after size={}",
                        path.display(),
                        before.0,
                        after.0
                    ));
                }
            }
        }
        let preview: Vec<String> = diff_lines.iter().take(10).cloned().collect();
        panic!(
            "DirManifest mismatch — {} diff entries (first {} shown):\n{}",
            diff_lines.len(),
            preview.len(),
            preview.join("\n")
        );
    }

    /// Convenience: `true` when the manifest contains zero entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn snapshot_of_nonexistent_path_is_empty() {
        let m = DirManifest::snapshot(Path::new("/nonexistent/never-existed"));
        assert!(m.is_empty());
    }

    #[test]
    fn snapshot_captures_files_and_subdirs() {
        let td = TempDir::new().expect("tempdir");
        fs::write(td.path().join("a.txt"), b"hello").expect("write a");
        fs::create_dir(td.path().join("sub")).expect("mkdir sub");
        fs::write(td.path().join("sub").join("b.txt"), b"hi there").expect("write b");

        let m = DirManifest::snapshot(td.path());
        // a.txt, sub/, sub/b.txt = 3 entries (root itself excluded).
        assert_eq!(m.entries.len(), 3, "{:?}", m.entries.keys().collect::<Vec<_>>());
        assert_eq!(m.entries.get(Path::new("a.txt")).map(|(s, _)| *s), Some(5));
        assert_eq!(
            m.entries
                .get(Path::new("sub").join("b.txt").as_path())
                .map(|(s, _)| *s),
            Some(8)
        );
    }

    #[test]
    fn assert_equal_passes_on_identical_trees() {
        let td = TempDir::new().expect("tempdir");
        fs::write(td.path().join("a.txt"), b"hello").expect("write a");
        let before = DirManifest::snapshot(td.path());
        let after = DirManifest::snapshot(td.path());
        before.assert_equal(&after);
    }

    #[test]
    #[should_panic(expected = "DirManifest mismatch")]
    fn assert_equal_panics_on_added_file() {
        let td = TempDir::new().expect("tempdir");
        let before = DirManifest::snapshot(td.path());
        fs::write(td.path().join("new.txt"), b"hi").expect("write new");
        let after = DirManifest::snapshot(td.path());
        before.assert_equal(&after);
    }

    #[test]
    #[should_panic(expected = "DirManifest mismatch")]
    fn assert_equal_panics_on_mutated_file() {
        let td = TempDir::new().expect("tempdir");
        fs::write(td.path().join("a.txt"), b"hello").expect("write");
        let before = DirManifest::snapshot(td.path());
        // Sleep a microsecond + rewrite with larger content to ensure
        // size differs (mtime alone may not change at sub-second resolution
        // on some filesystems).
        fs::write(td.path().join("a.txt"), b"hello world").expect("rewrite");
        let after = DirManifest::snapshot(td.path());
        before.assert_equal(&after);
    }
}
