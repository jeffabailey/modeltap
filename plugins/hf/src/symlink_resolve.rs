//! Resolve a Hugging Face snapshot symlink to its blob target.
//!
//! HF snapshot files are RELATIVE symlinks like
//! `../../blobs/<sha256>` that resolve into the same `models--<org>--<repo>`
//! directory's `blobs/` subtree. This module exposes a single pure function
//! `resolve_snapshot_target` that returns one of:
//!
//! - `Resolved::Ok { target_path, size_bytes }` — the symlink resolved to a
//!   real file we could `metadata()`.
//! - `Resolved::Broken { reason }` — the symlink target does not exist.
//!
//! We do not silently drop broken symlinks (per US-12 AC-5). The caller
//! emits a `DiscoveredModel` with `ModelStatus::BrokenSymlink` so the TUI
//! renders `[broken: missing blob]`.
//!
//! Loop / depth handling: HF snapshots only chain ONE symlink hop. We use
//! `std::fs::read_link` (not canonicalize) for the first hop so we can
//! distinguish "broken" from "unreadable for other reasons", then call
//! `metadata` on the resolved path to get the size + verify existence.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    Ok {
        /// The absolute path of the blob the snapshot symlink points to.
        target_path: PathBuf,
        /// Size of the blob file in bytes (apparent / on-disk).
        size_bytes: u64,
    },
    Broken {
        reason: String,
    },
}

/// Resolve a snapshot symlink. If `path` is not a symlink, returns
/// `Resolved::Ok` with the file's own metadata (treating the snapshot file
/// as the blob — supports the very rare case where HF copied instead of
/// symlinking, e.g., on Windows or when downloads fall back to copy).
pub fn resolve_snapshot_target(path: &Path) -> Resolved {
    let symlink_meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return Resolved::Broken {
                reason: format!("missing blob: cannot read {} ({e})", path.display()),
            };
        }
    };

    if !symlink_meta.file_type().is_symlink() {
        // Not a symlink — treat as direct file. Size from its own metadata.
        return Resolved::Ok {
            target_path: path.to_path_buf(),
            size_bytes: symlink_meta.len(),
        };
    }

    // Read raw symlink target (one hop). If it's relative, join against
    // the symlink's parent directory.
    let raw_target = match std::fs::read_link(path) {
        Ok(t) => t,
        Err(e) => {
            return Resolved::Broken {
                reason: format!("missing blob: read_link({}): {e}", path.display()),
            };
        }
    };
    let resolved = if raw_target.is_absolute() {
        raw_target
    } else {
        match path.parent() {
            Some(parent) => parent.join(&raw_target),
            None => raw_target,
        }
    };

    // metadata() follows symlinks transitively — and we want it to,
    // because `resolved` may itself be a symlink in pathological cases.
    // But we cap at one hop in the API contract — if `metadata` follows
    // chains it does so in the kernel and is bounded by the OS's symlink
    // depth limit. For HF specifically, chains > 1 don't occur.
    match std::fs::metadata(&resolved) {
        Ok(m) => Resolved::Ok {
            target_path: resolved,
            size_bytes: m.len(),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Resolved::Broken {
            reason: format!("missing blob: {}", resolved.display()),
        },
        Err(e) => Resolved::Broken {
            reason: format!("missing blob: stat({}): {e}", resolved.display()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    #[cfg(unix)]
    #[test]
    fn resolves_valid_symlink_to_blob_with_size() {
        let temp = tempfile::tempdir().unwrap();
        let blob = temp.path().join("blobs/aaaa");
        fs::create_dir_all(blob.parent().unwrap()).unwrap();
        // 1234-byte blob.
        let f = fs::File::create(&blob).unwrap();
        f.set_len(1234).unwrap();
        let snap_dir = temp.path().join("snapshots/rev1");
        fs::create_dir_all(&snap_dir).unwrap();
        let snap = snap_dir.join("model.safetensors");
        symlink("../../blobs/aaaa", &snap).unwrap();

        let res = resolve_snapshot_target(&snap);
        match res {
            Resolved::Ok {
                target_path,
                size_bytes,
            } => {
                assert_eq!(size_bytes, 1234);
                // target_path resolves through ../../ correctly.
                assert!(
                    target_path.ends_with("blobs/aaaa"),
                    "target {:?} should end with blobs/aaaa",
                    target_path
                );
            }
            other => panic!("expected Resolved::Ok, got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[test]
    fn flags_broken_symlink_with_reason() {
        let temp = tempfile::tempdir().unwrap();
        let snap_dir = temp.path().join("snapshots/rev1");
        fs::create_dir_all(&snap_dir).unwrap();
        // No blobs/ dir created — symlink target does not exist.
        let snap = snap_dir.join("model.bin");
        symlink("../../blobs/nope", &snap).unwrap();

        match resolve_snapshot_target(&snap) {
            Resolved::Broken { reason } => {
                assert!(
                    reason.contains("missing blob"),
                    "reason should mention 'missing blob'; got {:?}",
                    reason
                );
            }
            other => panic!("expected Resolved::Broken, got {:?}", other),
        }
    }

    #[test]
    fn missing_path_is_broken() {
        match resolve_snapshot_target(Path::new("/nonexistent/no/snap")) {
            Resolved::Broken { reason } => {
                assert!(reason.contains("missing"));
            }
            other => panic!(
                "expected Resolved::Broken for missing path, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn regular_file_treated_as_direct_blob() {
        let temp = tempfile::tempdir().unwrap();
        let direct = temp.path().join("model.safetensors");
        let f = fs::File::create(&direct).unwrap();
        f.set_len(42).unwrap();
        match resolve_snapshot_target(&direct) {
            Resolved::Ok {
                target_path,
                size_bytes,
            } => {
                assert_eq!(size_bytes, 42);
                assert_eq!(target_path, direct);
            }
            other => panic!("expected Resolved::Ok for direct file, got {:?}", other),
        }
    }
}
