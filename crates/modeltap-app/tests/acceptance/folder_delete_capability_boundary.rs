//! M5 — Plugin capability-boundary acceptance test for folder-group-bulk-delete.
//!
//! Source: `docs/feature/folder-group-bulk-delete/distill/features/folder-group-delete.feature`
//!   @us-05c @milestone-5 @ac-5 @plugin-trait
//!
//!   Scenario Outline: Non-HF plugins return Unsupported when asked to delete a folder
//!
//! Per `step-definitions-skeleton.md` §C and `wave-decisions.md` D5, this is
//! the **Layer A** counterpart of the per-plugin contract tests in
//! `plugins/<name>/tests/folder_delete_contract.rs` (Layer B).
//!
//! The keymap (AC-5) prevents Shift+F from dispatching to a non-HF tool in
//! the live UI, so this scenario is *defensive*: it asserts what happens IF
//! the orchestrator's `Tool::delete_folder` dispatch somehow reached a
//! plugin that inherits the ADR-010 default body. The contract:
//!
//!   1. `plugin.delete_folder(&plan).await` returns
//!      `Err(DeleteError::Unsupported { tool: plugin.name() })`.
//!   2. The fixture's filesystem state is byte-identical pre/post the call
//!      (no rmdir, no truncate, no unlink, no mkdir).
//!   3. The error message (via `Display`) contains the plugin's `ToolId`
//!      verbatim — this is what the right-pane banner would surface if the
//!      orchestrator routed Unsupported to the UI.
//!
//! Note on `Examples` table: the DISTILL feature file lists `llama-cli` as
//! one of the three Examples rows, but the workspace's third non-HF plugin
//! (per `Cargo.toml`) is `atomic-chat`. The `Unsupported` contract is
//! identical regardless of the plugin's identity — it derives from the
//! default `Tool::delete_folder` body in `modeltap-core/src/tool.rs`. The
//! feature file is updated in this step to match the workspace.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use modeltap_core::types::{DeleteError, FolderClassification, FolderDeletePlan, FolderGroup};
use modeltap_core::{Tool, ToolId};
use modeltap_plugin_atomic_chat::AtomicChatPlugin;
use modeltap_plugin_lm_studio::LmStudioPlugin;
use modeltap_plugin_ollama::OllamaPlugin;

/// Build a minimal `FolderDeletePlan` that is structurally valid but whose
/// contents are irrelevant. The default-body of `Tool::delete_folder` must
/// short-circuit before touching the filesystem, so even a plan pointing at a
/// non-existent path must be answered with `Err(Unsupported)` and zero side
/// effects.
///
/// We construct `FolderGroup` via direct field-init (rather than `::new`)
/// because the smart constructor rejects any `tool != ToolId("hf")` (per
/// B-FGD-1 / data-models §1), and the M5 contract is precisely about
/// non-HF tools.
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

/// Recursive directory manifest (path -> u64-len) used to assert byte-identical
/// pre/post filesystem state. Mirrors the parent's `DirManifest` helper used by
/// the confirmation-safety tests; copied verbatim here so this file is
/// self-contained (the parent helper lives in a sibling test module that does
/// not export across the integration-test seam).
fn dir_manifest(root: &Path) -> BTreeMap<PathBuf, u64> {
    let mut out = BTreeMap::new();
    if !root.exists() {
        return out;
    }
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
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

/// Build a fixture directory tree at `<tempdir>/fixture/` with a single
/// regular file, so the post-call manifest comparison is non-trivial (an empty
/// manifest would compare equal to ANY rmtree). The plugin's default-body
/// must NOT touch this fixture.
fn build_inert_fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path().join("fixture");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("witness.txt"), b"inert").unwrap();
    temp
}

/// Drive the M5 layer-A assertion for a single plugin instance.
///
/// Asserts:
/// 1. `Tool::delete_folder` returns `Err(DeleteError::Unsupported { tool })`.
/// 2. `tool == expected_id`.
/// 3. The `Display` of the error contains the plugin's `ToolId` verbatim
///    (this is the substring the right-pane banner would surface).
/// 4. The fixture directory's `DirManifest` is byte-identical pre/post.
async fn assert_plugin_returns_unsupported<T: Tool + ?Sized>(
    plugin: &T,
    expected_id: ToolId,
    fixture_root: &Path,
) {
    let pre = dir_manifest(fixture_root);

    let plan = minimal_unsupported_plan();
    let result = plugin.delete_folder(&plan).await;

    match result {
        Err(DeleteError::Unsupported { tool }) => {
            assert_eq!(
                tool, expected_id,
                "DeleteError::Unsupported.tool must equal plugin.name()",
            );
            // The `Display` impl carries the ToolId — the right pane greps
            // this substring to render "<plugin> does not support folder-delete".
            let msg = format!("{}", DeleteError::Unsupported { tool });
            assert!(
                msg.contains(expected_id.0),
                "DeleteError::Unsupported Display must contain the ToolId verbatim, got: {msg}",
            );
            assert!(
                msg.contains("does not support folder-delete"),
                "DeleteError::Unsupported Display must include the canonical 'does not support folder-delete' phrase, got: {msg}",
            );
        }
        Err(other) => panic!(
            "expected Err(DeleteError::Unsupported) from {} plugin, got Err({other:?})",
            expected_id.0,
        ),
        Ok(outcomes) => panic!(
            "expected Err(DeleteError::Unsupported) from {} plugin, got Ok({} outcomes) — \
             the default body must short-circuit",
            expected_id.0,
            outcomes.len(),
        ),
    }

    let post = dir_manifest(fixture_root);
    assert_eq!(
        pre, post,
        "fixture directory must be byte-identical pre/post Tool::delete_folder \
         for a plugin that inherits the ADR-010 default body",
    );
}

// ---------------------------------------------------------------------------
// THE M5 scenario outline — one #[tokio::test] per plugin row.
//
// Each test corresponds to one row of the feature file's Examples table:
//   | ollama      |
//   | lm-studio   |
//   | Atomic Chat | (replaces the spec's `llama-cli` — see file header note)
// ---------------------------------------------------------------------------

/// Examples row: `| ollama |`.
#[tokio::test]
async fn ollama_returns_unsupported_when_orchestrator_dispatches_folder_delete() {
    let fixture = build_inert_fixture();
    let plugin = OllamaPlugin::new_with_root(fixture.path().join("fixture"));
    assert_plugin_returns_unsupported(
        &plugin,
        ToolId("ollama"),
        fixture.path().join("fixture").as_path(),
    )
    .await;
}

/// Examples row: `| lm-studio |`.
#[tokio::test]
async fn lm_studio_returns_unsupported_when_orchestrator_dispatches_folder_delete() {
    let fixture = build_inert_fixture();
    let plugin = LmStudioPlugin::new_with_search_paths(vec![fixture.path().join("fixture")]);
    assert_plugin_returns_unsupported(
        &plugin,
        ToolId("lm-studio"),
        fixture.path().join("fixture").as_path(),
    )
    .await;
}

/// Examples row: `| Atomic Chat |` (replaces `llama-cli` from the DISTILL
/// feature file — see file header note for justification).
#[tokio::test]
async fn atomic_chat_returns_unsupported_when_orchestrator_dispatches_folder_delete() {
    let fixture = build_inert_fixture();
    let plugin = AtomicChatPlugin::new_with_search_paths(vec![fixture.path().join("fixture")]);
    assert_plugin_returns_unsupported(
        &plugin,
        ToolId("Atomic Chat"),
        fixture.path().join("fixture").as_path(),
    )
    .await;
}

// ---------------------------------------------------------------------------
// Dispatching via `&dyn Tool` (trait-object): the production orchestrator's
// `actions::folder_delete::run` does the same thing. This third test pins the
// object-safety of the call site — if a future change accidentally made
// `Tool::delete_folder` non-object-safe, this test would fail to compile.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unsupported_is_observable_through_trait_object_dispatch() {
    let fixture = build_inert_fixture();
    let plugin: Box<dyn Tool> =
        Box::new(OllamaPlugin::new_with_root(fixture.path().join("fixture")));
    let plan = minimal_unsupported_plan();
    let result = plugin.delete_folder(&plan).await;
    match result {
        Err(DeleteError::Unsupported { tool }) => {
            assert_eq!(tool, ToolId("ollama"));
        }
        other => panic!("trait-object dispatch must return Err(Unsupported), got {other:?}",),
    }
}

// ---------------------------------------------------------------------------
// Compile-time guard: ensure `Tool` remains object-safe. Cheap: never called.
// If a future change makes `Tool::delete_folder` non-object-safe, this fails.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _compile_time_tool_object_safe(_: &dyn Tool) {}
