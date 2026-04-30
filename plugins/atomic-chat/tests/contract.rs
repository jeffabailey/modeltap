//! Plugin contract tests for the Atomic Chat plugin.
//!
//! Mirrors `plugins/lm-studio/tests/contract.rs` and `plugins/hf/tests/contract.rs`
//! — duplicated rather than extracted because each plugin owns its own
//! contract under ADR-001's plugin-isolation rule. Verifies:
//!
//! - `name()` returns `"Atomic Chat"` and is stable across calls.
//! - `accepted_formats()` is non-empty, stable, and equals `[Format::Gguf]`
//!   exactly (MLX out of scope per intake C3 / ADR-004 OQ-3).
//! - `discover()` returns `Err(NotInstalled)` when all configured roots
//!   are missing.
//! - `discover()` is idempotent against a real fixture.
//! - `discover()` does not panic on a regular-file root.
//! - The plugin self-registers via `inventory::submit!`.

use std::fs;
use std::path::PathBuf;

use modeltap_core::{DiscoverError, Format, PluginFactory, Tool, ToolId};
use modeltap_plugin_atomic_chat::{AtomicChatPlugin, TOOL_NAME};

#[test]
fn name_is_atomic_chat_and_is_stable_across_calls() {
    let plugin = AtomicChatPlugin::new_with_search_paths(vec![PathBuf::from("/nonexistent")]);
    assert_eq!(plugin.name(), TOOL_NAME);
    assert_eq!(plugin.name(), ToolId("Atomic Chat"));
    assert_eq!(plugin.name(), plugin.name());
}

#[test]
fn accepted_formats_is_gguf_only_no_mlx() {
    // v1 contract: the plugin reports only Gguf. MLX is out of scope per
    // intake C3 / ADR-004 OQ-3 — when MLX support lands the Format enum
    // gains a parallel slot and this test gets updated explicitly.
    let plugin = AtomicChatPlugin::new_with_search_paths(vec![PathBuf::from("/nonexistent")]);
    let first = plugin.accepted_formats();
    let second = plugin.accepted_formats();
    assert!(!first.is_empty(), "accepted_formats must not be empty");
    assert_eq!(first.as_ptr(), second.as_ptr());
    assert_eq!(
        first,
        &[Format::Gguf],
        "atomic-chat v1 must accept exactly [Gguf] (no MLX); got {:?}",
        first
    );
}

#[tokio::test]
async fn discover_returns_not_installed_when_all_roots_missing() {
    let plugin = AtomicChatPlugin::new_with_search_paths(vec![PathBuf::from(
        "/nonexistent/no-such-atomic-chat",
    )]);
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
    let plugin = AtomicChatPlugin::new_with_search_paths(vec![temp.path().to_path_buf()]);
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
    let plugin = AtomicChatPlugin::new_with_search_paths(vec![file]);
    let res = plugin.discover().await;
    assert!(
        res.is_ok(),
        "discover must not error on a regular-file root, got {:?}",
        res
    );
}

#[test]
fn atomic_chat_plugin_is_present_in_inventory() {
    let factories: Vec<&PluginFactory> = inventory::iter::<PluginFactory>().collect();
    assert!(
        !factories.is_empty(),
        "inventory must contain at least one plugin factory"
    );
    let mut saw_atomic_chat = false;
    for f in factories {
        let plugin = (f.make)();
        if plugin.name() == ToolId("Atomic Chat") {
            saw_atomic_chat = true;
            assert_eq!(
                plugin.accepted_formats(),
                &[Format::Gguf],
                "registered Atomic Chat plugin must report [Gguf] only"
            );
        }
    }
    assert!(
        saw_atomic_chat,
        "Atomic Chat plugin must self-register via inventory::submit!"
    );
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn build_minimal_fixture(root: &std::path::Path) {
    // One model dir with a valid model.yml + model.gguf so discover returns
    // exactly one healthy entry.
    let id = "demo-7b";
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    let yaml = format!(
        "embedding: false\nmodel_path: llamacpp/models/{id}/model.gguf\nname: {id}\nsize_bytes: 1024\n"
    );
    fs::write(dir.join("model.yml"), yaml).unwrap();
    fs::write(dir.join("model.gguf"), b"GGUF\x03\x00\x00\x00").unwrap();
}
