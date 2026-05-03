//! Mutation behavior tests for the GPT4All plugin (`Tool::link`,
//! `Tool::delete_one`, `Tool::delete_all`) — step 01-05.
//!
//! These tests enter exclusively through the `Tool` trait surface (the driving
//! port for a plugin per ADR-001). They never construct internal helpers
//! directly — Outside-In TDD port-to-port discipline.
//!
//! GPT4All stores models as flat `*.gguf` files under one or more configured
//! roots; there is no manifest, no content-addressed store. Therefore:
//!
//! - `link()` — direct hardlink at `model.on_disk_path` (atomic-replace via
//!   tempfile + rename, identical to lm-studio per ADR-004 OQ-2). EXDEV
//!   surfaces as `LinkError::CrossFilesystem` so the orchestrator can apply
//!   the per-target `[s/c/x]` choice (ADR-008).
//! - `delete_one()` — single `fs::remove_file`; ENOENT → `NotFound`.
//! - `delete_all()` — flat enumeration of every existing configured root,
//!   `.gguf` files only, deterministic order (filenames sorted within each
//!   root, roots in config order). Non-`.gguf` files preserved.
//!
//! AC-G2.2 (link round-trip) and AC-G3.1 (delete bookkeeping).

use std::fs;
use std::path::Path;

use modeltap_core::{
    DedupKey, DeleteError, DisplayLabel, Format, LinkResult, ModelMeta, ModelStatus, Tool,
};
use modeltap_plugin_gpt4all::{Gpt4AllPlugin, TOOL_NAME};

// -- helpers ----------------------------------------------------------------

/// Build a `ModelMeta` whose `on_disk_path` lives at `path`. Other fields are
/// fixed; only the path matters for link/delete behavior tests.
fn meta_at(path: &Path, id_in_tool: &str, size: u64) -> ModelMeta {
    ModelMeta {
        tool: TOOL_NAME,
        id_in_tool: id_in_tool.to_string(),
        on_disk_path: path.to_path_buf(),
        size_bytes: size,
        format: Format::Gguf,
        display_label: DisplayLabel::from(id_in_tool),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from(id_in_tool)),
    }
}

/// On Unix, return the inode of `path`; otherwise `None`. Used to verify
/// hardlink success (two paths -> same inode) without leaking platform
/// details into every test.
fn inode_of(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path).ok().map(|m| m.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

// -- T1: link happy path — hardlink, both paths share inode ----------------

/// AC-G2.2: `link(canonical, model)` produces a hardlink at the model's
/// on-disk path so canonical and target share an inode (no double-write).
#[tokio::test]
async fn link_creates_hardlink_so_canonical_and_target_share_inode() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let canonical = tmp.path().join("canonical-source.gguf");
    let target = root.join("phi-3-mini-q4.gguf");
    let payload = b"GGUF binary blob payload bytes";
    write_file(&canonical, payload);
    write_file(&target, b"OLD CONTENT - different inode, will be replaced");

    let plugin = Gpt4AllPlugin::new_with_search_paths(vec![root.clone()]);
    let model = meta_at(&target, "phi-3-mini-q4.gguf", payload.len() as u64);

    let outcome = plugin.link(&canonical, &model).await.expect("link ok");

    assert_eq!(outcome.tool, TOOL_NAME);
    assert_eq!(outcome.model_id_in_tool, "phi-3-mini-q4.gguf");
    assert!(
        matches!(outcome.result, LinkResult::HardLinked { .. }),
        "expected HardLinked, got {:?}",
        outcome.result
    );
    // Observable outcome: target reads as canonical's bytes AND shares its inode.
    assert_eq!(fs::read(&target).unwrap(), payload);
    let canon_inode = inode_of(&canonical).expect("unix inode");
    let target_inode = inode_of(&target).expect("unix inode");
    assert_eq!(
        canon_inode, target_inode,
        "after link, canonical and target must share inode"
    );
}

// -- T2: link idempotent when target already shares canonical inode --------

/// `link()` is idempotent per ADR-002: re-invoking on an already-linked pair
/// is a no-op that returns `LinkResult::AlreadyLinked` rather than churning
/// the filesystem.
#[tokio::test]
async fn link_is_idempotent_when_target_already_shares_canonical_inode() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let canonical = tmp.path().join("canonical.gguf");
    let target = root.join("model-a.gguf");
    write_file(&canonical, b"identical bytes");
    fs::hard_link(&canonical, &target).unwrap();
    let inode_before = inode_of(&target).expect("unix inode");

    let plugin = Gpt4AllPlugin::new_with_search_paths(vec![root]);
    let model = meta_at(&target, "model-a.gguf", 15);

    let outcome = plugin.link(&canonical, &model).await.expect("link ok");
    assert!(
        matches!(outcome.result, LinkResult::AlreadyLinked { .. }),
        "expected AlreadyLinked, got {:?}",
        outcome.result
    );
    // Observable outcome: filesystem state unchanged (same inode).
    assert_eq!(inode_of(&target), Some(inode_before));
}

// -- T3: delete_one removes file and reports freed bytes -------------------

/// AC-G3.1: `delete_one(model)` unlinks the file and returns
/// `bytes_freed == file_size`. After return, the file is gone.
#[tokio::test]
async fn delete_one_removes_file_and_reports_freed_bytes_equal_to_file_size() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let target = root.join("doomed.gguf");
    let payload = vec![0xCDu8; 7_777];
    write_file(&target, &payload);

    let plugin = Gpt4AllPlugin::new_with_search_paths(vec![root]);
    let model = meta_at(&target, "doomed.gguf", payload.len() as u64);

    let outcome = plugin.delete_one(&model).await.expect("delete ok");

    assert_eq!(outcome.tool, TOOL_NAME);
    assert_eq!(outcome.model_id_in_tool, "doomed.gguf");
    assert!(outcome.file_deleted, "file_deleted must be true on success");
    assert!(
        outcome.registration_removed,
        "GPT4All has no separate manifest; registration === file"
    );
    assert_eq!(outcome.bytes_freed, payload.len() as u64);
    assert!(!target.exists(), "file must not exist after delete_one");
}

// -- T4: delete_one ENOENT → DeleteError::NotFound -------------------------

/// `delete_one()` on a missing file returns `DeleteError::NotFound`, not
/// `Io`. Lets the orchestrator surface a coherent "nothing to do" outcome
/// rather than a panic or generic IO error.
#[tokio::test]
async fn delete_one_returns_not_found_when_target_path_does_not_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let missing = root.join("does-not-exist.gguf");
    // Intentionally NOT writing the file.

    let plugin = Gpt4AllPlugin::new_with_search_paths(vec![root]);
    let model = meta_at(&missing, "does-not-exist.gguf", 0);

    let err = plugin
        .delete_one(&model)
        .await
        .expect_err("must surface NotFound");
    match err {
        DeleteError::NotFound(id) => assert_eq!(id, "does-not-exist.gguf"),
        other => panic!("expected DeleteError::NotFound, got {:?}", other),
    }
}

// -- T5: delete_all empties every configured root --------------------------

/// `delete_all()` removes every `.gguf` file across every existing
/// configured root and returns one `DeleteOutcome` per file. Order is
/// deterministic: filenames sorted within each root, roots in config order
/// (so the orchestrator's JSONL audit trail is reproducible).
#[tokio::test]
async fn delete_all_removes_every_gguf_across_every_configured_root() {
    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    let root_a = tmp_a.path().to_path_buf();
    let root_b = tmp_b.path().to_path_buf();

    // Root A: two models, written out of alphabetical order to prove sort.
    write_file(&root_a.join("zebra.gguf"), b"Z");
    write_file(&root_a.join("alpha.gguf"), b"AA");
    // Root B: one model.
    write_file(&root_b.join("bravo.gguf"), b"BBB");

    let plugin = Gpt4AllPlugin::new_with_search_paths(vec![root_a.clone(), root_b.clone()]);

    let outcomes = plugin.delete_all().await.expect("delete_all ok");

    // Three files removed, three outcomes in deterministic order:
    // root_a: alpha, zebra (sorted within root); then root_b: bravo.
    assert_eq!(outcomes.len(), 3, "expected 3 outcomes, got {:?}", outcomes);
    let ids: Vec<&str> = outcomes
        .iter()
        .map(|o| o.model_id_in_tool.as_str())
        .collect();
    assert_eq!(ids, vec!["alpha.gguf", "zebra.gguf", "bravo.gguf"]);

    for outcome in &outcomes {
        assert_eq!(outcome.tool, TOOL_NAME);
        assert!(outcome.file_deleted, "file_deleted must be true: {outcome:?}");
        assert!(outcome.registration_removed);
        assert!(outcome.bytes_freed > 0, "bytes_freed > 0: {outcome:?}");
    }
    // Bytes-freed must match each file's original size.
    let by_id: std::collections::HashMap<&str, u64> = outcomes
        .iter()
        .map(|o| (o.model_id_in_tool.as_str(), o.bytes_freed))
        .collect();
    assert_eq!(by_id["alpha.gguf"], 2);
    assert_eq!(by_id["zebra.gguf"], 1);
    assert_eq!(by_id["bravo.gguf"], 3);

    // Filesystem proof: every .gguf is gone.
    assert!(!root_a.join("alpha.gguf").exists());
    assert!(!root_a.join("zebra.gguf").exists());
    assert!(!root_b.join("bravo.gguf").exists());
}

// -- T6: delete_all preserves non-.gguf files ------------------------------

/// `delete_all()` only touches `.gguf` files. README, downloads.json, dot-
/// files, and files in subdirectories are preserved. (Subdirs are not
/// recursed — flat by design, mirroring discover.rs.)
#[tokio::test]
async fn delete_all_preserves_non_gguf_files_and_does_not_recurse_subdirs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();

    // .gguf victim
    write_file(&root.join("model.gguf"), b"GG");
    // Survivors (top-level non-.gguf)
    write_file(&root.join("README.md"), b"hi");
    write_file(&root.join("downloads.json"), b"{}");
    write_file(&root.join(".DS_Store"), b"\0\0");
    // Survivor (buried .gguf — flat scan must not recurse)
    let buried = root.join("subdir/buried.gguf");
    write_file(&buried, b"BURIED");

    let plugin = Gpt4AllPlugin::new_with_search_paths(vec![root.clone()]);

    let outcomes = plugin.delete_all().await.expect("delete_all ok");

    // Exactly ONE outcome (model.gguf); nothing else touched.
    assert_eq!(outcomes.len(), 1, "expected 1 outcome, got {:?}", outcomes);
    assert_eq!(outcomes[0].model_id_in_tool, "model.gguf");
    assert!(outcomes[0].file_deleted);

    // Survivors still present.
    assert!(!root.join("model.gguf").exists(), "victim must be gone");
    assert!(root.join("README.md").exists(), "README preserved");
    assert!(root.join("downloads.json").exists(), "downloads.json preserved");
    assert!(root.join(".DS_Store").exists(), "dotfile preserved");
    assert!(buried.exists(), "buried .gguf must NOT be recursed into");
}

