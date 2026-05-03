//! Synchronous discovery walker for the GPT4All plugin.
//!
//! GPT4All stores models as flat `*.gguf` files under one or more configured
//! root directories (resolved by `crate::config`). The walk is therefore
//! depth-1 — no manifests, no subdirectory recursion.
//!
//! Behavior contract:
//! - Every configured root missing → `Err(DiscoverError::NotInstalled)`.
//! - At least one root exists → `Ok(Vec<DiscoveredModel>)` (possibly empty).
//! - Files whose extension is not `.gguf` (case-insensitive) are silently
//!   skipped — README.md, .DS_Store, downloads.json, etc.
//! - Dotfiles (`.foo`) are skipped.
//! - Subdirectories are NOT recursed (depth = 1).
//! - A per-file `metadata()` failure is logged and skipped — it does NOT
//!   abort the whole scan.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

use modeltap_core::{DiscoverError, DiscoveredModel, DisplayLabel, Format, ModelStatus};
use walkdir::WalkDir;

/// Walk every configured search path and return one `DiscoveredModel` per
/// `*.gguf` file in any existing root. NEVER panics.
pub fn discover_in(roots: &[PathBuf]) -> Result<Vec<DiscoveredModel>, DiscoverError> {
    let any_exists = roots.iter().any(|p| p.exists());
    if !any_exists {
        return Err(DiscoverError::NotInstalled);
    }

    let mut models = Vec::new();
    for root in roots {
        if !root.exists() {
            continue;
        }
        walk_one_root(root, &mut models);
    }

    // Stable order — id_in_tool is the GGUF filename.
    models.sort_by(|a, b| a.id_in_tool.cmp(&b.id_in_tool));
    Ok(models)
}

/// Walk one root at depth = 1 (top-level files only). Per-entry errors are
/// logged and skipped; the scan continues.
fn walk_one_root(root: &Path, out: &mut Vec<DiscoveredModel>) {
    for entry in WalkDir::new(root).max_depth(1).follow_links(false) {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(target: "modeltap.gpt4all", "walkdir entry error: {e}");
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }

        let file_name = match entry.file_name().to_str() {
            Some(n) => n,
            None => continue,
        };
        if file_name.starts_with('.') {
            continue;
        }
        if !file_name.to_ascii_lowercase().ends_with(".gguf") {
            continue;
        }

        let path = entry.path();
        let size_bytes = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(e) => {
                tracing::debug!(
                    target: "modeltap.gpt4all",
                    "metadata({}) failed: {e}",
                    path.display()
                );
                continue;
            }
        };

        let id_in_tool = file_name.to_string();
        let display_stem = file_name
            .strip_suffix(".gguf")
            .or_else(|| file_name.strip_suffix(".GGUF"))
            .unwrap_or(file_name)
            .to_string();
        let display_label = DisplayLabel::from(display_stem);

        out.push(DiscoveredModel {
            id_in_tool,
            on_disk_path: path.to_path_buf(),
            size_bytes,
            format: Format::Gguf,
            display_label,
            status: ModelStatus::Healthy,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_gguf(root: &Path, file_name: &str, contents: &[u8]) -> PathBuf {
        let path = root.join(file_name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// T1 — two .gguf files in one root → 2 models, sorted by id_in_tool.
    #[test]
    fn discover_returns_two_models_sorted_by_id() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        write_gguf(&root, "qwen2-7b.gguf", b"GGUFQ");
        write_gguf(&root, "llama3-8b.gguf", b"GGUFL");

        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 2, "expected 2 entries; got {:?}", models);
        assert_eq!(models[0].id_in_tool, "llama3-8b.gguf");
        assert_eq!(models[1].id_in_tool, "qwen2-7b.gguf");
        for m in &models {
            assert_eq!(m.format, Format::Gguf);
            assert_eq!(m.status, ModelStatus::Healthy);
        }
    }

    /// T2 — every configured root missing → NotInstalled.
    #[test]
    fn discover_returns_not_installed_when_all_roots_missing() {
        let err = discover_in(&[
            PathBuf::from("/nonexistent/no-such-gpt4all-1"),
            PathBuf::from("/nonexistent/no-such-gpt4all-2"),
        ])
        .expect_err("must error");
        assert!(matches!(err, DiscoverError::NotInstalled));
    }

    /// T3 — empty existing root → Ok(empty), NOT NotInstalled.
    #[test]
    fn discover_returns_empty_ok_when_root_exists_but_holds_no_models() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let models = discover_in(&[root]).expect("ok");
        assert!(models.is_empty());
    }

    /// T4 — non-gguf files (README.md, .DS_Store, downloads.json) silently skipped.
    /// Subdirectories are not recursed.
    #[test]
    fn discover_skips_non_gguf_files_and_dotfiles() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        write_gguf(&root, "real-model.gguf", b"GGUF");
        std::fs::write(root.join("README.md"), b"hi").unwrap();
        std::fs::write(root.join(".DS_Store"), b"\0\0").unwrap();
        std::fs::write(root.join("downloads.json"), b"{}").unwrap();
        // Subdirectory containing a .gguf — must NOT be recursed (depth=1).
        let sub = root.join("nested");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("buried.gguf"), b"GGUFx").unwrap();

        let models = discover_in(&[root]).expect("ok");
        assert_eq!(
            models.len(),
            1,
            "only top-level .gguf counts; got {:?}",
            models
        );
        assert_eq!(models[0].id_in_tool, "real-model.gguf");
    }

    /// T5 — two roots both populated → union, sorted.
    #[test]
    fn discover_unions_models_across_two_existing_roots() {
        let temp_a = tempfile::tempdir().unwrap();
        let temp_b = tempfile::tempdir().unwrap();
        let root_a = temp_a.path().to_path_buf();
        let root_b = temp_b.path().to_path_buf();
        write_gguf(&root_a, "alpha.gguf", b"A");
        write_gguf(&root_a, "charlie.gguf", b"C");
        write_gguf(&root_b, "bravo.gguf", b"B");

        let models = discover_in(&[root_a, root_b]).expect("ok");
        let ids: Vec<&str> = models.iter().map(|m| m.id_in_tool.as_str()).collect();
        assert_eq!(ids, vec!["alpha.gguf", "bravo.gguf", "charlie.gguf"]);
    }

    /// T6 — size_bytes matches std::fs::metadata().len().
    #[test]
    fn discover_size_bytes_matches_filesystem_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let payload = vec![0xABu8; 4_321];
        let path = write_gguf(&root, "weights.gguf", &payload);
        let expected_len = std::fs::metadata(&path).unwrap().len();

        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].size_bytes, expected_len);
        assert_eq!(models[0].size_bytes, payload.len() as u64);
    }
}
