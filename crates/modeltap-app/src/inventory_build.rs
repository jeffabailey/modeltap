//! Cross-plugin inventory-build helpers — bridges `discovery::run_discovery`
//! (raw plugin outcomes) and the compatibility-engine inputs (the
//! `PluginCapabilityMap` from `modeltap-core`).
//!
//! ## Why the warning lives here, not in modeltap-core
//!
//! `modeltap-core::logic::compatibility::compute_indicator` is a pure-domain
//! function — no I/O, no side effects, no tracing. Its defensive branch
//! returns `Unknown` for plugins that declared an empty `accepted_formats()`
//! (per US-16.AC-3) but it cannot emit a developer-mode warning without
//! breaking purity.
//!
//! The composition root (modeltap-app) is the right place for the warning:
//! it owns the tracing subscriber wiring + the diagnostics log path. The
//! function below is called by `main.rs` shortly after the plugin registry
//! has produced its capability map but before discovery results are projected
//! into the TUI's `AppState`.
//!
//! ## Contract
//!
//! `warn_on_empty_capabilities(&PluginCapabilityMap) -> Vec<ToolId>`:
//! - Walks the map deterministically (BTreeMap iteration is sorted by key).
//! - For every plugin whose `accepted_formats()` slice is empty, emits a
//!   `tracing::warn!` to the `modeltap.discovery` target with text
//!   `"plugin {tool} returned empty accepted_formats()"` so the warning
//!   surfaces in the user's diagnostics log.
//! - Returns the list of offending `ToolId`s so callers (and tests) can
//!   assert on the list without parsing log text.
//!
//! ## Why `Vec<ToolId>` and not `()`
//!
//! Pure-style return: the function tells you what it found. Tests assert on
//! the return value (state) AND optionally capture the `tracing::warn!` event
//! (interaction at the tracing port boundary). Both are useful — return value
//! for the unit test, captured event for the AC-3 diagnostics-log scenario.

use modeltap_core::logic::compatibility::PluginCapabilityMap;
use modeltap_core::types::ToolId;

/// Emit one `tracing::warn!` per plugin whose `accepted_formats()` is empty,
/// and return the list of offending `ToolId`s in deterministic
/// (`PluginCapabilityMap` key-sorted) order.
///
/// Per US-16.AC-3 the developer-mode warning is the cue that pushes plugin
/// authors to ship a non-empty `accepted_formats()` slice. Without the
/// warning, the engine's defensive `Unknown` fallback would silently mask
/// the bug.
pub fn warn_on_empty_capabilities(plugin_capabilities: &PluginCapabilityMap) -> Vec<ToolId> {
    let mut offenders: Vec<ToolId> = Vec::new();
    for (tool, formats) in plugin_capabilities {
        if formats.is_empty() {
            tracing::warn!(
                target: "modeltap.discovery",
                "plugin {} returned empty accepted_formats()",
                tool.0
            );
            offenders.push(*tool);
        }
    }
    offenders
}

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_core::types::Format;

    fn caps(pairs: &[(&'static str, &[Format])]) -> PluginCapabilityMap {
        let mut m = PluginCapabilityMap::new();
        for (name, fmts) in pairs {
            m.insert(ToolId(name), fmts.to_vec());
        }
        m
    }

    #[test]
    fn warn_on_empty_capabilities_returns_empty_when_all_plugins_have_formats() {
        let caps = caps(&[
            ("hf", &[Format::Gguf, Format::Safetensors]),
            ("llama-cli", &[Format::Gguf]),
            ("ollama", &[Format::OllamaBlob]),
        ]);
        let offenders = warn_on_empty_capabilities(&caps);
        assert!(offenders.is_empty(), "got: {:?}", offenders);
    }

    #[test]
    fn warn_on_empty_capabilities_returns_offending_plugin_names() {
        let caps = caps(&[
            ("hf", &[Format::Gguf]),
            ("broken-plugin", &[]), // empty
            ("llama-cli", &[Format::Gguf]),
        ]);
        let offenders = warn_on_empty_capabilities(&caps);
        assert_eq!(offenders, vec![ToolId("broken-plugin")]);
    }

    #[test]
    fn warn_on_empty_capabilities_returns_all_offenders_in_sorted_order() {
        // BTreeMap iteration is sorted by key — assert the deterministic
        // contract.
        let caps = caps(&[
            ("zeta-plugin", &[]),
            ("alpha-plugin", &[]),
            ("hf", &[Format::Gguf]),
            ("middle-plugin", &[]),
        ]);
        let offenders = warn_on_empty_capabilities(&caps);
        assert_eq!(
            offenders,
            vec![
                ToolId("alpha-plugin"),
                ToolId("middle-plugin"),
                ToolId("zeta-plugin"),
            ]
        );
    }
}
