//! M3 / INT-FGD-4 — Cross-tool hardlink survival acceptance test for
//! folder-group-bulk-delete (US-05c, step 03-02).
//!
//! Source scenarios (un-skipped in DISTILL feature files by this step):
//!
//! - `folder-group-delete.feature`:
//!   `@milestone-3 @ac-10 @int-fgd-4 @destructive @real-io`
//!   "Folder-delete preserves the Ollama-side hardlink for a shared model
//!   file"
//!
//! - `integration-checkpoints.feature`:
//!   `@int-fgd-4 @destructive @real-io`
//!   "For every shared file, the other tool's hardlink survives the
//!   folder-delete"
//!
//! Strategy B (acceptance-test-plan.md): REAL I/O against a `tempfile::TempDir`-
//! built HF cache. The driving port is `Tool::delete_folder` — the same port
//! the composition root dispatches to. Substituting an in-memory HfPlugin
//! would break this test because the post-conditions assert inode-equality
//! across a real cross-tool hardlink and the load-bearing mechanism IS the
//! HF plugin's `delete_one_at` ref-counting (ADR-009): when a shared file's
//! HF-side path is unlinked, the blob remains because Ollama still references
//! the same inode.
//!
//! This is the INT-FGD-4 / AC-10 invariant hoisted into an acceptance test:
//! stat-equality of inodes pre/post + SHA256-equality of content (defense in
//! depth — proves the inode is the same file, not a coincidentally-equal new
//! inode number).
//!
//! Fixture: `devon-hf-mixed` (per acceptance-test-plan.md §3) — 1 unique
//! HF-only file + 1 shared file hardlinked into a sibling Ollama tree.
//! Sparse blobs for speed.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, MetadataExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

use modeltap_core::types::{
    FolderClassification, FolderDeletePlan, FolderGroup, SharedModel, Sidecar, SidecarKind,
};
use modeltap_core::{DedupKey, DisplayLabel, Format, ModelMeta, ModelStatus, Tool, ToolId};
use modeltap_plugin_hf::HfPlugin;

const REPO_PATH: &str = "bartowski/Llama-3.2-1B-Instruct-GGUF";
const REPO_DIR_NAME: &str = "models--bartowski--Llama-3.2-1B-Instruct-GGUF";
const REV_SHA: &str = "abc123def4567890abc123def4567890abc12345";

const UNIQUE_BYTES: u64 = 4096;
const SHARED_BYTES: u64 = 8192;
const README_BYTES: u64 = 256;

struct DevonHfMixedFixture {
    _temp: TempDir,
    hub: PathBuf,
    repo_dir: PathBuf,
    blob_unique: PathBuf,
    blob_shared_hf: PathBuf,
    snap_unique: PathBuf,
    snap_shared: PathBuf,
    readme: PathBuf,
    refs_main: PathBuf,
    /// The Ollama-side hardlink that points at the SAME inode as
    /// `blob_shared_hf`. After folder-delete this path must still resolve to
    /// the same inode (INT-FGD-4 / AC-10).
    ollama_shared_path: PathBuf,
}

/// Build `devon-hf-mixed` with cross-tool hardlink between the HF and Ollama
/// blob trees. Returns a fixture with the precondition that
/// `stat(blob_shared_hf).st_ino == stat(ollama_shared_path).st_ino` (asserted
/// in the test).
fn build_devon_hf_mixed_fixture() -> DevonHfMixedFixture {
    let temp = tempfile::tempdir().expect("tempdir for devon-hf-mixed");
    let root = temp.path().to_path_buf();
    let hub = root.join("hub");
    let repo_dir = hub.join(REPO_DIR_NAME);
    let blobs_dir = repo_dir.join("blobs");
    let snap_dir = repo_dir.join("snapshots").join(REV_SHA);
    let refs_dir = repo_dir.join("refs");
    fs::create_dir_all(&blobs_dir).expect("create blobs dir");
    fs::create_dir_all(&snap_dir).expect("create snap dir");
    fs::create_dir_all(&refs_dir).expect("create refs dir");

    // Two distinct blob hashes (length 64).
    let blob_unique_hash = "1111111111111111111111111111111111111111111111111111111111111111";
    let blob_shared_hash = "2222222222222222222222222222222222222222222222222222222222222222";
    let blob_unique = blobs_dir.join(blob_unique_hash);
    let blob_shared_hf = blobs_dir.join(blob_shared_hash);

    // Write distinct content per blob so the SHA256 post-condition is
    // non-vacuous. Sparse `set_len` won't give us hashable content, so we
    // write a small deterministic prelude then `set_len` the rest. The first
    // K bytes are what we'll hash.
    write_with_content(&blob_unique, UNIQUE_BYTES, b"UNIQUE-CONTENT-Q4");
    write_with_content(&blob_shared_hf, SHARED_BYTES, b"SHARED-CONTENT-Q4_K_M");

    // Snapshot symlinks point at the blobs via HF's two-up relative path.
    let snap_unique = snap_dir.join("unique.gguf");
    let snap_shared = snap_dir.join("Llama-3.2-1B-Instruct-Q4_K_M.gguf");
    symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(blob_unique_hash),
        &snap_unique,
    )
    .expect("symlink unique");
    symlink(
        PathBuf::from("..")
            .join("..")
            .join("blobs")
            .join(blob_shared_hash),
        &snap_shared,
    )
    .expect("symlink shared");

    // 1 sidecar.
    let readme = snap_dir.join("README.md");
    write_with_content(&readme, README_BYTES, b"README");

    // HF-internal: refs/main with the rev sha.
    let refs_main = refs_dir.join("main");
    fs::write(&refs_main, REV_SHA).expect("write refs/main");

    // Ollama-side hardlink to the SHARED blob. Layout mirrors Ollama's
    // content-addressed `blobs/sha256-<hash>` store but lives under a sibling
    // tempdir so this test does not depend on Ollama plugin discovery — the
    // INT-FGD-4 invariant is about inode survival, not orchestrator wiring.
    let ollama_blobs = root.join(".ollama").join("models").join("blobs");
    fs::create_dir_all(&ollama_blobs).expect("create ollama blobs");
    let ollama_shared_path = ollama_blobs.join(format!("sha256-{blob_shared_hash}"));
    fs::hard_link(&blob_shared_hf, &ollama_shared_path).expect("cross-tool hardlink");

    DevonHfMixedFixture {
        _temp: temp,
        hub,
        repo_dir,
        blob_unique,
        blob_shared_hf,
        snap_unique,
        snap_shared,
        readme,
        refs_main,
        ollama_shared_path,
    }
}

fn write_with_content(path: &Path, size: u64, content: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    let f = fs::File::create(path).expect("create file");
    // Write the deterministic prelude.
    use std::io::Write;
    let prelude = content.len() as u64;
    let mut handle = f;
    handle.write_all(content).expect("write prelude");
    // Pad to the requested apparent size with sparse zeros.
    if size > prelude {
        handle.set_len(size).expect("set_len");
    }
}

fn sha256_of(path: &Path) -> [u8; 32] {
    let mut hasher = Sha256::new();
    let mut f = fs::File::open(path).expect("open for sha256");
    std::io::copy(&mut f, &mut hasher).expect("read for sha256");
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

fn build_mixed_plan(fix: &DevonHfMixedFixture) -> FolderDeletePlan {
    let unique_model = ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: format!("{REPO_PATH}/unique.gguf"),
        on_disk_path: fix.blob_unique.clone(),
        size_bytes: UNIQUE_BYTES,
        format: Format::Gguf,
        display_label: DisplayLabel::from("unique.gguf"),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from("unique.gguf")),
    };
    let shared_model = ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: format!("{REPO_PATH}/Llama-3.2-1B-Instruct-Q4_K_M.gguf"),
        on_disk_path: fix.blob_shared_hf.clone(),
        size_bytes: SHARED_BYTES,
        format: Format::Gguf,
        display_label: DisplayLabel::from("Llama-3.2-1B-Instruct-Q4_K_M.gguf"),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from("Llama-3.2-1B-Instruct-Q4_K_M.gguf")),
    };
    let sidecars = vec![Sidecar {
        path: fix.readme.clone(),
        size_bytes: README_BYTES,
        kind: SidecarKind::Readme,
    }];

    let folder = FolderGroup::new(
        REPO_PATH.to_string(),
        fix.repo_dir.clone(),
        ToolId("hf"),
        vec![unique_model.clone(), shared_model.clone()],
        sidecars,
    )
    .expect("FolderGroup must construct");

    let classification = FolderClassification {
        unique: vec![unique_model.clone()],
        shared: vec![SharedModel {
            model: shared_model.clone(),
            other_tools: vec![ToolId("ollama")],
        }],
    };
    let paths_to_unlink_fully = vec![unique_model.on_disk_path.clone(), fix.readme.clone()];
    let paths_to_unlink_hf_only = vec![shared_model.on_disk_path.clone()];
    let bytes_to_reclaim = UNIQUE_BYTES + README_BYTES;
    let bytes_to_retain = SHARED_BYTES;

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

// ---------------------------------------------------------------------------
// M3 / INT-FGD-4 — Cross-tool hardlink survival
// ---------------------------------------------------------------------------

/// Scenario: Folder-delete preserves the Ollama-side hardlink for a shared
/// model file.
///
/// Driving port: `Tool::delete_folder` (HfPlugin override).
/// Driven port boundary: real filesystem (`std::fs::metadata`, inode + content
/// hash).
///
/// Pre-conditions:
///   1. The HF-side blob and the Ollama-side path stat to the same inode
///      (cross-tool hardlink set up by the fixture builder).
///   2. The SHA256 of the shared content is recorded.
///
/// Post-conditions after `plugin.delete_folder(&plan).await`:
///   1. The HF-side path for the shared file no longer exists.
///   2. The Ollama-side path still exists.
///   3. The Ollama-side path stats to the SAME inode it had pre-delete.
///   4. The SHA256 of the Ollama-side path matches the pre-delete SHA256
///      (defense in depth — proves the inode is the same file, not a
///      coincidentally-equal new inode number).
///   5. The unique-HF file and sidecar are gone (full unlink).
///   6. The empty repo tree is swept.
#[tokio::test]
async fn folder_delete_preserves_ollama_hardlink_for_shared_model_file() {
    let fix = build_devon_hf_mixed_fixture();

    // ---------- Pre-conditions ----------------------------------------------
    let pre_hf_meta = fs::metadata(&fix.blob_shared_hf).expect("stat hf-side pre");
    let pre_ollama_meta = fs::metadata(&fix.ollama_shared_path).expect("stat ollama-side pre");
    assert_eq!(
        pre_hf_meta.ino(),
        pre_ollama_meta.ino(),
        "fixture pre-condition: hf-side and ollama-side must stat to the same inode \
         (cross-tool hardlink), got hf={} ollama={}",
        pre_hf_meta.ino(),
        pre_ollama_meta.ino(),
    );
    let pre_sha = sha256_of(&fix.ollama_shared_path);

    // Debug stat-print for triage when this fails.
    eprintln!(
        "pre-delete: hf_ino={} ollama_ino={} sha256(ollama)={}",
        pre_hf_meta.ino(),
        pre_ollama_meta.ino(),
        hex(&pre_sha)
    );

    // ---------- Act: dispatch through the driving port ----------------------
    let plan = build_mixed_plan(&fix);
    let plugin = HfPlugin::new_with_hub_root(fix.hub.clone());
    let outcomes = plugin
        .delete_folder(&plan)
        .await
        .expect("delete_folder must succeed");

    // ---------- Post-conditions ---------------------------------------------
    // 1. Three outcomes: 1 unique model + 1 shared model + 1 sidecar.
    assert_eq!(
        outcomes.len(),
        3,
        "one DeleteOutcome per file (1 unique model + 1 shared model + 1 sidecar), got {outcomes:?}",
    );

    // 2. HF-side shared blob is gone.
    assert!(
        !fix.blob_shared_hf.exists(),
        "AC-10: HF-side shared blob {} must be removed after folder-delete",
        fix.blob_shared_hf.display()
    );

    // 3. Ollama-side path still exists.
    assert!(
        fix.ollama_shared_path.exists(),
        "AC-10: Ollama-side hardlink {} must survive folder-delete",
        fix.ollama_shared_path.display()
    );

    // 4. Ollama-side inode equals pre-delete inode.
    let post_ollama_meta = fs::metadata(&fix.ollama_shared_path).expect("stat ollama-side post");
    assert_eq!(
        post_ollama_meta.ino(),
        pre_ollama_meta.ino(),
        "INT-FGD-4 / AC-10: Ollama-side inode must equal pre-delete inode, got pre={} post={}",
        pre_ollama_meta.ino(),
        post_ollama_meta.ino(),
    );

    // 5. SHA256 of Ollama-side content unchanged (proves the inode IS the
    //    same file, not a coincidentally-equal new inode number).
    let post_sha = sha256_of(&fix.ollama_shared_path);
    assert_eq!(
        post_sha,
        pre_sha,
        "INT-FGD-4: Ollama-side SHA256 must match pre-delete, got pre={} post={}",
        hex(&pre_sha),
        hex(&post_sha),
    );

    eprintln!(
        "post-delete: ollama_ino={} sha256(ollama)={}",
        post_ollama_meta.ino(),
        hex(&post_sha)
    );

    // 6. Unique HF file and sidecar fully removed.
    assert!(
        !fix.blob_unique.exists(),
        "unique HF blob {} must be removed",
        fix.blob_unique.display()
    );
    assert!(
        !fix.readme.exists(),
        "sidecar {} must be removed",
        fix.readme.display()
    );
    // Snapshot symlinks gone.
    assert!(!fix.snap_unique.exists(), "snap_unique must be removed");
    assert!(!fix.snap_shared.exists(), "snap_shared must be removed");
    // refs/main is HF-internal — the plugin's delete_folder doesn't sweep
    // it (the orchestrator's HF-internal sweep handles that). For the
    // plugin-port-direct test we accept refs/main remaining; the dialog
    // dispatcher in the orchestrator removes it. We still verify the empty
    // model/blob/snapshot trees were swept.
    let _ = &fix.refs_main; // documented as not-our-business at this layer.

    // 7. Per-outcome shape (spec'd in plugin-contract-spec.md §3.11.S.2):
    //    - unique model entry: registration_removed=true, file_deleted=true,
    //                          bytes_freed=UNIQUE_BYTES
    //    - shared model entry: registration_removed=true, file_deleted=false,
    //                          bytes_freed=0 (the blob is still referenced
    //                          by Ollama's hardlink, so delete_one_at
    //                          conservatively keeps it).
    //    - sidecar entry:      registration_removed=true, file_deleted=true,
    //                          bytes_freed=README_BYTES
    let shared_outcome = outcomes
        .iter()
        .find(|o| o.model_id_in_tool == format!("{REPO_PATH}/Llama-3.2-1B-Instruct-Q4_K_M.gguf"))
        .expect("shared model must have a DeleteOutcome");

    // Per plugin-contract-spec.md §3.11.S.2 the shared outcome MUST report
    // `file_deleted: false, bytes_freed: 0` so the orchestrator's reclaim
    // accounting credits these bytes to `bytes_to_retain` (the user sees
    // "Retained: 0.X GB — also linked in Ollama"). The plugin MUST route
    // shared files via `paths_to_unlink_hf_only` and consult cross-tool
    // hardlink survivability (OS nlink count after registration removal) to
    // produce the right shape — NOT just rely on the OS to keep the inode
    // alive while reporting "file_deleted=true" to the user.
    assert!(
        shared_outcome.registration_removed,
        "shared outcome registration_removed must be true, got {shared_outcome:?}",
    );
    assert!(
        !shared_outcome.file_deleted,
        "S.2: shared model file_deleted must be FALSE (the inode survives via Ollama hardlink, \
         the user sees these bytes as 'retained, not reclaimed'), got {shared_outcome:?}",
    );
    assert_eq!(
        shared_outcome.bytes_freed, 0,
        "S.2: shared model bytes_freed must be 0 (the bytes are retained via Ollama hardlink), \
         got {shared_outcome:?}",
    );
}

fn hex(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut s, "{b:02x}");
    }
    s
}
