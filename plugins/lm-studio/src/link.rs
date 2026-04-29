//! `Tool::link` implementation for LM Studio.
//!
//! Per `plugins/lm-studio/PATHS.md` and ADR-004 OQ-2, LM Studio stores
//! model files as plain `.gguf` blobs at predictable
//! `<root>/<org>/<repo>/<file>.gguf` paths — no manifest, no
//! content-addressing. Therefore `link()` is **direct file replacement
//! via hardlink**, identical in shape to llama-cli (the two share the
//! atomic-rename pattern; the wrapper here exists so each plugin owns
//! its own `link.rs` per the ADR-001 plugin-isolation rule).

use std::path::{Path, PathBuf};

use modeltap_core::{LinkError, LinkOutcome, LinkResult, ToolId};

use crate::TOOL_NAME;

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

    // LM Studio's <root>/<org>/<repo>/<file>.gguf path may have parent dirs
    // that don't exist yet (newly-installed model into a fresh tree).
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| classify_io_error(parent, e))?;
        }
    }

    let temp = sibling_temp_path(target);
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

pub(crate) fn classify_io_error(path: &Path, e: std::io::Error) -> LinkError {
    let raw = e.raw_os_error();
    if raw == Some(18) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn link_at_creates_hardlink_when_target_missing() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("src.gguf");
        // Mimic LM Studio's <root>/<org>/<repo>/<file> shape — the parent
        // dirs may not exist yet.
        let target = temp.path().join("microsoft/phi-3-mini/phi-3-mini-q4.gguf");
        fs::write(&canonical, b"hello world").unwrap();

        let outcome =
            link_at(&canonical, &target, "microsoft/phi-3-mini/phi-3-mini-q4").expect("link ok");
        assert!(matches!(outcome.result, LinkResult::HardLinked { .. }));
        assert!(same_inode_or_none(&canonical, &target).is_some());
    }

    #[test]
    fn link_at_is_idempotent_when_already_linked() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("src.gguf");
        let target = temp.path().join("dst.gguf");
        fs::write(&canonical, b"x").unwrap();
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
        fs::write(&target, b"OLD").unwrap();

        let outcome = link_at(&canonical, &target, "demo").expect("link ok");
        assert!(matches!(outcome.result, LinkResult::HardLinked { .. }));
        assert_eq!(fs::read(&target).unwrap(), b"new content");
    }
}
