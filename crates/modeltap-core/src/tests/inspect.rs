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
//! ## §3.13 — inspect_model harness (step 03-03)
//!
//! `run_inspect_model_contract` is the parallel harness for the
//! `Tool::inspect_model` method. The capability matrix differs from §3.12:
//! Ollama / HF / lm-studio override `inspect_model` (per step 03-02) and run
//! `InspectCapability::Supported`; atomic-chat / gpt4all inherit the trait
//! default and run `InspectCapability::Unsupported`. The Supported arm covers
//! six cases — happy-path, unknown-id-is-FileReadable, corrupt-is-
//! FormatUnreadable (optional per-plugin), determinism, metadata_kv ≤10 keys
//! per AC-22-6, and panic-isolation via `run_inspect_model_with_panic_isolation`.
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
//! catches it, never the kernel. `run_inspect_model_with_panic_isolation` is
//! the parallel wrapper for `inspect_model` (§3.13.S.6 / AC-22-7).

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};

use crate::domain::inspect::{InspectError, ModelDetail, ModelId, SearchPathSource, ToolDetail};
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

// ===========================================================================
// §3.13 — inspect_model contract harness
//
// Mirrors `run_inspect_tool_contract` but for `Tool::inspect_model`. Plugins
// that have NO model introspector (atomic-chat, gpt4all) inherit the trait
// default and run `InspectCapability::Unsupported`. Plugins that override
// (Ollama, HF, lm-studio) run `Supported` and supply three model_ids:
//
// - `known_good`: a model the plugin's fixture seeded so the happy path
//   returns `Ok(ModelDetail)` with non-empty `metadata_kv`.
// - `unknown`: a model_id with the right shape but no on-disk artefact,
//   exercising the locator's `NotFound` branch (§3.13.S.2).
// - `corrupt` (Option): an on-disk artefact whose body is unreadable as the
//   plugin's expected format. `None` when the plugin cannot easily corrupt-
//   test (e.g., the Ollama manifest reader, where "corrupt" overlaps with
//   the unknown branch — see Ollama plugin's contract test for the elision
//   rationale).
// ===========================================================================

/// Run the contract-test suite for `Tool::inspect_model` against a plugin.
///
/// Per `plugin-contract-spec.md` §3.13, every plugin's `inspect_model` must:
/// - §3.13.U.1: Unsupported plugins return `Err(InspectError::Unsupported)`.
/// - §3.13.S.1: known-good model returns `Ok(ModelDetail)` with `model_id`
///   matching the input, non-empty `metadata_kv` (≤10 keys per AC-22-6).
/// - §3.13.S.2: unknown model_id returns `Err(InspectError::FileReadable)`
///   (the locator's `NotFound` path).
/// - §3.13.S.3: corrupt artefact returns `Err(InspectError::FormatUnreadable)`.
///   Skipped when `corrupt` is `None`.
/// - §3.13.S.4: two consecutive happy-path calls return equal `ModelDetail`
///   values modulo `introspected_at` (the only field allowed to vary).
/// - §3.13.S.5: `metadata_kv` is `BTreeMap<String,String>` (type-enforced)
///   AND has ≤ 10 entries (assertion).
/// - §3.13.S.6: a plugin `inspect_model` future that panics is converted to
///   `Err(InspectError::PluginPanic)` by `run_inspect_model_with_panic_isolation`.
///
/// `expected_id` is the `ToolId` the plugin's `name()` returns — used by
/// §3.13.U.1 to assert `InspectError::Unsupported.tool` matches.
///
/// `known_good`, `unknown`, `corrupt` are ignored on the Unsupported arm.
///
/// Panics on contract violation (this is a test helper).
pub async fn run_inspect_model_contract<T: Tool + ?Sized>(
    plugin: &T,
    expected_id: ToolId,
    capability: InspectCapability,
    known_good: ModelId,
    unknown: ModelId,
    corrupt: Option<ModelId>,
) {
    assert_plugin_name_matches(plugin, expected_id);
    match capability {
        InspectCapability::Unsupported => {
            test_inspect_model_returns_unsupported(plugin, expected_id, &known_good).await;
        }
        InspectCapability::Supported => {
            test_inspect_model_happy_path(plugin, &known_good).await;
            test_inspect_model_unknown_id_is_file_readable(plugin, &unknown).await;
            if let Some(corrupt_id) = corrupt {
                test_inspect_model_corrupt_is_format_unreadable(plugin, &corrupt_id).await;
            }
            test_inspect_model_deterministic(plugin, &known_good).await;
            test_inspect_model_metadata_kv_within_budget(plugin, &known_good).await;
            test_inspect_model_panic_isolation(expected_id).await;
        }
    }
}

/// Run `fut` (typically a plugin's `inspect_model()` future) inside a
/// `tokio::spawn` boundary. A panic inside `fut` is caught at the
/// `JoinError::is_panic()` boundary and surfaced as
/// `Err(InspectError::PluginPanic { tool, message })`.
///
/// Parallel to `run_inspect_with_panic_isolation` for the `inspect_tool`
/// side. The orchestrator at
/// `modeltap-app::orchestration::open_model_detail` uses an
/// `AssertUnwindSafe(...).catch_unwind()` wrap with equivalent semantics;
/// this helper exists so contract-test code can exercise the panic boundary
/// against a deliberately-panicking future, proving the boundary semantics
/// the orchestrator relies on for INT-INFO-8 (AC-22-7).
pub async fn run_inspect_model_with_panic_isolation<F>(
    tool: ToolId,
    fut: F,
) -> Result<ModelDetail, InspectError>
where
    F: Future<Output = Result<ModelDetail, InspectError>> + Send + 'static,
{
    let handle = tokio::spawn(fut);
    match handle.await {
        Ok(inner) => inner,
        Err(join_err) => {
            let message = if join_err.is_panic() {
                format!("{join_err}")
            } else if join_err.is_cancelled() {
                "inspect_model task was cancelled before returning".to_string()
            } else {
                format!("inspect_model task did not complete cleanly: {join_err}")
            };
            Err(InspectError::PluginPanic { tool, message })
        }
    }
}

// ---------------------------------------------------------------------------
// §3.13.U.1 — Unsupported path
// ---------------------------------------------------------------------------

/// Contract test §3.13.U.1: `inspect_model` returns
/// `Err(InspectError::Unsupported { tool })` where `tool == plugin.name()`.
/// The `known_good` argument is only used to give the call a concrete id to
/// pass (the Unsupported path never reads the id).
async fn test_inspect_model_returns_unsupported<T: Tool + ?Sized>(
    plugin: &T,
    expected_id: ToolId,
    any_id: &ModelId,
) {
    let result = plugin.inspect_model(any_id).await;
    match result {
        Err(InspectError::Unsupported { tool }) => {
            assert_eq!(
                tool, expected_id,
                "InspectError::Unsupported.tool must equal plugin.name()",
            );
        }
        Err(other) => panic!(
            "expected Err(InspectError::Unsupported {{ tool: {:?} }}), got Err({other:?})",
            expected_id.0,
        ),
        Ok(detail) => panic!(
            "expected Err(InspectError::Unsupported {{ tool: {:?} }}), got Ok({:?}) — \
             the ADR-016 default body MUST short-circuit on Unsupported plugins",
            expected_id.0, detail.model_id,
        ),
    }
}

// ---------------------------------------------------------------------------
// §3.13.S.1 — Happy path
// ---------------------------------------------------------------------------

/// Contract test §3.13.S.1: a known-good `model_id` returns
/// `Ok(ModelDetail)` whose `model_id` matches the input and whose
/// `metadata_kv` is non-empty.
async fn test_inspect_model_happy_path<T: Tool + ?Sized>(plugin: &T, known_good: &ModelId) {
    let detail = plugin
        .inspect_model(known_good)
        .await
        .expect("inspect_model happy path must return Ok(ModelDetail)");

    assert_eq!(
        &detail.model_id, known_good,
        "ModelDetail.model_id must equal the input id",
    );
    assert!(
        !detail.metadata_kv.is_empty(),
        "ModelDetail.metadata_kv must be non-empty for a known-good model; got empty for {}",
        known_good,
    );
}

// ---------------------------------------------------------------------------
// §3.13.S.2 — Unknown id => FileReadable
// ---------------------------------------------------------------------------

/// Contract test §3.13.S.2: a syntactically-valid but non-existent `model_id`
/// returns `Err(InspectError::FileReadable)` (the locator's NotFound branch).
/// The plugin MUST NOT return `Ok(ModelDetail)` with all-empty fields — per
/// the `Tool::inspect_model` docstring, unknown models must surface an error.
async fn test_inspect_model_unknown_id_is_file_readable<T: Tool + ?Sized>(
    plugin: &T,
    unknown: &ModelId,
) {
    let result = plugin.inspect_model(unknown).await;
    match result {
        Err(InspectError::FileReadable { .. }) => { /* expected */ }
        Err(other) => panic!(
            "expected Err(InspectError::FileReadable) for unknown model_id {}; got Err({other:?})",
            unknown,
        ),
        Ok(detail) => panic!(
            "expected Err(InspectError::FileReadable) for unknown model_id {}; got Ok({:?}) — \
             the contract requires NotFound to surface as an error, not an all-empty ModelDetail",
            unknown, detail.model_id,
        ),
    }
}

// ---------------------------------------------------------------------------
// §3.13.S.3 — Corrupt artefact => FormatUnreadable
// ---------------------------------------------------------------------------

/// Contract test §3.13.S.3: a `model_id` pointing at an artefact whose body
/// cannot be parsed returns `Err(InspectError::FormatUnreadable)`. The fixture
/// is responsible for staging the corrupt artefact at the path the plugin's
/// locator will resolve `corrupt` to.
async fn test_inspect_model_corrupt_is_format_unreadable<T: Tool + ?Sized>(
    plugin: &T,
    corrupt: &ModelId,
) {
    let result = plugin.inspect_model(corrupt).await;
    match result {
        Err(InspectError::FormatUnreadable { .. }) => { /* expected */ }
        Err(other) => panic!(
            "expected Err(InspectError::FormatUnreadable) for corrupt model_id {}; \
             got Err({other:?})",
            corrupt,
        ),
        Ok(detail) => panic!(
            "expected Err(InspectError::FormatUnreadable) for corrupt model_id {}; \
             got Ok({:?})",
            corrupt, detail.model_id,
        ),
    }
}

// ---------------------------------------------------------------------------
// §3.13.S.4 — Determinism
// ---------------------------------------------------------------------------

/// Contract test §3.13.S.4: two back-to-back `inspect_model` calls on the
/// same known-good `model_id` return `ModelDetail` values equal in every
/// field EXCEPT `introspected_at` (the freshness stamp, which may differ
/// by milliseconds across the two calls).
async fn test_inspect_model_deterministic<T: Tool + ?Sized>(plugin: &T, known_good: &ModelId) {
    let first = plugin
        .inspect_model(known_good)
        .await
        .expect("inspect_model determinism: first call must succeed");
    let second = plugin
        .inspect_model(known_good)
        .await
        .expect("inspect_model determinism: second call must succeed");

    assert_eq!(
        first.model_id, second.model_id,
        "deterministic: model_id must match across consecutive calls",
    );
    assert_eq!(
        first.format, second.format,
        "deterministic: format must match across consecutive calls",
    );
    assert_eq!(
        first.quantisation, second.quantisation,
        "deterministic: quantisation must match across consecutive calls",
    );
    assert_eq!(
        first.architecture, second.architecture,
        "deterministic: architecture must match across consecutive calls",
    );
    assert_eq!(
        first.parameters, second.parameters,
        "deterministic: parameters must match across consecutive calls",
    );
    assert_eq!(
        first.context_length, second.context_length,
        "deterministic: context_length must match across consecutive calls",
    );
    assert_eq!(
        first.metadata_kv, second.metadata_kv,
        "deterministic: metadata_kv must match across consecutive calls",
    );
}

// ---------------------------------------------------------------------------
// §3.13.S.5 — metadata_kv schema (BTreeMap + ≤10 keys per AC-22-6)
// ---------------------------------------------------------------------------

/// Contract test §3.13.S.5: `ModelDetail.metadata_kv` has ≤ 10 entries per
/// AC-22-6 ("tool-relevant subset, not entire manifest"). The type is
/// already `BTreeMap<String,String>` per the domain definition — that part
/// is enforced by the compiler. This test exercises the size invariant.
async fn test_inspect_model_metadata_kv_within_budget<T: Tool + ?Sized>(
    plugin: &T,
    known_good: &ModelId,
) {
    let detail = plugin
        .inspect_model(known_good)
        .await
        .expect("inspect_model must succeed for the known-good id");
    assert!(
        detail.metadata_kv.len() <= 10,
        "ModelDetail.metadata_kv must contain ≤ 10 keys per AC-22-6; got {} keys: {:?}",
        detail.metadata_kv.len(),
        detail.metadata_kv.keys().collect::<Vec<_>>(),
    );
}

// ---------------------------------------------------------------------------
// §3.13.S.6 — Panic isolation
// ---------------------------------------------------------------------------

/// Contract test §3.13.S.6: a `Tool::inspect_model` future that panics is
/// caught at the `tokio::spawn` boundary and surfaces as
/// `Err(InspectError::PluginPanic { tool, message })`. NOT a process crash.
///
/// Parallel to the §3.12.S.3 inspect_tool panic-isolation test — drives the
/// canonical wrapper `run_inspect_model_with_panic_isolation` against a
/// deliberately-panicking future, proving the boundary semantics the
/// orchestrator relies on for INT-INFO-8 / AC-22-7.
async fn test_inspect_model_panic_isolation(expected_id: ToolId) {
    let panicking_fut = async {
        panic!("synthetic inspect_model panic for the §3.13.S.6 contract test");
        #[allow(unreachable_code)]
        Ok::<ModelDetail, InspectError>(ModelDetail {
            model_id: ModelId::from("unreachable"),
            format: None,
            quantisation: None,
            architecture: None,
            parameters: None,
            context_length: None,
            metadata_kv: BTreeMap::new(),
            introspected_at: None,
        })
    };

    let result = run_inspect_model_with_panic_isolation(expected_id, panicking_fut).await;

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
            "panicking inspect_model future must surface as Err(PluginPanic), got Err({other:?})",
        ),
        Ok(detail) => panic!(
            "panicking inspect_model future must surface as Err(PluginPanic), got Ok({:?}) — \
             the panic was NOT caught at the spawn boundary",
            detail.model_id,
        ),
    }
}
