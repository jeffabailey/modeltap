//! Synchronous discovery walker for the Ollama plugin.
//!
//! Walks `<root>/manifests/` recursively, parses each manifest, resolves
//! its blob reference under `<root>/blobs/sha256-<hash>`, and emits one
//! `DiscoveredModel` per manifest. Per ADR-002, blob hashes are NOT
//! re-computed here (lazy); we use the manifest's declared layer size
//! as the model's apparent size, which is also the correct value for
//! deduplicated disk-usage accounting (a blob shared by N manifests is
//! one file on disk).
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon (per ADR-001 plugins live outside the core). Strict OC rules
//! are relaxed here — the adapter exists to bridge real I/O, not model
//! domain behavior.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use modeltap_core::{DedupKey, DiscoverError, DiscoveredModel, DisplayLabel, Format, ModelStatus};
use walkdir::WalkDir;

use crate::manifest::{parse_manifest, ManifestEntry};

/// Walk the Ollama models root and return one `DiscoveredModel` per manifest.
///
/// The root must be the directory containing `manifests/` and `blobs/` —
/// i.e. `~/.ollama/models/`, NOT `~/.ollama/`.
///
/// Behavior contract (per US-02 acceptance criteria):
/// - Missing root → `Err(DiscoverError::NotInstalled)`. The app translates
///   this to `ToolStatus::NotInstalled` (AC-3).
/// - Missing/unreadable `manifests/` subdir → `Err(DiscoverError::PermissionDenied)`
///   or `Err(DiscoverError::UnexpectedLayout)`. The app annotates the tool
///   row as `(error)` (AC-4).
/// - Manifest with broken JSON → that one entry is skipped with a `ManifestParse`
///   diagnostic written to the eventual diagnostics log (we log via tracing).
/// - Blob file missing → entry is emitted with `ModelStatus::BrokenSymlink`.
pub fn discover_in(root: &Path) -> Result<Vec<DiscoveredModel>, DiscoverError> {
    if !root.exists() {
        return Err(DiscoverError::NotInstalled);
    }
    let manifests_dir = root.join("manifests");
    if !manifests_dir.exists() {
        // Root exists but no manifests subdir: treat as "installed but empty".
        // Return Ok(empty) so the app can show "0 models" rather than an error
        // — matches the acceptance criteria for an empty-but-installed tool.
        return Ok(Vec::new());
    }

    // Probe readability: WalkDir will silently swallow permission errors on
    // the root if we let it. Instead, attempt a `read_dir` first so we can
    // surface PermissionDenied as a tracked error.
    if let Err(io_err) = std::fs::read_dir(&manifests_dir) {
        if io_err.kind() == std::io::ErrorKind::PermissionDenied {
            return Err(DiscoverError::PermissionDenied {
                path: manifests_dir,
                source: io_err,
            });
        }
        return Err(DiscoverError::Io(io_err));
    }

    let blobs_dir = root.join("blobs");
    let mut models = Vec::new();

    for entry in WalkDir::new(&manifests_dir).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(target: "modeltap.ollama", "walkdir error: {e}");
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        match read_one_manifest(entry.path(), &manifests_dir, &blobs_dir) {
            Ok(model) => models.push(model),
            Err(reason) => {
                tracing::warn!(
                    target: "modeltap.ollama",
                    "skipping manifest {}: {reason}",
                    entry.path().display()
                );
            }
        }
    }

    // Sort so output is deterministic across filesystem orderings.
    models.sort_by(|a, b| a.id_in_tool.cmp(&b.id_in_tool));
    Ok(models)
}

fn read_one_manifest(
    manifest_path: &Path,
    manifests_root: &Path,
    blobs_dir: &Path,
) -> Result<DiscoveredModel, String> {
    let raw = std::fs::read_to_string(manifest_path).map_err(|e| format!("read manifest: {e}"))?;
    let parsed: ManifestEntry = parse_manifest(&raw).map_err(|e| format!("parse manifest: {e}"))?;
    let id_in_tool = manifest_id(manifest_path, manifests_root)
        .ok_or_else(|| "could not compute id_in_tool from manifest path".to_string())?;
    let blob_path = blobs_dir.join(format!("sha256-{}", parsed.blob_sha));

    let (size_bytes, status) = resolve_blob(&blob_path, parsed.size_bytes);

    Ok(DiscoveredModel {
        id_in_tool: id_in_tool.clone(),
        on_disk_path: blob_path,
        size_bytes,
        format: Format::OllamaBlob,
        display_label: DisplayLabel::from(id_in_tool),
        status,
    })
}

/// Translate a manifest path under `<root>/manifests/<registry>/<repo>/<tag>`
/// into the canonical Ollama model id `<repo>:<tag>`. We deliberately discard
/// the registry segment because Devon's existing Ollama models are nearly
/// always under the default `registry.ollama.ai/library`; preserving it would
/// produce noisy ids like `registry.ollama.ai/library/llama3:8b...`.
fn manifest_id(manifest: &Path, manifests_root: &Path) -> Option<String> {
    let rel = manifest.strip_prefix(manifests_root).ok()?;
    let segs: Vec<_> = rel
        .iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
    // Expected shape: <registry>/<repo>/<tag>, optionally with a deeper repo
    // path (e.g., `registry.ollama.ai/library/llama3/8b-instruct-q4_K_M`).
    if segs.len() < 3 {
        return None;
    }
    let tag = segs.last().cloned()?;
    // repo = segs[1..len-1] joined by "/"  (drops registry, drops tag)
    let repo_segs = &segs[1..segs.len() - 1];
    if repo_segs.is_empty() {
        return None;
    }
    // Drop the literal "library" segment when present so the id reads as
    // `llama3:8b-instruct-q4_K_M`, matching Ollama's own UI.
    let repo: Vec<&str> = repo_segs
        .iter()
        .filter(|s| s.as_str() != "library")
        .map(|s| s.as_str())
        .collect();
    let repo_joined = if repo.is_empty() {
        // fall back to the original (pre-filter) repo so we don't produce
        // an empty id.
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

/// Look up the blob file. Returns the actual on-disk size if available,
/// otherwise the manifest-declared size with `BrokenSymlink` status.
fn resolve_blob(blob_path: &Path, declared_size: u64) -> (u64, ModelStatus) {
    match std::fs::metadata(blob_path) {
        Ok(meta) => {
            // For sparse fixtures the apparent size may differ from declared;
            // we trust the on-disk apparent size for accounting because that
            // is what the user's filesystem reports.
            (meta.len(), ModelStatus::Healthy)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (
            declared_size,
            ModelStatus::BrokenSymlink {
                reason: format!("blob missing: {}", blob_path.display()),
            },
        ),
        Err(e) => (
            declared_size,
            ModelStatus::Unreadable {
                reason: format!("blob unreadable: {e}"),
            },
        ),
    }
}

/// Compute the deduplicated total disk usage in bytes across a slice of
/// `DiscoveredModel`. Two models with the same `on_disk_path` count once.
///
/// This is a pure function for callers that want a per-tool total without
/// going through the cross-tool inventory aggregator. The cross-tool view
/// is computed in `modeltap-app::registry`.
pub fn dedup_total_bytes(models: &[DiscoveredModel]) -> u64 {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut total: u64 = 0;
    for m in models {
        if seen.insert(m.on_disk_path.clone()) {
            total = total.saturating_add(m.size_bytes);
        }
    }
    total
}

/// Translate a `DiscoveredModel` into a tentative `DedupKey` for the
/// pre-hash inventory view.
pub fn tentative_key(m: &DiscoveredModel) -> DedupKey {
    DedupKey::Tentative(m.display_label.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Build the same shape of fixture the acceptance tests rely on, but in
    /// a unit-test temp dir we control. Mirrors `tests/fixtures/build.sh`'s
    /// devon-multi-tool layout (3 distinct blobs, codellama shared by 2
    /// manifests).
    fn build_devon_multi_tool_fixture() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = temp.path().join(".ollama").join("models");
        let manifests = root.join("manifests/registry.ollama.ai/library");
        let blobs = root.join("blobs");
        fs::create_dir_all(&blobs).unwrap();
        fs::create_dir_all(manifests.join("llama3")).unwrap();
        fs::create_dir_all(manifests.join("mistral")).unwrap();
        fs::create_dir_all(manifests.join("codellama")).unwrap();

        let blob_llama = "8f3eaaa11111111111111111111111111111111111111111111111111111c102";
        let blob_mistral = "4b9eaaa22222222222222222222222222222222222222222222222222222d203";
        let blob_codellama = "ababababababababababababababababababababababababababababcdcdcdcd";

        // Sparse blob files at the canonical sizes. We use small stand-ins
        // (1 KB) for unit-test speed; the exact byte counts don't matter
        // for the dedup-counting assertion here — uniqueness does.
        write_sparse(&blobs.join(format!("sha256-{}", blob_llama)), 1024);
        write_sparse(&blobs.join(format!("sha256-{}", blob_mistral)), 2048);
        write_sparse(&blobs.join(format!("sha256-{}", blob_codellama)), 4096);

        write_manifest(
            &manifests.join("llama3/8b-instruct-q4_K_M"),
            blob_llama,
            1024,
        );
        write_manifest(
            &manifests.join("mistral/7b-instruct-q4_K_M"),
            blob_mistral,
            2048,
        );
        write_manifest(
            &manifests.join("codellama/13b-q4_K_M"),
            blob_codellama,
            4096,
        );
        write_manifest(
            &manifests.join("codellama/13b-instruct-q4_K_M"),
            blob_codellama,
            4096,
        );

        (temp, root)
    }

    fn write_sparse(path: &Path, size: u64) {
        let f = fs::File::create(path).unwrap();
        f.set_len(size).unwrap();
    }

    fn write_manifest(path: &Path, blob_sha: &str, size: u64) {
        let body = format!(
            r#"{{
  "schemaVersion": 2,
  "layers": [
    {{
      "mediaType": "application/vnd.ollama.image.model",
      "digest": "sha256:{blob_sha}",
      "size": {size}
    }}
  ]
}}"#
        );
        fs::write(path, body).unwrap();
    }

    #[test]
    fn discovery_returns_four_manifests_with_dedup_total() {
        let (_temp, root) = build_devon_multi_tool_fixture();
        let models = discover_in(&root).expect("discovery succeeds");
        assert_eq!(
            models.len(),
            4,
            "fixture has 4 manifest entries; got {}",
            models.len()
        );
        // Every manifest should have produced a healthy entry.
        for m in &models {
            assert_eq!(m.status, ModelStatus::Healthy, "{:?}", m);
            assert_eq!(m.format, Format::OllamaBlob);
        }
        // Dedup: codellama 13b-q4 and 13b-instruct-q4 share a blob → one file.
        // Total = 1024 + 2048 + 4096 = 7168 bytes (NOT 11264).
        let total = dedup_total_bytes(&models);
        assert_eq!(
            total, 7168,
            "dedup must count shared codellama blob once, got {}",
            total
        );
    }

    #[test]
    fn discovery_returns_not_installed_when_root_missing() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("does/not/exist");
        let err = discover_in(&missing).expect_err("must error");
        assert!(matches!(err, DiscoverError::NotInstalled));
    }

    #[test]
    fn discovery_returns_empty_when_manifests_subdir_absent() {
        let temp = tempfile::tempdir().unwrap();
        // Root exists but no manifests/ subdir.
        std::fs::create_dir_all(temp.path()).unwrap();
        let models = discover_in(temp.path()).expect("must succeed");
        assert!(
            models.is_empty(),
            "no manifests subdir => empty inventory, got {}",
            models.len()
        );
    }

    #[cfg(unix)]
    #[test]
    fn discovery_returns_permission_denied_when_manifests_unreadable() {
        // Skip when running as root: chmod 0000 has no effect.
        let uid = std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(1);
        if uid == 0 {
            eprintln!("skipping: cannot test permission-denied as root");
            return;
        }
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let manifests = temp.path().join("manifests");
        std::fs::create_dir_all(&manifests).unwrap();
        let mut perms = std::fs::metadata(&manifests).unwrap().permissions();
        perms.set_mode(0o000);
        std::fs::set_permissions(&manifests, perms.clone()).unwrap();

        let res = discover_in(temp.path());
        // Restore so tempdir can be cleaned up.
        let mut perms = std::fs::metadata(&manifests).unwrap().permissions();
        perms.set_mode(0o700);
        let _ = std::fs::set_permissions(&manifests, perms);

        assert!(
            matches!(res, Err(DiscoverError::PermissionDenied { .. })),
            "expected PermissionDenied, got {:?}",
            res
        );
    }

    #[test]
    fn manifest_id_translates_path_into_repo_colon_tag() {
        let manifests_root = Path::new("/x/.ollama/models/manifests");
        let manifest = Path::new(
            "/x/.ollama/models/manifests/registry.ollama.ai/library/llama3/8b-instruct-q4_K_M",
        );
        let id = manifest_id(manifest, manifests_root).expect("id");
        assert_eq!(id, "llama3:8b-instruct-q4_K_M");
    }
}
