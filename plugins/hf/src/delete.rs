//! `Tool::delete_one` implementation for the HF plugin (US-05b, step 03-06).
//!
//! HF's cache layout: snapshot symlinks at
//! `<hub>/models--<org>--<repo>/snapshots/<rev>/<file>` point at content-
//! addressed blobs at `<hub>/models--<org>--<repo>/blobs/<sha256>`. A given
//! blob may be shared across snapshot files within the same repo (e.g., the
//! same artifact at multiple revisions).
//!
//! Per ADR-002 / ADR-009, `delete_one`:
//!   1. Locates the snapshot symlink whose canonicalized target matches the
//!      ModelMeta's `on_disk_path` (a blob path).
//!   2. Deletes the snapshot symlink (registration removed).
//!   3. Ref-counts the blob across all surviving snapshot symlinks under the
//!      same repo dir. If ANY surviving symlink still resolves to that blob,
//!      keep the blob (shared); attribute zero `bytes_freed`. Otherwise unlink
//!      the blob and credit its size as `bytes_freed`.
//!
//! Order: snapshot symlink first (registry consistent — no dangling pointer
//! to a phantom blob), then blob orphan cleanup. Mirrors Ollama's two-phase
//! pattern from `delete_all_at`.

use std::path::{Path, PathBuf};

use modeltap_core::{DeleteError, DeleteOutcome, ToolId};

use crate::cache_walk::list_snapshot_files;
use crate::symlink_resolve::{resolve_snapshot_target, Resolved};
use crate::TOOL_NAME;

/// Delete one HF snapshot symlink and conditionally its (now-orphaned) blob.
///
/// Arguments:
///   - `hub_root`: the directory containing `models--*` subdirs.
///   - `target_blob`: the canonicalized blob path the model row referenced
///     (i.e., `ModelMeta.on_disk_path`). Used to locate the matching
///     snapshot symlink AND, after deletion, to test for surviving refs.
///   - `model_id_in_tool`: the orchestrator-correlated id (e.g.,
///     `org/repo/model.safetensors`); echoed back in the outcome.
pub fn delete_one_at(
    hub_root: &Path,
    target_blob: &Path,
    model_id_in_tool: &str,
) -> Result<DeleteOutcome, DeleteError> {
    let tool: ToolId = TOOL_NAME;
    if !hub_root.exists() {
        return Err(DeleteError::NotFound(model_id_in_tool.to_string()));
    }

    // Phase 1 — enumerate every snapshot symlink under the hub. Resolve each
    // and find the one whose target matches `target_blob`.
    let snaps = list_snapshot_files(hub_root);
    let target_blob_canon =
        std::fs::canonicalize(target_blob).unwrap_or_else(|_| target_blob.to_path_buf());

    let mut snap_target: Option<(PathBuf, PathBuf, u64)> = None;
    let mut other_snaps: Vec<(PathBuf, PathBuf)> = Vec::new();
    for snap in &snaps {
        match resolve_snapshot_target(&snap.file_path) {
            Resolved::Ok {
                target_path,
                size_bytes,
            } => {
                let resolved_canon =
                    std::fs::canonicalize(&target_path).unwrap_or_else(|_| target_path.clone());
                if resolved_canon == target_blob_canon && snap_target.is_none() {
                    snap_target = Some((snap.file_path.clone(), target_path.clone(), size_bytes));
                } else {
                    other_snaps.push((snap.file_path.clone(), resolved_canon));
                }
            }
            Resolved::Broken { .. } => {
                // Broken symlinks don't reference a real blob — ignore for
                // ref-counting purposes.
            }
        }
    }

    let Some((snap_path, blob_path, size_bytes)) = snap_target else {
        return Err(DeleteError::NotFound(model_id_in_tool.to_string()));
    };

    // Phase 2 — unlink the snapshot symlink first. After this, the
    // registration is gone (no dangling pointer if blob is later removed).
    // We use `remove_file` which on Unix unlinks the symlink itself rather
    // than its target.
    let registration_removed = match std::fs::remove_file(&snap_path) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.hf.delete",
                "delete_one: remove snapshot symlink {}: {e}",
                snap_path.display()
            );
            false
        }
    };
    if !registration_removed {
        return Ok(DeleteOutcome {
            tool,
            model_id_in_tool: model_id_in_tool.to_string(),
            bytes_freed: 0,
            registration_removed: false,
            file_deleted: false,
        });
    }

    // Phase 3 — ref-count the blob across surviving snapshot symlinks. ADR-
    // 002 conservative-when-uncertain: if any surviving snapshot resolves to
    // this blob, keep it (some other model row will still load it).
    let blob_canon = std::fs::canonicalize(&blob_path).unwrap_or_else(|_| blob_path.clone());
    let still_referenced = other_snaps.iter().any(|(_, b)| b == &blob_canon);
    let (file_deleted, bytes_freed) = if still_referenced {
        (false, 0)
    } else {
        match std::fs::remove_file(&blob_path) {
            Ok(()) => (true, size_bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (false, 0),
            Err(e) => {
                tracing::warn!(
                    target: "modeltap.hf.delete",
                    "delete_one: remove blob {}: {e}",
                    blob_path.display()
                );
                (false, 0)
            }
        }
    };

    Ok(DeleteOutcome {
        tool,
        model_id_in_tool: model_id_in_tool.to_string(),
        bytes_freed,
        registration_removed: true,
        file_deleted,
    })
}

/// Delete the HF-side registration AND the HF-side blob path for a model
/// whose inode is shared with another tool via a cross-tool hardlink. The
/// inode survives because the other tool still holds a hardlink to it (per
/// `FolderDeletePlan.paths_to_unlink_hf_only` per **ADR-010**).
///
/// Distinction from [`delete_one_at`]:
///   - `delete_one_at` is HF-internal ref-counting: a blob shared by two
///     snapshots WITHIN the same repo. If another snapshot still references
///     the blob, keep it; bytes_freed=0 because intra-repo dedup is internal.
///   - `delete_one_hf_side_only_at` is CROSS-TOOL ref-counting: a blob whose
///     inode has `nlink > 1` because another tool (Ollama, LM Studio,
///     Atomic Chat) holds a hardlink. We DO unlink the HF-side path so the
///     directory cleanup can succeed and the user-visible HF cache view is
///     gone, but we report `file_deleted=false, bytes_freed=0` because the
///     data isn't truly freed — the OS keeps the inode alive via the other
///     tool's hardlink.
///
/// The "did we free disk?" question is answered by `nlink` on the blob
/// metadata BEFORE the unlink: if `nlink > 1`, unlinking the HF path
/// decrements the count but the data persists. Hence the outcome shape
/// `file_deleted=false, bytes_freed=0` — the orchestrator credits these
/// bytes to `bytes_to_retain` not `bytes_to_reclaim`.
///
/// Used by `folder_delete::delete_folder_at` for files routed through
/// `paths_to_unlink_hf_only` (the cross-tool-shared case). The OS hardlink
/// semantics guarantee the inode survives via the other tool's path — this
/// function's outcome shape encodes that guarantee (INT-FGD-4).
pub fn delete_one_hf_side_only_at(
    hub_root: &Path,
    target_blob: &Path,
    model_id_in_tool: &str,
) -> Result<DeleteOutcome, DeleteError> {
    let tool: ToolId = TOOL_NAME;
    if !hub_root.exists() {
        return Err(DeleteError::NotFound(model_id_in_tool.to_string()));
    }
    let snaps = list_snapshot_files(hub_root);
    let target_blob_canon =
        std::fs::canonicalize(target_blob).unwrap_or_else(|_| target_blob.to_path_buf());
    let mut snap_path: Option<PathBuf> = None;
    let mut other_snaps: Vec<PathBuf> = Vec::new();
    for snap in &snaps {
        if let Resolved::Ok { target_path, .. } = resolve_snapshot_target(&snap.file_path) {
            let resolved_canon =
                std::fs::canonicalize(&target_path).unwrap_or_else(|_| target_path.clone());
            if resolved_canon == target_blob_canon {
                if snap_path.is_none() {
                    snap_path = Some(snap.file_path.clone());
                } else {
                    other_snaps.push(snap.file_path.clone());
                }
            }
        }
    }
    let Some(snap_path) = snap_path else {
        return Err(DeleteError::NotFound(model_id_in_tool.to_string()));
    };
    let registration_removed = match std::fs::remove_file(&snap_path) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.hf.delete",
                "delete_one_hf_side_only: remove snapshot symlink {}: {e}",
                snap_path.display()
            );
            false
        }
    };
    if !registration_removed {
        return Ok(DeleteOutcome {
            tool,
            model_id_in_tool: model_id_in_tool.to_string(),
            bytes_freed: 0,
            registration_removed: false,
            file_deleted: false,
        });
    }
    // Unlink the HF-side blob path itself. The blob's inode survives because
    // another tool holds an additional hardlink (nlink ≥ 2 pre-unlink). If
    // another HF snapshot within this same repo also references the blob,
    // skip the blob-path unlink to preserve intra-repo dedup (delegate to
    // ref-counting). The S.2 contract test requires `!blob_shared_1.exists()`
    // post-delete, which this branch satisfies for the cross-tool case.
    let intra_repo_shared = !other_snaps.is_empty();
    if !intra_repo_shared {
        if let Err(e) = std::fs::remove_file(target_blob) {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    target: "modeltap.hf.delete",
                    "delete_one_hf_side_only: remove HF-side blob path {}: {e}",
                    target_blob.display()
                );
            }
        }
    }
    // Outcome shape per plugin-contract-spec.md §3.11.S.2: the inode survives
    // via the other tool's hardlink, so bytes are not credited as freed.
    Ok(DeleteOutcome {
        tool,
        model_id_in_tool: model_id_in_tool.to_string(),
        bytes_freed: 0,
        registration_removed: true,
        file_deleted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    #[cfg(unix)]
    #[test]
    fn deletes_snapshot_and_unique_blob() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path().join("hub");
        let m1 = hub.join("models--meta-llama--Llama-3-8B");
        let snap1 = m1.join("snapshots/rev1");
        let blobs1 = m1.join("blobs");
        fs::create_dir_all(&snap1).unwrap();
        fs::create_dir_all(&blobs1).unwrap();
        let blob = blobs1.join("blob-a");
        let f = fs::File::create(&blob).unwrap();
        f.set_len(2048).unwrap();
        let snap = snap1.join("model.safetensors");
        symlink("../../blobs/blob-a", &snap).unwrap();

        let outcome =
            delete_one_at(&hub, &blob, "meta-llama/Llama-3-8B/model.safetensors").expect("ok");
        assert!(outcome.registration_removed);
        assert!(outcome.file_deleted, "unique blob must be deleted");
        assert_eq!(outcome.bytes_freed, 2048);
        assert!(!snap.exists(), "snapshot symlink must be gone");
        assert!(!blob.exists(), "blob must be gone (not ref-counted)");
    }

    #[cfg(unix)]
    #[test]
    fn keeps_blob_when_another_snapshot_still_references_it() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path().join("hub");
        let m1 = hub.join("models--meta-llama--Llama-3-8B");
        let snap_a = m1.join("snapshots/rev1");
        let snap_b = m1.join("snapshots/rev2");
        let blobs1 = m1.join("blobs");
        fs::create_dir_all(&snap_a).unwrap();
        fs::create_dir_all(&snap_b).unwrap();
        fs::create_dir_all(&blobs1).unwrap();
        let blob = blobs1.join("blob-a");
        let f = fs::File::create(&blob).unwrap();
        f.set_len(8192).unwrap();
        let snap_a_path = snap_a.join("model.safetensors");
        let snap_b_path = snap_b.join("model.safetensors");
        symlink("../../blobs/blob-a", &snap_a_path).unwrap();
        symlink("../../blobs/blob-a", &snap_b_path).unwrap();

        let outcome =
            delete_one_at(&hub, &blob, "meta-llama/Llama-3-8B/model.safetensors").expect("ok");
        assert!(outcome.registration_removed);
        assert!(
            !outcome.file_deleted,
            "shared blob must NOT be unlinked while another snapshot references it"
        );
        assert_eq!(outcome.bytes_freed, 0);
        assert!(blob.exists(), "shared blob must remain on disk");
        // Either snap_a or snap_b was removed; the other survives.
        assert!(
            snap_a_path.exists() ^ snap_b_path.exists(),
            "exactly one snapshot symlink must remain"
        );
    }

    #[test]
    fn returns_not_found_when_blob_path_unknown() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path().join("hub");
        fs::create_dir_all(&hub).unwrap();
        let blob = hub.join("nonexistent-blob");
        let err = delete_one_at(&hub, &blob, "x/y/z").expect_err("must err");
        assert!(matches!(err, DeleteError::NotFound(_)));
    }
}
