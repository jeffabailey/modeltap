//! `PluginFactory` — the static-registration type used by plugins to register
//! themselves at link time (per ADR-001).
//!
//! Each plugin crate emits one `inventory::submit!` block that adds a
//! `PluginFactory` to the inventory section. The composition root in
//! `modeltap-app` calls `inventory::iter::<PluginFactory>()` and constructs
//! one `Box<dyn Tool>` per submitted factory.
//!
//! Living in `modeltap-core` lets all plugins share the same `inventory::collect!`
//! slot without depending on each other (architecture rule R1: no plugin
//! crate depends on another plugin crate, per US-18 architecture-lint
//! scenario).

use crate::tool::Tool;

/// A factory function that constructs a boxed plugin instance. Stored in
/// the static inventory section by `inventory::submit!`; iterated at app
/// startup by `inventory::iter::<PluginFactory>()`.
pub struct PluginFactory {
    pub make: fn() -> Box<dyn Tool>,
}

inventory::collect!(PluginFactory);
