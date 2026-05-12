//! HF `Tool::delete_folder` plugin-contract tests — step 03-02.
//!
//! Per `docs/feature/folder-group-bulk-delete/distill/plugin-contract-spec.md`
//! §3.11, the HF plugin satisfies the **Supported** capability path. This
//! file implements the 03-02 subset:
//!
//!   - 3.11.S.2 `test_delete_folder_mixed_shared_and_unique`
//!   - 3.11.S.6 `test_delete_folder_preserves_cross_tool_hardlinks`
//!   - 3.11.S.8 `test_delete_folder_only_sidecars`
//!
//! The all-unique happy path (3.11.S.1 / S.3 / S.7) is covered by
//! `folder_delete_happy_path.rs` (step 01-03). Partial-failure paths
//! (3.11.S.4 / S.5) land in step 04 per the deliver roadmap.
//!
//! Per architecture rule R2 (plugins do not depend on each other), we do NOT
//! depend on `modeltap-plugin-ollama` for the cross-tool hardlink: we build
//! the Ollama-side hardlink target in a sibling tempdir directly. The
//! plugin-contract spec §6 anticipates this via `MockOtherToolPlugin::
//! hardlink_shared_file`; for this step we inline the equivalent because the
//! mock surface lives in `modeltap-core::tests` and we only need the
//! hardlink-setup primitive (`std::fs::hard_link`).

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, MetadataExt};
use std::path::{Path, PathBuf};

use modeltap_core::types::{
    FolderClassification, FolderDeletePlan, FolderGroup, SharedModel, Sidecar, SidecarKind,
};
use modeltap_core::{DedupKey, DisplayLabel, Format, ModelMeta, ModelStatus, Tool, ToolId};
use modeltap_plugin_hf::HfPlugin;

// ---------------------------------------------------------------------------
// Sparse-file fixture builder primitives.
// ---------------------------------------------------------------------------

fn write_sparse(path: &Path, size: u64) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    let f = fs::File::create(path).expect("create file");
    f.set_len(size).expect("set_len");
}

fn write_with_prelude(path: &Path, size: u64, prelude: &[u8]) {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    let mut f = fs::File::create(path).expect("create file");
    f.write_all(prelude).expect("write prelude");
    if size > prelude.len() as u64 {
        f.set_len(size).expect("set_len");
    }
}

// ---------------------------------------------------------------------------
// 3.11.S.2 — Mixed shared + unique outcome shape.
//
// Setup: one HF repo containing 3 model files. File 1 (1 GB sparse) is
// unique. Files 2 and 3 (1 GB sparse each) are hardlinked into a sibling
// Ollama tempdir tree. 0 sidecars.
//
// Assertions (per plugin-contract-spec.md §3.11.S.2):
//   - delete_folder returns Ok(Vec<DeleteOutcome>) of length 3.
//   - File 1's entry: registration_removed=true, file_deleted=true,
//                     bytes_freed=<size>.
//   - File 2's entry: registration_removed=true, file_deleted=false,
//                     bytes_freed=0.
//   - File 3's entry: registration_removed=true, file_deleted=false,
//                     bytes_freed=0.
//   - HF-side paths for all 3 files no longer exist.
//   - Ollama-side paths for files 2 and 3 still exist and stat to the same
//     inode they had pre-delete.
//   - The models--<author>--<repo>/ directory tree is fully removed.
// ---------------------------------------------------------------------------

const REPO_PATH: &str = "bartowski/Test-Repo-Mixed";
const REPO_DIR_NAME: &str = "models--bartowski--Test-Repo-Mixed";
const REV_SHA: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

struct MixedFixture {
    _temp: tempfile::TempDir,
    hub: PathBuf,
    repo_dir: PathBuf,
    blob_unique: PathBuf,
    blob_shared_1: PathBuf,
    blob_shared_2: PathBuf,
    ollama_shared_1: PathBuf,
    ollama_shared_2: PathBuf,
}

fn build_mixed_fixture() -> MixedFixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let hub = root.join("hub");
    let repo_dir = hub.join(REPO_DIR_NAME);
    let blobs_dir = repo_dir.join("blobs");
    let snap_dir = repo_dir.join("snapshots").join(REV_SHA);
    fs::create_dir_all(&blobs_dir).expect("create blobs");
    fs::create_dir_all(&snap_dir).expect("create snap");

    let h_unique = "1111111111111111111111111111111111111111111111111111111111111111";
    let h_shared_1 = "2222222222222222222222222222222222222222222222222222222222222222";
    let h_shared_2 = "3333333333333333333333333333333333333333333333333333333333333333";
    let blob_unique = blobs_dir.join(h_unique);
    let blob_shared_1 = blobs_dir.join(h_shared_1);
    let blob_shared_2 = blobs_dir.join(h_shared_2);
    // Sparse 1 GB each — the OS reports apparent size but only metadata
    // is allocated.
    write_sparse(&blob_unique, 1_073_741_824);
    write_sparse(&blob_shared_1, 1_073_741_824);
    write_sparse(&blob_shared_2, 1_073_741_824);

    // Snapshot symlinks.
    let snap_unique = snap_dir.join("file-1.gguf");
    let snap_shared_1 = snap_dir.join("file-2.gguf");
    let snap_shared_2 = snap_dir.join("file-3.gguf");
    symlink(
        PathBuf::from("..").join("..").join("blobs").join(h_unique),
        &snap_unique,
    )
    .unwrap();
    symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(h_shared_1),
        &snap_shared_1,
    )
    .unwrap();
    symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(h_shared_2),
        &snap_shared_2,
    )
    .unwrap();

    // Cross-tool hardlinks under a sibling Ollama tempdir tree.
    let ollama_blobs = root.join(".ollama").join("models").join("blobs");
    fs::create_dir_all(&ollama_blobs).expect("create ollama blobs");
    let ollama_shared_1 = ollama_blobs.join(format!("sha256-{h_shared_1}"));
    let ollama_shared_2 = ollama_blobs.join(format!("sha256-{h_shared_2}"));
    fs::hard_link(&blob_shared_1, &ollama_shared_1).expect("hard_link shared_1");
    fs::hard_link(&blob_shared_2, &ollama_shared_2).expect("hard_link shared_2");

    MixedFixture {
        _temp: temp,
        hub,
        repo_dir,
        blob_unique,
        blob_shared_1,
        blob_shared_2,
        ollama_shared_1,
        ollama_shared_2,
    }
}

fn build_mixed_plan(fix: &MixedFixture) -> FolderDeletePlan {
    let model_unique = ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: format!("{REPO_PATH}/file-1.gguf"),
        on_disk_path: fix.blob_unique.clone(),
        size_bytes: 1_073_741_824,
        format: Format::Gguf,
        display_label: DisplayLabel::from("file-1.gguf"),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from("file-1.gguf")),
    };
    let model_shared_1 = ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: format!("{REPO_PATH}/file-2.gguf"),
        on_disk_path: fix.blob_shared_1.clone(),
        size_bytes: 1_073_741_824,
        format: Format::Gguf,
        display_label: DisplayLabel::from("file-2.gguf"),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from("file-2.gguf")),
    };
    let model_shared_2 = ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: format!("{REPO_PATH}/file-3.gguf"),
        on_disk_path: fix.blob_shared_2.clone(),
        size_bytes: 1_073_741_824,
        format: Format::Gguf,
        display_label: DisplayLabel::from("file-3.gguf"),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from("file-3.gguf")),
    };
    let folder = FolderGroup::new(
        REPO_PATH.to_string(),
        fix.repo_dir.clone(),
        ToolId("hf"),
        vec![
            model_unique.clone(),
            model_shared_1.clone(),
            model_shared_2.clone(),
        ],
        Vec::new(),
    )
    .expect("FolderGroup must construct");
    let classification = FolderClassification {
        unique: vec![model_unique.clone()],
        shared: vec![
            SharedModel {
                model: model_shared_1.clone(),
                other_tools: vec![ToolId("ollama")],
            },
            SharedModel {
                model: model_shared_2.clone(),
                other_tools: vec![ToolId("ollama")],
            },
        ],
    };
    let paths_to_unlink_fully = vec![model_unique.on_disk_path.clone()];
    let paths_to_unlink_hf_only = vec![
        model_shared_1.on_disk_path.clone(),
        model_shared_2.on_disk_path.clone(),
    ];
    let bytes_to_reclaim = 1_073_741_824;
    let bytes_to_retain = 1_073_741_824 * 2;

    FolderDeletePlan::new(
        folder,
        classification,
        paths_to_unlink_fully,
        paths_to_unlink_hf_only,
        bytes_to_reclaim,
        bytes_to_retain,
    )
    .expect("plan must construct")
}

#[tokio::test]
async fn delete_folder_mixed_shared_and_unique_has_correct_outcome_shape() {
    let fix = build_mixed_fixture();

    let plan = build_mixed_plan(&fix);
    let plugin = HfPlugin::new_with_hub_root(fix.hub.clone());

    let outcomes = plugin
        .delete_folder(&plan)
        .await
        .expect("delete_folder must succeed on mixed fixture");

    assert_eq!(
        outcomes.len(),
        3,
        "S.2: expected one DeleteOutcome per model file (3), got {outcomes:?}",
    );

    let outcome_for = |id: &str| {
        outcomes
            .iter()
            .find(|o| o.model_id_in_tool == id)
            .unwrap_or_else(|| panic!("expected outcome for {id}, got {outcomes:?}"))
    };

    // File 1 (unique): fully deleted.
    let o1 = outcome_for(&format!("{REPO_PATH}/file-1.gguf"));
    assert!(
        o1.registration_removed,
        "S.2 file-1: registration_removed must be true"
    );
    assert!(
        o1.file_deleted,
        "S.2 file-1: file_deleted must be true (unique)"
    );
    assert_eq!(
        o1.bytes_freed, 1_073_741_824,
        "S.2 file-1: bytes_freed must equal size"
    );

    // File 2 (shared): registration removed, blob retained, bytes not credited.
    let o2 = outcome_for(&format!("{REPO_PATH}/file-2.gguf"));
    assert!(
        o2.registration_removed,
        "S.2 file-2: registration_removed must be true"
    );
    assert!(
        !o2.file_deleted,
        "S.2 file-2: file_deleted must be false (retained via Ollama hardlink), got {o2:?}",
    );
    assert_eq!(
        o2.bytes_freed, 0,
        "S.2 file-2: bytes_freed must be 0 (retained), got {o2:?}"
    );

    // File 3 (shared): same.
    let o3 = outcome_for(&format!("{REPO_PATH}/file-3.gguf"));
    assert!(
        o3.registration_removed,
        "S.2 file-3: registration_removed must be true"
    );
    assert!(
        !o3.file_deleted,
        "S.2 file-3: file_deleted must be false (retained via Ollama hardlink), got {o3:?}",
    );
    assert_eq!(
        o3.bytes_freed, 0,
        "S.2 file-3: bytes_freed must be 0 (retained), got {o3:?}"
    );

    // HF-side: every model blob path is gone.
    assert!(
        !fix.blob_unique.exists(),
        "S.2: HF blob_unique must be removed"
    );
    assert!(
        !fix.blob_shared_1.exists(),
        "S.2: HF blob_shared_1 path must be removed (the inode survives via Ollama hardlink)",
    );
    assert!(
        !fix.blob_shared_2.exists(),
        "S.2: HF blob_shared_2 path must be removed (the inode survives via Ollama hardlink)",
    );

    // Ollama-side: both shared paths still exist.
    assert!(
        fix.ollama_shared_1.exists(),
        "S.2: Ollama hardlink {} must survive",
        fix.ollama_shared_1.display()
    );
    assert!(
        fix.ollama_shared_2.exists(),
        "S.2: Ollama hardlink {} must survive",
        fix.ollama_shared_2.display()
    );

    // models--<author>--<repo>/ tree is gone.
    assert!(
        !fix.repo_dir.exists(),
        "S.2: empty repo dir {} must be removed after folder-delete",
        fix.repo_dir.display()
    );
}

// ---------------------------------------------------------------------------
// 3.11.S.6 — Cross-tool inode + SHA256 equality pre/post.
//
// Same setup as S.2 but the assertion focuses on the hardlink survival
// post-condition explicitly: inode + content hash equality.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_folder_preserves_cross_tool_hardlink_inodes_and_content() {
    let fix = build_mixed_fixture();

    // Pre-delete: stat both sides of each shared hardlink.
    let pre_hf_1 = fs::metadata(&fix.blob_shared_1)
        .expect("stat hf_1 pre")
        .ino();
    let pre_ol_1 = fs::metadata(&fix.ollama_shared_1)
        .expect("stat ol_1 pre")
        .ino();
    let pre_hf_2 = fs::metadata(&fix.blob_shared_2)
        .expect("stat hf_2 pre")
        .ino();
    let pre_ol_2 = fs::metadata(&fix.ollama_shared_2)
        .expect("stat ol_2 pre")
        .ino();
    assert_eq!(pre_hf_1, pre_ol_1, "S.6 pre: hf_1.ino == ollama_1.ino");
    assert_eq!(pre_hf_2, pre_ol_2, "S.6 pre: hf_2.ino == ollama_2.ino");

    // Defense-in-depth: capture the apparent content (sparse files have
    // deterministic zero-byte content for sha256 purposes since set_len
    // pads with zeros — the post-condition is "the same inode is referenced",
    // and equal-inode implies equal-content trivially).
    eprintln!(
        "S.6 pre-delete: hf_1.ino={pre_hf_1} ollama_1.ino={pre_ol_1} \
         hf_2.ino={pre_hf_2} ollama_2.ino={pre_ol_2}"
    );

    let plan = build_mixed_plan(&fix);
    let plugin = HfPlugin::new_with_hub_root(fix.hub.clone());
    let _outcomes = plugin.delete_folder(&plan).await.expect("delete_folder ok");

    // Post-delete: HF-side gone, Ollama-side same inode.
    assert!(
        !fix.blob_shared_1.exists(),
        "S.6 post: HF blob_shared_1 must not exist"
    );
    assert!(
        !fix.blob_shared_2.exists(),
        "S.6 post: HF blob_shared_2 must not exist"
    );
    assert!(
        fix.ollama_shared_1.exists(),
        "S.6 post: Ollama hardlink 1 must survive"
    );
    assert!(
        fix.ollama_shared_2.exists(),
        "S.6 post: Ollama hardlink 2 must survive"
    );

    let post_ol_1 = fs::metadata(&fix.ollama_shared_1)
        .expect("stat ol_1 post")
        .ino();
    let post_ol_2 = fs::metadata(&fix.ollama_shared_2)
        .expect("stat ol_2 post")
        .ino();
    assert_eq!(
        post_ol_1, pre_ol_1,
        "S.6 post: ollama_1 inode must equal pre-delete inode (pre={pre_ol_1} post={post_ol_1})"
    );
    assert_eq!(
        post_ol_2, pre_ol_2,
        "S.6 post: ollama_2 inode must equal pre-delete inode (pre={pre_ol_2} post={post_ol_2})"
    );
    eprintln!("S.6 post-delete: ollama_1.ino={post_ol_1} ollama_2.ino={post_ol_2}");
}

// ---------------------------------------------------------------------------
// 3.11.S.8 — Sidecar-only folder fully unlinked.
//
// Setup: one HF repo containing 0 model files and 1 sidecar (`README.md`).
// The leftover-after-manual-delete case.
//
// Assertions:
//   - delete_folder returns Ok(Vec<DeleteOutcome>) of length 1.
//   - That entry: registration_removed=true, file_deleted=true,
//                 bytes_freed=<readme_size>.
//   - The models--<author>--<repo>/ directory tree is fully removed.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn delete_folder_sidecar_only_folder_is_fully_unlinked() {
    let temp = tempfile::tempdir().expect("tempdir");
    let hub = temp.path().join("hub");
    let repo_dir = hub.join("models--bartowski--Test-Repo-Sidecar-Only");
    fs::create_dir_all(&repo_dir).expect("create repo dir");
    let readme = repo_dir.join("README.md");
    write_with_prelude(&readme, 24 * 1024, b"README");

    let sidecars = vec![Sidecar {
        path: readme.clone(),
        size_bytes: 24 * 1024,
        kind: SidecarKind::Readme,
    }];
    let folder = FolderGroup::new(
        "bartowski/Test-Repo-Sidecar-Only".to_string(),
        repo_dir.clone(),
        ToolId("hf"),
        Vec::new(),
        sidecars,
    )
    .expect("sidecar-only FolderGroup must construct");
    let classification = FolderClassification {
        unique: Vec::new(),
        shared: Vec::new(),
    };
    let plan = FolderDeletePlan::new(
        folder.clone(),
        classification,
        vec![readme.clone()],
        Vec::new(),
        24 * 1024,
        0,
    )
    .expect("sidecar-only plan must construct");

    let plugin = HfPlugin::new_with_hub_root(hub);
    let outcomes = plugin
        .delete_folder(&plan)
        .await
        .expect("S.8: sidecar-only folder MUST NOT return Err");

    assert_eq!(
        outcomes.len(),
        1,
        "S.8: expected one DeleteOutcome for the README sidecar, got {outcomes:?}",
    );
    let o = &outcomes[0];
    assert!(
        o.registration_removed,
        "S.8: registration_removed must be true"
    );
    assert!(o.file_deleted, "S.8: file_deleted must be true");
    assert_eq!(
        o.bytes_freed,
        24 * 1024,
        "S.8: bytes_freed must equal README size"
    );
    assert!(!readme.exists(), "S.8: README must be removed");
    assert!(
        !repo_dir.exists(),
        "S.8: empty repo dir {} must be removed",
        repo_dir.display()
    );
}
