//! Plugin-contract test for `Tool::inspect_model` on the Atomic Chat plugin.
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.13.U.1 — Atomic Chat does NOT override `inspect_model` (no canonical
//! model-introspection format on disk for the chat-history layout), so it
//! inherits the ADR-016 default body and MUST return
//! `Err(InspectError::Unsupported { tool: ToolId("Atomic Chat") })` for any
//! invocation.
//!
//! The parametrized harness in `modeltap_core::tests::inspect` is the single
//! source of truth for the contract. The `known_good` / `unknown` / `corrupt`
//! arguments are passed through but unused on the Unsupported arm — the
//! plugin's default-body short-circuits before reading the model_id.

use modeltap_core::domain::inspect::ModelId;
use modeltap_core::tests::inspect::{run_inspect_model_contract, InspectCapability};
use modeltap_core::ToolId;
use modeltap_plugin_atomic_chat::AtomicChatPlugin;

#[tokio::test]
async fn atomic_chat_returns_unsupported_for_inspect_model() {
    let fixture =
        tempfile::tempdir().expect("tempdir for atomic-chat inspect_model contract fixture");
    let plugin = AtomicChatPlugin::new_with_search_paths(vec![fixture.path().to_path_buf()]);
    run_inspect_model_contract(
        &plugin,
        ToolId("Atomic Chat"),
        InspectCapability::Unsupported,
        ModelId::from("any-model-id"),
        ModelId::from("unused-on-unsupported-arm"),
        None,
    )
    .await;
}
