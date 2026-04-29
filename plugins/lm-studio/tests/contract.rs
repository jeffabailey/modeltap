//! Plugin contract tests for the LM Studio plugin.
//!
//! Per `docs/feature/modeltap-tui/distill/plugin-contract-spec.md`, every
//! plugin must satisfy the same Tool-trait invariants. Mirrors
//! `plugins/llama-cli/tests/contract.rs` and `plugins/hf/tests/contract.rs`
//! — duplicated rather than extracted because the suite is small. The full
//! parametric suite lands once the cross-tool mock plugin exists in
//! `modeltap-core::tests`. For 02-04 we verify:
//!
//! - `name()` returns `"lm-studio"` and is stable.
//! - `accepted_formats()` is non-empty, stable, and equals `[Format::Gguf]`
//!   exactly (MLX is out of scope per intake C3 / ADR-004 OQ-3).
//! - `discover()` returns `Err(NotInstalled)` when all configured roots
//!   are missing.
//! - `discover()` is idempotent against a real fixture.
//! - `discover()` does not panic on a regular-file root.
//! - The plugin self-registers via `inventory::submit!`.

use std::fs;
use std::path::PathBuf;

use modeltap_core::{DiscoverError, Format, PluginFactory, Tool, ToolId};
use modeltap_plugin_lm_studio::{LmStudioPlugin, TOOL_NAME};

#[test]
fn name_is_lm_studio_and_is_stable_across_calls() {
    let plugin = LmStudioPlugin::new_with_search_paths(vec![PathBuf::from("/nonexistent")]);
    assert_eq!(plugin.name(), TOOL_NAME);
    assert_eq!(plugin.name(), ToolId("lm-studio"));
    assert_eq!(plugin.name(), plugin.name());
}

#[test]
fn accepted_formats_is_gguf_only_no_mlx() {
    // v1 contract: lm-studio plugin reports only Gguf. MLX is out of scope per
    // intake C3 / ADR-004 OQ-3 framing — when MLX support lands (v1.x), the
    // Format enum gains a parallel slot and this test gets updated explicitly.
    let plugin = LmStudioPlugin::new_with_search_paths(vec![PathBuf::from("/nonexistent")]);
    let first = plugin.accepted_formats();
    let second = plugin.accepted_formats();
    assert!(!first.is_empty(), "accepted_formats must not be empty");
    // `&'static [Format]` => identical pointer on repeat call.
    assert_eq!(first.as_ptr(), second.as_ptr());
    assert_eq!(
        first,
        &[Format::Gguf],
        "lm-studio v1 must accept exactly [Gguf] (no MLX); got {:?}",
        first
    );
}

#[tokio::test]
async fn discover_returns_not_installed_when_all_roots_missing() {
    let plugin = LmStudioPlugin::new_with_search_paths(vec![
        PathBuf::from("/nonexistent/no-such-lm-studio"),
        PathBuf::from("/nonexistent/no-such-lm-studio-2"),
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
    let plugin = LmStudioPlugin::new_with_search_paths(vec![temp.path().to_path_buf()]);
    let first = plugin.discover().await.expect("first discover ok");
    let second = plugin.discover().await.expect("second discover ok");
    assert_eq!(first.len(), second.len(), "discover must be idempotent");
    let ids_first: Vec<_> = first.iter().map(|m| m.id_in_tool.clone()).collect();
    let ids_second: Vec<_> = second.iter().map(|m| m.id_in_tool.clone()).collect();
    assert_eq!(ids_first, ids_second);
}

#[tokio::test]
async fn discover_does_not_panic_on_regular_file_root() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("not-a-dir");
    std::fs::write(&file, b"hello").unwrap();
    let plugin = LmStudioPlugin::new_with_search_paths(vec![file]);
    let res = plugin.discover().await;
    assert!(
        res.is_ok(),
        "discover must not error on a regular-file root, got {:?}",
        res
    );
}

#[test]
fn lm_studio_plugin_is_present_in_inventory() {
    let factories: Vec<&PluginFactory> = inventory::iter::<PluginFactory>().collect();
    assert!(
        !factories.is_empty(),
        "inventory must contain at least one plugin factory"
    );
    let mut saw_lm_studio = false;
    for f in factories {
        let plugin = (f.make)();
        if plugin.name() == ToolId("lm-studio") {
            saw_lm_studio = true;
            assert_eq!(
                plugin.accepted_formats(),
                &[Format::Gguf],
                "registered lm-studio plugin must report [Gguf] only"
            );
        }
    }
    assert!(
        saw_lm_studio,
        "lm-studio plugin must self-register via inventory::submit!"
    );
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn build_minimal_fixture(root: &std::path::Path) {
    // One valid GGUF file in an org/repo subtree so discover returns at least
    // one healthy entry. Keeps the contract test independent of build.sh.
    let dir = root.join("microsoft").join("phi-3-mini");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("phi-3-mini-q4.gguf");

    let arch = "phi3";
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
