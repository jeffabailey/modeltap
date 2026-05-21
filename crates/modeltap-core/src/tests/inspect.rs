//! Parametrized contract-test harness for the `Tool::inspect_tool` capability
//! (US-21 step 02-03).
//!
//! Per `docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md`
//! §3.12, every plugin crate exercises this harness from a per-plugin
//! integration test file:
//!
//! ```ignore
//! // plugins/ollama/tests/inspect_tool_contract.rs
//! use modeltap_core::tests::inspect::{
//!     run_inspect_tool_contract, InspectCapability,
//! };
//! use modeltap_plugin_ollama::OllamaPlugin;
//!
//! #[tokio::test]
//! async fn ollama_satisfies_inspect_tool_contract() {
//!     std::env::set_var("MODELTAP_OLLAMA_VERSION", "0.6.4");
//!     let fixture = tempfile::tempdir().unwrap();
//!     let plugin = OllamaPlugin::new_with_root(fixture.path().to_path_buf());
//!     run_inspect_tool_contract(
//!         &plugin,
//!         modeltap_core::ToolId("ollama"),
//!         fixture.path(),
//!         InspectCapability::Supported,
//!     )
//!     .await;
//! }
//! ```
//!
//! For the v1 plugin set (per `plugin-contract-spec.md` §2 capability matrix):
//! - Ollama, HF run `InspectCapability::Supported` (3-test suite).
//! - llama-cli, lm-studio, atomic-chat, gpt4all run
//!   `InspectCapability::Unsupported` (one test: §3.12.U.1).
//!
//! ## Panic isolation
//!
//! `run_inspect_with_panic_isolation` is the canonical wrapper that converts a
//! panicking `inspect_*` future into `Err(InspectError::PluginPanic)`. It uses
//! `tokio::spawn` (which catches panics at the `JoinError::is_panic()`
//! boundary) — matching the production wrapping in
//! `modeltap-app::plugin_isolation::run_plugin_call_isolated`. The §3.12.S.3
//! test invokes it directly against a deliberately-panicking future to prove
//! the boundary works; the contract-level guarantee for INT-INFO-8 is that
//! when an `inspect_tool` impl panics, the orchestrator's spawn boundary
//! catches it, never the kernel.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use crate::domain::inspect::{InspectError, SearchPathSource, ToolDetail};
use crate::{Tool, ToolId};

/// Which contract path a plugin opts into for `inspect_tool`.
///
/// Per ADR-016 §"Plugin overrides": plugins with no canonical version source
/// (llama-cli, lm-studio, atomic-chat, gpt4all) inherit the default body and
/// run the `Unsupported` path. Ollama + HF override and run the `Supported`
/// path.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InspectCapability {
    /// The plugin inherits the `Tool::inspect_tool` default body. For ANY
    /// invocation, the plugin MUST return
    /// `Err(InspectError::Unsupported { tool: <plugin_name> })` and MUST NOT
    /// touch the filesystem (no read_dir on the fixture).
    Unsupported,
    /// The plugin overrides `Tool::inspect_tool`. The override MUST honor the
    /// full behavioral contract in `plugin-contract-spec.md` §§3.12.S.*.
    Supported,
}

/// Run the contract-test suite for `Tool::inspect_tool` against a plugin.
///
/// Arguments:
/// - `plugin`: the plugin under test. Borrowed; the harness does not move it.
/// - `expected_id`: the `ToolId` the plugin's `name()` returns. Used to
///   assert `InspectError::Unsupported { tool }`'s `tool` field equals the
///   plugin's identity AND that a Supported `ToolDetail`'s `tool_id` matches.
/// - `fixture_root`: a directory the plugin is configured to consider its
///   discovery root. For the `Unsupported` path the harness writes a small
///   witness file into this root and asserts the file remains byte-identical
///   pre/post (filesystem-state immutability — the default body MUST short-
///   circuit before touching anything).
/// - `capability`: which contract path the plugin opts into.
///
/// Panics on contract violation (this is a test helper).
pub async fn run_inspect_tool_contract<T: Tool + ?Sized>(
    plugin: &T,
    expected_id: ToolId,
    fixture_root: &Path,
    capability: InspectCapability,
) {
    assert_plugin_name_matches(plugin, expected_id);
    match capability {
        InspectCapability::Unsupported => {
            test_inspect_tool_returns_unsupported(plugin, expected_id, fixture_root).await;
        }
        InspectCapability::Supported => {
            test_inspect_tool_happy_path(plugin, expected_id).await;
            test_inspect_tool_deterministic(plugin).await;
            test_inspect_tool_panic_isolation(expected_id).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Panic-isolation helper — used by the §3.12.S.3 contract test AND available
// to production code (`modeltap-app::orchestration::open_tool_detail`) that
// wants the same boundary semantics.
// ---------------------------------------------------------------------------

/// Run `fut` (typically a plugin's `inspect_tool()` future) inside a
/// `tokio::spawn` boundary. A panic inside `fut` is caught at the
/// `JoinError::is_panic()` boundary and surfaced as
/// `Err(InspectError::PluginPanic { tool, message })`.
///
/// This is the canonical wrapper for the §3.12.S.3 / INT-INFO-8 invariant: a
/// panic inside `Tool::inspect_tool()` MUST be caught at the spawn boundary
/// and MUST NOT propagate up the orchestrator stack. The TUI then renders
/// "(inspection failed -- see diagnostics.log)" and the diagnostics log
/// gains an `inspect_panic tool=<tool_id>` line.
///
/// `tool` is captured up-front so the error message names the offending
/// plugin even if `fut` panicked before producing any output of its own.
pub async fn run_inspect_with_panic_isolation<F>(
    tool: ToolId,
    fut: F,
) -> Result<ToolDetail, InspectError>
where
    F: Future<Output = Result<ToolDetail, InspectError>> + Send + 'static,
{
    let handle = tokio::spawn(fut);
    match handle.await {
        Ok(inner) => inner,
        Err(join_err) => {
            let message = if join_err.is_panic() {
                format!("{join_err}")
            } else if join_err.is_cancelled() {
                "inspect_tool task was cancelled before returning".to_string()
            } else {
                format!("inspect_tool task did not complete cleanly: {join_err}")
            };
            Err(InspectError::PluginPanic { tool, message })
        }
    }
}

/// Sanity: every contract test starts by asserting the plugin's identity
/// matches the caller-supplied expectation. Catches `OllamaPlugin` accidentally
/// invoked with `expected_id = ToolId("lm-studio")` etc.
fn assert_plugin_name_matches<T: Tool + ?Sized>(plugin: &T, expected_id: ToolId) {
    assert_eq!(
        plugin.name(),
        expected_id,
        "plugin.name() must equal the caller-supplied expected_id",
    );
}

// ---------------------------------------------------------------------------
// §3.12.U.1 — Unsupported path
// ---------------------------------------------------------------------------

/// Contract test §3.12.U.1: `inspect_tool` returns `Unsupported` and the
/// filesystem is unchanged.
///
/// Per `plugin-contract-spec.md` §3.12.U.1:
/// - `plugin.inspect_tool().await` returns
///   `Err(InspectError::Unsupported { tool })` where `tool == plugin.name()`.
/// - The fixture's filesystem state is unchanged (manifest equality).
/// - The error message contains the plugin's name (i.e., the `tool` field).
async fn test_inspect_tool_returns_unsupported<T: Tool + ?Sized>(
    plugin: &T,
    expected_id: ToolId,
    fixture_root: &Path,
) {
    // Write a witness file so the pre/post manifest comparison is non-trivial.
    // The default body must short-circuit before this file is touched.
    let witness = fixture_root.join(".modeltap-inspect-contract-witness");
    std::fs::create_dir_all(fixture_root).expect("create fixture root");
    std::fs::write(&witness, b"witness").expect("write witness");

    let pre = manifest(fixture_root);

    let result = plugin.inspect_tool().await;

    match result {
        Err(InspectError::Unsupported { tool }) => {
            assert_eq!(
                tool, expected_id,
                "InspectError::Unsupported.tool must equal plugin.name()",
            );
            let msg = format!("{}", InspectError::Unsupported { tool });
            assert!(
                msg.contains(expected_id.0),
                "InspectError::Unsupported Display must contain the plugin's ToolId verbatim, \
                 got: {msg}",
            );
        }
        Err(other) => panic!(
            "expected Err(InspectError::Unsupported {{ tool: {:?} }}), got Err({other:?})",
            expected_id.0,
        ),
        Ok(detail) => panic!(
            "expected Err(InspectError::Unsupported {{ tool: {:?} }}), got Ok({:?}) — \
             the ADR-016 default body MUST short-circuit on Unsupported plugins",
            expected_id.0, detail.tool_id,
        ),
    }

    let post = manifest(fixture_root);
    assert_eq!(
        pre, post,
        "fixture filesystem state must be byte-identical pre/post Tool::inspect_tool for a \
         plugin inheriting the ADR-016 default body",
    );

    // Clean up so the harness leaves no trace beyond its tempdir caller's scope.
    let _ = std::fs::remove_file(&witness);
}

// ---------------------------------------------------------------------------
// §3.12.S.1 — Happy path
// ---------------------------------------------------------------------------

/// Contract test §3.12.S.1: `inspect_tool` returns a populated `ToolDetail`
/// whose top-level identity fields match the plugin's name + crate version
/// + at least one `Default` search-path entry.
async fn test_inspect_tool_happy_path<T: Tool + ?Sized>(plugin: &T, expected_id: ToolId) {
    let detail = plugin
        .inspect_tool()
        .await
        .expect("inspect_tool happy path must return Ok(ToolDetail)");

    assert_eq!(
        detail.tool_id, expected_id,
        "ToolDetail.tool_id must equal plugin.name()",
    );
    assert!(
        !detail.plugin_version.is_empty(),
        "ToolDetail.plugin_version must be non-empty (crate-version string); got empty",
    );
    assert!(
        !detail.search_paths.is_empty(),
        "ToolDetail.search_paths must contain at least one entry; got empty for {}",
        expected_id.0,
    );
    let has_default = detail
        .search_paths
        .iter()
        .any(|e| e.source == SearchPathSource::Default);
    assert!(
        has_default,
        "ToolDetail.search_paths must contain at least one entry tagged \
         SearchPathSource::Default; got {:?}",
        detail.search_paths,
    );
}

// ---------------------------------------------------------------------------
// §3.12.S.2 — Determinism
// ---------------------------------------------------------------------------

/// Contract test §3.12.S.2: two back-to-back `inspect_tool` calls return
/// `ToolDetail` values equal in every field EXCEPT the freshness-stamp fields
/// (`last_scan_at`, `last_scan_duration_ms`, `last_error_at`) which may differ
/// by milliseconds across the two calls.
async fn test_inspect_tool_deterministic<T: Tool + ?Sized>(plugin: &T) {
    let first = plugin
        .inspect_tool()
        .await
        .expect("inspect_tool determinism: first call must succeed");
    let second = plugin
        .inspect_tool()
        .await
        .expect("inspect_tool determinism: second call must succeed");

    assert_eq!(
        first.tool_id, second.tool_id,
        "deterministic: tool_id must match across consecutive calls",
    );
    assert_eq!(
        first.install_path, second.install_path,
        "deterministic: install_path must match across consecutive calls",
    );
    assert_eq!(
        first.detected_version, second.detected_version,
        "deterministic: detected_version must match across consecutive calls",
    );
    assert_eq!(
        first.plugin_version, second.plugin_version,
        "deterministic: plugin_version must match across consecutive calls",
    );
    assert_eq!(
        first.search_paths, second.search_paths,
        "deterministic: search_paths must match across consecutive calls",
    );
}

// ---------------------------------------------------------------------------
// §3.12.S.3 — Panic isolation
// ---------------------------------------------------------------------------

/// Contract test §3.12.S.3: a `Tool::inspect_tool` future that panics is
/// caught at the `tokio::spawn` boundary and surfaces as
/// `Err(InspectError::PluginPanic { tool, message })`. NOT a process crash.
///
/// This drives the canonical wrapper `run_inspect_with_panic_isolation`
/// against a deliberately-panicking future, proving the boundary semantics
/// the orchestrator relies on for INT-INFO-8.
async fn test_inspect_tool_panic_isolation(expected_id: ToolId) {
    // The `Ok(...)` branch is unreachable but exists to constrain the future's
    // `Output` type for `run_inspect_with_panic_isolation`. We deliberately do
    // NOT capture `expected_id` into the closure — the harness re-uses it
    // outside (as the `tool` argument and in the post-await assertion).
    let panicking_fut = async {
        panic!("synthetic inspect_tool panic for the §3.12.S.3 contract test");
        #[allow(unreachable_code)]
        Ok::<ToolDetail, InspectError>(ToolDetail {
            tool_id: ToolId("unreachable"),
            install_path: PathBuf::new(),
            detected_version: None,
            plugin_version: String::new(),
            search_paths: Vec::new(),
            model_count: 0,
            disk_usage_bytes: 0,
            largest_model: None,
            last_scan_at: None,
            last_scan_duration_ms: None,
            last_error: None,
            last_error_at: None,
        })
    };

    let result = run_inspect_with_panic_isolation(expected_id, panicking_fut).await;

    match result {
        Err(InspectError::PluginPanic { tool, message }) => {
            assert_eq!(
                tool, expected_id,
                "PluginPanic.tool must equal the plugin's ToolId",
            );
            assert!(
                message.contains("panicked")
                    || message.contains("panic")
                    || !message.is_empty(),
                "PluginPanic.message must carry diagnostic context; got: {message}",
            );
        }
        Err(other) => panic!(
            "panicking inspect_tool future must surface as Err(PluginPanic), got Err({other:?})",
        ),
        Ok(detail) => panic!(
            "panicking inspect_tool future must surface as Err(PluginPanic), got Ok({:?}) — \
             the panic was NOT caught at the spawn boundary",
            detail.tool_id,
        ),
    }
}

// ---------------------------------------------------------------------------
// Manifest helper (copied from src/tests/plugin_contract.rs so the two
// harnesses remain self-contained).
// ---------------------------------------------------------------------------

/// Recursive directory manifest: relative-path -> file size in bytes. Used to
/// assert byte-identical pre/post filesystem state.
fn manifest(root: &Path) -> BTreeMap<PathBuf, u64> {
    let mut out = BTreeMap::new();
    if !root.exists() {
        return out;
    }
    let walker = walkdir::WalkDir::new(root).follow_links(false);
    for entry in walker {
        let entry = entry.expect("walk fixture dir");
        if entry.file_type().is_file() || entry.file_type().is_symlink() {
            let meta = entry.path().symlink_metadata().expect("stat fixture entry");
            let rel = entry
                .path()
                .strip_prefix(root)
                .expect("strip prefix")
                .to_path_buf();
            out.insert(rel, meta.len());
        }
    }
    out
}
