//! `Tool::delete_folder` implementation for the HF plugin
//! (folder-group-bulk-delete, step 01-03 — all-unique happy path).
//!
//! Per **ADR-010 §"Implementation Guidance"**, the HF plugin owns:
//!
//! 1. **Sidecar enumeration** ([`enumerate_sidecars`]) — discovers
//!    non-model files inside a `models--<author>--<repo>/` tree:
//!    `README.md` / `LICENSE` (Readme), `*.imatrix` (Imatrix),
//!    `*.gguf.urls` (Urls), anything under `refs/` or `blobs/` that is not
//!    one of the supplied model files (HfInternal), and any other file
//!    (Other). The sidecar suffix list lives ONLY in this module — never
//!    in `modeltap-core` (component-boundaries §13). The unlink loop treats
//!    every variant identically; the classification is purely diagnostic.
//!
//! 2. **Per-file unlink loop** ([`delete_folder_at`]) — iterates the plan's
//!    `paths_to_unlink_fully` (and, in future steps, `paths_to_unlink_hf_only`)
//!    and produces one [`DeleteOutcome`] per file. Model files (those whose
//!    path appears in [`FolderGroup::models`]) route through
//!    [`crate::delete::delete_one_at`] to reuse the ADR-009 ref-counting
//!    semantics — a blob also referenced by a surviving snapshot stays put,
//!    its bytes are NOT credited as freed. Non-model files (sidecars and
//!    blobs exclusive to the deleted snapshot) are unlinked directly via
//!    `std::fs::remove_file`.
//!
//! 3. **Empty-tree cleanup** ([`remove_empty_repo_tree`]) — after the
//!    per-file unlinks, the now-empty `snapshots/<rev>/`, `snapshots/`,
//!    `blobs/`, `refs/`, and finally `models--<author>--<repo>/` directories
//!    are best-effort `remove_dir`'d. Non-empty subdirs (e.g., a partial
//!    failure left a file behind) are silently absorbed; the user re-runs
//!    `Shift+F` to complete the work.
//!
//! Step 01-03 implements the all-unique happy path: no `paths_to_unlink_hf_only`,
//! no EBUSY, no shared files. Partial failure (one read-only file, the rest
//! succeed) lands in step 04. Shared-file classification lands in step 03.

use std::path::{Path, PathBuf};

use modeltap_core::types::{
    DeleteError, DeleteOutcome, FolderDeletePlan, Sidecar, SidecarKind, ToolId,
};

use walkdir::WalkDir;

use crate::delete::{delete_one_at, delete_one_hf_side_only_at};
use crate::TOOL_NAME;

// ---------------------------------------------------------------------------
// Sidecar enumeration (AC-14 / B-FGD-2; software-crafter-owned per
// architecture-design.md §13)
// ---------------------------------------------------------------------------

/// Walk `repo_dir` and return one [`Sidecar`] per non-model file. `model_files`
/// is the set of paths that BELONG to model rows in this folder's
/// `FolderGroup.models` (their blob paths) — they MUST NOT be classified as
/// sidecars even when they live under `blobs/`.
///
/// The walk follows no symlinks (HF snapshot files ARE symlinks pointing at
/// blobs already covered by `model_files`). Hidden files (leading dot in the
/// filename, except `.imatrix` / `.gguf.urls` matched on suffix) are skipped.
///
/// Returns an empty `Vec` if `repo_dir` does not exist.
pub fn enumerate_sidecars(repo_dir: &Path, model_files: &[PathBuf]) -> Vec<Sidecar> {
    if !repo_dir.exists() {
        return Vec::new();
    }
    let model_set: Vec<PathBuf> = model_files
        .iter()
        .map(|p| canonicalize_or_self(p))
        .collect();
    let mut out = Vec::new();
    for entry in WalkDir::new(repo_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let ft = entry.file_type();
        if !(ft.is_file() || ft.is_symlink()) {
            continue;
        }
        let path = entry.path().to_path_buf();
        // Exclude paths that ARE one of the model rows' blob paths.
        let canon = canonicalize_or_self(&path);
        if model_set.iter().any(|m| m == &canon) || model_set.contains(&path) {
            continue;
        }
        let size_bytes = match std::fs::symlink_metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        let kind = classify_sidecar(repo_dir, &path);
        out.push(Sidecar {
            path,
            size_bytes,
            kind,
        });
    }
    out
}

/// Suffix / location-based classifier. Local to this module so the rules
/// never leak into `modeltap-core` (component-boundaries §13).
fn classify_sidecar(repo_dir: &Path, path: &Path) -> SidecarKind {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return SidecarKind::Other,
    };
    if name == "README.md" || name == "LICENSE" || name == "LICENSE.md" {
        return SidecarKind::Readme;
    }
    if name.ends_with(".imatrix") {
        return SidecarKind::Imatrix;
    }
    if name.ends_with(".gguf.urls") || name.ends_with(".urls") {
        return SidecarKind::Urls;
    }
    if path_starts_with_subdir(repo_dir, path, "refs")
        || path_starts_with_subdir(repo_dir, path, "blobs")
    {
        return SidecarKind::HfInternal;
    }
    SidecarKind::Other
}

fn path_starts_with_subdir(repo_dir: &Path, path: &Path, subdir: &str) -> bool {
    path.strip_prefix(repo_dir)
        .ok()
        .and_then(|rel| rel.components().next())
        .map(|c| c.as_os_str() == subdir)
        .unwrap_or(false)
}

fn canonicalize_or_self(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

// ---------------------------------------------------------------------------
// Per-file unlink loop (ADR-010 §"Implementation Guidance")
// ---------------------------------------------------------------------------

/// Synchronous body of `Tool::delete_folder`. Iterates the plan, emits one
/// [`DeleteOutcome`] per file, then sweeps the empty repo tree.
///
/// Model files (those listed in `plan.folder.models`) route through
/// [`delete_one_at`] to reuse ADR-009 ref-counting. Non-model files are
/// unlinked directly. Per ADR-010 the loop CONTINUES on per-file failure —
/// step 01-03 only exercises the all-success path, but the loop is correct
/// for partial-failure too.
pub fn delete_folder_at(
    hub_root: &Path,
    plan: &FolderDeletePlan,
) -> Result<Vec<DeleteOutcome>, DeleteError> {
    let tool: ToolId = TOOL_NAME;
    let mut outcomes: Vec<DeleteOutcome> = Vec::with_capacity(plan.folder.file_count());

    let model_paths: Vec<PathBuf> = plan
        .folder
        .models
        .iter()
        .map(|m| m.on_disk_path.clone())
        .collect();

    // Model files first — branch by plan classification:
    //
    //   - paths_to_unlink_hf_only ⇒ shared with another tool via cross-tool
    //     hardlink. Unlink only the HF-side snapshot symlink; leave the blob
    //     so the surviving tool's hardlink keeps the inode alive. Outcome
    //     reports registration_removed=true, file_deleted=false, bytes_freed=0
    //     (plugin-contract-spec.md §3.11.S.2; ADR-010).
    //
    //   - paths_to_unlink_fully ⇒ unique (or assumed unique by classifier).
    //     Route through delete_one_at to reuse ADR-009 ref-counting and
    //     credit bytes_freed accordingly.
    //
    // Anything not appearing in either list is conservatively skipped — the
    // plan is the source of truth for what to touch (ADR-010 §"Plan as
    // Source of Truth").
    let hf_only: std::collections::HashSet<&Path> = plan
        .paths_to_unlink_hf_only
        .iter()
        .map(|p| p.as_path())
        .collect();
    let unlink_fully: std::collections::HashSet<&Path> = plan
        .paths_to_unlink_fully
        .iter()
        .map(|p| p.as_path())
        .collect();

    for model in &plan.folder.models {
        let is_shared = hf_only.contains(model.on_disk_path.as_path());
        let is_unique = unlink_fully.contains(model.on_disk_path.as_path());
        // Step 04-01 EBUSY test seam (ADR-010 §D4): if the test-harness has
        // marked this blob path as busy, short-circuit BEFORE either filesystem
        // call so both the snapshot symlink and the blob remain. Production
        // builds compile this branch out entirely (cfg(test) for in-crate use;
        // cfg(feature = "test-harness") for downstream acceptance tests).
        #[cfg(any(test, feature = "test-harness"))]
        if is_test_ebusy_path(&model.on_disk_path) {
            outcomes.push(DeleteOutcome {
                tool,
                model_id_in_tool: model.id_in_tool.clone(),
                bytes_freed: 0,
                registration_removed: false,
                file_deleted: false,
                failure_reason: Some("file open by ollama".to_string()),
            });
            continue;
        }
        let result = if is_shared {
            delete_one_hf_side_only_at(hub_root, &model.on_disk_path, &model.id_in_tool)
        } else if is_unique {
            delete_one_at(hub_root, &model.on_disk_path, &model.id_in_tool)
        } else {
            // Defensive: model not in either bucket. Treat as a no-op outcome
            // so the orchestrator's per-file accounting stays balanced.
            tracing::warn!(
                target: "modeltap.hf.delete_folder",
                "model {} not in paths_to_unlink_fully or paths_to_unlink_hf_only; skipping",
                model.id_in_tool,
            );
            outcomes.push(DeleteOutcome {
                tool,
                model_id_in_tool: model.id_in_tool.clone(),
                bytes_freed: 0,
                registration_removed: false,
                file_deleted: false,
                failure_reason: None,
            });
            continue;
        };
        let outcome = match result {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(
                    target: "modeltap.hf.delete_folder",
                    "delete model {}: {e}",
                    model.id_in_tool,
                );
                DeleteOutcome {
                    tool,
                    model_id_in_tool: model.id_in_tool.clone(),
                    bytes_freed: 0,
                    registration_removed: false,
                    file_deleted: false,
                    failure_reason: None,
                }
            }
        };
        outcomes.push(outcome);
    }

    // Sidecars — direct unlink. Each sidecar maps to one outcome whose
    // `model_id_in_tool` is the sidecar's filename (diagnostic only).
    for sidecar in &plan.folder.sidecars {
        // Skip anything that overlaps with a model row — defensive; the
        // higher-level FolderGroup constructor should already prevent this.
        if model_paths.iter().any(|m| m == &sidecar.path) {
            continue;
        }
        let id = sidecar
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<sidecar>")
            .to_string();
        let outcome = match std::fs::remove_file(&sidecar.path) {
            Ok(()) => DeleteOutcome {
                tool,
                model_id_in_tool: id,
                bytes_freed: sidecar.size_bytes,
                registration_removed: true,
                file_deleted: true,
                failure_reason: None,
            },
            Err(e) => {
                tracing::warn!(
                    target: "modeltap.hf.delete_folder",
                    "delete sidecar {}: {e}",
                    sidecar.path.display(),
                );
                DeleteOutcome {
                    tool,
                    model_id_in_tool: id,
                    bytes_freed: 0,
                    registration_removed: false,
                    file_deleted: false,
                    failure_reason: None,
                }
            }
        };
        outcomes.push(outcome);
    }

    // Best-effort empty-tree cleanup.
    remove_empty_repo_tree(&plan.folder.absolute_path);

    Ok(outcomes)
}

// ---------------------------------------------------------------------------
// Empty-tree cleanup
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// EBUSY test seam (step 04-01)
// ---------------------------------------------------------------------------
//
// Gated behind cfg(any(test, feature = "test-harness")) so production builds
// (which compile this crate without `--features test-harness`) do not include
// the env-var read or the string match. Per ADR-010 §D4, verification: the
// stripped release binary MUST NOT contain the byte sequence
// `MODELTAP_TEST_EBUSY_PATHS`.

/// Returns `true` iff `MODELTAP_TEST_EBUSY_PATHS` is set and lists `path`
/// (canonicalised) in its colon-separated entries. Used by the per-file unlink
/// loop to simulate an in-use-by-another-tool file without a real `flock` or
/// sibling process — portable across macOS and Linux.
#[cfg(any(test, feature = "test-harness"))]
fn is_test_ebusy_path(path: &Path) -> bool {
    let raw = match std::env::var("MODELTAP_TEST_EBUSY_PATHS") {
        Ok(v) if !v.is_empty() => v,
        _ => return false,
    };
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    raw.split(':').any(|entry| {
        if entry.is_empty() {
            return false;
        }
        let entry_path = std::path::Path::new(entry);
        let entry_canon =
            std::fs::canonicalize(entry_path).unwrap_or_else(|_| entry_path.to_path_buf());
        entry_canon == canon
    })
}

/// Best-effort: bottom-up `remove_dir` on every directory under `repo_dir`,
/// then `repo_dir` itself. Non-empty directories are silently skipped — they
/// represent the partial-failure case ("some unlink left a file behind, user
/// will re-run `Shift+F`").
pub fn remove_empty_repo_tree(repo_dir: &Path) {
    if !repo_dir.exists() {
        return;
    }
    // Collect directories in depth order (deepest first) and try to remove
    // each. WalkDir's `contents_first(true)` yields children before parents.
    let mut dirs: Vec<PathBuf> = WalkDir::new(repo_dir)
        .contents_first(true)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_dir())
        .map(|e| e.path().to_path_buf())
        .collect();
    // contents_first yields children-first, but to be safe sort by depth desc.
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    for d in dirs {
        let _ = std::fs::remove_dir(&d);
    }
}
