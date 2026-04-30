//! Plugin call supervisor (US-18 AC-4).
//!
//! Wraps every call into a plugin in a dedicated `tokio::task::spawn` so that
//! a panic in the plugin's body surfaces as a `JoinError::is_panic()` on the
//! supervisor side and CANNOT propagate up the orchestrator stack. Without
//! this seam a `panic!` inside `Tool::discover()` (a buggy 3rd-party plugin,
//! a malformed manifest, an unwrap on a None) would tear down the whole TUI.
//!
//! Per ADR-001 §"Enforcement" + ADR-005, the host (modeltap-app) owns the
//! tokio runtime and the supervision policy. Plugins remain pure
//! `async fn` implementations.
//!
//! ## Contract
//!
//! `run_plugin_call_isolated::<T>(tool_name, fut) -> Result<T, PluginCallError>`:
//!
//! - On clean completion: `Ok(value)` — the plugin's return value, untouched.
//! - On panic anywhere in `fut`: `Err(PluginCallError::Panic { tool, message })`.
//!   The `message` carries the panic payload formatted by `JoinError::Display`
//!   so the diagnostics log captures enough to triage the bug. The TUI SHALL
//!   render `(error)` against the tool's left-pane row (per AC-4).
//! - On non-panic JoinError (cancellation): treated as a panic for purposes of
//!   the supervisor — the plugin call did not return cleanly.
//!
//! ## Why a custom error type instead of `anyhow::Error`
//!
//! Per ADR-007, anyhow lives at edges (main, JSONL writer); the supervisor is
//! one level inward and benefits from a structured error so the orchestrator
//! can pattern-match on it (e.g., "did this tool panic? annotate (error)").

use std::future::Future;

use thiserror::Error;

/// What the supervisor reports when a plugin call fails to return cleanly.
#[derive(Debug, Error)]
pub enum PluginCallError {
    /// The plugin's future panicked. `tool` is the `Tool::name()` we were
    /// supervising; `message` is the panic payload + JoinError context.
    #[error("plugin {tool} panicked: {message}")]
    Panic { tool: String, message: String },
}

/// Run `fut` in a supervised tokio task. A panic inside `fut` is caught at
/// the JoinError boundary and converted to `Err(PluginCallError::Panic)` so
/// the orchestrator can annotate the tool's slot without crashing.
///
/// `tool_name` is captured up-front (before spawning) so the error message
/// names the offending plugin even if `fut` panicked before producing any
/// output of its own.
pub async fn run_plugin_call_isolated<T, F>(tool_name: &str, fut: F) -> Result<T, PluginCallError>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let owned_name = tool_name.to_string();
    let handle = tokio::spawn(fut);
    match handle.await {
        Ok(value) => Ok(value),
        Err(join_err) => {
            // `JoinError::Display` formats panic payloads cleanly when the
            // payload is `&'static str` or `String`; for everything else it
            // renders as "task panicked". That is sufficient detail for the
            // diagnostics log without unsafe downcasting.
            let message = if join_err.is_panic() {
                format!("{join_err}")
            } else if join_err.is_cancelled() {
                "plugin task was cancelled before returning".to_string()
            } else {
                format!("plugin task did not complete cleanly: {join_err}")
            };
            Err(PluginCallError::Panic {
                tool: owned_name,
                message,
            })
        }
    }
}
