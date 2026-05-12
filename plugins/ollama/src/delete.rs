//! `Tool::delete_all` implementation for the Ollama plugin.
//!
//! Walks `<root>/manifests/` and `<root>/blobs/`, removing every manifest
//! and the blobs they reference. Implements the transactional pattern
//! required by US-05 / step 01-04:
//!
//!   1. Enumerate the manifest set (read-only) and resolve each to its blob.
//!   2. Delete the manifests FIRST. After this step, the registry is in a
//!      consistent "nothing-to-load" state — no manifest points at a phantom
//!      blob.
//!   3. Delete the orphaned blobs. If any blob delete fails (e.g., permission
//!      denied), the manifest is already gone, so there is no dangling
//!      registry entry. The orphaned blob remains on disk; the per-target
//!      `DeleteOutcome` records `file_deleted = false` for that entry, and
//!      the caller surfaces a `partial` outcome at the action level.
//!
//! This satisfies the injected-fault test: a mid-loop blob-delete failure
//! does NOT leave dangling manifest entries because manifests are deleted
//! before any blob unlink is attempted.
//!
//! Per ADR-002, blobs are deleted only when the manifest that referenced
//! them was actually removed. Two manifests sharing the same blob count
//! that blob ONCE for `bytes_freed` accounting.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use modeltap_core::{DeleteError, DeleteOutcome, ToolId};
use walkdir::WalkDir;

use crate::manifest::parse_manifest;
use crate::TOOL_NAME;

// ---------------------------------------------------------------------------
// `delete_one_at` — single-model delete (US-05b, step 03-06; ADR-009).
//
// Removes ONE manifest by `id_in_tool` and conditionally cleans up its blob:
//   - If the blob is referenced by ANY other surviving manifest, keep the
//     blob (ref-counted); attribute zero bytes_freed (shared blob retained).
//   - Otherwise unlink the blob and attribute its declared size as bytes_freed.
//
// Order: manifest first (registry consistent), then blob (orphan cleanup).
// Same transactional pattern as `delete_all_at`.
// ---------------------------------------------------------------------------

/// Delete ONE manifest (and its now-orphaned blob, if not still referenced
/// by another manifest). `target_id_in_tool` is the canonical Ollama id
/// `<repo>:<tag>` (matches `discovery::manifest_id`).
///
/// Returns:
///   - `Ok(DeleteOutcome { registration_removed: true, .. })` on success.
///   - `Ok(DeleteOutcome { registration_removed: false, .. })` when the
///     manifest cannot be found / removed (caller surfaces "not found").
pub fn delete_one_at(root: &Path, target_id_in_tool: &str) -> Result<DeleteOutcome, DeleteError> {
    let tool: ToolId = TOOL_NAME;
    if !root.exists() {
        return Err(DeleteError::NotFound(target_id_in_tool.to_string()));
    }
    let manifests_dir = root.join("manifests");
    if !manifests_dir.exists() {
        return Err(DeleteError::NotFound(target_id_in_tool.to_string()));
    }

    // Enumerate every manifest so we can look up our target AND know which
    // blobs are still referenced after the target is removed (ref-counting).
    let plans = enumerate_plans(&manifests_dir, &root.join("blobs"));
    let target_idx = plans.iter().position(|p| p.id_in_tool == target_id_in_tool);
    let Some(target_idx) = target_idx else {
        return Err(DeleteError::NotFound(target_id_in_tool.to_string()));
    };
    let target = plans[target_idx].clone();

    // Phase 2 — unlink the target manifest first. Registry is consistent the
    // moment a manifest is gone (no dangling registration -> phantom blob).
    let manifest_removed = match std::fs::remove_file(&target.manifest_path) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.ollama.delete",
                "delete_one: remove manifest {}: {e}",
                target.manifest_path.display()
            );
            false
        }
    };

    if !manifest_removed {
        return Ok(DeleteOutcome {
            tool,
            model_id_in_tool: target.id_in_tool,
            bytes_freed: 0,
            registration_removed: false,
            file_deleted: false,
            failure_reason: None,
        });
    }

    // Phase 3 — ref-count the blob across surviving manifests. ADR-002
    // conservative-when-uncertain: if any surviving manifest references this
    // blob, we keep it. Only when the target was the LAST referrer do we
    // unlink and credit `bytes_freed`.
    let (file_deleted, bytes_freed) = match &target.blob_path {
        Some(blob_path) => {
            let still_referenced = plans
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != target_idx)
                .any(|(_, p)| p.blob_path.as_deref() == Some(blob_path));
            if still_referenced {
                (false, 0)
            } else {
                match std::fs::remove_file(blob_path) {
                    Ok(()) => (true, target.declared_size),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (false, 0),
                    Err(e) => {
                        tracing::warn!(
                            target: "modeltap.ollama.delete",
                            "delete_one: remove blob {}: {e}",
                            blob_path.display()
                        );
                        (false, 0)
                    }
                }
            }
        }
        None => (false, 0),
    };

    Ok(DeleteOutcome {
        tool,
        model_id_in_tool: target.id_in_tool,
        bytes_freed,
        registration_removed: true,
        file_deleted,
        failure_reason: None,
    })
}

/// Synchronous implementation of Ollama's `delete_all`. Caller (the async
/// `Tool::delete_all` impl) wraps this in `tokio::task::spawn_blocking` so
/// the directory walk + unlinks do not block the runtime thread.
///
/// Returns one `DeleteOutcome` per manifest. `bytes_freed` is non-zero only
/// for the manifest that "won" deletion of a blob (shared blobs are
/// attributed to one outcome to keep the total honest).
pub fn delete_all_at(root: &Path) -> Result<Vec<DeleteOutcome>, DeleteError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let manifests_dir = root.join("manifests");
    if !manifests_dir.exists() {
        return Ok(Vec::new());
    }

    // Phase 1 — enumerate. Each `Plan` records the manifest path and the
    // referenced blob path (or None if the manifest is unparseable).
    let plans = enumerate_plans(&manifests_dir, &root.join("blobs"));
    if plans.is_empty() {
        return Ok(Vec::new());
    }

    // Phase 2 — delete manifests first. The registry is consistent the
    // moment a manifest is unlinked.
    let manifest_results = delete_manifests(&plans);

    // Phase 3 — delete orphaned blobs. A blob is "orphaned" once its
    // referencing manifest is gone. Shared blobs (referenced by multiple
    // manifests) are unlinked exactly ONCE; we track which we have already
    // attempted with a HashSet.
    let blob_results = delete_orphan_blobs(&plans, &manifest_results);

    Ok(zip_outcomes(plans, manifest_results, blob_results))
}

#[derive(Debug, Clone)]
struct Plan {
    /// Absolute path to the manifest file.
    manifest_path: PathBuf,
    /// Stable id-in-tool for this manifest (e.g. "llama3:8b-instruct-q4_K_M").
    id_in_tool: String,
    /// Absolute path to the blob this manifest references; `None` if the
    /// manifest could not be parsed.
    blob_path: Option<PathBuf>,
    /// Apparent size of the referenced blob in bytes, taken from the
    /// manifest declaration (for unique accounting).
    declared_size: u64,
}

fn enumerate_plans(manifests_dir: &Path, blobs_dir: &Path) -> Vec<Plan> {
    let mut plans = Vec::new();
    for entry in WalkDir::new(manifests_dir).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    target: "modeltap.ollama.delete",
                    "walkdir error during enumerate: {e}"
                );
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let manifest_path = entry.path().to_path_buf();
        let id_in_tool = manifest_id(&manifest_path, manifests_dir)
            .unwrap_or_else(|| manifest_path.display().to_string());

        let (blob_path, declared_size) = match std::fs::read_to_string(&manifest_path) {
            Ok(raw) => match parse_manifest(&raw) {
                Ok(m) => (
                    Some(blobs_dir.join(format!("sha256-{}", m.blob_sha))),
                    m.size_bytes,
                ),
                Err(e) => {
                    tracing::warn!(
                        target: "modeltap.ollama.delete",
                        "parse manifest {}: {e}",
                        manifest_path.display()
                    );
                    (None, 0)
                }
            },
            Err(e) => {
                tracing::warn!(
                    target: "modeltap.ollama.delete",
                    "read manifest {}: {e}",
                    manifest_path.display()
                );
                (None, 0)
            }
        };

        plans.push(Plan {
            manifest_path,
            id_in_tool,
            blob_path,
            declared_size,
        });
    }
    plans
}

/// Phase 2: unlink every manifest file. Returns one bool per plan in plan
/// order indicating whether the manifest was successfully removed.
fn delete_manifests(plans: &[Plan]) -> Vec<bool> {
    plans
        .iter()
        .map(|plan| match std::fs::remove_file(&plan.manifest_path) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(
                    target: "modeltap.ollama.delete",
                    "remove manifest {}: {e}",
                    plan.manifest_path.display()
                );
                false
            }
        })
        .collect()
}

/// Phase 3: unlink every orphaned blob. Each blob is unlinked at most once
/// even when multiple manifests reference it. Returns a vector of outcomes
/// in plan order; each entry says (file_deleted, bytes_freed).
fn delete_orphan_blobs(plans: &[Plan], manifest_results: &[bool]) -> Vec<(bool, u64)> {
    let mut already_deleted: HashSet<PathBuf> = HashSet::new();
    plans
        .iter()
        .zip(manifest_results.iter())
        .map(|(plan, &manifest_removed)| {
            if !manifest_removed {
                // Manifest still exists → registry is still consistent.
                // We must NOT delete the blob (would leave a dangling
                // manifest pointing at a missing blob).
                return (false, 0);
            }
            let blob_path = match &plan.blob_path {
                Some(p) => p,
                None => return (false, 0),
            };
            if already_deleted.contains(blob_path) {
                // Shared blob — already attempted by an earlier plan. Don't
                // double-count bytes.
                return (false, 0);
            }
            match std::fs::remove_file(blob_path) {
                Ok(()) => {
                    already_deleted.insert(blob_path.clone());
                    (true, plan.declared_size)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // Blob already gone (e.g., previous run partially
                    // succeeded). Treat as no-op — manifest is now gone too.
                    already_deleted.insert(blob_path.clone());
                    (false, 0)
                }
                Err(e) => {
                    tracing::warn!(
                        target: "modeltap.ollama.delete",
                        "remove blob {}: {e}",
                        blob_path.display()
                    );
                    already_deleted.insert(blob_path.clone());
                    (false, 0)
                }
            }
        })
        .collect()
}

fn zip_outcomes(
    plans: Vec<Plan>,
    manifest_results: Vec<bool>,
    blob_results: Vec<(bool, u64)>,
) -> Vec<DeleteOutcome> {
    let tool: ToolId = TOOL_NAME;
    plans
        .into_iter()
        .zip(manifest_results)
        .zip(blob_results)
        .map(
            |((plan, manifest_removed), (file_deleted, bytes_freed))| DeleteOutcome {
                tool,
                model_id_in_tool: plan.id_in_tool,
                bytes_freed,
                registration_removed: manifest_removed,
                file_deleted,
                failure_reason: None,
            },
        )
        .collect()
}

/// Translate a manifest path under `<root>/manifests/<registry>/<repo>/<tag>`
/// into the canonical Ollama model id `<repo>:<tag>`. Mirrors discovery::
/// manifest_id; duplicated locally to keep delete.rs self-contained.
fn manifest_id(manifest: &Path, manifests_root: &Path) -> Option<String> {
    let rel = manifest.strip_prefix(manifests_root).ok()?;
    let segs: Vec<_> = rel
        .iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
    if segs.len() < 3 {
        return None;
    }
    let tag = segs.last().cloned()?;
    let repo_segs = &segs[1..segs.len() - 1];
    if repo_segs.is_empty() {
        return None;
    }
    let repo: Vec<&str> = repo_segs
        .iter()
        .filter(|s| s.as_str() != "library")
        .map(|s| s.as_str())
        .collect();
    let repo_joined = if repo.is_empty() {
        repo_segs
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("/")
    } else {
        repo.join("/")
    };
    Some(format!("{repo_joined}:{tag}"))
}
