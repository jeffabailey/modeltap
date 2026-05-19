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
//!
//! ## Test-harness seam (`MODELTAP_TEST_PLUGINS`)
//!
//! The `tool-model-info-sqlite-cache` US-23 acceptance suite needs an
//! in-process `Tool` impl to drive the orchestrator end-to-end without a real
//! Ollama/HF install. `crates/modeltap-acceptance/src/test_tool.rs` defines
//! the canonical `TestTool` for that purpose, but the acceptance crate is not
//! linked into the modeltap binary, so it cannot self-register via `inventory`.
//!
//! Instead, this module reads `MODELTAP_TEST_PLUGINS=test-tool` at registry
//! construction time and appends an inline `TestToolRegistration` to the
//! collected plugin list — but ONLY when built with `cfg(test)` or with the
//! `test-harness` Cargo feature. In release builds (`cargo build --release`
//! with no feature flags) the env-var read and the `TestToolRegistration`
//! body are compiled out entirely; the string `"MODELTAP_TEST_PLUGINS"` does
//! not appear in the binary. Step 06-02 verifies the absence via `strings
//! target/release/modeltap`.

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
///
/// In test / `test-harness` builds the function additionally honours
/// `MODELTAP_TEST_PLUGINS` (see `maybe_register_test_plugins`).
pub fn collect_plugins() -> Vec<Box<dyn Tool>> {
    let fixture_enabled = std::env::var_os(ATOMIC_CHAT_FIXTURE_ENABLE_ENV).is_some();
    let mut plugins: Vec<Box<dyn Tool>> = inventory::iter::<PluginFactory>()
        .map(|f| (f.make)())
        .filter(|p| fixture_enabled || p.name().0 != "atomic-chat")
        .collect();
    maybe_register_test_plugins(&mut plugins);
    plugins
}

// ---------------------------------------------------------------------------
// Test-harness-only seam: MODELTAP_TEST_PLUGINS
// ---------------------------------------------------------------------------

/// In test / `test-harness` builds, appends one in-process plugin per name
/// listed in `MODELTAP_TEST_PLUGINS` (comma-separated). Only `"test-tool"` is
/// supported today; unknown names are silently skipped so an old fixture env
/// var does not break newer tests.
///
/// Compiled away to a no-op in release builds (no `cfg(test)` and no
/// `feature = "test-harness"`), so the binary contains no env-var read and
/// no `TestToolRegistration` body. Step 06-02 asserts via `strings
/// target/release/modeltap` that the env-var name does not appear.
#[cfg(any(test, feature = "test-harness"))]
fn maybe_register_test_plugins(plugins: &mut Vec<Box<dyn Tool>>) {
    use std::path::PathBuf;
    let Ok(spec) = std::env::var("MODELTAP_TEST_PLUGINS") else {
        return;
    };
    for raw in spec.split(',') {
        let name = raw.trim();
        if name == "test-tool" {
            // Root path is read from a sibling env var so the harness can
            // point the TestTool at a tempdir without recompiling. Absent /
            // empty -> use a deterministic placeholder; `discover()` will
            // still report one model with `size_bytes = 0`.
            let root = std::env::var_os("MODELTAP_TEST_TOOL_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/nonexistent/test-tool"));
            plugins.push(Box::new(test_harness::TestToolRegistration::new(root)));
        }
    }
}

/// Release builds: no env-var read, no plugin push. The function body
/// collapses to a single drop of the `&mut Vec` argument so the call site
/// in `collect_plugins` stays uniform.
#[cfg(not(any(test, feature = "test-harness")))]
fn maybe_register_test_plugins(_plugins: &mut Vec<Box<dyn Tool>>) {}

#[cfg(any(test, feature = "test-harness"))]
mod test_harness {
    //! Inline minimal `Tool` impl used by `MODELTAP_TEST_PLUGINS=test-tool`.
    //!
    //! Distinct from `modeltap_acceptance::test_tool::TestTool` — that crate
    //! is the canonical TestTool for unit-style acceptance tests, but it is
    //! not linked into the modeltap binary. The struct below is a parallel
    //! minimal impl wired straight into the registry so the binary, when
    //! built with `feature = "test-harness"`, can be driven end-to-end by
    //! the US-23 cache acceptance suite without dragging in
    //! `modeltap-acceptance` as a binary dependency (which would invert
    //! the workspace's test-crate-imports-app-crate edge).
    //!
    //! The two TestTools agree on the constants `TEST_TOOL_NAME = "test-tool"`,
    //! `TEST_MODEL_ID = "test-model-7b"`, and the synthetic file name
    //! `"test-model-7b.gguf"` so a fixture that wrote the seed file for
    //! either crate sees the same model on the other side.

    use std::path::{Path, PathBuf};

    use async_trait::async_trait;
    use modeltap_core::domain::inspect::{
        InspectError, ModelDetail, ModelId, SearchPathEntry, SearchPathSource, ToolDetail,
    };
    use modeltap_core::{
        DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, DisplayLabel, Format,
        LinkError, LinkOutcome, LinkResult, ModelMeta, ModelStatus, Tool, ToolId,
    };

    pub(super) const TEST_TOOL_NAME: ToolId = ToolId("test-tool");
    const ACCEPTED: &[Format] = &[Format::Gguf];
    const TEST_MODEL_ID: &str = "test-model-7b";
    const TEST_MODEL_DISPLAY: &str = "Test Model 7B";
    const TEST_MODEL_FILENAME: &str = "test-model-7b.gguf";
    const TEST_TOOL_VERSION: &str = "test-1.0.0";

    pub(super) struct TestToolRegistration {
        root: PathBuf,
    }

    impl TestToolRegistration {
        pub(super) fn new(root: PathBuf) -> Self {
            Self { root }
        }
        fn model_path(&self) -> PathBuf {
            self.root.join(TEST_MODEL_FILENAME)
        }
    }

    #[async_trait]
    impl Tool for TestToolRegistration {
        fn name(&self) -> ToolId {
            TEST_TOOL_NAME
        }
        fn accepted_formats(&self) -> &'static [Format] {
            ACCEPTED
        }
        async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
            let path = self.model_path();
            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            Ok(vec![DiscoveredModel {
                id_in_tool: TEST_MODEL_ID.to_string(),
                display_label: DisplayLabel::from(TEST_MODEL_DISPLAY),
                format: Format::Gguf,
                size_bytes,
                on_disk_path: path,
                status: ModelStatus::Healthy,
            }])
        }
        async fn link(
            &self,
            _canonical_src: &Path,
            model: &ModelMeta,
        ) -> Result<LinkOutcome, LinkError> {
            Ok(LinkOutcome {
                tool: TEST_TOOL_NAME,
                model_id_in_tool: model.id_in_tool.clone(),
                result: LinkResult::AlreadyLinked {
                    canonical: model.on_disk_path.clone(),
                    target: model.on_disk_path.clone(),
                    inode: 0,
                },
            })
        }
        async fn delete_one(&self, model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
            Ok(DeleteOutcome {
                tool: TEST_TOOL_NAME,
                model_id_in_tool: model.id_in_tool.clone(),
                bytes_freed: 0,
                registration_removed: true,
                file_deleted: false,
                failure_reason: None,
            })
        }
        async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
            Ok(vec![])
        }
        async fn inspect_tool(&self) -> Result<ToolDetail, InspectError> {
            // Step 02-01 (US-21) AC-21-3 seam: when
            // `MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED=1` is set on the
            // process, simulate the default-Unsupported path so the
            // tool-detail acceptance suite can drive AC-21-3 ("Undetectable
            // version"), AC-21-4 ("Last error surfaces from cache"), and
            // AC-21-7 ("Esc returns") without modifying any production
            // plugin. Mirrors the same seam in
            // `modeltap-acceptance::test_tool::TestTool::inspect_tool`. Step
            // 02-02 lands the Ollama inspect_tool override; until then every
            // production plugin uses the trait default (Unsupported) and so
            // does this in-binary test-harness registration when the env-var
            // is set.
            if std::env::var("MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED").as_deref() == Ok("1") {
                return Err(InspectError::Unsupported {
                    tool: TEST_TOOL_NAME,
                });
            }
            Ok(ToolDetail {
                tool_id: TEST_TOOL_NAME,
                install_path: self.root.clone(),
                detected_version: Some(TEST_TOOL_VERSION.to_string()),
                plugin_version: "modeltap-app test-harness::TestToolRegistration 0.0.0".to_string(),
                search_paths: vec![SearchPathEntry {
                    path: self.root.clone(),
                    source: SearchPathSource::Default,
                }],
                model_count: 1,
                disk_usage_bytes: std::fs::metadata(self.model_path())
                    .map(|m| m.len())
                    .unwrap_or(0),
                largest_model: Some(ModelId::from(TEST_MODEL_ID)),
                last_scan_at: None,
                last_scan_duration_ms: None,
                last_error: None,
                last_error_at: None,
            })
        }
        async fn inspect_model(&self, id: &ModelId) -> Result<ModelDetail, InspectError> {
            use std::collections::BTreeMap;
            if id.as_str() != TEST_MODEL_ID {
                return Err(InspectError::FileReadable {
                    path: self.root.join(format!("{}.gguf", id.as_str())),
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "unknown test model"),
                });
            }
            let mut metadata_kv = BTreeMap::new();
            metadata_kv.insert("test.kind".to_string(), "synthetic".to_string());
            Ok(ModelDetail {
                model_id: id.clone(),
                format: Some("GGUF (synthetic)".to_string()),
                quantisation: Some("Q4_K_M".to_string()),
                architecture: Some("test-architecture".to_string()),
                parameters: Some(7.0),
                context_length: Some(4096),
                metadata_kv,
                introspected_at: None,
            })
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests for the registry seam.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// `collect_plugins` honours `MODELTAP_TEST_PLUGINS=test-tool` under
    /// `cfg(test)`. Asserts the resulting Vec contains a plugin whose
    /// `name()` is `"test-tool"`.
    ///
    /// Env-var manipulation is consolidated into one test (single thread of
    /// observation) so it cannot race with sibling tests. `inventory` may
    /// have linker-elided most plugin crates in this lib unittest binary
    /// (no `use modeltap_plugin_X as _;` here), so we filter by name
    /// instead of asserting on count.
    #[test]
    fn registry_honours_test_plugins_env_var_under_cfg_test() {
        // Branch 1: env var absent -> no test-tool entry.
        std::env::remove_var("MODELTAP_TEST_PLUGINS");
        std::env::remove_var("MODELTAP_TEST_TOOL_ROOT");
        let before = collect_plugins();
        assert!(
            !before.iter().any(|p| p.name().0 == "test-tool"),
            "test-tool must NOT be registered without the env var"
        );

        // Branch 2: env var present -> exactly one test-tool entry appended.
        std::env::set_var("MODELTAP_TEST_PLUGINS", "test-tool");
        let after = collect_plugins();
        let count = after.iter().filter(|p| p.name().0 == "test-tool").count();
        assert_eq!(
            count, 1,
            "MODELTAP_TEST_PLUGINS=test-tool must register exactly one TestTool plugin"
        );

        // Cleanup so the env var does not leak into sibling tests.
        std::env::remove_var("MODELTAP_TEST_PLUGINS");
    }
}
