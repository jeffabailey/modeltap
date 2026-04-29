//! Per-tool incremental rediscovery (US-06.AC-4 / US-11.AC-1).
//!
//! After a mutating action (zap in WS scope; unify/delete in 03-02/03-06),
//! the summary bar must reflect the post-mutation disk total within 500 ms.
//! Re-running every plugin's `discover()` is too slow once 4 plugins are
//! installed; instead we re-run ONLY the affected tool's discover() and
//! dispatch a `Msg::RefreshTool(view)` to replace its slot in the cross-tool
//! inventory.
//!
//! Pure orchestration: this module owns the projection from the plugin's
//! `Vec<DiscoveredModel>` to a `ToolView` (matching the same shape produced
//! at startup in `main::plugin_outcome_to_view`). Errors during refresh
//! surface as `RefreshError::DiscoveryFailed` — the caller decides whether
//! to leave the previous slot in place or fall back to a `ToolStatus::Error`
//! annotation. For the WS slice the caller is `headless::apply_effect` and
//! it falls back to leaving the slot unchanged on error.

use modeltap_core::{DiscoverError, Tool, ToolStatus};
use modeltap_tui::ToolView;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RefreshError {
    #[error("plugin discovery failed during refresh: {0}")]
    DiscoveryFailed(#[from] DiscoverError),
}

/// Re-run `plugin.discover()` and project the result into a `ToolView`
/// matching the startup projection. Same shape semantics as the startup
/// path: `Ok(models)` -> `ToolStatus::Ok`; the model_ids/sizes are extracted
/// from the discovered models.
pub async fn refresh_tool(plugin: &dyn Tool) -> Result<ToolView, RefreshError> {
    let tool_id = plugin.name();
    let models = plugin.discover().await?;
    Ok(ToolView {
        tool: tool_id,
        status: ToolStatus::Ok,
        model_ids: models.iter().map(|m| m.id_in_tool.clone()).collect(),
        model_sizes_bytes: models.iter().map(|m| m.size_bytes).collect(),
    })
}
