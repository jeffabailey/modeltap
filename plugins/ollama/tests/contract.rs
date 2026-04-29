//! Plugin contract tests for the Ollama plugin.
//!
//! Per `docs/feature/modeltap-tui/distill/plugin-contract-spec.md`, every
//! plugin must satisfy the same Tool-trait invariants. The full suite
//! (`run_full_contract_suite`) lands in step 01-04 once the cross-tool
//! mock plugin exists in `modeltap-core::tests`. For 01-02 we verify the
//! discover-only invariants that 01-02 actually implements:
//!
//! - `name()` returns `"ollama"` and is stable across calls.
//! - `accepted_formats()` is non-empty and stable.
//! - `discover()` is idempotent (no side effects).
//! - `discover()` returns `Err(NotInstalled)` for a missing root.
//! - The plugin self-registers via `inventory::submit!` so the app crate's
//!   `inventory::iter::<PluginFactory>()` finds it without a direct import.

use std::fs;
use std::path::Path;

use modeltap_core::{DiscoverError, Tool, ToolId};
use modeltap_plugin_ollama::{OllamaPlugin, PluginFactory, TOOL_NAME};

#[test]
fn name_is_ollama_and_is_stable_across_calls() {
    let plugin = OllamaPlugin::new_with_root("/nonexistent".into());
    assert_eq!(plugin.name(), TOOL_NAME);
    assert_eq!(plugin.name(), ToolId("ollama"));
    // Two calls must return identical IDs (Tool::name is deterministic).
    assert_eq!(plugin.name(), plugin.name());
}

#[test]
fn accepted_formats_is_non_empty_and_stable() {
    let plugin = OllamaPlugin::new_with_root("/nonexistent".into());
    let first = plugin.accepted_formats();
    let second = plugin.accepted_formats();
    assert!(!first.is_empty(), "accepted_formats must not be empty");
    // `&'static [Format]` => identical pointer on repeat call.
    assert_eq!(first.as_ptr(), second.as_ptr());
}

#[tokio::test]
async fn discover_returns_not_installed_for_missing_root() {
    let plugin = OllamaPlugin::new_with_root("/nonexistent/no-such-ollama".into());
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
    let plugin = OllamaPlugin::new_with_root(temp.path().join(".ollama/models"));
    let first = plugin.discover().await.expect("first discover ok");
    let second = plugin.discover().await.expect("second discover ok");
    assert_eq!(first.len(), second.len(), "discover must be idempotent");
    let ids_first: Vec<_> = first.iter().map(|m| m.id_in_tool.clone()).collect();
    let ids_second: Vec<_> = second.iter().map(|m| m.id_in_tool.clone()).collect();
    assert_eq!(ids_first, ids_second);
}

#[test]
fn ollama_plugin_is_present_in_inventory() {
    let factories: Vec<&PluginFactory> = inventory::iter::<PluginFactory>().collect();
    assert!(
        !factories.is_empty(),
        "inventory must contain at least one plugin factory"
    );
    let mut saw_ollama = false;
    for f in factories {
        let plugin = (f.make)();
        if plugin.name() == ToolId("ollama") {
            saw_ollama = true;
            assert!(!plugin.accepted_formats().is_empty());
        }
    }
    assert!(
        saw_ollama,
        "ollama plugin must self-register via inventory::submit!"
    );
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn build_minimal_fixture(root: &Path) {
    let models = root.join(".ollama/models");
    let manifests = models.join("manifests/registry.ollama.ai/library/llama3");
    let blobs = models.join("blobs");
    fs::create_dir_all(&manifests).unwrap();
    fs::create_dir_all(&blobs).unwrap();
    let blob_sha = "8f3eaaa11111111111111111111111111111111111111111111111111111c102";
    let blob_path = blobs.join(format!("sha256-{}", blob_sha));
    let blob_file = fs::File::create(&blob_path).unwrap();
    blob_file.set_len(1024).unwrap();
    let manifest_body = format!(
        r#"{{
  "schemaVersion": 2,
  "layers": [
    {{
      "mediaType": "application/vnd.ollama.image.model",
      "digest": "sha256:{blob_sha}",
      "size": 1024
    }}
  ]
}}"#
    );
    fs::write(manifests.join("8b-instruct-q4_K_M"), manifest_body).unwrap();
}
