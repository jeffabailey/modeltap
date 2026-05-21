//! Plugin-contract test for `Tool::inspect_tool` on the Atomic Chat plugin.
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.12.U.1 — Atomic Chat does NOT have a canonical version source, so it
//! inherits the ADR-016 default body and MUST return
//! `Err(InspectError::Unsupported { tool: ToolId("Atomic Chat") })` for any
//! invocation, leaving the filesystem byte-identical.
//!
//! The parametrized harness in `modeltap_core::tests::inspect` is the single
//! source of truth for the contract. Future plugin authors (US-18) follow
//! the same one-shim-per-plugin pattern.

use modeltap_core::tests::inspect::{run_inspect_tool_contract, InspectCapability};
use modeltap_core::ToolId;
use modeltap_plugin_atomic_chat::AtomicChatPlugin;

#[tokio::test]
async fn atomic_chat_returns_unsupported_for_inspect_tool() {
    let fixture = tempfile::tempdir().expect("tempdir for atomic-chat inspect contract fixture");
    let plugin = AtomicChatPlugin::new_with_search_paths(vec![fixture.path().to_path_buf()]);
    run_inspect_tool_contract(
        &plugin,
        ToolId("Atomic Chat"),
        fixture.path(),
        InspectCapability::Unsupported,
    )
    .await;
}
