//! Model-detail orchestrator (US-22 AC-22-* happy paths + edge cases).
//!
//! Step 03-01 part 1: composes the cached `CachedModel` row from the SQLite
//! cache with the live `Tool::inspect_model()` result from the plugin and
//! returns a unified `ModelDetail` for the TUI to render. Mirrors the
//! composition pattern of `open_tool_detail` (step 02-01).
//!
//! ## Merge semantics
//!
//! The plugin's `inspect_model()` is canonical for:
//!   - `format`, `quantisation`, `architecture`, `parameters`, `context_length`
//!   - `metadata_kv`
//!   - `introspected_at`
//!
//! The cache is canonical for:
//!   - `metadata_kv` + `metadata_introspected_at` (warm-path fallback when
//!     `inspect_model()` returns `Unsupported`)
//!
//! When `inspect_model()` returns `Ok(detail)` the orchestrator UPSERTs the
//! cache row's `metadata_kv_json` + `metadata_introspected_at` columns
//! atomically. Per AC-22-2, this is the cache writeback hook.
//!
//! ## Warm-path / 100 ms target (AC-22-1)
//!
//! When the cached row already carries a populated `metadata_kv` (non-empty)
//! AND `metadata_introspected_at` is `Some(_)`, the orchestrator skips
//! `inspect_model()` entirely and returns the cached `metadata_kv` verbatim.
//! This is the warm path that meets the K-INFO-1 100 ms target. The `[r]
//! re-introspect` keybinding bypasses this via `RunMode::ForceReintrospect`.
//!
//! ## Panic isolation
//!
//! `inspect_model()` is wrapped in `AssertUnwindSafe(...).catch_unwind()` so a
//! panicking plugin surfaces as `Err(InspectError::PluginPanic { ... })`
//! rather than unwinding through the orchestrator. The panic message is
//! written to `<diagnostics_dir>/diagnostics.log` as an `inspect_panic
//! model=<id>` line. Reuses the same panic-isolation contract as
//! `open_tool_detail::run` (INT-INFO-8 / US-21 AC-21-9 / US-22 AC-22-7).
//!
//! ## I/O policy
//!
//! Every `modeltap-store` call is wrapped in `tokio::task::spawn_blocking`
//! (per architecture-design.md §7.1). The orchestrator measures wall-clock
//! from function entry to merge completion and emits a JSONL
//! `model_detail.open_ms` event to `<log_dir>/launch.log`. Best-effort: a
//! missing log dir or unwritable file is swallowed so observability never
//! blocks the user-visible TUI transition.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use futures_util::FutureExt;
use modeltap_core::domain::inspect::{InspectError, ModelDetail, ModelId};
use modeltap_core::{Tool, ToolId};
use modeltap_store::types::CachedModel;
use modeltap_store::{Cache, CacheError, CacheOpenResult};
use serde_json::json;
use thiserror::Error;

use crate::orchestration::open_tool_detail::INSPECT_PANIC_SENTINEL;

/// Sentinel rendered in the Metadata section when `inspect_model()` returned
/// `Err(InspectError::Unsupported { .. })` — the plugin opted out (its
/// default trait body). Public so step definitions can assert against the
/// literal without duplicating the string. Per AC-22-3 / AC-22-5 (the
/// default-Unsupported render path; atomic-chat / gpt4all behavior + every
/// production plugin in this step before 03-02 overrides land).
pub const METADATA_UNSUPPORTED_SENTINEL: &str = "(metadata unsupported for this tool)";

/// Sentinel for the diagnostics filename — same shape as
/// `open_tool_detail::DIAGNOSTICS_LOG_FILENAME`. We re-declare locally rather
/// than re-export to keep the panic-isolation surface symmetric between the
/// two orchestrators while letting each evolve independently.
const DIAGNOSTICS_LOG_FILENAME: &str = "diagnostics.log";

/// How the orchestrator should treat the cache when composing the detail.
///
/// `WarmIfCached`: when the cache row's `metadata_kv` is populated AND
/// `metadata_introspected_at` is `Some(_)`, skip `inspect_model()` and
/// return the cached metadata directly — meets the K-INFO-1 100 ms warm path
/// target (AC-22-1). Otherwise call `inspect_model()` and write back.
///
/// `ForceReintrospect`: always call `inspect_model()` and write back,
/// regardless of cache state. Dispatched by the `[r] re-introspect` keymap
/// binding (AC-22-2 / AC-22-8).
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RunMode {
    WarmIfCached,
    ForceReintrospect,
}

/// Inputs into the model-detail orchestrator.
#[derive(Debug, Clone, Default)]
pub struct OpenModelDetailConfig {
    /// JSONL log directory (matches `WarmStartConfig::log_dir`). `None`
    /// disables JSONL emission entirely.
    pub log_dir: Option<PathBuf>,

    /// Directory under which `diagnostics.log` is written when a plugin's
    /// `inspect_model` panics (US-22 AC-22-7 / INT-INFO-8). Best-effort I/O
    /// policy matches `log_dir`: a missing or unwritable directory is
    /// swallowed.
    pub diagnostics_dir: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum OpenModelDetailError {
    /// `tokio::task::spawn_blocking` failed (panic or runtime shutdown).
    #[error("blocking task failed: {0}")]
    Join(#[from] tokio::task::JoinError),

    /// Cache I/O failed.
    #[error("cache I/O failed: {0}")]
    Cache(#[from] CacheError),

    /// The tool was not registered in the live plugin list. The composition
    /// root passed a `ToolId` the registry no longer knows about.
    #[error("tool {0} not in plugin registry")]
    UnknownTool(String),
}

/// Open the model-detail orchestration. Returns a merged `ModelDetail`
/// suitable for `Msg::ModelDetailReady(Box::new(detail))`.
///
/// `cache_path` may be `None` when the user launched with the cache disabled.
/// In that mode the cache half of the merge is skipped: every call goes
/// straight to `inspect_model()` and no writeback occurs.
pub async fn run(
    tool_id: ToolId,
    model_id: ModelId,
    plugin: &dyn Tool,
    cache_path: Option<&Path>,
    config: &OpenModelDetailConfig,
    mode: RunMode,
) -> Result<ModelDetail, OpenModelDetailError> {
    let run_start = Instant::now();

    // Load the cached row (if any). The warm path uses this verbatim; the
    // re-introspect path uses it only to carry the file-shape fields forward.
    let cached: Option<CachedModel> = match cache_path {
        Some(p) => load_cached_model(p, tool_id, model_id.clone()).await?,
        None => None,
    };

    // Warm path (AC-22-1): when cached metadata is populated AND we are NOT
    // forcing re-introspect, return the cached `metadata_kv` directly. Skips
    // the `inspect_model()` call entirely (the whole point of the cache).
    if mode == RunMode::WarmIfCached {
        if let Some(c) = &cached {
            if !c.metadata_kv.is_empty() && c.metadata_introspected_at.is_some() {
                let detail = build_detail_from_cache(model_id.clone(), c);
                let elapsed_ms = run_start.elapsed().as_millis() as u64;
                emit_open_event(config.log_dir.as_deref(), &model_id, elapsed_ms);
                return Ok(detail);
            }
        }
    }

    // Cold / re-introspect path: call inspect_model() under catch_unwind.
    let inspect_fut = AssertUnwindSafe(plugin.inspect_model(&model_id)).catch_unwind();
    let inspect_result = match inspect_fut.await {
        Ok(inner) => inner,
        Err(panic_payload) => {
            let message = format_panic_payload(panic_payload);
            write_diagnostics_panic_line(
                config.diagnostics_dir.as_deref(),
                tool_id,
                &model_id,
                &message,
            );
            Err(InspectError::PluginPanic {
                tool: tool_id,
                message,
            })
        }
    };

    let detail = merge(model_id.clone(), inspect_result, cached.as_ref());

    // Writeback (AC-22-2 / AC-22-5): when inspect returned Ok, UPSERT the
    // cache row's `metadata_kv_json` + `metadata_introspected_at` so the
    // next launch's warm path can hit it. Best-effort — failures are logged
    // via tracing but do NOT block the detail screen.
    if !detail.metadata_kv.is_empty() && detail.introspected_at.is_some() {
        if let Some(path) = cache_path {
            if let Err(e) = writeback_metadata(
                path,
                tool_id,
                model_id.clone(),
                detail.metadata_kv.clone(),
                detail.introspected_at,
                cached.clone(),
            )
            .await
            {
                tracing::warn!(
                    target: "modeltap.model_detail",
                    "cache writeback failed for tool={} model={}: {e}",
                    tool_id.0, model_id
                );
            }
        }
    }

    let elapsed_ms = run_start.elapsed().as_millis() as u64;
    emit_open_event(config.log_dir.as_deref(), &model_id, elapsed_ms);

    Ok(detail)
}

/// Best-effort downcast of a `catch_unwind` payload into a human-readable
/// message. Mirror of `open_tool_detail::format_panic_payload` (kept local
/// rather than re-exported so the two orchestrators can diverge if needed).
fn format_panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic payload>".to_string()
}

/// Open the cache, pull the row for `(tool_id, model_id)`, return it. All
/// rusqlite calls happen inside `spawn_blocking` per architecture-design.md
/// §7.1.
async fn load_cached_model(
    cache_path: &Path,
    tool_id: ToolId,
    model_id: ModelId,
) -> Result<Option<CachedModel>, OpenModelDetailError> {
    let path = cache_path.to_path_buf();
    let opt = tokio::task::spawn_blocking(move || -> Result<Option<CachedModel>, CacheError> {
        let opened = Cache::open(&path)?;
        let cache = match opened {
            CacheOpenResult::OpenedFresh(c)
            | CacheOpenResult::OpenedExisting(c)
            | CacheOpenResult::OpenedAfterMigration { cache: c, .. }
            | CacheOpenResult::OpenedAfterRecovery { cache: c, .. } => c,
        };
        let models = cache.models_for_tool(&tool_id)?;
        Ok(models.into_iter().find(|m| m.model_id == model_id.0))
    })
    .await??;
    Ok(opt)
}

/// Atomically UPSERT the cache row with new metadata. When a cached row
/// already exists we preserve every non-metadata field; when no row exists
/// we construct a minimum-shaped row from the model_id + tool_id (the file-
/// shape fields stay defaulted — the next discover() will fill them in).
async fn writeback_metadata(
    cache_path: &Path,
    tool_id: ToolId,
    model_id: ModelId,
    metadata_kv: BTreeMap<String, String>,
    metadata_introspected_at: Option<SystemTime>,
    cached: Option<CachedModel>,
) -> Result<(), OpenModelDetailError> {
    let path = cache_path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<(), CacheError> {
        let opened = Cache::open(&path)?;
        let cache = match opened {
            CacheOpenResult::OpenedFresh(c)
            | CacheOpenResult::OpenedExisting(c)
            | CacheOpenResult::OpenedAfterMigration { cache: c, .. }
            | CacheOpenResult::OpenedAfterRecovery { cache: c, .. } => c,
        };
        let now = SystemTime::now();
        let row = match cached {
            Some(mut c) => {
                c.metadata_kv = metadata_kv;
                c.metadata_introspected_at = metadata_introspected_at;
                c.last_validated_at = Some(now);
                c
            }
            None => CachedModel {
                model_id: model_id.0.clone(),
                tool_id,
                display_name: model_id.0.clone(),
                format: None,
                quantisation: None,
                size_bytes: 0,
                sha256: None,
                architecture: None,
                parameters_billions: None,
                context_length: None,
                dedup_group_id: None,
                metadata_kv,
                metadata_introspected_at,
                last_seen_at: now,
                last_validated_at: Some(now),
            },
        };
        cache.write_models(&tool_id, std::slice::from_ref(&row))?;
        Ok(())
    })
    .await??;
    Ok(())
}

/// Pure merge fn — exposed for unit testing.
///
/// Behavior:
/// - `Ok(detail)`: returned verbatim (inspect is canonical for every field).
/// - `Err(Unsupported)`: render with `metadata_kv = {"_status":
///   METADATA_UNSUPPORTED_SENTINEL}` so the Metadata section paints the
///   AC-22 sentinel without otherwise polluting the field grid.
/// - `Err(PluginPanic | FormatUnreadable | FileReadable)`: render with
///   `metadata_kv = {"_status": INSPECT_PANIC_SENTINEL}` so the Metadata
///   section paints "(inspection failed -- see diagnostics.log)" without
///   crashing the screen.
///
/// In every error case we still preserve the cached file-shape fields
/// (format, quantisation, size, architecture, parameters, context_length)
/// so the rest of the detail screen renders normally per AC-22-7's "the
/// other panels still render" requirement.
pub fn merge(
    model_id: ModelId,
    inspect: Result<ModelDetail, InspectError>,
    cached: Option<&CachedModel>,
) -> ModelDetail {
    match inspect {
        Ok(detail) => detail,
        Err(InspectError::Unsupported { .. }) => {
            let mut metadata_kv = BTreeMap::new();
            metadata_kv.insert(
                "_status".to_string(),
                METADATA_UNSUPPORTED_SENTINEL.to_string(),
            );
            build_error_detail(model_id, cached, metadata_kv)
        }
        Err(_) => {
            let mut metadata_kv = BTreeMap::new();
            metadata_kv.insert("_status".to_string(), INSPECT_PANIC_SENTINEL.to_string());
            build_error_detail(model_id, cached, metadata_kv)
        }
    }
}

/// Build a `ModelDetail` purely from the cached row. Used by the warm-path
/// short-circuit: the file-shape fields come from cache; the metadata
/// section comes from cache (already non-empty by the warm-path predicate).
fn build_detail_from_cache(model_id: ModelId, c: &CachedModel) -> ModelDetail {
    ModelDetail {
        model_id,
        format: c.format.clone(),
        quantisation: c.quantisation.clone(),
        architecture: c.architecture.clone(),
        parameters: c.parameters_billions,
        context_length: c.context_length,
        metadata_kv: c.metadata_kv.clone(),
        introspected_at: c.metadata_introspected_at,
    }
}

/// Build a `ModelDetail` carrying the cached file-shape fields and the
/// caller-provided `metadata_kv` (a sentinel-only map). Used for every
/// error branch in `merge`.
fn build_error_detail(
    model_id: ModelId,
    cached: Option<&CachedModel>,
    metadata_kv: BTreeMap<String, String>,
) -> ModelDetail {
    match cached {
        Some(c) => ModelDetail {
            model_id,
            format: c.format.clone(),
            quantisation: c.quantisation.clone(),
            architecture: c.architecture.clone(),
            parameters: c.parameters_billions,
            context_length: c.context_length,
            metadata_kv,
            introspected_at: None,
        },
        None => ModelDetail {
            model_id,
            format: None,
            quantisation: None,
            architecture: None,
            parameters: None,
            context_length: None,
            metadata_kv,
            introspected_at: None,
        },
    }
}

/// Append a single `inspect_panic tool=<id> model=<mid> message=<msg>` line
/// to `<diagnostics_dir>/diagnostics.log`. Same shape as
/// `open_tool_detail::write_diagnostics_panic_line` with the `model=` field
/// added so triage can scope to a specific model_id.
fn write_diagnostics_panic_line(
    diagnostics_dir: Option<&Path>,
    tool_id: ToolId,
    model_id: &ModelId,
    message: &str,
) {
    let Some(dir) = diagnostics_dir else {
        return;
    };
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(DIAGNOSTICS_LOG_FILENAME);
    let sanitised = message.replace('\n', "\\n");
    let mut line = format!(
        "inspect_panic tool={} model={} message={}",
        tool_id.0, model_id.0, sanitised
    );
    line.push('\n');
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// Append a single `model_detail.open_ms` JSONL line to
/// `<log_dir>/launch.log`. Best-effort; failures are swallowed.
fn emit_open_event(log_dir: Option<&Path>, model_id: &ModelId, duration_ms: u64) {
    let Some(dir) = log_dir else {
        return;
    };
    let path = dir.join("launch.log");
    let envelope = json!({
        "schema": "modeltap.launch.v1",
        "event": "model_detail.open_ms",
        "model_id": model_id.0,
        "duration_ms": duration_ms,
    });
    let mut serialized = envelope.to_string();
    serialized.push('\n');
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(serialized.as_bytes()));
}

// ---------------------------------------------------------------------------
// Unit tests — exercise the pure merge surface + the JSONL emission.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_core::ToolId;
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    fn fixture_tool_id() -> ToolId {
        ToolId("test-tool")
    }

    fn fixture_model_id() -> ModelId {
        ModelId::from("test-model-7b")
    }

    fn cached_with_format_only() -> CachedModel {
        CachedModel {
            model_id: fixture_model_id().0,
            tool_id: fixture_tool_id(),
            display_name: "Test Model 7B".to_string(),
            format: Some("GGUF v3".to_string()),
            quantisation: Some("Q4_K_M".to_string()),
            size_bytes: 4_400_000_000,
            sha256: None,
            architecture: Some("llama".to_string()),
            parameters_billions: Some(7.0),
            context_length: Some(4096),
            dedup_group_id: None,
            metadata_kv: BTreeMap::new(),
            metadata_introspected_at: None,
            last_seen_at: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            last_validated_at: None,
        }
    }

    fn inspect_ok_with_metadata() -> ModelDetail {
        let mut metadata_kv = BTreeMap::new();
        metadata_kv.insert("general.architecture".to_string(), "llama".to_string());
        metadata_kv.insert("llama.context_length".to_string(), "32768".to_string());
        ModelDetail {
            model_id: fixture_model_id(),
            format: Some("GGUF v3".to_string()),
            quantisation: Some("Q4_K_M".to_string()),
            architecture: Some("llama".to_string()),
            parameters: Some(7.0),
            context_length: Some(32768),
            metadata_kv,
            introspected_at: Some(UNIX_EPOCH + Duration::from_secs(1_700_000_100)),
        }
    }

    /// RED_UNIT — Unsupported branch with cached file-shape fields: yield a
    /// ModelDetail whose metadata_kv carries the AC-22 unsupported sentinel
    /// AND whose file-shape fields come from cache so the rest of the screen
    /// still renders (AC-22-3 / AC-22-7).
    #[test]
    fn merge_with_unsupported_yields_unsupported_sentinel_and_cached_shape() {
        let cached = cached_with_format_only();
        let inspect_err = Err(InspectError::Unsupported {
            tool: fixture_tool_id(),
        });

        let detail = merge(fixture_model_id(), inspect_err, Some(&cached));

        assert_eq!(detail.model_id, fixture_model_id());
        assert_eq!(detail.format.as_deref(), Some("GGUF v3"));
        assert_eq!(detail.quantisation.as_deref(), Some("Q4_K_M"));
        assert_eq!(detail.architecture.as_deref(), Some("llama"));
        assert_eq!(detail.parameters, Some(7.0));
        assert_eq!(detail.context_length, Some(4096));
        assert_eq!(
            detail.metadata_kv.get("_status").map(String::as_str),
            Some(METADATA_UNSUPPORTED_SENTINEL),
            "Unsupported branch must carry the unsupported sentinel in metadata_kv"
        );
    }

    /// RED_UNIT — Unsupported branch with NO cache row: yield a ModelDetail
    /// whose file-shape fields default to None so the screen renders "(not
    /// detectable)" for each panel, plus the unsupported sentinel in
    /// metadata_kv.
    #[test]
    fn merge_with_unsupported_and_no_cache_yields_empty_shape() {
        let inspect_err = Err(InspectError::Unsupported {
            tool: fixture_tool_id(),
        });

        let detail = merge(fixture_model_id(), inspect_err, None);

        assert_eq!(detail.format, None);
        assert_eq!(detail.quantisation, None);
        assert_eq!(detail.architecture, None);
        assert_eq!(detail.parameters, None);
        assert_eq!(detail.context_length, None);
        assert_eq!(
            detail.metadata_kv.get("_status").map(String::as_str),
            Some(METADATA_UNSUPPORTED_SENTINEL)
        );
    }

    /// RED_UNIT — FormatUnreadable branch yields the INSPECT_PANIC_SENTINEL
    /// in metadata_kv per AC-22-7 ("(introspection failed -- see
    /// diagnostics.log)"). The file-shape fields still flow from cache so
    /// the rest of the screen renders.
    #[test]
    fn merge_with_format_unreadable_yields_inspect_panic_sentinel() {
        let cached = cached_with_format_only();
        let inspect_err = Err(InspectError::FormatUnreadable {
            path: PathBuf::from("/x/corrupt.gguf"),
            detail: "bad magic".to_string(),
        });

        let detail = merge(fixture_model_id(), inspect_err, Some(&cached));

        assert_eq!(
            detail.metadata_kv.get("_status").map(String::as_str),
            Some(INSPECT_PANIC_SENTINEL),
            "FormatUnreadable must surface the panic sentinel in metadata_kv"
        );
        // Belt-and-braces: file-shape fields preserved.
        assert_eq!(detail.format.as_deref(), Some("GGUF v3"));
    }

    /// RED_UNIT — PluginPanic branch yields the same INSPECT_PANIC_SENTINEL
    /// (US-22 AC-22-7 / INT-INFO-8 alignment with `open_tool_detail`).
    #[test]
    fn merge_with_plugin_panic_yields_inspect_panic_sentinel() {
        let inspect_err = Err(InspectError::PluginPanic {
            tool: fixture_tool_id(),
            message: "boom".to_string(),
        });

        let detail = merge(fixture_model_id(), inspect_err, None);

        assert_eq!(
            detail.metadata_kv.get("_status").map(String::as_str),
            Some(INSPECT_PANIC_SENTINEL)
        );
    }

    /// RED_UNIT — Ok branch returns inspect verbatim (inspect is canonical
    /// for every model-level field per the orchestrator contract).
    #[test]
    fn merge_with_inspect_ok_returns_inspect_verbatim() {
        let inspect_ok = inspect_ok_with_metadata();
        let cached = cached_with_format_only();

        let detail = merge(fixture_model_id(), Ok(inspect_ok.clone()), Some(&cached));

        assert_eq!(detail.format, inspect_ok.format);
        assert_eq!(detail.context_length, Some(32768));
        assert_eq!(
            detail
                .metadata_kv
                .get("general.architecture")
                .map(String::as_str),
            Some("llama")
        );
        assert_eq!(
            detail
                .metadata_kv
                .get("llama.context_length")
                .map(String::as_str),
            Some("32768")
        );
        assert_eq!(detail.introspected_at, inspect_ok.introspected_at);
    }

    /// RED_UNIT — emit_open_event writes a JSONL `model_detail.open_ms`
    /// envelope to `<log_dir>/launch.log`.
    #[test]
    fn emit_open_event_appends_jsonl_event_to_launch_log() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let log_dir = tmp.path();

        emit_open_event(Some(log_dir), &fixture_model_id(), 42);

        let log_path = log_dir.join("launch.log");
        let raw = std::fs::read_to_string(&log_path).expect("launch.log readable");
        let lines: Vec<&str> = raw.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 1, "exactly one JSONL line written");
        let parsed: serde_json::Value =
            serde_json::from_str(lines[0]).expect("valid JSONL envelope");
        assert_eq!(parsed["event"], "model_detail.open_ms");
        assert_eq!(parsed["model_id"], "test-model-7b");
        assert_eq!(parsed["duration_ms"], 42);
        assert_eq!(parsed["schema"], "modeltap.launch.v1");
    }

    /// RED_UNIT — None log_dir is a no-op (best-effort observability).
    #[test]
    fn emit_open_event_is_a_noop_when_log_dir_is_none() {
        emit_open_event(None, &fixture_model_id(), 7);
    }

    /// RED_UNIT — write_diagnostics_panic_line is a no-op for None dir.
    #[test]
    fn write_diagnostics_panic_line_is_a_noop_when_dir_is_none() {
        write_diagnostics_panic_line(None, fixture_tool_id(), &fixture_model_id(), "boom");
    }

    /// RED_UNIT — write_diagnostics_panic_line appends a single
    /// `inspect_panic tool=<id> model=<mid>` line.
    #[test]
    fn write_diagnostics_panic_line_appends_one_line() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let dir = tmp.path();

        write_diagnostics_panic_line(Some(dir), fixture_tool_id(), &fixture_model_id(), "boom");

        let raw = std::fs::read_to_string(dir.join(DIAGNOSTICS_LOG_FILENAME))
            .expect("diagnostics.log readable");
        assert!(
            raw.contains("inspect_panic"),
            "diagnostics.log must contain the inspect_panic tag — got:\n{raw}"
        );
        assert!(
            raw.contains("tool=test-tool"),
            "diagnostics.log must record the tool id"
        );
        assert!(
            raw.contains("model=test-model-7b"),
            "diagnostics.log must record the model id"
        );
        assert!(
            raw.contains("message=boom"),
            "diagnostics.log must record the panic message"
        );
    }
}
