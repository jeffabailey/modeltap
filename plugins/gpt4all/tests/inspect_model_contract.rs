//! Plugin-contract test for `Tool::inspect_model` on the GPT4All plugin.
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.13.U.1 — GPT4All does NOT override `inspect_model` (no canonical
//! model-introspection format on disk beyond the `.gpt4all` registration
//! file the discover path consumes), so it inherits the ADR-016 default
//! body and MUST return `Err(InspectError::Unsupported { tool: ToolId("gpt4all") })`
//! for any invocation.
//!
//! The parametrized harness in `modeltap_core::tests::inspect` is the single
//! source of truth for the contract. The `known_good` / `unknown` / `corrupt`
//! arguments are passed through but unused on the Unsupported arm.

use modeltap_core::domain::inspect::ModelId;
use modeltap_core::tests::inspect::{run_inspect_model_contract, InspectCapability};
use modeltap_core::ToolId;
use modeltap_plugin_gpt4all::Gpt4AllPlugin;

#[tokio::test]
async fn gpt4all_returns_unsupported_for_inspect_model() {
    let fixture = tempfile::tempdir().expect("tempdir for gpt4all inspect_model contract fixture");
    let plugin = Gpt4AllPlugin::new_with_search_paths(vec![fixture.path().to_path_buf()]);
    run_inspect_model_contract(
        &plugin,
        ToolId("gpt4all"),
        InspectCapability::Unsupported,
        ModelId::from("any-model-id"),
        ModelId::from("unused-on-unsupported-arm"),
        None,
    )
    .await;
}
