//! Parametrized contract-test harness for the `Tool::delete_folder` capability.
//!
//! Per `plugin-contract-spec.md` §2 and §3.11.U.1, every plugin crate
//! exercises this harness from a per-plugin integration test file:
//!
//! ```ignore
//! // plugins/ollama/tests/folder_delete_contract.rs
//! use modeltap_core::tests::plugin_contract::{
//!     run_folder_delete_contract, FolderDeleteCapability,
//! };
//! use modeltap_plugin_ollama::OllamaPlugin;
//!
//! #[tokio::test]
//! async fn ollama_satisfies_folder_delete_contract() {
//!     let fixture = tempfile::tempdir().unwrap();
//!     let plugin = OllamaPlugin::new_with_root(fixture.path().to_path_buf());
//!     run_folder_delete_contract(
//!         &plugin,
//!         modeltap_core::ToolId("ollama"),
//!         fixture.path(),
//!         FolderDeleteCapability::Unsupported,
//!     )
//!     .await;
//! }
//! ```
//!
//! For the v1 plugin set (per `plugin-contract-spec.md` §1):
//! - HF plugin runs `FolderDeleteCapability::Supported` (full 8-test suite).
//! - Ollama / llama-cli / lm-studio / Atomic Chat all run
//!   `FolderDeleteCapability::Unsupported` (one test: §3.11.U.1).
//!
//! Future 5th-plugin authors (US-18 / K4 / ADR-010): when adding a new plugin
//! that does NOT have folder-grouped storage, add one `tokio::test` file
//! invoking this harness with `Unsupported`. If the new plugin DOES have
//! folder-grouped storage, override `Tool::delete_folder` AND invoke with
//! `Supported` (the full 8-test suite enforces the override is contract-correct).
//!
//! ## Scope of this step (05-01)
//!
//! Only `FolderDeleteCapability::Unsupported` is implemented here. The
//! `Supported` arm (8-test suite for the HF plugin override) is currently a
//! `todo!` placeholder; HF's own `folder_delete_happy_path.rs` covers the
//! supported invariants for step 01-03 / 03-01 / 04-01 / 04-02. The
//! `Supported` arm will be filled in by a future step that consolidates
//! HF's contract tests into the parametrized harness.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::types::{DeleteError, FolderClassification, FolderDeletePlan, FolderGroup};
use crate::{Tool, ToolId};

/// Which contract path a plugin opts into.
///
/// Per ADR-010: plugins whose storage layout is NOT folder-grouped (Ollama,
/// llama-cli, LM Studio, Atomic Chat) inherit the default body of
/// `Tool::delete_folder` and run the `Unsupported` path. The HF plugin
/// overrides the default and runs the `Supported` path (the full 8-test
/// suite in `plugin-contract-spec.md` §§3.11.S.1–3.11.S.8).
///
/// Adding a future plugin: pick `Unsupported` unless the tool has true
/// folder-grouped storage AND the plugin author has implemented the override.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FolderDeleteCapability {
    /// The plugin inherits the `Tool::delete_folder` default body. For ANY
    /// input `FolderDeletePlan`, the plugin MUST return
    /// `Err(DeleteError::Unsupported { tool: <plugin_name> })` and MUST NOT
    /// touch the filesystem (no rmdir, no unlink, no mkdir, no truncate).
    Unsupported,
    /// The plugin overrides `Tool::delete_folder`. The override MUST honor
    /// the full behavioral contract in `plugin-contract-spec.md` §§3.11.S.*.
    /// Currently a TODO in this harness — HF's own integration tests cover
    /// the supported invariants until the harness is filled in.
    Supported,
}

/// Run the contract-test suite for `Tool::delete_folder` against a plugin.
///
/// Arguments:
/// - `plugin`: the plugin under test. Borrowed; the harness does not move it.
/// - `expected_id`: the `ToolId` the plugin's `name()` returns. Used to
///   assert `DeleteError::Unsupported { tool }`'s `tool` field equals the
///   plugin's identity.
/// - `fixture_root`: a directory the plugin is configured to consider its
///   discovery root. For the `Unsupported` path, the harness writes a small
///   witness file into this root and asserts the file remains byte-identical
///   pre/post (filesystem-state immutability).
/// - `capability`: which contract path the plugin opts into.
///
/// Panics on contract violation (this is a test helper).
pub async fn run_folder_delete_contract<T: Tool + ?Sized>(
    plugin: &T,
    expected_id: ToolId,
    fixture_root: &Path,
    capability: FolderDeleteCapability,
) {
    match capability {
        FolderDeleteCapability::Unsupported => {
            assert_plugin_name_matches(plugin, expected_id);
            test_delete_folder_returns_unsupported(plugin, expected_id, fixture_root).await;
        }
        FolderDeleteCapability::Supported => {
            assert_plugin_name_matches(plugin, expected_id);
            // The 8-test supported suite (3.11.S.1 .. 3.11.S.8) is covered by
            // the HF plugin's own integration tests today. The harness
            // consolidation lands in a future step; flag it loudly here so a
            // mis-invocation surfaces immediately rather than silently passing.
            unimplemented!(
                "FolderDeleteCapability::Supported is not yet wired into the parametrized \
                 harness (step 05-01 covers Unsupported only). HF plugin owns the supported \
                 invariants via `plugins/hf/tests/folder_delete_*.rs`. See \
                 plugin-contract-spec.md §§3.11.S.1–3.11.S.8 for the contract."
            );
        }
    }
}

/// Sanity: every contract test starts by asserting the plugin's identity
/// matches the caller-supplied expectation. Catches `OllamaPlugin` accidentally
/// invoked with `expected_id = ToolId("lm-studio")` etc.
fn assert_plugin_name_matches<T: Tool + ?Sized>(plugin: &T, expected_id: ToolId) {
    assert_eq!(
        plugin.name(),
        expected_id,
        "plugin.name() must equal the caller-supplied expected_id",
    );
}

/// Contract test §3.11.U.1: `delete_folder` returns `Unsupported` and the
/// filesystem is unchanged.
///
/// Per `plugin-contract-spec.md` §3.11.U.1:
/// - `plugin.delete_folder(&plan).await` returns
///   `Err(DeleteError::Unsupported { tool })` where `tool == plugin.name()`.
/// - The fixture's filesystem state is unchanged (manifest equality).
/// - The error message contains the plugin's name (i.e., the `tool` field).
async fn test_delete_folder_returns_unsupported<T: Tool + ?Sized>(
    plugin: &T,
    expected_id: ToolId,
    fixture_root: &Path,
) {
    // Write a witness file so the pre/post manifest comparison is non-trivial.
    // The default body must short-circuit before this file is touched.
    let witness = fixture_root.join(".modeltap-contract-witness");
    std::fs::create_dir_all(fixture_root).expect("create fixture root");
    std::fs::write(&witness, b"witness").expect("write witness");

    let pre = manifest(fixture_root);

    let plan = minimal_unsupported_plan();
    let result = plugin.delete_folder(&plan).await;

    match result {
        Err(DeleteError::Unsupported { tool }) => {
            assert_eq!(
                tool, expected_id,
                "DeleteError::Unsupported.tool must equal plugin.name()",
            );
            let msg = format!("{}", DeleteError::Unsupported { tool });
            assert!(
                msg.contains(expected_id.0),
                "DeleteError::Unsupported Display must contain the plugin's ToolId verbatim, \
                 got: {msg}",
            );
        }
        Err(other) => panic!(
            "expected Err(DeleteError::Unsupported {{ tool: {:?} }}), got Err({other:?})",
            expected_id.0,
        ),
        Ok(outcomes) => panic!(
            "expected Err(DeleteError::Unsupported {{ tool: {:?} }}), got Ok({} outcomes) — \
             the ADR-010 default body MUST short-circuit on Unsupported plugins",
            expected_id.0,
            outcomes.len(),
        ),
    }

    let post = manifest(fixture_root);
    assert_eq!(
        pre, post,
        "fixture filesystem state must be byte-identical pre/post Tool::delete_folder for a \
         plugin inheriting the ADR-010 default body",
    );

    // Clean up so the harness leaves no trace beyond its tempdir caller's scope.
    let _ = std::fs::remove_file(&witness);
}

/// Build a minimal `FolderDeletePlan` whose contents the default body MUST
/// ignore. The plan is structurally valid (smart-constructor invariants hold)
/// but points at a non-existent path — if any plugin actually tried to
/// dispatch this plan to an unlink loop, it would fail loudly on the missing
/// directory, surfacing the contract violation.
///
/// Built via direct field-init (rather than `FolderGroup::new`) because the
/// smart constructor rejects `tool != ToolId("hf")`. The `Unsupported` path
/// is explicitly for plugins that are NOT `ToolId("hf")`.
fn minimal_unsupported_plan() -> FolderDeletePlan {
    let folder = FolderGroup {
        path: "stub/repo".to_string(),
        absolute_path: PathBuf::from("/nonexistent/no-such-folder"),
        tool: ToolId("hf"),
        models: vec![],
        sidecars: vec![],
    };
    let classification = FolderClassification {
        unique: vec![],
        shared: vec![],
    };
    FolderDeletePlan {
        folder,
        classification,
        paths_to_unlink_fully: vec![],
        paths_to_unlink_hf_only: vec![],
        bytes_to_reclaim: 0,
        bytes_to_retain: 0,
    }
}

/// Recursive directory manifest: relative-path -> file size in bytes. Used to
/// assert byte-identical pre/post filesystem state. Mirrors the parent's
/// `DirManifest` helper used by the confirmation-safety acceptance tests;
/// copied verbatim here so this module remains self-contained (no internal
/// dep on app-crate test helpers).
fn manifest(root: &Path) -> BTreeMap<PathBuf, u64> {
    let mut out = BTreeMap::new();
    if !root.exists() {
        return out;
    }
    let walker = walkdir::WalkDir::new(root).follow_links(false);
    for entry in walker {
        let entry = entry.expect("walk fixture dir");
        if entry.file_type().is_file() || entry.file_type().is_symlink() {
            let meta = entry.path().symlink_metadata().expect("stat fixture entry");
            let rel = entry
                .path()
                .strip_prefix(root)
                .expect("strip prefix")
                .to_path_buf();
            out.insert(rel, meta.len());
        }
    }
    out
}
