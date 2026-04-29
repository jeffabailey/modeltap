//! Hugging Face cache layout walker.
//!
//! The HF hub cache is organized as:
//!
//! ```text
//! <hub>/
//!   models--<org>--<repo>/
//!     snapshots/<rev-sha>/<file>     ← relative symlink → ../../blobs/<sha256>
//!     blobs/<sha256>                  ← real content-addressed file
//!     refs/main                       ← text file with the rev sha
//! ```
//!
//! This module exposes a pure listing function: walk every
//! `models--<org>--<repo>/snapshots/<rev>/` directory and return the file
//! paths INSIDE those revision directories. It does NOT resolve symlinks —
//! that is `symlink_resolve`'s job. Separation of concerns keeps each piece
//! easy to test against a tmpdir fixture.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

/// One snapshot file the plugin must consider for emission. The directory
/// name encodes the org/repo (`models--<org>--<repo>`) and the path under
/// it tells us the revision and filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFile {
    /// The `models--<org>--<repo>` directory under `<hub>/`.
    pub repo_dir: PathBuf,
    /// The full path to the snapshot file (a symlink in production).
    pub file_path: PathBuf,
}

/// Walk `<hub>/` and return every snapshot file path. Files that are NOT
/// inside `models--*/snapshots/*/` are ignored — we only care about
/// snapshot artifacts. Hidden / temp / lock files at the snapshot level
/// (those starting with `.`) are skipped.
///
/// Never panics: walkdir errors are logged at debug and skipped.
pub fn list_snapshot_files(hub: &Path) -> Vec<SnapshotFile> {
    if !hub.exists() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let model_dirs = match std::fs::read_dir(hub) {
        Ok(rd) => rd,
        Err(e) => {
            tracing::debug!(target: "modeltap.hf", "read_dir({}) failed: {e}", hub.display());
            return out;
        }
    };
    for entry in model_dirs.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        // Only `models--<org>--<repo>` directories are HF model dirs.
        if !name.starts_with("models--") {
            continue;
        }
        let snapshots_dir = path.join("snapshots");
        if !snapshots_dir.exists() {
            continue;
        }
        // For each <rev-sha>/ directory, list its files (one level only —
        // HF snapshots are flat).
        for snap_entry in WalkDir::new(&snapshots_dir)
            .min_depth(2) // <rev-sha>/<file>
            .max_depth(2)
            .follow_links(false)
        {
            let snap_entry = match snap_entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::debug!(target: "modeltap.hf", "walkdir error: {e}");
                    continue;
                }
            };
            // We accept both regular files AND symlinks (HF snapshots ARE
            // symlinks; walkdir reports them as `file_type().is_symlink()`).
            let ft = snap_entry.file_type();
            if !(ft.is_file() || ft.is_symlink()) {
                continue;
            }
            let file_path = snap_entry.path().to_path_buf();
            // Skip hidden / temp files.
            if file_path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.starts_with('.'))
            {
                continue;
            }
            out.push(SnapshotFile {
                repo_dir: path.clone(),
                file_path,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    fn write_file(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn list_snapshot_files_finds_files_under_models_repo_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path().join("hub");
        // models--meta-llama--Llama-3-8B/snapshots/rev1/model.safetensors
        let m1 = hub.join("models--meta-llama--Llama-3-8B");
        let snap1 = m1.join("snapshots/rev1");
        let blobs1 = m1.join("blobs");
        fs::create_dir_all(&snap1).unwrap();
        fs::create_dir_all(&blobs1).unwrap();
        write_file(&blobs1.join("blob-a"), b"x");
        symlink("../../blobs/blob-a", snap1.join("model.safetensors")).unwrap();
        // models--mistralai--Mistral-7B/snapshots/rev2/model.gguf
        let m2 = hub.join("models--mistralai--Mistral-7B");
        let snap2 = m2.join("snapshots/rev2");
        let blobs2 = m2.join("blobs");
        fs::create_dir_all(&snap2).unwrap();
        fs::create_dir_all(&blobs2).unwrap();
        write_file(&blobs2.join("blob-b"), b"y");
        symlink("../../blobs/blob-b", snap2.join("model.gguf")).unwrap();

        let files = list_snapshot_files(&hub);
        assert_eq!(files.len(), 2, "expected 2 snapshot files; got {:?}", files);
        let names: Vec<String> = files
            .iter()
            .map(|f| f.file_path.file_name().unwrap().to_string_lossy().into())
            .collect();
        assert!(names.iter().any(|n| n == "model.safetensors"));
        assert!(names.iter().any(|n| n == "model.gguf"));
    }

    #[test]
    fn list_snapshot_files_returns_empty_when_hub_missing() {
        let files = list_snapshot_files(Path::new("/nonexistent/no-such-hub"));
        assert!(files.is_empty());
    }

    #[test]
    fn list_snapshot_files_ignores_non_models_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path().join("hub");
        // A datasets-- directory is NOT a model dir; we must ignore it.
        let datasets = hub.join("datasets--foo--bar");
        let snap = datasets.join("snapshots/rev/data.parquet");
        write_file(&snap, b"x");
        // A loose file under hub/ is also not a model dir.
        write_file(&hub.join("README.md"), b"hi");
        let files = list_snapshot_files(&hub);
        assert!(
            files.is_empty(),
            "non-models-- dirs must be ignored; got {:?}",
            files
        );
    }
}
