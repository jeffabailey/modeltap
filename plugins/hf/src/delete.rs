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
