//! Plugin registry — the composition root's view of the plugin set.
//!
//! Per ADR-001, plugins self-register via `inventory::submit!`; the registry
//! collects every submitted `PluginFactory` and instantiates one `Box<dyn Tool>`
//! per factory. modeltap-core has zero knowledge of which plugins exist; this
//! module is the seam where the static plugin list is materialized.
//!
//! `PluginFactory` is re-exported from each plugin crate — for step 01-02 only
//! the Ollama plugin's factory exists; subsequent steps add llama-cli, hf,
//! lm-studio without changing this module's surface.

use modeltap_core::Tool;
use modeltap_plugin_ollama::PluginFactory;

/// Construct one `Box<dyn Tool>` per registered plugin. The set of plugins is
/// determined entirely by which plugin crates are linked into the binary —
/// no runtime configuration, no dynamic loading.
pub fn collect_plugins() -> Vec<Box<dyn Tool>> {
    inventory::iter::<PluginFactory>()
        .map(|f| (f.make)())
        .collect()
}
