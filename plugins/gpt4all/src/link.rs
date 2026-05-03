//! `Tool::link` implementation for the GPT4All plugin (step 01-05; AC-G2.2).
//!
//! GPT4All stores models as flat `*.gguf` files under one of the configured
//! roots — no manifest, no content-addressing. The link operation is therefore
//! direct file replacement via the standard atomic-rename pattern (identical
//! in shape to `lm-studio` / `_loose-gguf.archived`):
//!
//!   1. tempfile (sibling path to `target`, same parent dir → same fs)
//!   2. `fs::hard_link(canonical_src, temp)` → temp now shares canonical inode
//!   3. `fs::rename(temp, target)` → POSIX-atomic; readers see either the old
//!      file or the new (never half-formed)
//!
//! Idempotent: if `target` already shares `canonical_src`'s inode (verified
//! by `stat`), the function short-circuits with `LinkResult::AlreadyLinked`.
//!
//! ### EXDEV / cross-filesystem handling — ADR-008
//!
//! The roadmap step text mentions a "fall back to `std::fs::copy` per
//! ADR-008". Re-reading ADR-008: the orchestrator (NOT the plugin) owns the
//! cross-fs decision (the per-target `[s]kip / [c]opy / [x]ancel` UI is in
//! `actions/unify.rs`). The contract for `Tool::link` is therefore: surface
//! `LinkError::CrossFilesystem` on EXDEV, let the orchestrator apply the
//! user's choice. This matches every other plugin (`ollama`, `lm-studio`,
//! `_loose-gguf.archived`) and ADR-008's "no implicit default — no auto-skip,
//! no auto-copy" rule. A copy fallback hidden inside the plugin would
//! silently double bytes against the user's expectation (ADR-008 alternative
//! A, REJECTED).

use std::path::{Path, PathBuf};

use modeltap_core::{LinkError, LinkOutcome, LinkResult, ToolId};

use crate::TOOL_NAME;

/// Atomic-replace `target` with a hardlink to `canonical_src`.
pub fn link_at(
    canonical_src: &Path,
    target: &Path,
    model_id_in_tool: &str,
) -> Result<LinkOutcome, LinkError> {
    perform_link(canonical_src, target, TOOL_NAME, model_id_in_tool)
}

pub(crate) fn perform_link(
    canonical_src: &Path,
    target: &Path,
    tool: ToolId,
    model_id_in_tool: &str,
) -> Result<LinkOutcome, LinkError> {
    // Step 1: idempotency. If target already shares canonical's inode, no-op.
    if let Some(inode) = same_inode_or_none(canonical_src, target) {
        return Ok(LinkOutcome {
            tool,
            model_id_in_tool: model_id_in_tool.to_string(),
            result: LinkResult::AlreadyLinked {
                canonical: canonical_src.to_path_buf(),
                target: target.to_path_buf(),
                inode,
            },
        });
    }

    // Step 2: ensure parent dir exists. Defensive — the configured root may
    // be present but a sub-path within it may not be (rare for GPT4All's flat
    // layout, but cheap to handle).
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| classify_io_error(parent, e))?;
        }
    }

    // Step 3: tempfile + hard_link + rename.
    let temp = sibling_temp_path(target);
    // Best-effort cleanup of any stale temp from a prior crash.
    let _ = std::fs::remove_file(&temp);
    std::fs::hard_link(canonical_src, &temp).map_err(|e| classify_io_error(&temp, e))?;
    if let Err(e) = std::fs::rename(&temp, target) {
        let _ = std::fs::remove_file(&temp);
        return Err(classify_io_error(target, e));
    }

    let inode = inode_of(target).unwrap_or(0);
    Ok(LinkOutcome {
        tool,
        model_id_in_tool: model_id_in_tool.to_string(),
        result: LinkResult::HardLinked {
            canonical: canonical_src.to_path_buf(),
            target: target.to_path_buf(),
            inode,
        },
    })
}

/// `Some(inode)` iff both paths exist and share the same inode on the same
/// filesystem. `None` otherwise.
pub(crate) fn same_inode_or_none(a: &Path, b: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let ma = std::fs::metadata(a).ok()?;
        let mb = std::fs::metadata(b).ok()?;
        if ma.dev() == mb.dev() && ma.ino() == mb.ino() {
            Some(ma.ino())
        } else {
            None
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        None
    }
}

pub(crate) fn inode_of(p: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(p).ok().map(|m| m.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        None
    }
}

/// Build a sibling path next to `target` for the temp-then-rename pattern.
/// The temp lives in the same directory so the rename is fs-local.
pub(crate) fn sibling_temp_path(target: &Path) -> PathBuf {
    let pid = std::process::id();
    let nano = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let stamp = format!(".modeltap-tmp-{pid}-{nano}");
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    let name = target
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "modeltap-target".to_string());
    parent.join(format!("{name}.{stamp}"))
}

/// Map `std::io::Error` from `hard_link` / `rename` to a structured
/// `LinkError`. EXDEV (raw 18) → `CrossFilesystem`; PermissionDenied → its
/// own variant; everything else → `Io`.
pub(crate) fn classify_io_error(path: &Path, e: std::io::Error) -> LinkError {
    let raw = e.raw_os_error();
    if raw == Some(18) {
        // EXDEV. Per ADR-008, the plugin surfaces the condition; the
        // orchestrator chooses copy-vs-skip-vs-cancel.
        return LinkError::CrossFilesystem {
            canonical: PathBuf::new(),
            target: path.to_path_buf(),
        };
    }
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        return LinkError::PermissionDenied {
            path: path.to_path_buf(),
            source: e,
        };
    }
    LinkError::Io(e)
}
