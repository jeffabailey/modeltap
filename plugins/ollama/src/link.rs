//! `Tool::link` implementation for Ollama (ADR-004 OQ-3).
//!
//! Ollama is content-addressed: blobs live at
//! `<root>/blobs/sha256-<hash>` where `<hash>` IS the lowercase hex SHA-256
//! of the blob's content bytes (verified in
//! `plugins/ollama/OLLAMA_BLOB_VERIFICATION.md`). Manifests reference blobs
//! by `digest: sha256:<hash>` — the same value as the filename suffix.
//!
//! Therefore a safe `link()` MUST verify, before any filesystem mutation,
//! that the canonical's content hash equals the `<hash>` parsed from the
//! target filename. If it does not, refuse with `LinkError::ContentMismatch`
//! per ADR-008's "no partial-state corruption" rule.
//!
//! After verification, the operation is identical to llama-cli / lm-studio:
//! atomic-replace via tempfile + `fs::hard_link` + `fs::rename`.

use std::io::Read;
use std::path::{Path, PathBuf};

use modeltap_core::{LinkError, LinkOutcome, LinkResult, ToolId};
use sha2::{Digest, Sha256};

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

    // Step 2: extract the expected sha256 from the target filename. If the
    // target name does not parse as `sha256-<64-hex>`, refuse — modeltap
    // does not invent identifiers for content-addressed stores.
    let expected = parse_sha256_from_filename(target).ok_or_else(|| LinkError::MalformedMeta {
        reason: format!(
            "ollama target filename must be `sha256-<hex>`; got {:?}",
            target.file_name()
        ),
    })?;

    // Step 3: verify sha256(canonical_src) == expected before any mutation.
    verify_sha256_match(canonical_src, target, &expected)?;

    // Step 4: ensure parent dir exists (defensive — `<root>/blobs/` is
    // typically present if Ollama is installed at all).
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| classify_io_error(parent, e))?;
        }
    }

    // Step 5: tempfile + hard_link + rename.
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

/// Parse a hex sha256 out of an Ollama blob filename of the form
/// `sha256-<64-hex>`. Returns `None` for any other shape.
pub(crate) fn parse_sha256_from_filename(target: &Path) -> Option<String> {
    let name = target.file_name()?.to_str()?;
    let rest = name.strip_prefix("sha256-")?;
    if rest.len() != 64 || !rest.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(rest.to_ascii_lowercase())
}

/// Stream-hash `canonical_src` and compare against `expected_hex`. Returns
/// `LinkError::ContentMismatch` if they differ. The hash uses the same
/// 64 KiB chunking strategy as `Sha2Hasher` in
/// `crates/modeltap-app/src/sha256_cache.rs` so the cost is identical to
/// what the detail screen would have already paid.
pub(crate) fn verify_sha256_match(
    canonical_src: &Path,
    target: &Path,
    expected_hex: &str,
) -> Result<(), LinkError> {
    let mut file =
        std::fs::File::open(canonical_src).map_err(|e| classify_io_error(canonical_src, e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| classify_io_error(canonical_src, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let actual_hex = hex_lower(&digest);
    if actual_hex != expected_hex {
        return Err(LinkError::ContentMismatch {
            target: target.to_path_buf(),
            expected: expected_hex.to_string(),
            actual: actual_hex,
        });
    }
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(nibble_to_hex(b >> 4));
        s.push(nibble_to_hex(b & 0x0f));
    }
    s
}

fn nibble_to_hex(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + n - 10) as char,
        _ => unreachable!(),
    }
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

    /// Compute sha256 hex of a byte slice (test helper).
    fn sha256_hex(bytes: &[u8]) -> String {
        let mut h = Sha256::new();
        h.update(bytes);
        hex_lower(&h.finalize())
    }

    #[test]
    fn parse_sha256_extracts_hex_from_blob_filename() {
        let p = Path::new(
            "/x/blobs/sha256-abc123000000000000000000000000000000000000000000000000000000abcd",
        );
        let h = parse_sha256_from_filename(p).expect("parse");
        assert_eq!(
            h,
            "abc123000000000000000000000000000000000000000000000000000000abcd"
        );
    }

    #[test]
    fn parse_sha256_rejects_non_sha256_filenames() {
        assert!(parse_sha256_from_filename(Path::new("/x/foo.gguf")).is_none());
        assert!(parse_sha256_from_filename(Path::new("/x/sha256-tooshort")).is_none());
        assert!(parse_sha256_from_filename(Path::new(
            "/x/sha256-zzzz00000000000000000000000000000000000000000000000000000000zzzz"
        ))
        .is_none());
    }

    #[test]
    fn link_at_succeeds_when_canonical_matches_blob_hash() {
        let temp = tempfile::tempdir().unwrap();
        let blobs = temp.path().join("blobs");
        fs::create_dir_all(&blobs).unwrap();
        let content = b"hello ollama world";
        let hex = sha256_hex(content);
        let target = blobs.join(format!("sha256-{hex}"));
        fs::write(&target, b"OLD CONTENT").unwrap();

        let canonical = temp.path().join("canonical.gguf");
        fs::write(&canonical, content).unwrap();

        let outcome = link_at(&canonical, &target, "demo:7b").expect("link ok");
        assert!(matches!(outcome.result, LinkResult::HardLinked { .. }));
        assert_eq!(fs::read(&target).unwrap(), content);
        assert!(same_inode_or_none(&canonical, &target).is_some());
    }

    #[test]
    fn link_at_refuses_with_content_mismatch_when_canonical_hash_differs() {
        let temp = tempfile::tempdir().unwrap();
        let blobs = temp.path().join("blobs");
        fs::create_dir_all(&blobs).unwrap();
        // Target name claims sha256 of `b"foo"`, but canonical is `b"bar"`.
        let foo_hex = sha256_hex(b"foo");
        let target = blobs.join(format!("sha256-{foo_hex}"));
        fs::write(&target, b"original ollama bytes").unwrap();
        let original_inode = inode_of(&target);

        let canonical = temp.path().join("canonical.gguf");
        fs::write(&canonical, b"bar").unwrap();

        let res = link_at(&canonical, &target, "demo:7b");
        match res {
            Err(LinkError::ContentMismatch {
                expected, actual, ..
            }) => {
                assert_eq!(expected, foo_hex);
                assert_eq!(actual, sha256_hex(b"bar"));
            }
            other => panic!("expected ContentMismatch, got {:?}", other),
        }

        // Defensive: target was NOT mutated (no partial-state corruption).
        assert_eq!(fs::read(&target).unwrap(), b"original ollama bytes");
        assert_eq!(inode_of(&target), original_inode);
    }

    #[test]
    fn link_at_refuses_with_malformed_meta_when_target_filename_unparseable() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("not-a-blob-name.gguf");
        let canonical = temp.path().join("canonical");
        fs::write(&canonical, b"x").unwrap();
        let res = link_at(&canonical, &target, "demo:7b");
        assert!(
            matches!(res, Err(LinkError::MalformedMeta { .. })),
            "{:?}",
            res
        );
    }

    #[test]
    fn link_at_is_idempotent_when_already_linked() {
        let temp = tempfile::tempdir().unwrap();
        let blobs = temp.path().join("blobs");
        fs::create_dir_all(&blobs).unwrap();
        let content = b"identical bytes";
        let hex = sha256_hex(content);
        let target = blobs.join(format!("sha256-{hex}"));
        fs::write(&target, content).unwrap();
        let canonical = temp.path().join("canonical");
        fs::hard_link(&target, &canonical).unwrap();

        // Already linked → no sha256 verification, no fs mutation.
        let outcome = link_at(&canonical, &target, "demo:7b").expect("ok");
        assert!(matches!(outcome.result, LinkResult::AlreadyLinked { .. }));
    }
}
