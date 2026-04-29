//! Hugging Face cache discovery orchestrator.
//!
//! Composes `cache_walk::list_snapshot_files` with
//! `symlink_resolve::resolve_snapshot_target` to produce one
//! `DiscoveredModel` per snapshot file under `<HF_HOME>/hub/`. Per ADR-002
//! we do NOT compute SHA256 here — the blob's filename in the HF cache
//! IS its sha256, so the eager-hash work is already done by `huggingface_hub`
//! when the file was downloaded. We capture the on-disk size and let the
//! cross-tool aggregator decide what to do with the dedup key.
//!
//! Behavior contract (per US-12 acceptance criteria):
//! - `<hub>/` missing → `Err(DiscoverError::NotInstalled)` (AC: not-installed signal).
//! - `<hub>/` exists but holds no `models--*` dirs → `Ok(empty)` ("installed but empty").
//! - `models--<org>--<repo>/` directory name → id `<org>/<repo>` (AC-3).
//! - File suffix → format (AC-4): `.gguf`/`.safetensors`/`.bin`/`.awq`/`.gptq` known,
//!   anything else → `Format::Other`.
//! - Broken snapshot symlink → `DiscoveredModel` with `ModelStatus::BrokenSymlink`
//!   and `size_bytes = 0` so it is excluded from disk totals (AC-5).
//!
//! Object-Calisthenics scope: this module is on the adapter side of the
//! hexagon. Strict OC rules are relaxed.

use std::path::{Path, PathBuf};

use modeltap_core::{DiscoverError, DiscoveredModel, DisplayLabel, Format, ModelStatus};

use crate::cache_walk::{list_snapshot_files, SnapshotFile};
use crate::symlink_resolve::{resolve_snapshot_target, Resolved};

/// Walk the HF hub root and emit one `DiscoveredModel` per snapshot file.
///
/// `hub_root` is the directory containing `models--*` subdirs — i.e.,
/// `<HF_HOME>/hub/`, NOT `<HF_HOME>` itself.
pub fn discover_in(hub_root: &Path) -> Result<Vec<DiscoveredModel>, DiscoverError> {
    if !hub_root.exists() {
        return Err(DiscoverError::NotInstalled);
    }
    let snaps = list_snapshot_files(hub_root);
    let mut out = Vec::with_capacity(snaps.len());
    for snap in &snaps {
        out.push(build_discovered_model(snap));
    }
    // Stable order so launch.inventory is deterministic.
    out.sort_by(|a, b| a.id_in_tool.cmp(&b.id_in_tool));
    Ok(out)
}

fn build_discovered_model(snap: &SnapshotFile) -> DiscoveredModel {
    let repo_id = repo_id_from_dir(&snap.repo_dir).unwrap_or_else(|| {
        // Defensive: surface the raw dir name rather than panicking.
        snap.repo_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    });
    let filename = snap
        .file_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    // The `id_in_tool` includes the filename so two artifacts under the
    // same repo (model.safetensors + config.json) are distinct rows. The
    // assertion in the AC-3 acceptance test only requires `starts_with
    // "<org>/<repo>"`, which this satisfies.
    let id_in_tool = if filename.is_empty() {
        repo_id.clone()
    } else {
        format!("{repo_id}/{filename}")
    };
    let format = format_from_suffix(filename);
    let display_label = DisplayLabel::from(id_in_tool.clone());

    match resolve_snapshot_target(&snap.file_path) {
        Resolved::Ok {
            target_path,
            size_bytes,
        } => DiscoveredModel {
            id_in_tool,
            on_disk_path: target_path,
            size_bytes,
            format,
            display_label,
            status: ModelStatus::Healthy,
        },
        Resolved::Broken { reason } => DiscoveredModel {
            id_in_tool,
            on_disk_path: snap.file_path.clone(),
            // Size 0 → does not contribute to dedup_total_bytes (AC-5).
            size_bytes: 0,
            format,
            display_label,
            status: ModelStatus::BrokenSymlink { reason },
        },
    }
}

/// Translate a `models--<org>--<repo>` directory name into the canonical
/// HF id `<org>/<repo>`. Returns `None` if the directory name does not
/// match the expected encoding.
///
/// The split is on the FIRST `--` after the `models--` prefix. If the repo
/// name itself contains `--` (very rare; HF allows it), we keep them in the
/// repo segment.
pub fn repo_id_from_dir(repo_dir: &Path) -> Option<String> {
    let name = repo_dir.file_name()?.to_str()?;
    let rest = name.strip_prefix("models--")?;
    // First `--` separates org from repo.
    let (org, repo) = rest.split_once("--")?;
    if org.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{org}/{repo}"))
}

/// Map a snapshot filename's suffix (case-insensitive) to a `Format`.
/// Unknown suffix → `Format::Other`.
pub fn format_from_suffix(filename: &str) -> Format {
    let suffix = match filename.rsplit_once('.') {
        Some((_, s)) => s.to_ascii_lowercase(),
        None => return Format::Other,
    };
    match suffix.as_str() {
        "gguf" => Format::Gguf,
        "safetensors" => Format::Safetensors,
        "bin" => Format::Bin,
        "awq" => Format::Awq,
        "gptq" => Format::Gptq,
        _ => Format::Other,
    }
}

/// Resolve the HF hub root from the process environment.
///
/// 1. `HF_HOME` env var → `<HF_HOME>/hub/` (the HF convention).
/// 2. Default: `$HOME/.cache/huggingface/hub/` (XDG-style on macOS + Linux).
/// 3. No `$HOME`: returns a sentinel non-existent path so `discover_in` will
///    surface `DiscoverError::NotInstalled` cleanly.
pub fn resolve_hub_root() -> PathBuf {
    if let Some(hf_home) = std::env::var_os("HF_HOME") {
        return PathBuf::from(hf_home).join("hub");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".cache")
            .join("huggingface")
            .join("hub");
    }
    PathBuf::from("/nonexistent/no-such-hf-cache/hub")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    #[test]
    fn repo_id_meta_llama_llama3() {
        let dir = Path::new("/x/hub/models--meta-llama--Llama-3-8B");
        assert_eq!(
            repo_id_from_dir(dir).unwrap(),
            "meta-llama/Llama-3-8B".to_string()
        );
    }

    #[test]
    fn repo_id_returns_none_for_non_models_dir() {
        let dir = Path::new("/x/hub/datasets--foo--bar");
        assert!(repo_id_from_dir(dir).is_none());
        let dir2 = Path::new("/x/hub/notamodel");
        assert!(repo_id_from_dir(dir2).is_none());
    }

    #[test]
    fn format_inference_covers_known_and_unknown_suffixes() {
        // Parametrized over every AC-4 suffix variant.
        let cases = [
            ("model.gguf", Format::Gguf),
            ("model.safetensors", Format::Safetensors),
            ("model.bin", Format::Bin),
            ("model.awq", Format::Awq),
            ("model.gptq", Format::Gptq),
            ("config.json", Format::Other),
            ("MODEL.SAFETENSORS", Format::Safetensors), // case-insensitive
            ("README", Format::Other),                  // no suffix
        ];
        for (name, expected) in cases {
            assert_eq!(
                format_from_suffix(name),
                expected,
                "format_from_suffix({:?})",
                name
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn discover_in_emits_one_entry_per_snapshot_file_with_correct_id_and_format() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path().join("hub");
        // meta-llama/Llama-3-8B with .safetensors snapshot.
        let m1 = hub.join("models--meta-llama--Llama-3-8B");
        let snap1 = m1.join("snapshots/rev1");
        let blobs1 = m1.join("blobs");
        fs::create_dir_all(&snap1).unwrap();
        fs::create_dir_all(&blobs1).unwrap();
        let blob_a = blobs1.join("blob-a");
        let f = fs::File::create(&blob_a).unwrap();
        f.set_len(1024).unwrap();
        symlink("../../blobs/blob-a", snap1.join("model.safetensors")).unwrap();
        // mistralai/Mistral-7B with .gguf.
        let m2 = hub.join("models--mistralai--Mistral-7B");
        let snap2 = m2.join("snapshots/rev2");
        let blobs2 = m2.join("blobs");
        fs::create_dir_all(&snap2).unwrap();
        fs::create_dir_all(&blobs2).unwrap();
        let blob_b = blobs2.join("blob-b");
        let f = fs::File::create(&blob_b).unwrap();
        f.set_len(2048).unwrap();
        symlink("../../blobs/blob-b", snap2.join("model.gguf")).unwrap();

        let models = discover_in(&hub).expect("discover ok");
        assert_eq!(models.len(), 2, "expected 2 entries; got {:?}", models);
        // ids are <org>/<repo>/<filename>.
        let ids: Vec<&str> = models.iter().map(|m| m.id_in_tool.as_str()).collect();
        assert!(
            ids.iter()
                .any(|i| i == &"meta-llama/Llama-3-8B/model.safetensors"),
            "got {:?}",
            ids
        );
        assert!(
            ids.iter().any(|i| i == &"mistralai/Mistral-7B/model.gguf"),
            "got {:?}",
            ids
        );
        // Formats inferred correctly.
        let safet = models
            .iter()
            .find(|m| m.format == Format::Safetensors)
            .unwrap();
        assert_eq!(safet.size_bytes, 1024);
        assert_eq!(safet.status, ModelStatus::Healthy);
        let gguf = models.iter().find(|m| m.format == Format::Gguf).unwrap();
        assert_eq!(gguf.size_bytes, 2048);
    }

    #[cfg(unix)]
    #[test]
    fn discover_in_flags_broken_symlinks_and_excludes_size() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path().join("hub");
        // Healthy entry first.
        let m1 = hub.join("models--good--repo");
        let snap1 = m1.join("snapshots/rev1");
        let blobs1 = m1.join("blobs");
        fs::create_dir_all(&snap1).unwrap();
        fs::create_dir_all(&blobs1).unwrap();
        let f = fs::File::create(blobs1.join("blob-a")).unwrap();
        f.set_len(8000).unwrap();
        symlink("../../blobs/blob-a", snap1.join("model.gguf")).unwrap();
        // Broken entry — points at a missing blob.
        let m2 = hub.join("models--broken--repo");
        let snap2 = m2.join("snapshots/rev2");
        fs::create_dir_all(&snap2).unwrap();
        // No blobs/ dir — symlink target is missing.
        symlink("../../blobs/nope", snap2.join("model.bin")).unwrap();

        let models = discover_in(&hub).expect("discover ok");
        assert_eq!(models.len(), 2, "both entries must be listed");
        let broken = models
            .iter()
            .find(|m| m.id_in_tool.starts_with("broken/repo"))
            .expect("broken entry present");
        assert!(
            matches!(broken.status, ModelStatus::BrokenSymlink { .. }),
            "broken entry must have ModelStatus::BrokenSymlink; got {:?}",
            broken.status
        );
        // AC-5: broken symlink size MUST be 0 so it does not contribute to
        // disk totals.
        assert_eq!(
            broken.size_bytes, 0,
            "broken entry size must be 0 to exclude from totals; got {}",
            broken.size_bytes
        );
    }

    #[test]
    fn discover_in_returns_not_installed_when_hub_missing() {
        let err =
            discover_in(Path::new("/nonexistent/no-such-hub")).expect_err("must error on missing");
        assert!(matches!(err, DiscoverError::NotInstalled));
    }

    #[test]
    fn discover_in_returns_empty_when_hub_exists_but_no_models() {
        let temp = tempfile::tempdir().unwrap();
        let hub = temp.path().join("hub");
        fs::create_dir_all(&hub).unwrap();
        let models = discover_in(&hub).expect("ok");
        assert!(models.is_empty(), "got {:?}", models);
    }
}
