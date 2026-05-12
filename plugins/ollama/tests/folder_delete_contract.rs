//! Plugin-contract test for `Tool::delete_folder` on the Ollama plugin.
//!
//! Per `docs/feature/folder-group-bulk-delete/distill/plugin-contract-spec.md`
//! §3.11.U.1 — the Ollama plugin does NOT have folder-grouped storage, so it
//! inherits the ADR-010 default body of `Tool::delete_folder` and MUST return
//! `Err(DeleteError::Unsupported { tool: ToolId("ollama") })` for any input
//! plan, leaving the filesystem byte-identical.
//!
//! The parametrized harness in `modeltap_core::tests::plugin_contract` is the
//! single source of truth for the contract — this file is a thin shim that
//! wires the Ollama plugin instance into it. Future 5th-plugin authors (US-18)
//! follow the same one-shim-per-plugin pattern.

use modeltap_core::tests::plugin_contract::{run_folder_delete_contract, FolderDeleteCapability};
use modeltap_core::ToolId;
use modeltap_plugin_ollama::OllamaPlugin;

#[tokio::test]
async fn ollama_returns_unsupported_for_delete_folder() {
    let fixture = tempfile::tempdir().expect("tempdir for ollama contract fixture");
    let plugin = OllamaPlugin::new_with_root(fixture.path().to_path_buf());
    run_folder_delete_contract(
        &plugin,
        ToolId("ollama"),
        fixture.path(),
        FolderDeleteCapability::Unsupported,
    )
    .await;
}
