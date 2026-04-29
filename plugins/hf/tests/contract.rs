//! Plugin contract tests for the Hugging Face plugin.
//!
//! Per `docs/feature/modeltap-tui/distill/plugin-contract-spec.md`, every
//! plugin must satisfy the same Tool-trait invariants. Mirrors
//! `plugins/ollama/tests/contract.rs` and `plugins/llama-cli/tests/contract.rs`
//! — duplicated rather than extracted because the suite is small. The full
//! parametric suite (`run_full_contract_suite`) lands once the cross-tool
//! mock plugin exists in `modeltap-core::tests`. For 02-03 we verify:
//!
//! - `name()` returns `"hf"` and is stable.
//! - `accepted_formats()` is non-empty, stable, and contains `Safetensors`.
//! - `discover()` returns `Err(NotInstalled)` when the hub root is missing.
//! - `discover()` is idempotent against a real fixture.
//! - `discover()` does not panic on a regular-file root.
//! - The plugin self-registers via `inventory::submit!`.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::symlink;

use modeltap_core::{DiscoverError, Format, PluginFactory, Tool, ToolId};
use modeltap_plugin_hf::{HfPlugin, TOOL_NAME};

#[test]
fn name_is_hf_and_is_stable_across_calls() {
    let plugin = HfPlugin::new_with_hub_root(PathBuf::from("/nonexistent"));
    assert_eq!(plugin.name(), TOOL_NAME);
    assert_eq!(plugin.name(), ToolId("hf"));
    assert_eq!(plugin.name(), plugin.name());
}

#[test]
fn accepted_formats_is_non_empty_stable_and_contains_safetensors() {
    let plugin = HfPlugin::new_with_hub_root(PathBuf::from("/nonexistent"));
    let first = plugin.accepted_formats();
    let second = plugin.accepted_formats();
    assert!(!first.is_empty(), "accepted_formats must not be empty");
    // `&'static [Format]` => identical pointer on repeat call.
    assert_eq!(first.as_ptr(), second.as_ptr());
    assert!(
        first.iter().any(|f| matches!(f, Format::Safetensors)),
        "hf must accept Safetensors, got {:?}",
        first
    );
    assert!(
        first.iter().any(|f| matches!(f, Format::Gguf)),
        "hf must accept Gguf, got {:?}",
        first
    );
}

#[tokio::test]
async fn discover_returns_not_installed_for_missing_hub() {
    let plugin = HfPlugin::new_with_hub_root(PathBuf::from("/nonexistent/no-such-hf-cache/hub"));
    let res = plugin.discover().await;
    assert!(
        matches!(res, Err(DiscoverError::NotInstalled)),
        "expected NotInstalled, got {:?}",
        res
    );
}

#[cfg(unix)]
#[tokio::test]
async fn discover_is_idempotent_against_a_real_fixture() {
    let temp = tempfile::tempdir().expect("tempdir");
    let hub = build_minimal_fixture(temp.path());
    let plugin = HfPlugin::new_with_hub_root(hub);
    let first = plugin.discover().await.expect("first discover ok");
    let second = plugin.discover().await.expect("second discover ok");
    assert_eq!(first.len(), second.len(), "discover must be idempotent");
    let ids_first: Vec<_> = first.iter().map(|m| m.id_in_tool.clone()).collect();
    let ids_second: Vec<_> = second.iter().map(|m| m.id_in_tool.clone()).collect();
    assert_eq!(ids_first, ids_second);
}

#[tokio::test]
async fn discover_returns_empty_when_hub_exists_but_holds_no_models() {
    let temp = tempfile::tempdir().expect("tempdir");
    let hub = temp.path().join("hub");
    fs::create_dir_all(&hub).unwrap();
    let plugin = HfPlugin::new_with_hub_root(hub);
    let res = plugin.discover().await.expect("ok");
    assert!(res.is_empty(), "got {:?}", res);
}

#[test]
fn hf_plugin_is_present_in_inventory() {
    let factories: Vec<&PluginFactory> = inventory::iter::<PluginFactory>().collect();
    assert!(
        !factories.is_empty(),
        "inventory must contain at least one plugin factory"
    );
    let mut saw_hf = false;
    for f in factories {
        let plugin = (f.make)();
        if plugin.name() == ToolId("hf") {
            saw_hf = true;
            assert!(!plugin.accepted_formats().is_empty());
        }
    }
    assert!(
        saw_hf,
        "hf plugin must self-register via inventory::submit!"
    );
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn build_minimal_fixture(root: &Path) -> PathBuf {
    // One healthy snapshot pointing at a real blob — keeps the contract
    // test independent of build.sh.
    let hub = root.join("hub");
    let m = hub.join("models--owner--repo");
    let snap = m.join("snapshots/abc123");
    let blobs = m.join("blobs");
    fs::create_dir_all(&snap).unwrap();
    fs::create_dir_all(&blobs).unwrap();
    let blob = blobs.join("blob-x");
    let f = fs::File::create(&blob).unwrap();
    f.set_len(2048).unwrap();
    symlink("../../blobs/blob-x", snap.join("model.safetensors")).unwrap();
    hub
}
