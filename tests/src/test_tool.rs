//! In-process `TestTool` — the harness-side plugin used by acceptance tests
//! to drive the orchestrator end-to-end without touching a real on-disk
//! Ollama/HF/LM-Studio install.
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/acceptance-test-plan.md`
//! §3 "The walking-skeleton in-process TestTool" and OQ-3.
//!
//! Lifecycle: a test fixture constructs a `TestTool { root }` where `root` is
//! a `tempfile::TempDir` path containing one synthetic `.gguf` file. The
//! harness then either:
//!
//! 1. drives the tool directly (unit tests below, exercising `discover`,
//!    `inspect_tool`, `inspect_model`), or
//! 2. registers it into the modeltap composition root via the cfg-gated
//!    `MODELTAP_TEST_PLUGINS=test-tool` env-var seam in
//!    `crates/modeltap-app/src/registry.rs`. That seam is the integration
//!    point for the US-23 cache acceptance suite (step 01-05+).
//!
//! `TestTool` implements the FULL 9-method `Tool` trait — `name`,
//! `accepted_formats`, `discover`, `link`, `delete_one`, `delete_all`,
//! `delete_folder`, `inspect_tool`, `inspect_model`. Methods unused by the
//! acceptance suites return stub values (`Ok(())`-style no-op outcomes) so
//! they never panic if a future test reaches for them; `inspect_tool` and
//! `inspect_model` return populated values to drive the SQLite-cache
//! acceptance scenarios.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use modeltap_core::domain::inspect::{
    InspectError, ModelDetail, ModelId, SearchPathEntry, SearchPathSource, ToolDetail,
};
use modeltap_core::{
    DeleteError, DeleteOutcome, DiscoverError, DiscoveredModel, DisplayLabel, Format, LinkError,
    LinkOutcome, LinkResult, ModelMeta, ModelStatus, Tool, ToolId,
};

/// Stable tool-id for the in-process TestTool. The string lives behind a
/// `ToolId` newtype because plugin authors construct `ToolId` from a
/// `&'static str` only — runtime data cannot accidentally become a `ToolId`.
pub const TEST_TOOL_NAME: ToolId = ToolId("test-tool");

/// Formats the TestTool claims to host. GGUF is the only format the
/// acceptance suites' synthetic file pretends to be.
const ACCEPTED: &[Format] = &[Format::Gguf];

/// The synthetic model id the TestTool reports — stable so acceptance tests
/// can hard-code expectations against `id_in_tool`.
pub const TEST_MODEL_ID: &str = "test-model-7b";

/// Display label paired with `TEST_MODEL_ID` in `discover()`.
const TEST_MODEL_DISPLAY: &str = "Test Model 7B";

/// File name written under `root/` by the test fixture before constructing
/// `TestTool`. The fixture creates the file; `TestTool::discover` does not.
pub const TEST_MODEL_FILENAME: &str = "test-model-7b.gguf";

/// The synthetic version `inspect_tool()` reports. Acceptance tests assert
/// against the literal so accidental drift is caught at the test boundary.
pub const TEST_TOOL_VERSION: &str = "test-1.0.0";

/// Metadata key that `inspect_model()` returns. Used by the SQLite-cache
/// acceptance tests (US-23, step 01-05+) to assert the round-trip from
/// `Tool::inspect_model` -> cache write -> cache read preserves plugin-defined
/// KVs verbatim.
pub const TEST_METADATA_KIND_KEY: &str = "test.kind";

/// Value paired with `TEST_METADATA_KIND_KEY`. "synthetic" marks the model as
/// fixture-origin so any test that lets a TestTool leak into a production
/// inventory dump can be spotted by the keyword.
pub const TEST_METADATA_KIND_VALUE: &str = "synthetic";

/// The in-process TestTool — `root` points at a directory the fixture owns
/// and populates with exactly one model file. `discover()` reports that one
/// file; the inspect methods return populated `ToolDetail` / `ModelDetail`
/// values that the SQLite-cache layer (US-23) round-trips through.
///
/// Holds only the root path so the type stays cheap to clone and so the
/// `MODELTAP_TEST_PLUGINS` env-var seam can construct one from a single
/// `PathBuf` read out of `MODELTAP_TEST_TOOL_ROOT`.
pub struct TestTool {
    root: PathBuf,
}

impl TestTool {
    /// Construct a TestTool rooted at `root`. The fixture is responsible for
    /// creating `root` (typically a `tempfile::TempDir`) and writing the
    /// synthetic model file before calling `discover()`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Absolute path the test fixture should write the synthetic model file
    /// to before invoking `discover()`. Exposed so the fixture and the tool
    /// agree on the path without duplicating the join logic.
    pub fn model_path(&self) -> PathBuf {
        self.root.join(TEST_MODEL_FILENAME)
    }
}

#[async_trait]
impl Tool for TestTool {
    fn name(&self) -> ToolId {
        TEST_TOOL_NAME
    }

    fn accepted_formats(&self) -> &'static [Format] {
        ACCEPTED
    }

    async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
        // The fixture writes one synthetic file; the tool reports exactly one
        // `DiscoveredModel` referencing it. Size is read from the on-disk
        // file so the acceptance tests can verify the byte count flowed
        // through correctly without the fixture having to communicate it
        // out-of-band.
        let path = self.model_path();
        let size_bytes = match std::fs::metadata(&path) {
            Ok(meta) => meta.len(),
            Err(_) => 0, // fixture didn't seed -- still report the model so the
                         // discover() AC ("returns exactly one model") holds.
        };
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
        // The TestTool never touches the filesystem on link -- it pretends
        // the target was already linked. Returning `AlreadyLinked` keeps the
        // outcome shape honest (the orchestrator's unify path branches on
        // the result variant) without requiring real hardlink syscalls.
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
        // Synthetic delete: report success with zero bytes freed so the
        // accounting in the acceptance suite is deterministic.
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
        // No persistent state to clear -- return an empty vec to mean
        // "nothing was registered with this tool" rather than fabricating
        // synthetic outcomes that would skew acceptance-test counts.
        Ok(vec![])
    }

    // delete_folder defaults to `Err(DeleteError::Unsupported)` -- the TestTool
    // has no folder-grouped layout (mirrors Ollama / lm-studio / atomic-chat).
    // No override needed.

    async fn inspect_tool(&self) -> Result<ToolDetail, InspectError> {
        // Step 02-03 (US-21 / AC-21-9 / INT-INFO-8) panic-isolation seam:
        // when `MODELTAP_TEST_TOOL_INSPECT_PANIC=1` is set, panic deliberately
        // from inspect_tool() so the acceptance suite can drive the
        // orchestrator's panic-catch boundary end-to-end (real modeltap
        // binary, real catch_unwind wrap, real diagnostics.log write). Step
        // 02-03 part 1 (12f9559) landed the in-harness panic-isolation
        // contract test via `run_inspect_with_panic_isolation`; this seam
        // exercises the END-TO-END orchestrator boundary in the modeltap
        // binary. Placed at the TOP of the function so the panic precedes
        // any normal logic (including the Unsupported seam check below).
        if std::env::var("MODELTAP_TEST_TOOL_INSPECT_PANIC").as_deref() == Ok("1") {
            panic!("MODELTAP_TEST_TOOL_INSPECT_PANIC=1 -- deliberate test panic in inspect_tool");
        }
        // Step 02-01 (US-21) AC-21-3 seam: simulate the default-Unsupported
        // path so the tool-detail acceptance suite can drive AC-21-3
        // ("Undetectable version") + AC-21-4 ("Last error surfaces from
        // cache") + AC-21-7 ("Esc returns") without modifying any production
        // plugin. Step 02-02 lands the Ollama inspect_tool override; until
        // then, every production plugin uses the trait default (Unsupported)
        // and so does this seam when MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED=1.
        if std::env::var("MODELTAP_TEST_TOOL_INSPECT_UNSUPPORTED").as_deref() == Ok("1") {
            return Err(InspectError::Unsupported {
                tool: TEST_TOOL_NAME,
            });
        }
        Ok(ToolDetail {
            tool_id: TEST_TOOL_NAME,
            install_path: self.root.clone(),
            detected_version: Some(TEST_TOOL_VERSION.to_string()),
            // Plugin crate version -- the TestTool lives in the acceptance
            // crate, but for cache-round-trip purposes we surface a stable
            // version string. Matching the `modeltap-acceptance` crate
            // version would couple this to Cargo.toml; the literal here is
            // stable enough for the cache layer to assert on.
            plugin_version: "modeltap-acceptance-test-tool 0.0.0".to_string(),
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
        // The TestTool reports only `TEST_MODEL_ID`. Inspect for any other
        // id is a FileReadable error -- matches the production contract that
        // unknown models surface an error rather than an all-empty
        // ModelDetail (see Tool::inspect_model docstring).
        if id.as_str() != TEST_MODEL_ID {
            return Err(InspectError::FileReadable {
                path: self.root.join(format!("{}.gguf", id.as_str())),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "unknown test model"),
            });
        }
        let mut metadata_kv = BTreeMap::new();
        metadata_kv.insert(
            TEST_METADATA_KIND_KEY.to_string(),
            TEST_METADATA_KIND_VALUE.to_string(),
        );
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

// ---------------------------------------------------------------------------
// Unit tests -- exercise the three inspect/discover paths the TestTool will
// be relied on for. Per the step's AC #6: prove discover, inspect_tool, and
// inspect_model each produce the documented shape.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a TestTool over a tempdir with the synthetic model file
    /// pre-written so `discover()` reports a non-zero `size_bytes`.
    fn seeded_test_tool() -> (tempfile::TempDir, TestTool) {
        let dir = tempfile::tempdir().expect("create tempdir for TestTool");
        let path = dir.path().join(TEST_MODEL_FILENAME);
        std::fs::write(&path, b"synthetic-gguf-bytes").expect("write synthetic model");
        let tool = TestTool::new(dir.path());
        (dir, tool)
    }

    #[tokio::test]
    async fn test_tool_discover_returns_one_model() {
        let (_dir, tool) = seeded_test_tool();
        let models = tool.discover().await.expect("discover succeeds");
        assert_eq!(
            models.len(),
            1,
            "TestTool::discover must return exactly one model"
        );
        assert_eq!(models[0].id_in_tool, TEST_MODEL_ID);
        assert_eq!(models[0].format, Format::Gguf);
        assert_eq!(models[0].on_disk_path, tool.model_path());
        assert_eq!(models[0].size_bytes, b"synthetic-gguf-bytes".len() as u64);
    }

    #[tokio::test]
    async fn test_tool_inspect_tool_returns_test_1_0_0() {
        let (_dir, tool) = seeded_test_tool();
        let detail = tool.inspect_tool().await.expect("inspect_tool succeeds");
        assert_eq!(detail.tool_id, TEST_TOOL_NAME);
        assert_eq!(detail.detected_version, Some(TEST_TOOL_VERSION.to_string()));
        assert_eq!(detail.model_count, 1);
        assert_eq!(detail.search_paths.len(), 1);
        assert_eq!(detail.search_paths[0].source, SearchPathSource::Default);
        assert_eq!(detail.search_paths[0].path, tool.root);
        assert_eq!(detail.largest_model, Some(ModelId::from(TEST_MODEL_ID)));
    }

    #[tokio::test]
    async fn test_tool_inspect_model_returns_synthetic_metadata() {
        let (_dir, tool) = seeded_test_tool();
        let detail = tool
            .inspect_model(&ModelId::from(TEST_MODEL_ID))
            .await
            .expect("inspect_model succeeds");
        assert_eq!(detail.model_id, ModelId::from(TEST_MODEL_ID));
        assert_eq!(
            detail
                .metadata_kv
                .get(TEST_METADATA_KIND_KEY)
                .map(String::as_str),
            Some(TEST_METADATA_KIND_VALUE),
            "metadata_kv must contain test.kind = synthetic"
        );
        assert_eq!(detail.parameters, Some(7.0));
        assert_eq!(detail.context_length, Some(4096));
    }

    #[tokio::test]
    async fn test_tool_inspect_model_unknown_id_returns_file_readable_error() {
        let (_dir, tool) = seeded_test_tool();
        let err = tool
            .inspect_model(&ModelId::from("no-such-model"))
            .await
            .expect_err("unknown id must surface error, not empty ModelDetail");
        assert!(matches!(err, InspectError::FileReadable { .. }));
    }

    #[test]
    fn test_tool_accepts_gguf_only() {
        let (_dir, tool) = seeded_test_tool();
        assert_eq!(tool.accepted_formats(), &[Format::Gguf]);
        assert_eq!(tool.name(), TEST_TOOL_NAME);
    }
}
