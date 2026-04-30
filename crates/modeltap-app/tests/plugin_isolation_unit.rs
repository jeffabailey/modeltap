//! Unit tests for `plugin_isolation::run_plugin_call_isolated` (US-18 AC-4).
//!
//! The supervisor wraps every plugin call in `tokio::task::spawn` and converts
//! a `JoinError::is_panic()` into a structured `Err(PluginPanic)` so the
//! orchestrator can annotate the tool's slot with `(error)` without
//! propagating the panic upward.
//!
//! Test budget (US-18 has 4 distinct behaviors -> 8 unit-test ceiling). This
//! file covers 2 behaviors: panic isolation (Err) and clean pass-through (Ok).

use modeltap_app::plugin_isolation::{run_plugin_call_isolated, PluginCallError};

#[tokio::test]
async fn run_plugin_call_isolated_returns_ok_for_a_normal_call() {
    let result: Result<u32, PluginCallError> =
        run_plugin_call_isolated("ok-plugin", async { 42u32 }).await;
    assert_eq!(result.unwrap(), 42);
}

#[tokio::test]
async fn run_plugin_call_isolated_catches_a_panic_and_returns_panic_error() {
    let result: Result<u32, PluginCallError> = run_plugin_call_isolated("bad-plugin", async {
        panic!("synthetic plugin panic for the panic-isolation test");
    })
    .await;
    let err = result.expect_err("a panicking future must surface as Err");
    let PluginCallError::Panic { tool, message } = err;
    assert_eq!(tool, "bad-plugin");
    assert!(
        message.contains("synthetic plugin panic"),
        "panic message must be captured for diagnostics.log, got: {message:?}"
    );
}
