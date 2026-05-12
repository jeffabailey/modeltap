//! HF `Tool::delete_folder` happy-path tests — step 01-03.
//!
//! Covers the all-unique scenario (B-FGD-2, M1): every model file in the
//! folder is unique to HF, every sidecar is a non-model file, no EBUSY, no
//! shared files. The plugin produces one [`DeleteOutcome`] per file and
//! sweeps the now-empty `models--<author>--<repo>/` tree.
//!
//! Shared-file classification (paths_to_unlink_hf_only) and partial-failure
//! behavior land in subsequent steps (03 and 04 respectively).
//!
//! Cross-refs:
//! - ADR-010 §"Implementation Guidance" — unlink loop contract.
//! - ADR-009 — `delete_one_at` ref-counting reused for model files.
//! - `architecture-design.md` §4.4 — `HfPlugin::delete_folder` decomposition.
//! - `plugin-contract-spec.md` §§3.11.S.1 / 3.11.S.3 / 3.11.S.7.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use modeltap_core::types::{
    FolderClassification, FolderDeletePlan, FolderGroup, Sidecar, SidecarKind,
};
use modeltap_core::{DedupKey, DisplayLabel, Format, ModelMeta, ModelStatus, Tool, ToolId};
use modeltap_plugin_hf::folder_delete::enumerate_sidecars;
use modeltap_plugin_hf::HfPlugin;

// ---------------------------------------------------------------------------
// Test 1: delete_folder returns one outcome per file (all-unique happy path)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_folder_returns_one_outcome_per_file_for_all_unique_folder() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_unique_folder_fixture(temp.path());

    let plan = build_plan(&fixture);
    let plugin = HfPlugin::new_with_hub_root(fixture.hub.clone());

    let outcomes = plugin
        .delete_folder(&plan)
        .await
        .expect("delete_folder must succeed on all-unique happy path");

    // One outcome per file (2 models + 3 sidecars in this fixture).
    assert_eq!(
        outcomes.len(),
        fixture.expected_outcome_count(),
        "expected one DeleteOutcome per file (models + sidecars), got {outcomes:?}",
    );
    // Every outcome is tagged with `ToolId("hf")`.
    for o in &outcomes {
        assert_eq!(o.tool, ToolId("hf"));
    }
    // Every outcome reports the registration removed AND the file deleted on
    // the all-unique happy path.
    for o in &outcomes {
        assert!(
            o.registration_removed,
            "registration_removed must be true on happy path, got {o:?}",
        );
        assert!(
            o.file_deleted,
            "file_deleted must be true on the all-unique happy path, got {o:?}",
        );
    }
    // Sum of bytes_freed equals the plan's bytes_to_reclaim (INT-FGD-3).
    let bytes_freed: u64 = outcomes.iter().map(|o| o.bytes_freed).sum();
    assert_eq!(
        bytes_freed, plan.bytes_to_reclaim,
        "sum(bytes_freed) must equal plan.bytes_to_reclaim",
    );
    // Snapshot symlinks and blobs are gone.
    for snap in &fixture.snap_files {
        assert!(!snap.exists(), "snapshot symlink {snap:?} must be removed");
    }
    for blob in &fixture.blobs {
        assert!(!blob.exists(), "blob {blob:?} must be removed (all-unique)");
    }
    for s in &fixture.sidecar_paths {
        assert!(!s.exists(), "sidecar {s:?} must be removed");
    }
}

// ---------------------------------------------------------------------------
// Test 2: delete_folder removes the now-empty repo tree
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_folder_removes_empty_repo_tree() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_unique_folder_fixture(temp.path());

    let plan = build_plan(&fixture);
    let plugin = HfPlugin::new_with_hub_root(fixture.hub.clone());

    let _outcomes = plugin
        .delete_folder(&plan)
        .await
        .expect("delete_folder must succeed");

    assert!(
        !fixture.repo_dir.exists(),
        "models--<author>--<repo>/ tree must be removed after all files unlinked, but {:?} still exists",
        fixture.repo_dir,
    );
}

// ---------------------------------------------------------------------------
// Test 3: enumerate_sidecars discovers README.md, .imatrix, .gguf.urls, and
//         HF-internal refs/blobs entries
// ---------------------------------------------------------------------------

#[test]
fn enumerate_sidecars_finds_readme_imatrix_urls_and_refs_blobs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo_dir = temp
        .path()
        .join("models--bartowski--Llama-3.2-1B-Instruct-GGUF");
    let snap_dir = repo_dir.join("snapshots/abc123");
    let blobs_dir = repo_dir.join("blobs");
    let refs_dir = repo_dir.join("refs");
    fs::create_dir_all(&snap_dir).unwrap();
    fs::create_dir_all(&blobs_dir).unwrap();
    fs::create_dir_all(&refs_dir).unwrap();

    // Sidecars that are physically inside the snapshot directory (sit
    // alongside the model files).
    write_with_size(&snap_dir.join("README.md"), 256);
    write_with_size(&snap_dir.join("imatrix.dat.imatrix"), 1024);
    write_with_size(&snap_dir.join("model.gguf.urls"), 64);

    // HF-internal: refs/main is a text file with the rev sha; blobs/<sha>
    // entries are exclusive to this repo's snapshot.
    write_with_size(&refs_dir.join("main"), 40);
    write_with_size(&blobs_dir.join("0123abcd"), 4096);

    // A model file that MUST NOT be enumerated as a sidecar — it is a model.
    let model_blob = blobs_dir.join("model-blob");
    write_with_size(&model_blob, 8192);
    let model_symlink = snap_dir.join("model.safetensors");
    symlink("../../blobs/model-blob", &model_symlink).unwrap();

    let sidecars = enumerate_sidecars(&repo_dir, &[model_blob.clone(), model_symlink.clone()]);

    let names: Vec<String> = sidecars
        .iter()
        .map(|s| s.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().any(|n| n == "README.md"),
        "expected README.md sidecar; got {names:?}",
    );
    assert!(
        names.iter().any(|n| n.ends_with(".imatrix")),
        "expected .imatrix sidecar; got {names:?}",
    );
    assert!(
        names.iter().any(|n| n.ends_with(".gguf.urls")),
        "expected .gguf.urls sidecar; got {names:?}",
    );
    // HF-internal refs/main MUST be enumerated.
    assert!(
        sidecars
            .iter()
            .any(|s| s.path.ends_with("refs/main") && s.kind == SidecarKind::HfInternal),
        "expected refs/main as HfInternal sidecar; got {sidecars:?}",
    );
    // HF-internal blob entry that's NOT a model file MUST be enumerated.
    assert!(
        sidecars.iter().any(|s| s
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "0123abcd")
            .unwrap_or(false)
            && s.kind == SidecarKind::HfInternal),
        "expected blobs/0123abcd as HfInternal sidecar; got {sidecars:?}",
    );
    // The model files MUST NOT appear in the sidecar list.
    for s in &sidecars {
        assert!(
            s.path != model_blob,
            "model blob {model_blob:?} must not be classified as a sidecar",
        );
        assert!(
            s.path != model_symlink,
            "model symlink {model_symlink:?} must not be classified as a sidecar",
        );
    }
    // Every sidecar reports a positive size.
    for s in &sidecars {
        assert!(
            s.size_bytes > 0,
            "sidecar {s:?} must have positive size_bytes",
        );
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

struct UniqueFolderFixture {
    hub: PathBuf,
    repo_dir: PathBuf,
    blobs: Vec<PathBuf>,
    snap_files: Vec<PathBuf>,
    sidecar_paths: Vec<PathBuf>,
    folder: FolderGroup,
}

impl UniqueFolderFixture {
    fn expected_outcome_count(&self) -> usize {
        self.folder.models.len() + self.folder.sidecars.len()
    }
}

/// Build a realistic HF cache fixture under `root`:
///
/// ```text
/// hub/
///   models--bartowski--Llama-3.2-1B-Instruct-GGUF/
///     blobs/
///       blob-q4
///       blob-q8
///     snapshots/abc123/
///       model-q4.gguf      -> ../../blobs/blob-q4
///       model-q8.gguf      -> ../../blobs/blob-q8
///       README.md          (256 bytes — regular file sidecar)
///       imatrix.dat.imatrix
///       model.gguf.urls
///     refs/main            (40 bytes — HF-internal sidecar)
/// ```
fn build_unique_folder_fixture(root: &Path) -> UniqueFolderFixture {
    let hub = root.join("hub");
    let repo_dir = hub.join("models--bartowski--Llama-3.2-1B-Instruct-GGUF");
    let snap_dir = repo_dir.join("snapshots/abc123");
    let blobs_dir = repo_dir.join("blobs");
    let refs_dir = repo_dir.join("refs");
    fs::create_dir_all(&snap_dir).unwrap();
    fs::create_dir_all(&blobs_dir).unwrap();
    fs::create_dir_all(&refs_dir).unwrap();

    // Model blobs.
    let blob_q4 = blobs_dir.join("blob-q4");
    write_with_size(&blob_q4, 4096);
    let blob_q8 = blobs_dir.join("blob-q8");
    write_with_size(&blob_q8, 8192);

    // Snapshot symlinks.
    let snap_q4 = snap_dir.join("model-q4.gguf");
    let snap_q8 = snap_dir.join("model-q8.gguf");
    symlink("../../blobs/blob-q4", &snap_q4).unwrap();
    symlink("../../blobs/blob-q8", &snap_q8).unwrap();

    // Sidecars (in snapshot dir).
    let readme = snap_dir.join("README.md");
    write_with_size(&readme, 256);
    let imatrix = snap_dir.join("imatrix.dat.imatrix");
    write_with_size(&imatrix, 1024);
    let urls = snap_dir.join("model.gguf.urls");
    write_with_size(&urls, 64);
    // HF-internal: refs/main.
    let refs_main = refs_dir.join("main");
    write_with_size(&refs_main, 40);

    let model_q4 = ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: "bartowski/Llama-3.2-1B-Instruct-GGUF/model-q4.gguf".to_string(),
        on_disk_path: blob_q4.clone(),
        size_bytes: 4096,
        format: Format::Gguf,
        display_label: DisplayLabel::from("model-q4.gguf"),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from("model-q4.gguf")),
    };
    let model_q8 = ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: "bartowski/Llama-3.2-1B-Instruct-GGUF/model-q8.gguf".to_string(),
        on_disk_path: blob_q8.clone(),
        size_bytes: 8192,
        format: Format::Gguf,
        display_label: DisplayLabel::from("model-q8.gguf"),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from("model-q8.gguf")),
    };
    let sidecars = vec![
        Sidecar {
            path: readme.clone(),
            size_bytes: 256,
            kind: SidecarKind::Readme,
        },
        Sidecar {
            path: imatrix.clone(),
            size_bytes: 1024,
            kind: SidecarKind::Imatrix,
        },
        Sidecar {
            path: urls.clone(),
            size_bytes: 64,
            kind: SidecarKind::Urls,
        },
        Sidecar {
            path: refs_main.clone(),
            size_bytes: 40,
            kind: SidecarKind::HfInternal,
        },
    ];

    let folder = FolderGroup::new(
        "bartowski/Llama-3.2-1B-Instruct-GGUF".to_string(),
        repo_dir.clone(),
        ToolId("hf"),
        vec![model_q4, model_q8],
        sidecars,
    )
    .expect("fixture FolderGroup must construct");

    UniqueFolderFixture {
        hub,
        repo_dir,
        blobs: vec![blob_q4, blob_q8],
        snap_files: vec![snap_q4, snap_q8],
        sidecar_paths: vec![readme, imatrix, urls, refs_main],
        folder,
    }
}

fn build_plan(fixture: &UniqueFolderFixture) -> FolderDeletePlan {
    // All-unique: every model + every sidecar goes through paths_to_unlink_fully.
    // The plugin walks plan.folder.{models,sidecars}; the paths_to_unlink_*
    // vectors are byte-count book-keeping artifacts of the plan.
    let mut paths_to_unlink_fully: Vec<PathBuf> = fixture
        .folder
        .models
        .iter()
        .map(|m| m.on_disk_path.clone())
        .collect();
    paths_to_unlink_fully.extend(fixture.folder.sidecars.iter().map(|s| s.path.clone()));
    let bytes_to_reclaim = fixture.folder.total_bytes();
    let classification = FolderClassification {
        unique: fixture.folder.models.clone(),
        shared: vec![],
    };
    FolderDeletePlan::new(
        fixture.folder.clone(),
        classification,
        paths_to_unlink_fully,
        vec![],
        bytes_to_reclaim,
        0,
    )
    .expect("plan must construct (reclaim == total, retain == 0)")
}

fn write_with_size(path: &Path, size: u64) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let f = fs::File::create(path).unwrap();
    f.set_len(size).unwrap();
}
