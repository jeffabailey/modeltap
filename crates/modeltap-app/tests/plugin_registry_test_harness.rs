//! Integration test for the `MODELTAP_TEST_PLUGINS=test-tool` registry seam
//! (tool-model-info-sqlite-cache step 01-03).
//!
//! Compiled only when the `test-harness` Cargo feature is enabled. The
//! production build of modeltap-app has no `test-harness = []` default, so
//! release binaries never see this test binary, never see the env-var read
//! code, and never see the inline `TestToolRegistration` body. Step 06-02
//! verifies the absence via `strings target/release/modeltap`.
//!
//! The test's job here is narrow: exercise the registry path that the US-23
//! cache acceptance suite will rely on. It does NOT verify cache behaviour
//! (that lands in steps 02-xx) and it does NOT spawn the binary (the lib half
//! exposes `registry::collect_plugins` directly).

#![cfg(feature = "test-harness")]

// Force linkage of plugin crates so their `inventory::submit!` blocks register
// — without these `as _` imports, this integration-test binary's
// `inventory::iter::<PluginFactory>()` would see zero entries. Mirrors the
// pattern in `tests/plugin_linkage.rs`.
use modeltap_plugin_atomic_chat as _;
use modeltap_plugin_hf as _;
use modeltap_plugin_lm_studio as _;
use modeltap_plugin_ollama as _;

use modeltap_app::registry::collect_plugins;

/// AC-5 + AC-6 of step 01-03: with `MODELTAP_TEST_PLUGINS=test-tool` set and
/// the `test-harness` feature enabled, `collect_plugins()` must include
/// exactly one plugin whose `name()` is `"test-tool"`. With the env var
/// unset, no `test-tool` entry appears.
///
/// The two branches are folded into one `#[test]` to serialise the env-var
/// mutation — cargo runs `#[test]` functions on multiple threads and
/// `std::env::set_var` is process-global.
#[test]
fn test_plugins_env_var_registers_test_tool_under_feature_flag() {
    // Branch 1: env var absent.
    std::env::remove_var("MODELTAP_TEST_PLUGINS");
    std::env::remove_var("MODELTAP_TEST_TOOL_ROOT");
    let before = collect_plugins();
    assert!(
        !before.iter().any(|p| p.name().0 == "test-tool"),
        "test-tool must NOT appear in collect_plugins() without the env var"
    );

    // Branch 2: env var set -> exactly one test-tool entry appended.
    std::env::set_var("MODELTAP_TEST_PLUGINS", "test-tool");
    let after = collect_plugins();
    let matches: Vec<_> = after.iter().filter(|p| p.name().0 == "test-tool").collect();
    assert_eq!(
        matches.len(),
        1,
        "MODELTAP_TEST_PLUGINS=test-tool must register exactly one TestTool plugin"
    );

    // Branch 3: unknown plugin name -> silently skipped, no test-tool entry.
    std::env::set_var("MODELTAP_TEST_PLUGINS", "no-such-plugin");
    let unknown = collect_plugins();
    assert!(
        !unknown.iter().any(|p| p.name().0 == "test-tool"),
        "unknown plugin names must be silently skipped"
    );

    // Cleanup so this env var does not leak into sibling integration tests.
    std::env::remove_var("MODELTAP_TEST_PLUGINS");
}

/// AC-2 / AC-3 / AC-4 of step 01-03: the registered TestTool's discover,
/// inspect_tool, and inspect_model produce the documented shapes. This is the
/// integration-test mirror of the unit tests in
/// `tests/src/test_tool.rs` — proving the registry-seam-constructed TestTool
/// honours the same contract as the canonical TestTool.
#[tokio::test]
async fn registered_test_tool_returns_documented_shapes() {
    use modeltap_core::domain::inspect::ModelId;

    let dir = tempfile::tempdir().expect("create tempdir for TestTool root");
    std::fs::write(
        dir.path().join("test-model-7b.gguf"),
        b"synthetic-gguf-bytes",
    )
    .expect("write synthetic model");

    std::env::set_var("MODELTAP_TEST_PLUGINS", "test-tool");
    std::env::set_var("MODELTAP_TEST_TOOL_ROOT", dir.path());

    let plugins = collect_plugins();
    let test_tool = plugins
        .iter()
        .find(|p| p.name().0 == "test-tool")
        .expect("registry must register test-tool when MODELTAP_TEST_PLUGINS is set");

    // discover() returns exactly one model.
    let models = test_tool.discover().await.expect("discover succeeds");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id_in_tool, "test-model-7b");

    // inspect_tool() reports detected_version = "test-1.0.0".
    let detail = test_tool
        .inspect_tool()
        .await
        .expect("inspect_tool succeeds");
    assert_eq!(detail.detected_version, Some("test-1.0.0".to_string()));

    // inspect_model() reports metadata_kv["test.kind"] = "synthetic".
    let model_detail = test_tool
        .inspect_model(&ModelId::from("test-model-7b"))
        .await
        .expect("inspect_model succeeds");
    assert_eq!(
        model_detail
            .metadata_kv
            .get("test.kind")
            .map(String::as_str),
        Some("synthetic")
    );

    // Cleanup.
    std::env::remove_var("MODELTAP_TEST_PLUGINS");
    std::env::remove_var("MODELTAP_TEST_TOOL_ROOT");
}
