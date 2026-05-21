//! Plugin-contract test for `Tool::inspect_tool` on the LM Studio plugin.
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.12.U.1 — LM Studio does NOT override `inspect_tool` in this release
//! (best-effort version detection is left to a future step), so it inherits
//! the ADR-016 default body and MUST return
//! `Err(InspectError::Unsupported { tool: ToolId("lm-studio") })` for any
//! invocation, leaving the filesystem byte-identical.
//!
//! The parametrized harness in `modeltap_core::tests::inspect` is the single
//! source of truth for the contract — this file is a thin shim that wires
//! the LM Studio plugin instance into it. Future plugin authors (US-18)
//! follow the same one-shim-per-plugin pattern.

use modeltap_core::tests::inspect::{run_inspect_tool_contract, InspectCapability};
use modeltap_core::ToolId;
use modeltap_plugin_lm_studio::LmStudioPlugin;

#[tokio::test]
async fn lm_studio_returns_unsupported_for_inspect_tool() {
    let fixture = tempfile::tempdir().expect("tempdir for lm-studio inspect contract fixture");
    let plugin = LmStudioPlugin::new_with_search_paths(vec![fixture.path().to_path_buf()]);
    run_inspect_tool_contract(
        &plugin,
        ToolId("lm-studio"),
        fixture.path(),
        InspectCapability::Unsupported,
    )
    .await;
}
