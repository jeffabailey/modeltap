//! Discovery orchestrator — runs every plugin's `discover()` in parallel,
//! tracks per-plugin timings, aggregates the cross-tool inventory, and
//! returns a structured summary the app uses to emit launch.timing /
//! launch.inventory JSONL events.
//!
//! Per ADR-005 each plugin's discover() is invoked via `tokio::spawn` so a
//! slow plugin does not gate the others. Per US-18 AC-4 the orchestrator
//! catches plugin panics at the spawn boundary — a panic does NOT crash
//! modeltap, it surfaces as `ToolStatus::Error` for that one tool.
//!
//! Step 01-02 only has one plugin (Ollama), but the orchestrator is written
//! to handle N plugins so adding the 4th, 5th, etc. is mechanical.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use modeltap_core::{DiscoverError, DiscoveredModel, Tool, ToolId};

/// Outcome for one plugin's discovery pass.
pub struct PluginOutcome {
    pub tool: ToolId,
    pub elapsed_ms: u128,
    /// Either the discovered models or a structured error. We preserve the
    /// error so the app can write a diagnostic log line and annotate the
    /// tool's left-pane row as `(error)` per AC-4.
    pub result: Result<Vec<DiscoveredModel>, DiscoverError>,
}

impl PluginOutcome {
    /// True if the plugin reported a hard error (not just NotInstalled).
    pub fn is_error(&self) -> bool {
        matches!(
            self.result,
            Err(DiscoverError::PermissionDenied { .. })
                | Err(DiscoverError::UnexpectedLayout { .. })
                | Err(DiscoverError::Io(_))
                | Err(DiscoverError::ManifestParse { .. })
        )
    }
}

/// Aggregated inventory across all plugins, post-discovery, pre-hash.
pub struct InventorySummary {
    pub outcomes: Vec<PluginOutcome>,
}

impl InventorySummary {
    pub fn total_models(&self) -> u64 {
        self.outcomes
            .iter()
            .map(|o| o.result.as_ref().map(|v| v.len() as u64).unwrap_or(0))
            .sum()
    }

    /// Total disk usage in bytes, deduplicating shared `on_disk_path` entries
    /// across ALL plugins. For Ollama-only this matches the per-plugin dedup;
    /// once HF and lm-studio plugins land, the cross-tool dedup matters.
    pub fn total_disk_usage_bytes(&self) -> u64 {
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut total: u64 = 0;
        for outcome in &self.outcomes {
            if let Ok(models) = &outcome.result {
                for m in models {
                    if seen.insert(m.on_disk_path.clone()) {
                        total = total.saturating_add(m.size_bytes);
                    }
                }
            }
        }
        total
    }

    /// Names of plugins that produced a hard error during discovery (per
    /// AC-4). Ordered alphabetically for deterministic output.
    pub fn tool_errors(&self) -> Vec<String> {
        let mut errs: Vec<String> = self
            .outcomes
            .iter()
            .filter(|o| o.is_error())
            .map(|o| o.tool.to_string())
            .collect();
        errs.sort();
        errs
    }

    /// Step 01-02 placeholder: until SHA256 hashing lands in 01-05, the
    /// dedup engine cannot tell shared from format-locked. We return 0 for
    /// both counts; the launch.inventory event is still emitted so the
    /// schema is locked in for downstream work.
    pub fn dedupable_count(&self) -> u64 {
        0
    }

    pub fn format_locked_count(&self) -> u64 {
        0
    }

    /// Per-plugin elapsed milliseconds for the launch.timing event's
    /// `plugin_timings_ms` field.
    pub fn plugin_timings_ms(&self) -> Vec<(String, u64)> {
        self.outcomes
            .iter()
            .map(|o| (o.tool.to_string(), o.elapsed_ms as u64))
            .collect()
    }
}

/// Run every plugin's discover() and collect timings + outcomes. Each plugin
/// runs in its own tokio task so a slow plugin can't gate the others. A
/// panic in any one plugin is caught at the JoinHandle boundary and turned
/// into a synthetic `DiscoverError::Io` outcome — the orchestrator never
/// propagates a panic upward.
pub async fn run_discovery(plugins: Vec<Box<dyn Tool>>) -> InventorySummary {
    // Capture each plugin's `ToolId` BEFORE moving the plugin into its task
    // so a panicking discover() can still be attributed to its tool name on
    // the outer JoinError branch (US-18 AC-4: tool_errors must include the
    // panicking plugin's name, not "unknown").
    let mut handles = Vec::with_capacity(plugins.len());
    for plugin in plugins {
        let tool = plugin.name();
        let join_handle = tokio::spawn(async move {
            let start = Instant::now();
            let result = plugin.discover().await;
            let elapsed_ms = start.elapsed().as_millis();
            PluginOutcome {
                tool,
                elapsed_ms,
                result,
            }
        });
        handles.push((tool, join_handle));
    }

    let mut outcomes = Vec::with_capacity(handles.len());
    for (tool, handle) in handles {
        match handle.await {
            Ok(outcome) => outcomes.push(outcome),
            Err(join_err) => {
                // Per US-18 AC-4: a plugin panic must not crash modeltap.
                // Synthesize an Error outcome so the inventory still shows
                // the tool with `(error)` annotation. We preserve the
                // panicking plugin's `ToolId` (captured before spawn) so
                // `tool_errors()` lists it by name rather than "unknown".
                let reason = format!("plugin task panicked: {join_err}");
                outcomes.push(PluginOutcome {
                    tool,
                    elapsed_ms: 0,
                    result: Err(DiscoverError::Io(std::io::Error::other(reason))),
                });
            }
        }
    }

    // Sort outcomes by tool name so the launch.timing event is deterministic
    // regardless of which plugin finished first.
    outcomes.sort_by(|a, b| a.tool.to_string().cmp(&b.tool.to_string()));

    InventorySummary { outcomes }
}
