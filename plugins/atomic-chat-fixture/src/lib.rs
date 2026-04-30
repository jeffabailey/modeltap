//! Atomic Chat — TEST-ONLY 5th-plugin fixture for US-18.
//!
//! This crate exists for ONE reason: to prove the ADR-001 plugin contract
//! end-to-end. Adding a 5th tool — Atomic Chat — must require ZERO changes
//! to `modeltap-core/src/`. The whole crate fits in a single file:
//!
//!   - one `Tool` impl returning a single synthetic `DiscoveredModel`,
//!   - one `inventory::submit!` block that registers the plugin,
//!   - two env-var test seams that opt the fixture's runtime behaviour in
//!     and out so it does not leak into prior acceptance tests:
//!       - `MODELTAP_ENABLE_TEST_FIXTURE_ATOMIC_CHAT=1` opts the fixture's
//!         `discover()` into returning a synthetic model. Without it, the
//!         plugin is REGISTERED (so `inventory::iter` sees it and the
//!         architecture-trait certification still works) but `discover()`
//!         returns `Err(NotInstalled)` — exactly like a real plugin whose
//!         tool isn't on the host.
//!       - `MODELTAP_FIXTURE_ATOMIC_CHAT_PANIC=1` makes `discover()` panic
//!         so the panic-isolation acceptance scenario can verify the
//!         supervisor behaviour without rebuilding the binary.
//!
//! ## Why opt-in
//!
//! The fixture is wired into the `modeltap-app` binary unconditionally via the
//! `[features] test-fixtures = ["dep:..."]` Cargo feature so the binary always
//! has access to it for tests. But every existing acceptance test that
//! enumerates the inventory (US-02, US-03, US-05, US-07, US-12, etc.) was
//! written against the 4 production plugins. If the fixture's `discover()`
//! always returned a model, those tests would all fail with off-by-one
//! inventory counts. Mirroring the `MODELTAP_LMSTUDIO_DIRS` opt-in pattern
//! used by the production plugins keeps the fixture invisible to prior tests
//! while still letting the US-18 acceptance suite enable it surgically.
//!
//! ## Why this lives in `plugins/` instead of `tests/`
//!
//! Architecture rule R1 lints the workspace's `Cargo.toml` graph: every plugin
//! lives at `plugins/*` and depends only on `modeltap-core`. If the fixture
//! lived under `tests/` the lint could not assert the same invariant on it.
//! By living among the production plugins, the fixture is itself a test of
//! the architecture rule — it cannot accidentally pull in a sibling plugin
//! without the lint catching it.

#![forbid(unsafe_code)]

use std::path::PathBuf;

use async_trait::async_trait;
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, DisplayLabel, Format, LinkError,
    LinkOutcome, ModelMeta, ModelStatus, PluginFactory, Tool, ToolId,
};

pub const TOOL_NAME: ToolId = ToolId("atomic-chat");
const ACCEPTED: &[Format] = &[Format::Gguf];

/// Opt-in env var: when set to `1`, the fixture's `discover()` returns its
/// synthetic single-model inventory. Otherwise it returns
/// `Err(DiscoverError::NotInstalled)` so prior acceptance tests don't see the
/// fixture in their inventory counts. Mirrors `MODELTAP_LMSTUDIO_DIRS` from
/// the production LM Studio plugin.
const ENABLE_ENV: &str = "MODELTAP_ENABLE_TEST_FIXTURE_ATOMIC_CHAT";

/// Panic-on-discover seam: when set, flips the discover() branch to panic so
/// the panic-isolation acceptance test can drive the supervisor without a
/// rebuild. Honoured ONLY when the fixture is also enabled (i.e. ENABLE_ENV is
/// set), so a stray panic env var alone won't tip prior tests over.
const PANIC_ENV: &str = "MODELTAP_FIXTURE_ATOMIC_CHAT_PANIC";

/// The Atomic Chat plugin instance. Holds nothing — the fixture's state is
/// entirely synthetic.
pub struct AtomicChatPlugin;

impl AtomicChatPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for AtomicChatPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for AtomicChatPlugin {
    fn name(&self) -> ToolId {
        TOOL_NAME
    }

    fn accepted_formats(&self) -> &'static [Format] {
        ACCEPTED
    }

    async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
        // Opt-in gate: prior acceptance tests do NOT set ENABLE_ENV, so the
        // fixture must look exactly like an uninstalled tool to them.
        // `NotInstalled` is the canonical signal for "this plugin is
        // registered but its tool isn't on this host"; the TUI renders it as
        // a `(not installed)` row that does NOT count toward totals or
        // dedupable bytes (per US-02).
        if std::env::var_os(ENABLE_ENV).is_none() {
            return Err(DiscoverError::NotInstalled);
        }

        // Panic-on-discover seam: only meaningful once the fixture is enabled
        // (a panic from a "not installed" plugin would be nonsense).
        if std::env::var_os(PANIC_ENV).is_some() {
            panic!("atomic-chat-fixture: synthetic panic for US-18 AC-4");
        }

        // Synthetic single-model inventory. `on_disk_path` is intentionally
        // a stable nonexistent path — the fixture is for plugin-trait
        // certification, not real disk I/O.
        Ok(vec![DiscoveredModel {
            id_in_tool: "atomic-chat-demo-7b".to_string(),
            display_label: DisplayLabel("Atomic Chat Demo 7B".to_string()),
            format: Format::Gguf,
            size_bytes: 4_096,
            on_disk_path: PathBuf::from("/nonexistent/atomic-chat/demo-7b.gguf"),
            status: ModelStatus::Healthy,
        }])
    }

    async fn link(
        &self,
        _canonical_src: &std::path::Path,
        _model: &ModelMeta,
    ) -> Result<LinkOutcome, LinkError> {
        // The fixture exists only to certify the trait surface; it never
        // mutates the filesystem.
        Err(LinkError::NotYetImplemented(
            "atomic-chat-fixture is for plugin-trait certification only; \
             link() is not implemented"
                .to_string(),
        ))
    }

    async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "atomic-chat-fixture is for plugin-trait certification only; \
             delete_one() is not implemented"
                .to_string(),
        ))
    }

    async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
        Err(DeleteError::NotYetImplemented(
            "atomic-chat-fixture is for plugin-trait certification only; \
             delete_all() is not implemented"
                .to_string(),
        ))
    }
}

// Self-registration via `inventory::submit!` — same mechanism the four
// production plugins use. This is THE proof of the ADR-001 contract:
// `modeltap-core` exposes the `PluginFactory` slot and is the ONLY crate the
// fixture imports from. The factory is registered unconditionally so
// `inventory::iter::<PluginFactory>()` always sees the fixture; runtime opt-in
// happens inside `discover()` above (see ENABLE_ENV).
inventory::submit! {
    PluginFactory {
        make: || Box::new(AtomicChatPlugin::new()),
    }
}

// ---------------------------------------------------------------------------
// Unit tests — keep the fixture honest. If the synthetic inventory drifts the
// US-18 acceptance test will fail loudly, but a tight unit test pinpoints the
// regression.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard: covers BOTH branches of the opt-in gate in a single test so the
    /// env-var manipulation cannot race with a parallel sibling test (Rust's
    /// `cargo test` runs `#[tokio::test]` functions on multiple threads, and
    /// `std::env::set_var` is process-global). Mutating env vars in two
    /// independent tests would be a data race; one serialised test sidesteps
    /// it without pulling in a `serial_test` dev-dep.
    #[tokio::test]
    async fn discover_honours_the_opt_in_env_var() {
        let plugin = AtomicChatPlugin::new();

        // Branch 1 — disabled: discover() returns NotInstalled. This is THE
        // invariant that keeps the fixture from leaking into prior acceptance
        // tests (US-02, US-03, US-05, etc.).
        std::env::remove_var(ENABLE_ENV);
        std::env::remove_var(PANIC_ENV);
        let err = plugin
            .discover()
            .await
            .expect_err("must report NotInstalled when not opted in");
        assert!(
            matches!(err, DiscoverError::NotInstalled),
            "expected NotInstalled, got {err:?}"
        );

        // Branch 2 — enabled: discover() returns the synthetic single-model
        // inventory unchanged.
        std::env::set_var(ENABLE_ENV, "1");
        let models = plugin.discover().await.expect("discover when enabled");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id_in_tool, "atomic-chat-demo-7b");
        assert_eq!(models[0].format, Format::Gguf);
        assert_eq!(plugin.name().0, "atomic-chat");

        // Cleanup so we don't leak the env var to other tests in this binary.
        std::env::remove_var(ENABLE_ENV);
    }
}
