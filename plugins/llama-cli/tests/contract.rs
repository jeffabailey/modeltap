//! Plugin contract tests for the llama-cli plugin.
//!
//! Per `docs/feature/modeltap-tui/distill/plugin-contract-spec.md`, every
//! plugin must satisfy the same Tool-trait invariants. This file mirrors
//! `plugins/ollama/tests/contract.rs` — duplicated rather than extracted to
//! a shared dev-dep because the suite is small (4 invariants relevant for
//! step 02-02). The full parametric suite (`run_full_contract_suite`) lands
//! in step 01-04 once the cross-tool mock plugin exists in
//! `modeltap-core::tests`. For 02-02 we verify the discover-only invariants:
//!
//! - `name()` returns `"llama-cli"` and is stable across calls.
//! - `accepted_formats()` is non-empty, stable, and contains `Format::Gguf`.
//! - `discover()` is idempotent (no side effects).
//! - `discover()` returns `Err(NotInstalled)` when all configured roots are
//!   missing.
//! - The plugin self-registers via `inventory::submit!` so the app crate's
//!   `inventory::iter::<PluginFactory>()` finds it without a direct import.
//! - `discover()` does not panic when given an unreadable path.

use std::fs;
use std::path::PathBuf;

use modeltap_core::{DiscoverError, Format, PluginFactory, Tool, ToolId};
use modeltap_plugin_llama_cli::{LlamaCliPlugin, TOOL_NAME};

#[test]
fn name_is_llama_cli_and_is_stable_across_calls() {
    let plugin = LlamaCliPlugin::new_with_search_paths(vec![PathBuf::from("/nonexistent")]);
    assert_eq!(plugin.name(), TOOL_NAME);
    assert_eq!(plugin.name(), ToolId("llama-cli"));
    // Two calls must return identical IDs (Tool::name is deterministic).
    assert_eq!(plugin.name(), plugin.name());
}

#[test]
fn accepted_formats_is_non_empty_and_stable_and_contains_gguf() {
    let plugin = LlamaCliPlugin::new_with_search_paths(vec![PathBuf::from("/nonexistent")]);
    let first = plugin.accepted_formats();
    let second = plugin.accepted_formats();
    assert!(!first.is_empty(), "accepted_formats must not be empty");
    // `&'static [Format]` => identical pointer on repeat call.
    assert_eq!(first.as_ptr(), second.as_ptr());
    assert!(
        first.iter().any(|f| matches!(f, Format::Gguf)),
        "llama-cli must accept Gguf, got {:?}",
        first
    );
}

#[tokio::test]
async fn discover_returns_not_installed_when_all_roots_missing() {
    let plugin = LlamaCliPlugin::new_with_search_paths(vec![
        PathBuf::from("/nonexistent/no-such-llama-cli"),
        PathBuf::from("/nonexistent/no-such-llama-cli-2"),
    ]);
    let res = plugin.discover().await;
    assert!(
        matches!(res, Err(DiscoverError::NotInstalled)),
        "expected NotInstalled, got {:?}",
        res
    );
}

#[tokio::test]
async fn discover_is_idempotent_against_a_real_fixture() {
    let temp = tempfile::tempdir().expect("tempdir");
    build_minimal_fixture(temp.path());
    let plugin = LlamaCliPlugin::new_with_search_paths(vec![temp.path().to_path_buf()]);
    let first = plugin.discover().await.expect("first discover ok");
    let second = plugin.discover().await.expect("second discover ok");
    assert_eq!(first.len(), second.len(), "discover must be idempotent");
    let ids_first: Vec<_> = first.iter().map(|m| m.id_in_tool.clone()).collect();
    let ids_second: Vec<_> = second.iter().map(|m| m.id_in_tool.clone()).collect();
    assert_eq!(ids_first, ids_second);
}

#[tokio::test]
async fn discover_does_not_panic_on_unreadable_dir() {
    // Pass a path to a regular file as a "root" — walkdir cannot recurse
    // into a non-directory. The plugin must NOT panic; it should treat the
    // root as having zero usable entries.
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("not-a-dir");
    std::fs::write(&file, b"hello").unwrap();
    let plugin = LlamaCliPlugin::new_with_search_paths(vec![file]);
    // We don't assert the exact return value because walkdir's behavior on a
    // non-directory root is to yield the file itself as a single entry; what
    // matters is "does not panic". A valid result is Ok(empty) (file is not
    // .gguf) or Ok with one Corrupt entry (.gguf-named regular file).
    let res = plugin.discover().await;
    assert!(
        res.is_ok(),
        "discover must not error on a regular-file root, got {:?}",
        res
    );
}

#[test]
fn llama_cli_plugin_is_present_in_inventory() {
    let factories: Vec<&PluginFactory> = inventory::iter::<PluginFactory>().collect();
    assert!(
        !factories.is_empty(),
        "inventory must contain at least one plugin factory"
    );
    let mut saw_llama_cli = false;
    for f in factories {
        let plugin = (f.make)();
        if plugin.name() == ToolId("llama-cli") {
            saw_llama_cli = true;
            assert!(!plugin.accepted_formats().is_empty());
        }
    }
    assert!(
        saw_llama_cli,
        "llama-cli plugin must self-register via inventory::submit!"
    );
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn build_minimal_fixture(root: &std::path::Path) {
    // One valid GGUF file (handcrafted) so discover returns at least one
    // healthy entry. Keeps the contract test independent of build.sh.
    let llms = root.join("llms");
    fs::create_dir_all(&llms).unwrap();
    let path = llms.join("alpha.gguf");

    let arch = "llama";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"GGUF");
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&2u64.to_le_bytes());
    let key = b"general.architecture";
    bytes.extend_from_slice(&(key.len() as u64).to_le_bytes());
    bytes.extend_from_slice(key);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&(arch.len() as u64).to_le_bytes());
    bytes.extend_from_slice(arch.as_bytes());
    let key = b"general.file_type";
    bytes.extend_from_slice(&(key.len() as u64).to_le_bytes());
    bytes.extend_from_slice(key);
    bytes.extend_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&15u32.to_le_bytes());

    fs::write(path, &bytes).unwrap();
}
