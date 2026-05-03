//! `Tool::delete_one` and `Tool::delete_all` for the GPT4All plugin
//! (step 01-05; AC-G3.1).
//!
//! GPT4All is flat-file: each model is a single `*.gguf` blob in one of the
//! configured roots. There is no manifest and no ref-counted blob — therefore
//! delete is just `fs::remove_file` against the model's `on_disk_path`, with
//! `bytes_freed` taken from a stat at the moment of unlink (mirror of
//! `lm-studio::delete::delete_one_at`).
//!
//! `delete_all` walks every existing configured root at depth 1 (no recursion
//! — flat by design, matching `discover::discover_in`) and unlinks every
//! `.gguf` file. Order is deterministic: filenames sorted within each root,
//! roots in config order. A missing root is silently skipped (consistent with
//! discover's "not installed" semantics — there's nothing to delete in a
//! root that does not exist).
//!
//! Partial-success policy: per-file outcomes carry `file_deleted: true|false`.
//! If a single file fails to unlink (permission, disk error), its
//! `DeleteOutcome` records `file_deleted = false` and the loop continues.
//! The orchestrator surfaces a "partial" verdict at the action level — same
//! convention as `ollama::delete::delete_all_at`.

use std::path::{Path, PathBuf};

use modeltap_core::{DeleteError, DeleteOutcome, ToolId};

use crate::TOOL_NAME;

// -- delete_one -------------------------------------------------------------

/// Delete one GPT4All model file at `target_path`. Stat-then-unlink so we
/// can report `bytes_freed` accurately. ENOENT → `DeleteError::NotFound` so
/// the orchestrator can surface a coherent "nothing to do" outcome rather
/// than a panic or generic IO error.
pub fn delete_one_at(
    target_path: &Path,
    model_id_in_tool: &str,
) -> Result<DeleteOutcome, DeleteError> {
    let tool: ToolId = TOOL_NAME;
    let size_bytes = std::fs::metadata(target_path).map(|m| m.len()).ok();
    match std::fs::remove_file(target_path) {
        Ok(()) => Ok(DeleteOutcome {
            tool,
            model_id_in_tool: model_id_in_tool.to_string(),
            bytes_freed: size_bytes.unwrap_or(0),
            registration_removed: true,
            file_deleted: true,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(DeleteError::NotFound(model_id_in_tool.to_string()))
        }
        Err(e) => {
            tracing::warn!(
                target: "modeltap.gpt4all.delete",
                "delete_one: remove {}: {e}",
                target_path.display()
            );
            Err(DeleteError::Io(e))
        }
    }
}

// -- delete_all -------------------------------------------------------------

/// Delete every `*.gguf` file across every existing configured root.
/// Returns one `DeleteOutcome` per file in deterministic order
/// (filenames sorted within each root, roots in config order).
pub fn delete_all_at(roots: &[PathBuf]) -> Result<Vec<DeleteOutcome>, DeleteError> {
    let plans = enumerate_plans(roots);
    let mut outcomes = Vec::with_capacity(plans.len());
    for plan in plans {
        outcomes.push(delete_one_plan(plan));
    }
    Ok(outcomes)
}

#[derive(Debug, Clone)]
struct Plan {
    /// Absolute path to the `*.gguf` file.
    file_path: PathBuf,
    /// `id_in_tool` is the bare filename — same convention discover uses.
    id_in_tool: String,
    /// Size at enumeration time (best-effort; 0 if stat fails).
    size_bytes: u64,
}

/// Enumerate every `*.gguf` file at depth 1 across every existing root.
/// Order: filenames sorted lexicographically within each root, roots in
/// config order. Mirrors `discover::discover_in`'s flat-walk semantics so
/// the JSONL audit trail can correlate the two.
fn enumerate_plans(roots: &[PathBuf]) -> Vec<Plan> {
    let mut plans = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        let mut in_root = collect_gguf_in_root(root);
        in_root.sort_by(|a, b| a.id_in_tool.cmp(&b.id_in_tool));
        plans.extend(in_root);
    }
    plans
}

fn collect_gguf_in_root(root: &Path) -> Vec<Plan> {
    let read_dir = match std::fs::read_dir(root) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.gpt4all.delete",
                "delete_all: read_dir({}) failed: {e}",
                root.display()
            );
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for entry in read_dir {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(
                    target: "modeltap.gpt4all.delete",
                    "delete_all: read_dir entry error in {}: {e}",
                    root.display()
                );
                continue;
            }
        };
        // Depth = 1: skip subdirectories (flat by design — same as discover).
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if !file_type.is_file() {
            continue;
        }
        let file_name = match entry.file_name().into_string() {
            Ok(n) => n,
            Err(_) => continue,
        };
        if file_name.starts_with('.') {
            continue;
        }
        if !file_name.to_ascii_lowercase().ends_with(".gguf") {
            continue;
        }
        let file_path = entry.path();
        let size_bytes = std::fs::metadata(&file_path).map(|m| m.len()).unwrap_or(0);
        out.push(Plan {
            file_path,
            id_in_tool: file_name,
            size_bytes,
        });
    }
    out
}

fn delete_one_plan(plan: Plan) -> DeleteOutcome {
    let tool: ToolId = TOOL_NAME;
    match std::fs::remove_file(&plan.file_path) {
        Ok(()) => DeleteOutcome {
            tool,
            model_id_in_tool: plan.id_in_tool,
            bytes_freed: plan.size_bytes,
            registration_removed: true,
            file_deleted: true,
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Race: file vanished between enumerate and unlink. Treat as
            // "already gone" — registration intent is satisfied; no bytes to
            // attribute (file size at enumerate time may now be misleading).
            DeleteOutcome {
                tool,
                model_id_in_tool: plan.id_in_tool,
                bytes_freed: 0,
                registration_removed: true,
                file_deleted: false,
            }
        }
        Err(e) => {
            tracing::warn!(
                target: "modeltap.gpt4all.delete",
                "delete_all: remove {}: {e}",
                plan.file_path.display()
            );
            DeleteOutcome {
                tool,
                model_id_in_tool: plan.id_in_tool,
                bytes_freed: 0,
                registration_removed: false,
                file_deleted: false,
            }
        }
    }
}
