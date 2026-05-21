//! Plugin-contract test for `Tool::inspect_tool` on the GPT4All plugin.
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.12.U.1 — GPT4All does NOT have a canonical version source, so it
//! inherits the ADR-016 default body and MUST return
//! `Err(InspectError::Unsupported { tool: ToolId("gpt4all") })` for any
//! invocation, leaving the filesystem byte-identical.
//!
//! The parametrized harness in `modeltap_core::tests::inspect` is the single
//! source of truth for the contract. Future plugin authors (US-18) follow
//! the same one-shim-per-plugin pattern.

use modeltap_core::tests::inspect::{run_inspect_tool_contract, InspectCapability};
use modeltap_core::ToolId;
use modeltap_plugin_gpt4all::Gpt4AllPlugin;

#[tokio::test]
async fn gpt4all_returns_unsupported_for_inspect_tool() {
    let fixture = tempfile::tempdir().expect("tempdir for gpt4all inspect contract fixture");
    let plugin = Gpt4AllPlugin::new_with_search_paths(vec![fixture.path().to_path_buf()]);
    run_inspect_tool_contract(
        &plugin,
        ToolId("gpt4all"),
        fixture.path(),
        InspectCapability::Unsupported,
    )
    .await;
}
