//! `Tool::link` implementation for llama-cli.
//!
//! llama-cli stores models as loose `.gguf` files. No manifest, no
//! content-addressing — the user invokes `llama-cli -m <path>` directly.
//!
//! Per ADR-004 OQ-1 / OQ-2, the link operation is atomic-replace via:
//!
//!   1. tempfile (sibling path to `target`, same parent dir → same fs)
//!   2. `fs::hard_link(canonical_src, temp)` → temp now shares canonical inode
//!   3. `fs::rename(temp, target)` → POSIX-atomic; readers see either the old
//!      file or the new (never half-formed)
//!
//! Idempotent: if `target` already shares `canonical_src`'s inode (verified
//! by stat), the function short-circuits with `LinkResult::AlreadyLinked`.
//!
//! Cross-filesystem failures (`EXDEV`) surface as `LinkError::CrossFilesystem`
//! — the user-facing [s/c/x] dialog is the orchestrator's concern (step 03-03).

use std::path::{Path, PathBuf};

use modeltap_core::{LinkError, LinkOutcome, LinkResult, ToolId};

use crate::TOOL_NAME;

/// Atomic-replace `target` with a hardlink to `canonical_src`.
///
/// `model_id_in_tool` is included so the returned `LinkOutcome` carries the
/// id the orchestrator can correlate back to its inventory snapshot.
pub fn link_at(
    canonical_src: &Path,
    target: &Path,
    model_id_in_tool: &str,
) -> Result<LinkOutcome, LinkError> {
    perform_link(canonical_src, target, TOOL_NAME, model_id_in_tool)
}

/// Generic helper shared with the other plugins (kept here as the canonical
/// implementation — no shared crate; copy it to each plugin if behavior
/// diverges). Parameterised by the `tool` id so the returned `LinkOutcome`
/// carries the correct identity.
pub(crate) fn perform_link(
    canonical_src: &Path,
    target: &Path,
    tool: ToolId,
    model_id_in_tool: &str,
) -> Result<LinkOutcome, LinkError> {
    // Step 1: idempotency check. If `target` already shares the canonical's
    // inode, we are done — emit AlreadyLinked.
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

    // Step 2: ensure the target's parent dir exists (loose-file plugins
    // sometimes create the dir on first install).
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

/// Returns `Some(inode)` iff both paths exist and share the same inode on
/// the same filesystem. `None` otherwise (different inode, missing target,
/// non-Unix). This is the canonical "already hardlinked" check.
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

/// On Unix, return `target`'s inode after the rename completes; `None`
/// otherwise. The orchestrator uses this for confirmation logging.
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

/// Map `std::io::Error` from `hard_link` / `rename` to a structured `LinkError`.
pub(crate) fn classify_io_error(path: &Path, e: std::io::Error) -> LinkError {
    // EXDEV — cross-device link. ErrorKind::CrossesDevices is unstable, so
    // we fall back to raw_os_error == 18 (Linux) / EXDEV.
    let raw = e.raw_os_error();
    let is_exdev = raw == Some(libc_exdev());
    if is_exdev {
        return LinkError::CrossFilesystem {
            canonical: PathBuf::new(), // filled by caller if needed; placeholder for now
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

/// Best-effort EXDEV detection. macOS, Linux, FreeBSD all use 18.
#[inline]
fn libc_exdev() -> i32 {
    18
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn link_at_creates_hardlink_when_target_missing() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("src.gguf");
        let target = temp.path().join("dst.gguf");
        fs::write(&canonical, b"hello world").unwrap();

        let outcome = link_at(&canonical, &target, "demo").expect("link ok");
        match &outcome.result {
            LinkResult::HardLinked { inode, .. } => {
                assert!(*inode > 0, "expected non-zero inode on Unix");
            }
            other => panic!("expected HardLinked, got {:?}", other),
        }
        // Both paths now share an inode.
        assert!(same_inode_or_none(&canonical, &target).is_some());
        assert_eq!(fs::read(&target).unwrap(), b"hello world");
    }

    #[test]
    fn link_at_is_idempotent_when_already_linked() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("src.gguf");
        let target = temp.path().join("dst.gguf");
        fs::write(&canonical, b"hello world").unwrap();
        fs::hard_link(&canonical, &target).unwrap();

        let outcome = link_at(&canonical, &target, "demo").expect("link ok");
        assert!(matches!(outcome.result, LinkResult::AlreadyLinked { .. }));
    }

    #[test]
    fn link_at_replaces_unrelated_target_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("src.gguf");
        let target = temp.path().join("dst.gguf");
        fs::write(&canonical, b"new content").unwrap();
        fs::write(&target, b"OLD CONTENT, DIFFERENT INODE").unwrap();

        let outcome = link_at(&canonical, &target, "demo").expect("link ok");
        assert!(matches!(outcome.result, LinkResult::HardLinked { .. }));
        // After the link, target reads as canonical's bytes (same inode).
        assert_eq!(fs::read(&target).unwrap(), b"new content");
        assert!(same_inode_or_none(&canonical, &target).is_some());
    }

    #[test]
    fn link_at_creates_parent_dir_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("src.gguf");
        let target = temp.path().join("nested/sub/dst.gguf");
        fs::write(&canonical, b"x").unwrap();

        let outcome = link_at(&canonical, &target, "demo").expect("link ok");
        assert!(matches!(outcome.result, LinkResult::HardLinked { .. }));
    }

    #[test]
    fn sibling_temp_path_is_in_same_parent_as_target() {
        let target = PathBuf::from("/some/dir/file.gguf");
        let temp = sibling_temp_path(&target);
        assert_eq!(temp.parent(), target.parent());
    }
}
