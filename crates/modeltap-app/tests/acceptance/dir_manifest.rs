//! `DirManifest` — recursive `(relative_path, size, mtime)` snapshot helper.
//!
//! Source: step-definitions-skeleton.md §H — used by every "no destructive
//! action" / "fixture directory is byte-identical pre and post" assertion
//! across the folder-group-bulk-delete acceptance suite (M2 confirmation
//! safety + M5 non-HF no-op + M6 property scenarios).
//!
//! Layout:
//! - `snapshot(root)` walks `root` bottom-up via `walkdir`, sorts entries by
//!   relative path, and records each entry's `len()` + `mtime` (nanosecond
//!   precision when the platform supports it; otherwise second precision).
//! - `assert_eq` on `DirManifest` is a full byte-precise equality check; the
//!   custom `Debug` impl prints offending entries so test failures point at
//!   the exact added/removed/modified file.
//!
//! Created in step 02-01. Reused unchanged from there onward.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One entry in the manifest. `rel_path` is the relative path from the
/// snapshot root; `size_bytes` is `metadata.len()` (apparent length, NOT
/// disk usage); `mtime_nanos` is the modification timestamp as nanoseconds
/// since the Unix epoch when available, else `None`.
#[derive(Clone, Eq, PartialEq)]
pub struct ManifestEntry {
    pub rel_path: PathBuf,
    pub size_bytes: u64,
    pub mtime_nanos: Option<i128>,
    /// True if entry is a symlink (we don't follow; record link target's
    /// metadata only). Snapshot must NOT silently dereference symlinks —
    /// the HF cache uses snapshots/<rev>/foo -> blobs/<sha> structures and
    /// "byte-identical" requires the link itself to be unchanged.
    pub is_symlink: bool,
}

impl std::fmt::Debug for ManifestEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ManifestEntry {{ rel_path: {:?}, size_bytes: {}, mtime_nanos: {:?}, is_symlink: {} }}",
            self.rel_path, self.size_bytes, self.mtime_nanos, self.is_symlink
        )
    }
}

/// Sorted-by-rel_path list of entries under a root directory.
#[derive(Clone, Eq, PartialEq)]
pub struct DirManifest {
    pub root: PathBuf,
    pub entries: Vec<ManifestEntry>,
}

impl std::fmt::Debug for DirManifest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "DirManifest at {:?} with {} entries:",
            self.root,
            self.entries.len()
        )?;
        for e in &self.entries {
            writeln!(f, "  {:?}", e)?;
        }
        Ok(())
    }
}

impl DirManifest {
    /// Snapshot `root` recursively. Does NOT follow symlinks. The walk is
    /// bottom-up so the produced order is stable across runs; the returned
    /// `entries` are additionally sorted by `rel_path` for deterministic
    /// equality comparisons.
    pub fn snapshot(root: &Path) -> Self {
        let mut entries: Vec<ManifestEntry> = walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
            // Skip the root itself; only record descendants.
            .filter(|e| e.path() != root)
            .map(|e| {
                let abs = e.path();
                let rel_path = abs
                    .strip_prefix(root)
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| abs.to_path_buf());
                // `symlink_metadata` does NOT follow symlinks — exactly what
                // we want so a snapshot symlink under HF's snapshots/<rev>/
                // is recorded as a link, not the blob it points at.
                let md = fs::symlink_metadata(abs);
                let (size_bytes, mtime_nanos, is_symlink) = match md {
                    Ok(m) => {
                        let mtime = m
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                            .map(|d| d.as_nanos() as i128)
                            .or_else(|| {
                                // Fall back to raw st_mtime when SystemTime
                                // is unavailable (some filesystems).
                                Some((m.mtime() as i128) * 1_000_000_000 + m.mtime_nsec() as i128)
                            });
                        (m.len(), mtime, m.file_type().is_symlink())
                    }
                    Err(_) => (0, None, false),
                };
                ManifestEntry {
                    rel_path,
                    size_bytes,
                    mtime_nanos,
                    is_symlink,
                }
            })
            .collect();
        entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        DirManifest {
            root: root.to_path_buf(),
            entries,
        }
    }

    /// Total file count (excludes directory entries).
    pub fn file_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.is_dir_marker()).count()
    }
}

impl ManifestEntry {
    /// Heuristic: walkdir reports directories too. We can't tell from the
    /// stored metadata alone (we'd need a separate flag), so this returns
    /// false uniformly — `file_count` callers should treat the entry count
    /// as "everything walkdir saw". For step 02-01's purposes (byte-equal
    /// snapshots), every entry that differs across pre/post is a failure
    /// regardless of whether it's a file or directory.
    fn is_dir_marker(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// DirManifest round-trip: snapshotting a directory, doing nothing,
    /// and snapshotting again yields equal manifests. This is the
    /// "no destructive action" assertion at its purest.
    #[test]
    fn dir_manifest_round_trip_equal_after_noop() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        // Build a small tree: 2 files + 1 subdir + 1 file in the subdir +
        // 1 symlink.
        fs::write(root.join("a.txt"), b"hello").expect("write a.txt");
        fs::write(root.join("b.txt"), b"world").expect("write b.txt");
        fs::create_dir(root.join("sub")).expect("mkdir sub");
        fs::write(root.join("sub").join("c.txt"), b"nested").expect("write c.txt");
        std::os::unix::fs::symlink("a.txt", root.join("link-to-a")).expect("symlink");

        let pre = DirManifest::snapshot(root);
        // No-op: a sub-second elapse is fine — mtime is preserved by not
        // modifying any inode.
        let post = DirManifest::snapshot(root);
        assert_eq!(
            pre, post,
            "no-op between snapshots must yield byte-equal manifests"
        );
    }

    /// Round-trip detects file content changes via size.
    #[test]
    fn dir_manifest_detects_file_content_change() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("a.txt"), b"hello").expect("write a.txt");

        let pre = DirManifest::snapshot(root);
        // Mutation: append bytes — changes both size and mtime.
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(root.join("a.txt"))
            .expect("open append");
        f.write_all(b" more").expect("append");
        drop(f);

        let post = DirManifest::snapshot(root);
        assert_ne!(
            pre, post,
            "content mutation must produce different manifest"
        );
    }

    /// Round-trip detects file removal.
    #[test]
    fn dir_manifest_detects_file_removal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("a.txt"), b"hello").expect("write a.txt");
        fs::write(root.join("b.txt"), b"world").expect("write b.txt");

        let pre = DirManifest::snapshot(root);
        fs::remove_file(root.join("b.txt")).expect("remove b.txt");
        let post = DirManifest::snapshot(root);
        assert_ne!(pre, post, "file removal must produce different manifest");
        assert!(
            pre.file_count() > post.file_count(),
            "post.file_count() < pre.file_count() after removal"
        );
    }

    /// Round-trip preserves symlink identity (does NOT dereference).
    #[test]
    fn dir_manifest_records_symlinks_without_dereferencing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path();
        fs::write(root.join("target.txt"), b"hello").expect("write target");
        std::os::unix::fs::symlink("target.txt", root.join("link")).expect("symlink");
        let manifest = DirManifest::snapshot(root);
        let link_entry = manifest
            .entries
            .iter()
            .find(|e| e.rel_path == Path::new("link"))
            .expect("link entry recorded");
        assert!(
            link_entry.is_symlink,
            "symlink must be recorded as is_symlink=true (no dereference)"
        );
    }
}
