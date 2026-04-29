//! `Tool::link` implementation for Hugging Face.
//!
//! Per `plugins/hf/LINKING.md` (closes ADR-004 OQ-1), HF's cache is
//! content-addressed: each blob is stored at `<hub>/blobs/<sha256>`, and
//! `models--<org>--<repo>/snapshots/<rev>/<file>` symlinks point at the blob
//! by content hash.
//!
//! `link()` therefore replaces the blob in place. The snapshot symlinks are
//! NEVER touched — they continue to resolve correctly because their target
//! filename (the blob's `sha256-X`) is unchanged, and the file at that
//! filename now has matching sha256 (the canonical's bytes by precondition).
//!
//! `model.on_disk_path` from HF discovery is already the resolved blob path
//! (`<hub>/<repo>/blobs/<sha256>`), so we use it directly as the target.

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

    // HF's blob target lives under `<hub>/<repo>/blobs/`, which always
    // exists if the snapshot symlinks resolved during discovery — but be
    // defensive in case a user nuked the parent dir between discover() and
    // link().
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
#[cfg(unix)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;

    /// Build a minimal HF cache shape: one repo, one snapshot symlink → blob.
    /// Returns (hub_root, blob_path, snapshot_link_path).
    fn build_hf_cache(temp: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let hub = temp.join("hub");
        let repo = hub.join("models--owner--repo");
        let blobs = repo.join("blobs");
        let snaps = repo.join("snapshots/abc123");
        fs::create_dir_all(&blobs).unwrap();
        fs::create_dir_all(&snaps).unwrap();
        let blob = blobs.join("sha256-deadbeef");
        fs::write(&blob, b"OLD CONTENT").unwrap();
        let snap = snaps.join("model.safetensors");
        symlink("../../blobs/sha256-deadbeef", &snap).unwrap();
        (hub, blob, snap)
    }

    #[test]
    fn link_at_replaces_blob_in_place_and_snapshot_still_resolves() {
        let temp = tempfile::tempdir().unwrap();
        let (_hub, blob, snap) = build_hf_cache(temp.path());

        // The canonical source: a different file with the "right" content.
        let canonical = temp.path().join("canonical.gguf");
        fs::write(&canonical, b"NEW CANONICAL CONTENT").unwrap();

        // Pre-link: snapshot reads OLD CONTENT.
        assert_eq!(fs::read(&snap).unwrap(), b"OLD CONTENT");

        let outcome = link_at(&canonical, &blob, "owner/repo/model.safetensors").expect("link ok");
        assert!(matches!(outcome.result, LinkResult::HardLinked { .. }));

        // Snapshot symlink (unchanged) now resolves to the canonical's bytes.
        assert_eq!(fs::read(&snap).unwrap(), b"NEW CANONICAL CONTENT");
        // Blob and canonical share an inode.
        assert!(same_inode_or_none(&canonical, &blob).is_some());
    }

    #[test]
    fn link_at_is_idempotent_when_blob_already_shares_canonical_inode() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("canonical");
        let target = temp.path().join("target");
        fs::write(&canonical, b"x").unwrap();
        fs::hard_link(&canonical, &target).unwrap();

        let outcome = link_at(&canonical, &target, "demo").expect("link ok");
        assert!(matches!(outcome.result, LinkResult::AlreadyLinked { .. }));
    }

    #[test]
    fn link_at_creates_blob_when_target_missing_after_user_purge() {
        // Defensive case: user deleted the blob between discover and link.
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().join("canonical");
        let target = temp.path().join("blobs/sha256-x");
        fs::write(&canonical, b"x").unwrap();

        let outcome = link_at(&canonical, &target, "demo").expect("link ok");
        assert!(matches!(outcome.result, LinkResult::HardLinked { .. }));
    }
}
