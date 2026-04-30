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

use modeltap_core::{PluginFactory, Tool};

/// US-18 test fixture toggle: the `atomic-chat` plugin is wired into the
/// binary unconditionally for the `test-fixtures` Cargo feature, but it's
/// only opted INTO the materialized plugin list when this env var is set.
/// Without the opt-in, prior acceptance tests (US-02/03/05/...) would see
/// the fixture as a 5th `(not installed)` row and break their inventory
/// expectations.
///
/// The factory remains visible to `inventory::iter::<PluginFactory>()` either
/// way, so architecture rule R1 (and the trait certification check) still
/// proves the contract.
const ATOMIC_CHAT_FIXTURE_ENABLE_ENV: &str = "MODELTAP_ENABLE_TEST_FIXTURE_ATOMIC_CHAT";

/// Construct one `Box<dyn Tool>` per registered plugin. The set of plugins is
/// determined entirely by which plugin crates are linked into the binary —
/// no runtime configuration, no dynamic loading — with one carve-out: the
/// US-18 atomic-chat test fixture is filtered out unless the opt-in env var
/// (`MODELTAP_ENABLE_TEST_FIXTURE_ATOMIC_CHAT=1`) is set. This keeps the
/// fixture's `inventory::submit!` registration always linked (so R1 lints +
/// certification checks see all 5 factories) without leaking it into prior
/// acceptance tests' tool lists.
pub fn collect_plugins() -> Vec<Box<dyn Tool>> {
    let fixture_enabled = std::env::var_os(ATOMIC_CHAT_FIXTURE_ENABLE_ENV).is_some();
    inventory::iter::<PluginFactory>()
        .map(|f| (f.make)())
        .filter(|p| fixture_enabled || p.name().0 != "atomic-chat")
        .collect()
}
