//! Synchronous discovery walker for the Atomic Chat plugin.
//!
//! Atomic Chat's `<root>/<model-id>/` directories each carry a `model.yml`
//! manifest declaring the model's name, on-disk size, and a relative path to
//! the GGUF blob. The walk is therefore depth-2: enumerate the top-level
//! children of each search root, and for each child that holds a `model.yml`
//! parse it and emit a `DiscoveredModel`.
//!
//! Behavior contract:
//! - Every configured root missing → `Err(DiscoverError::NotInstalled)`.
//! - At least one root exists with model dirs → `Ok(Vec<DiscoveredModel>)`.
//! - A model dir whose `model.yml` is malformed → emitted as a single entry
//!   with `format = Format::Other` and `status = ModelStatus::Corrupt` so
//!   the user sees the broken model in the right pane without aborting the
//!   whole scan. Per US-15's pattern, a single bad file does NOT taint the
//!   tool's overall status.
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

use modeltap_core::{DiscoverError, DiscoveredModel, DisplayLabel, Format, ModelStatus};
use walkdir::WalkDir;

use crate::manifest::{parse_model_yml, ModelYml};

/// Walk every configured search path and return one `DiscoveredModel` per
/// `<root>/<id>/model.yml`. NEVER panics.
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
        walk_one_root(root, &mut models)?;
    }

    // Stable order — id_in_tool is the model name from model.yml.
    models.sort_by(|a, b| a.id_in_tool.cmp(&b.id_in_tool));
    Ok(models)
}

/// Walk one root directly (depth = 1: each child is a model dir; the
/// `model.yml` and `model.gguf` live one level down).
fn walk_one_root(root: &Path, out: &mut Vec<DiscoveredModel>) -> Result<(), DiscoverError> {
    // Use WalkDir with depth 1 to enumerate model directories. WalkDir bubbles
    // unreadable-root errors via the first iterator yield.
    let mut child_seen = false;
    let mut first_err: Option<walkdir::Error> = None;

    for entry in WalkDir::new(root).max_depth(1).follow_links(false) {
        match entry {
            Ok(e) => {
                if e.depth() == 0 {
                    continue;
                }
                child_seen = true;
                if !e.file_type().is_dir() {
                    continue;
                }
                if let Some(model) = read_model_dir(e.path()) {
                    out.push(model);
                }
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                } else {
                    tracing::debug!(target: "modeltap.atomic_chat", "walkdir error: {e}");
                }
            }
        }
    }

    if !child_seen {
        if let Some(e) = first_err {
            let io = e
                .into_io_error()
                .unwrap_or_else(|| std::io::Error::other("walkdir failed to read root"));
            return Err(DiscoverError::Io(io));
        }
    }

    Ok(())
}

/// Inspect `<dir>/model.yml`. Returns `None` if no manifest is present (the
/// directory is not an Atomic Chat model dir — silently ignored). Returns a
/// `DiscoveredModel` with `Corrupt` status if the manifest exists but is
/// unparseable.
fn read_model_dir(dir: &Path) -> Option<DiscoveredModel> {
    let manifest = dir.join("model.yml");
    let bytes = match std::fs::read(&manifest) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(
                target: "modeltap.atomic_chat",
                "read {}: {e}",
                manifest.display()
            );
            // Surface the bad dir as a Corrupt entry keyed on the dir name —
            // the user sees something rather than a silent skip.
            let id = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            return Some(corrupt_entry(id, dir.join("model.gguf")));
        }
    };

    match parse_model_yml(&bytes) {
        Ok(parsed) => Some(build_healthy(dir, parsed)),
        Err(e) => {
            tracing::warn!(
                target: "modeltap.atomic_chat",
                "parse {}: {e}",
                manifest.display()
            );
            let id = dir
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            Some(corrupt_entry(id, dir.join("model.gguf")))
        }
    }
}

/// Build a healthy `DiscoveredModel` from a parsed manifest. The
/// `on_disk_path` is resolved by joining `model_path` (relative to the
/// Atomic Chat data root) onto the directory two levels up from the model
/// dir — i.e. the `<data>` root that contains `llamacpp/models/<id>/`.
///
/// The data root is derived from the model dir's path: `<data>/llamacpp/models/<id>`.
/// We strip the trailing `llamacpp/models/<id>` to get `<data>` and then join
/// the manifest's `model_path` (which is already prefixed with
/// `llamacpp/models/<id>/`).
fn build_healthy(dir: &Path, parsed: ModelYml) -> DiscoveredModel {
    let on_disk_path = resolve_model_path(dir, &parsed.model_path);
    DiscoveredModel {
        id_in_tool: parsed.name.clone(),
        on_disk_path,
        size_bytes: parsed.size_bytes,
        format: Format::Gguf,
        display_label: DisplayLabel::from(parsed.name),
        status: ModelStatus::Healthy,
    }
}

/// Resolve the absolute on-disk GGUF path. Strategy:
///
/// 1. If the model dir itself contains `model.gguf`, prefer that — it's the
///    canonical path Jan/Atomic Chat actually opens.
/// 2. Otherwise, climb two levels up from the model dir (out of
///    `llamacpp/models/`) to find the data root and join `model_path`.
fn resolve_model_path(model_dir: &Path, declared: &str) -> PathBuf {
    let local = model_dir.join("model.gguf");
    if local.exists() {
        return local;
    }

    // <data>/llamacpp/models/<id> -> <data>
    let data_root = model_dir
        .parent() // models
        .and_then(|p| p.parent()) // llamacpp
        .and_then(|p| p.parent()); // data

    match data_root {
        Some(root) => root.join(declared),
        None => model_dir.join(declared),
    }
}

fn corrupt_entry(id: String, on_disk_path: PathBuf) -> DiscoveredModel {
    DiscoveredModel {
        id_in_tool: id.clone(),
        on_disk_path,
        size_bytes: 0,
        format: Format::Other,
        display_label: DisplayLabel::from(id),
        status: ModelStatus::Corrupt {
            reason: "model.yml could not be parsed".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_model(root: &Path, id: &str, size: u64) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("model.yml"),
            format!(
                "embedding: false\nmodel_path: llamacpp/models/{id}/model.gguf\nname: {id}\nsize_bytes: {size}\n"
            ),
        )
        .unwrap();
        // Sparse-ish "model.gguf" so the local-resolution branch finds it.
        std::fs::write(dir.join("model.gguf"), b"GGUF").unwrap();
    }

    /// Behavior — populated tree yields one entry per <id>/model.yml.
    #[test]
    fn discover_returns_healthy_models_for_valid_yml() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        write_model(&root, "Qwen3-7B", 7_000_000_000);
        write_model(&root, "Llama-3-8B", 8_000_000_000);

        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 2, "expected 2 entries; got {:?}", models);
        for m in &models {
            assert_eq!(m.format, Format::Gguf);
            assert_eq!(m.status, ModelStatus::Healthy);
            assert!(m.size_bytes > 0);
        }
        // Sorted by id_in_tool.
        assert_eq!(models[0].id_in_tool, "Llama-3-8B");
        assert_eq!(models[1].id_in_tool, "Qwen3-7B");
    }

    /// Behavior — id_in_tool is the manifest's `name` field.
    #[test]
    fn discover_id_is_the_manifest_name_field() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        write_model(&root, "Qwen3_5-35B-A3B-Q4_K_M", 22_915_306_816);

        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id_in_tool, "Qwen3_5-35B-A3B-Q4_K_M");
        assert_eq!(models[0].size_bytes, 22_915_306_816);
    }

    /// Behavior — every root missing → NotInstalled.
    #[test]
    fn discover_returns_not_installed_when_all_roots_missing() {
        let err = discover_in(&[PathBuf::from("/nonexistent/no-such-atomic-chat")])
            .expect_err("must error");
        assert!(matches!(err, DiscoverError::NotInstalled));
    }

    /// Behavior — empty existing root → Ok(empty), NOT NotInstalled.
    #[test]
    fn discover_returns_empty_ok_when_root_exists_but_holds_no_models() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        let models = discover_in(&[root]).expect("ok");
        assert!(models.is_empty());
    }

    /// Behavior — a model dir with a malformed `model.yml` becomes a
    /// `Corrupt` entry. The good model alongside it is unaffected.
    #[test]
    fn discover_emits_corrupt_entry_for_malformed_yml() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        write_model(&root, "good-model", 1_000);

        // Malformed sibling.
        let bad = root.join("bad-model");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("model.yml"), b"name: bad\nsize_byt").unwrap();

        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 2, "expected 1 healthy + 1 corrupt");
        let healthy = models.iter().find(|m| m.id_in_tool == "good-model");
        let corrupt = models.iter().find(|m| m.id_in_tool == "bad-model");
        assert!(healthy.is_some(), "good model must still be discovered");
        let corrupt = corrupt.expect("bad model must surface as Corrupt entry");
        assert!(matches!(corrupt.status, ModelStatus::Corrupt { .. }));
        assert_eq!(corrupt.format, Format::Other);
    }

    /// Behavior — directories without a `model.yml` are silently skipped
    /// (they're not Atomic Chat model dirs).
    #[test]
    fn discover_skips_dirs_without_model_yml() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        write_model(&root, "real", 1_000);
        // Distractor dir — no model.yml.
        std::fs::create_dir_all(root.join("not-a-model")).unwrap();
        std::fs::write(root.join("not-a-model").join("README"), b"hi").unwrap();

        let models = discover_in(&[root]).expect("ok");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id_in_tool, "real");
    }
}
