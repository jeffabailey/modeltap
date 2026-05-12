//! Plugin-contract test for `Tool::delete_folder` on the Atomic Chat plugin.
//!
//! Per `docs/feature/folder-group-bulk-delete/distill/plugin-contract-spec.md`
//! §3.11.U.1 — Atomic Chat does NOT have folder-grouped storage (it has
//! flat-per-model `<root>/<id>/model.yml`), so it inherits the ADR-010
//! default body of `Tool::delete_folder` and MUST return
//! `Err(DeleteError::Unsupported { tool: ToolId("Atomic Chat") })` for any
//! input plan, leaving the filesystem byte-identical.
//!
//! ## Note on the DISTILL spec
//!
//! The DISTILL `plugin-contract-spec.md` (DISTILL wave) lists three non-HF
//! plugins: ollama, llama-cli, lm-studio. The workspace's actual third
//! non-HF plugin is `atomic-chat` (per the root `Cargo.toml`). The
//! `Unsupported` contract is identical regardless of plugin identity — it
//! derives from the default `Tool::delete_folder` body in
//! `modeltap-core/src/tool.rs` — so this test exercises the same code path
//! the spec describes.
//!
//! The parametrized harness in `modeltap_core::tests::plugin_contract` is the
//! single source of truth for the contract. Future 5th-plugin authors (US-18)
//! follow the same one-shim-per-plugin pattern.

use modeltap_core::tests::plugin_contract::{run_folder_delete_contract, FolderDeleteCapability};
use modeltap_core::ToolId;
use modeltap_plugin_atomic_chat::AtomicChatPlugin;

#[tokio::test]
async fn atomic_chat_returns_unsupported_for_delete_folder() {
    let fixture = tempfile::tempdir().expect("tempdir for atomic-chat contract fixture");
    let plugin = AtomicChatPlugin::new_with_search_paths(vec![fixture.path().to_path_buf()]);
    run_folder_delete_contract(
        &plugin,
        ToolId("Atomic Chat"),
        fixture.path(),
        FolderDeleteCapability::Unsupported,
    )
    .await;
}
