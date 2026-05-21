//! Tool-detail orchestrator (US-21 AC-21-* happy paths + edge cases).
//!
//! Step 02-01 part 2: composes the cached `CachedTool` row from the SQLite
//! cache with the live `Tool::inspect_tool()` result from the plugin and
//! returns a unified `ToolDetail` for the TUI to render.
//!
//! ## Merge semantics
//!
//! The plugin's `inspect_tool()` is canonical for:
//!   - `detected_version`
//!   - `search_paths`
//!   - `plugin_version`
//!   - `install_path` (the plugin knows where it discovers from)
//!
//! The cache is canonical for:
//!   - `last_scan_at`, `last_scan_duration_ms`
//!   - `last_error`, `last_error_at`
//!   - `model_count`, `disk_usage_bytes`, `largest_model`
//!
//! When the plugin returns `InspectError::Unsupported` (the trait default —
//! production plugins land overrides in step 02-02), the cache supplies every
//! field that has a cache analogue and `detected_version` / `search_paths`
//! degrade to `None` / empty.
//!
//! ## I/O policy
//!
//! Every `modeltap-store` call is wrapped in `tokio::task::spawn_blocking`
//! (per architecture-design.md §7.1). The orchestrator measures wall-clock
//! from function entry to merge completion and emits a JSONL
//! `tool_detail.open_ms` event to `<log_dir>/launch.log`. Best-effort: a
//! missing log dir or unwritable file is swallowed so observability never
//! blocks the user-visible TUI transition.

use std::fs::OpenOptions;
use std::io::Write;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::time::Instant;

use futures_util::FutureExt;
use modeltap_core::domain::inspect::{
    InspectError, ModelId, SearchPathEntry as DomainSearchPathEntry,
    SearchPathSource as DomainSearchPathSource, ToolDetail,
};
use modeltap_core::{Tool, ToolId};
use modeltap_store::types::{CachedTool, SearchPathSource as StoreSearchPathSource};
use modeltap_store::{Cache, CacheError, CacheOpenResult};
use serde_json::json;
use thiserror::Error;

/// Sentinel message rendered in the detail screen's `Last error:` field when a
/// plugin's `inspect_tool` panicked. Public so step definitions can assert
/// against the literal without duplicating the string. Per INT-INFO-8 (US-21
/// AC-21-9, US-22 AC-22-7): the TUI MUST NOT crash on a plugin panic — it
/// surfaces this sentinel and the operator consults diagnostics.log for
/// triage detail.
pub const INSPECT_PANIC_SENTINEL: &str = "(inspection failed -- see diagnostics.log)";

/// Filename under `<diagnostics_dir>` for the panic-isolation log. The
/// composition root resolves the directory from `MODELTAP_DIAGNOSTICS_DIR`
/// (test override) or falls back to `~/.modeltap` (production). The
/// orchestrator only sees a fully-resolved path via `OpenToolDetailConfig`.
pub const DIAGNOSTICS_LOG_FILENAME: &str = "diagnostics.log";

/// Inputs into the tool-detail orchestrator.
#[derive(Debug, Clone, Default)]
pub struct OpenToolDetailConfig {
    /// JSONL log directory (matches `WarmStartConfig::log_dir`). `None`
    /// disables JSONL emission entirely.
    pub log_dir: Option<PathBuf>,

    /// Directory under which `diagnostics.log` is written when a plugin's
    /// `inspect_tool` panics (US-21 AC-21-9 / INT-INFO-8). Typically resolved
    /// from `MODELTAP_DIAGNOSTICS_DIR` (test override) or `~/.modeltap`
    /// (production). `None` disables panic-isolation logging entirely — the
    /// panic is still caught and surfaced in `ToolDetail.last_error`, only
    /// the on-disk audit trail is skipped. Best-effort I/O policy matches
    /// `log_dir`: a missing or unwritable directory is swallowed so
    /// observability never blocks the user-visible TUI transition.
    pub diagnostics_dir: Option<PathBuf>,
}

#[derive(Debug, Error)]
pub enum OpenToolDetailError {
    /// `tokio::task::spawn_blocking` failed (panic or runtime shutdown).
    #[error("blocking task failed: {0}")]
    Join(#[from] tokio::task::JoinError),

    /// Cache I/O failed. Callers should fall back to a `ToolDetail` shaped
    /// from inspect() alone, but the orchestrator surfaces the error so the
    /// composition root can choose its degradation policy.
    #[error("cache I/O failed: {0}")]
    Cache(#[from] CacheError),

    /// The tool was not registered in the live plugin list. The composition
    /// root passed a `ToolId` the registry no longer knows about (race after
    /// hot-reload, or a stale Msg).
    #[error("tool {0} not in plugin registry")]
    UnknownTool(String),
}

/// Open the tool-detail orchestration. Returns a merged `ToolDetail`
/// suitable for `Msg::ToolDetailReady(Box::new(detail))`.
///
/// `cache_path` may be `None` when the user launched with `--no-cache` /
/// `MODELTAP_CACHE_PATH` unset. In that mode the cache half of the merge
/// is skipped and every cache-sourced field falls back to its empty /
/// `None` shape; the plugin's `inspect_tool()` result is still consulted.
pub async fn run(
    tool_id: ToolId,
    plugin: &dyn Tool,
    cache_path: Option<&Path>,
    config: &OpenToolDetailConfig,
) -> Result<ToolDetail, OpenToolDetailError> {
    let run_start = Instant::now();

    let cached: Option<CachedTool> = match cache_path {
        Some(p) => load_cached_tool(p, tool_id).await?,
        None => None,
    };

    // INT-INFO-8 / US-21 AC-21-9 / US-22 AC-22-7: wrap the plugin's
    // inspect_tool future in catch_unwind so a panic in the plugin body
    // surfaces as Err(InspectError::PluginPanic) instead of unwinding into
    // the orchestrator. `AssertUnwindSafe` is sound here: the plugin owns no
    // mutable state we share, and any internal partial-mutation a panic
    // leaves behind is irrelevant because we discard the plugin reference
    // immediately after the catch (we do not re-call inspect_tool on the
    // same plugin instance after a panic in the same run).
    let inspect_fut = AssertUnwindSafe(plugin.inspect_tool()).catch_unwind();
    let inspect_result = match inspect_fut.await {
        Ok(inner) => inner,
        Err(panic_payload) => {
            let message = format_panic_payload(panic_payload);
            write_diagnostics_panic_line(
                config.diagnostics_dir.as_deref(),
                tool_id,
                &message,
            );
            Err(InspectError::PluginPanic { tool: tool_id, message })
        }
    };

    let detail = merge(tool_id, inspect_result, cached);

    let elapsed_ms = run_start.elapsed().as_millis() as u64;
    emit_open_event(config.log_dir.as_deref(), tool_id, elapsed_ms);

    Ok(detail)
}

/// Best-effort downcast of a `catch_unwind` payload into a human-readable
/// message. Mirrors the formatting `JoinError::Display` would have produced
/// had we used `tokio::spawn` (per modeltap-core's
/// `run_inspect_with_panic_isolation`), so step definitions can assert
/// against a stable substring regardless of which catch path the
/// orchestrator took.
fn format_panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic payload>".to_string()
}

/// Open the cache, pull the row for `tool_id`, return it (or `None` when
/// no row exists yet for this tool). All rusqlite calls happen inside
/// `spawn_blocking` per architecture-design.md §7.1.
async fn load_cached_tool(
    cache_path: &Path,
    tool_id: ToolId,
) -> Result<Option<CachedTool>, OpenToolDetailError> {
    let path = cache_path.to_path_buf();
    let opt = tokio::task::spawn_blocking(move || -> Result<Option<CachedTool>, CacheError> {
        let opened = Cache::open(&path)?;
        let cache = match opened {
            CacheOpenResult::OpenedFresh(c)
            | CacheOpenResult::OpenedExisting(c)
            | CacheOpenResult::OpenedAfterMigration { cache: c, .. }
            | CacheOpenResult::OpenedAfterRecovery { cache: c, .. } => c,
        };
        let tools = cache.tools()?;
        Ok(tools.into_iter().find(|t| t.tool_id == tool_id))
    })
    .await??;
    Ok(opt)
}

/// Pure merge fn — exposed for unit testing.
///
/// When `inspect` is `Err(InspectError::PluginPanic { .. })`, the merge
/// overrides the `last_error` field with `INSPECT_PANIC_SENTINEL` so the
/// detail screen's `Last error:` line surfaces
/// `(inspection failed -- see diagnostics.log)` regardless of what the
/// cache row says — the panic supersedes any prior error because it
/// happened on the just-now invocation that opened the detail screen.
pub fn merge(
    tool_id: ToolId,
    inspect: Result<ToolDetail, InspectError>,
    cached: Option<CachedTool>,
) -> ToolDetail {
    match inspect {
        Ok(insp) => merge_with_inspect_ok(insp, cached),
        Err(InspectError::PluginPanic { .. }) => {
            let mut detail = from_cache_only(tool_id, cached);
            detail.last_error = Some(INSPECT_PANIC_SENTINEL.to_string());
            detail.last_error_at = Some(std::time::SystemTime::now());
            detail
        }
        Err(_) => from_cache_only(tool_id, cached),
    }
}

/// Inspect returned a fresh `ToolDetail`. Use inspect as the canonical
/// source for `install_path`, `detected_version`, `plugin_version`,
/// `search_paths`; overlay cache-sourced scan-state / model-stats fields.
fn merge_with_inspect_ok(insp: ToolDetail, cached: Option<CachedTool>) -> ToolDetail {
    match cached {
        None => insp,
        Some(c) => ToolDetail {
            tool_id: insp.tool_id,
            install_path: insp.install_path,
            detected_version: insp.detected_version,
            plugin_version: insp.plugin_version,
            search_paths: insp.search_paths,
            // ---- cache-sourced fields override inspect's view ------------
            model_count: c.model_count as usize,
            disk_usage_bytes: c.disk_usage_bytes,
            largest_model: c.largest_model_id.as_deref().map(ModelId::from),
            last_scan_at: Some(c.last_scan_at),
            last_scan_duration_ms: Some(c.last_scan_duration_ms),
            last_error: c.last_error,
            last_error_at: c.last_error_at,
        },
    }
}

/// Inspect returned an error (Unsupported / FileReadable / etc). Build a
/// `ToolDetail` from cache alone; `detected_version` and `search_paths`
/// degrade because the plugin opted out of detection.
fn from_cache_only(tool_id: ToolId, cached: Option<CachedTool>) -> ToolDetail {
    match cached {
        Some(c) => ToolDetail {
            tool_id: c.tool_id,
            install_path: c.install_path,
            detected_version: c.detected_version,
            plugin_version: c.plugin_version,
            search_paths: c
                .search_paths
                .into_iter()
                .map(store_search_path_to_domain)
                .collect(),
            model_count: c.model_count as usize,
            disk_usage_bytes: c.disk_usage_bytes,
            largest_model: c.largest_model_id.as_deref().map(ModelId::from),
            last_scan_at: Some(c.last_scan_at),
            last_scan_duration_ms: Some(c.last_scan_duration_ms),
            last_error: c.last_error,
            last_error_at: c.last_error_at,
        },
        None => ToolDetail {
            tool_id,
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
        },
    }
}

fn store_search_path_to_domain(e: modeltap_store::types::SearchPathEntry) -> DomainSearchPathEntry {
    DomainSearchPathEntry {
        path: e.path,
        source: match e.source {
            StoreSearchPathSource::Default => DomainSearchPathSource::Default,
            StoreSearchPathSource::UserConfig => DomainSearchPathSource::UserConfig,
        },
    }
}

/// Append a single `inspect_panic tool=<id> message=<msg>` line to
/// `<diagnostics_dir>/diagnostics.log` (US-21 AC-21-9 / INT-INFO-8). Newline-
/// delimited plain text (NOT JSONL — diagnostics.log is the human-readable
/// triage trail per architecture-design.md; the structured JSONL stream lives
/// in `<log_dir>/launch.log`). Best-effort: a missing diagnostics_dir or an
/// unwritable file is swallowed so a panic-isolation event never compounds
/// into a second user-visible failure.
///
/// The `message` field is sanitised to a single line (any embedded `\n` is
/// replaced with `\\n`) so a multi-line panic payload does not corrupt the
/// line-oriented file format.
fn write_diagnostics_panic_line(
    diagnostics_dir: Option<&Path>,
    tool_id: ToolId,
    message: &str,
) {
    let Some(dir) = diagnostics_dir else {
        return;
    };
    // Ensure the directory exists — release builds may launch with a fresh
    // ~/.modeltap that has never been touched. Best-effort: ignore errors and
    // let the subsequent `open` call surface them (which we also swallow).
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(DIAGNOSTICS_LOG_FILENAME);
    let sanitised = message.replace('\n', "\\n");
    let mut line = format!("inspect_panic tool={} message={}", tool_id.0, sanitised);
    line.push('\n');
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// Append a single `tool_detail.open_ms` JSONL line to `<log_dir>/launch.log`.
/// Best-effort; failures are swallowed so an unwritable log dir never blocks
/// the detail-screen transition.
fn emit_open_event(log_dir: Option<&Path>, tool_id: ToolId, duration_ms: u64) {
    let Some(dir) = log_dir else {
        return;
    };
    let path = dir.join("launch.log");
    let envelope = json!({
        "schema": "modeltap.launch.v1",
        "event": "tool_detail.open_ms",
        "tool_id": tool_id.0,
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
    use modeltap_core::domain::inspect::SearchPathEntry;
    use modeltap_store::types::{CachedTool, SearchPathEntry as StoreSearchPathEntry};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::{Duration, UNIX_EPOCH};

    fn fixture_tool_id() -> ToolId {
        ToolId("test-tool")
    }

    fn cached_with_full_stats() -> CachedTool {
        let _ = BTreeMap::<String, String>::new();
        CachedTool {
            tool_id: fixture_tool_id(),
            install_path: PathBuf::from("/cache/install"),
            detected_version: Some("cache-version-1.0.0".to_string()),
            plugin_version: "cache-plugin-version 0.0.0".to_string(),
            model_count: 12,
            disk_usage_bytes: 47_300_000_000,
            largest_model_id: Some("llama3:70b-instruct-q4_K_M".to_string()),
            last_scan_at: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            last_scan_duration_ms: 250,
            last_error: None,
            last_error_at: None,
            search_paths: vec![StoreSearchPathEntry {
                path: PathBuf::from("/cache/search"),
                source: StoreSearchPathSource::Default,
            }],
        }
    }

    fn inspect_ok_with_fresh_fields() -> ToolDetail {
        ToolDetail {
            tool_id: fixture_tool_id(),
            install_path: PathBuf::from("/inspect/install"),
            detected_version: Some("inspect-version-2.0.0".to_string()),
            plugin_version: "inspect-plugin-version 9.9.9".to_string(),
            search_paths: vec![SearchPathEntry {
                path: PathBuf::from("/inspect/search"),
                source: DomainSearchPathSource::UserConfig,
            }],
            // inspect's transient model_count etc are deliberately wrong so
            // we can prove the cache value wins.
            model_count: 0,
            disk_usage_bytes: 0,
            largest_model: None,
            last_scan_at: None,
            last_scan_duration_ms: None,
            last_error: None,
            last_error_at: None,
        }
    }

    /// RED_UNIT — Unsupported branch with cache present: yield a ToolDetail
    /// whose detected_version + search_paths come from cache, and whose
    /// model-stats fields come from cache.
    #[test]
    fn merge_with_unsupported_yields_cache_sourced_fields() {
        let cached = cached_with_full_stats();
        let inspect_err = Err(InspectError::Unsupported {
            tool: fixture_tool_id(),
        });

        let detail = merge(fixture_tool_id(), inspect_err, Some(cached));

        assert_eq!(detail.tool_id, fixture_tool_id());
        assert_eq!(detail.install_path, PathBuf::from("/cache/install"));
        assert_eq!(
            detail.detected_version,
            Some("cache-version-1.0.0".to_string()),
            "cache-only path carries cache's detected_version forward"
        );
        assert_eq!(detail.plugin_version, "cache-plugin-version 0.0.0");
        assert_eq!(detail.model_count, 12);
        assert_eq!(detail.disk_usage_bytes, 47_300_000_000);
        assert_eq!(
            detail.largest_model.as_ref().map(|m| m.0.clone()),
            Some("llama3:70b-instruct-q4_K_M".to_string())
        );
        assert_eq!(detail.search_paths.len(), 1);
        assert_eq!(detail.search_paths[0].path, PathBuf::from("/cache/search"));
        assert_eq!(
            detail.search_paths[0].source,
            DomainSearchPathSource::Default
        );
        assert_eq!(
            detail.last_scan_at,
            Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        );
        assert_eq!(detail.last_scan_duration_ms, Some(250));
    }

    /// RED_UNIT — Unsupported branch with NO cache row: yield an
    /// all-default ToolDetail with `detected_version: None` and empty
    /// `search_paths` so the TUI renders "(not detectable)".
    #[test]
    fn merge_with_unsupported_and_no_cache_yields_empty_detail() {
        let inspect_err = Err(InspectError::Unsupported {
            tool: fixture_tool_id(),
        });

        let detail = merge(fixture_tool_id(), inspect_err, None);

        assert_eq!(detail.tool_id, fixture_tool_id());
        assert_eq!(detail.detected_version, None);
        assert!(detail.search_paths.is_empty());
        assert_eq!(detail.model_count, 0);
        assert_eq!(detail.disk_usage_bytes, 0);
        assert_eq!(detail.largest_model, None);
        assert_eq!(detail.last_scan_at, None);
        assert_eq!(detail.last_error, None);
    }

    /// RED_UNIT — inspect Ok + cache present: inspect canonical for
    /// detected_version / search_paths / plugin_version; cache canonical
    /// for last_scan_* / last_error / model_count / disk_usage / largest_model.
    #[test]
    fn merge_with_inspect_ok_uses_inspect_for_freshness_and_cache_for_stats() {
        let inspect_ok = inspect_ok_with_fresh_fields();
        let cached = cached_with_full_stats();

        let detail = merge(fixture_tool_id(), Ok(inspect_ok), Some(cached));

        // From inspect (fresh authoritative source).
        assert_eq!(
            detail.detected_version,
            Some("inspect-version-2.0.0".to_string()),
            "inspect is canonical for detected_version when Ok"
        );
        assert_eq!(detail.plugin_version, "inspect-plugin-version 9.9.9");
        assert_eq!(detail.install_path, PathBuf::from("/inspect/install"));
        assert_eq!(detail.search_paths.len(), 1);
        assert_eq!(
            detail.search_paths[0].path,
            PathBuf::from("/inspect/search")
        );
        assert_eq!(
            detail.search_paths[0].source,
            DomainSearchPathSource::UserConfig
        );

        // From cache (rolling scan-state + model stats).
        assert_eq!(detail.model_count, 12);
        assert_eq!(detail.disk_usage_bytes, 47_300_000_000);
        assert_eq!(
            detail.largest_model.as_ref().map(|m| m.0.clone()),
            Some("llama3:70b-instruct-q4_K_M".to_string())
        );
        assert_eq!(
            detail.last_scan_at,
            Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000))
        );
        assert_eq!(detail.last_scan_duration_ms, Some(250));
        assert_eq!(detail.last_error, None);
    }

    /// RED_UNIT — inspect Ok with NO cache row: the inspect ToolDetail is
    /// returned verbatim (no cache to overlay).
    #[test]
    fn merge_with_inspect_ok_and_no_cache_returns_inspect_verbatim() {
        let mut inspect_ok = inspect_ok_with_fresh_fields();
        // Show the inspect-only path preserves all inspect fields including
        // model_count (which we set non-zero here for the assertion).
        inspect_ok.model_count = 7;
        inspect_ok.disk_usage_bytes = 123_000;

        let detail = merge(fixture_tool_id(), Ok(inspect_ok.clone()), None);

        assert_eq!(detail.detected_version, inspect_ok.detected_version);
        assert_eq!(detail.model_count, 7);
        assert_eq!(detail.disk_usage_bytes, 123_000);
        assert_eq!(detail.last_scan_at, None);
    }

    /// RED_UNIT — last_error and last_error_at flow from cache when inspect
    /// returned Unsupported (the AC-21-4 scenario: discovery failed at last
    /// scan, surfaces in detail screen).
    #[test]
    fn merge_with_unsupported_propagates_cache_last_error_with_timestamp() {
        let mut cached = cached_with_full_stats();
        cached.last_error =
            Some("permission denied reading ~/.ollama/models/manifests/ (errno 13)".to_string());
        cached.last_error_at = Some(UNIX_EPOCH + Duration::from_secs(1_700_001_000));

        let inspect_err = Err(InspectError::Unsupported {
            tool: fixture_tool_id(),
        });

        let detail = merge(fixture_tool_id(), inspect_err, Some(cached));

        assert_eq!(
            detail.last_error.as_deref(),
            Some("permission denied reading ~/.ollama/models/manifests/ (errno 13)")
        );
        assert_eq!(
            detail.last_error_at,
            Some(UNIX_EPOCH + Duration::from_secs(1_700_001_000))
        );
    }

    /// RED_UNIT — emit_open_event writes a JSONL `tool_detail.open_ms`
    /// envelope with the tool_id + duration_ms to `<log_dir>/launch.log`.
    #[test]
    fn emit_open_event_appends_jsonl_event_to_launch_log() {
        let tmp = tempfile::tempdir().expect("create tempdir");
        let log_dir = tmp.path();

        emit_open_event(Some(log_dir), fixture_tool_id(), 42);

        let log_path = log_dir.join("launch.log");
        let raw = std::fs::read_to_string(&log_path).expect("launch.log readable");
        let lines: Vec<&str> = raw.lines().filter(|l| !l.is_empty()).collect();
        assert_eq!(lines.len(), 1, "exactly one JSONL line written");
        let parsed: serde_json::Value =
            serde_json::from_str(lines[0]).expect("valid JSONL envelope");
        assert_eq!(parsed["event"], "tool_detail.open_ms");
        assert_eq!(parsed["tool_id"], "test-tool");
        assert_eq!(parsed["duration_ms"], 42);
        assert_eq!(parsed["schema"], "modeltap.launch.v1");
    }

    /// RED_UNIT — None log_dir is a no-op (best-effort observability).
    #[test]
    fn emit_open_event_is_a_noop_when_log_dir_is_none() {
        // No panic, no file written, returns unit.
        emit_open_event(None, fixture_tool_id(), 7);
    }

    /// RED_UNIT — the async `run` function invokes the plugin's
    /// `inspect_tool` and returns a merged `ToolDetail`. This proves the
    /// public surface drives end-to-end without panicking for the
    /// Unsupported + no-cache path.
    #[tokio::test]
    async fn run_returns_detail_for_unsupported_plugin_without_cache() {
        struct UnsupportedPlugin;

        #[async_trait::async_trait]
        impl Tool for UnsupportedPlugin {
            fn name(&self) -> ToolId {
                fixture_tool_id()
            }
            fn accepted_formats(&self) -> &'static [modeltap_core::Format] {
                &[]
            }
            async fn discover(
                &self,
            ) -> Result<Vec<modeltap_core::DiscoveredModel>, modeltap_core::DiscoverError>
            {
                Ok(Vec::new())
            }
            async fn link(
                &self,
                _canonical_src: &std::path::Path,
                _model: &modeltap_core::ModelMeta,
            ) -> Result<modeltap_core::LinkOutcome, modeltap_core::LinkError> {
                Err(modeltap_core::LinkError::NotYetImplemented(
                    "test stub".to_string(),
                ))
            }
            async fn delete_one(
                &self,
                _model: &modeltap_core::ModelMeta,
            ) -> Result<modeltap_core::DeleteOutcome, modeltap_core::DeleteError> {
                Err(modeltap_core::DeleteError::Unsupported {
                    tool: fixture_tool_id(),
                })
            }
            async fn delete_all(
                &self,
            ) -> Result<Vec<modeltap_core::DeleteOutcome>, modeltap_core::DeleteError> {
                Err(modeltap_core::DeleteError::Unsupported {
                    tool: fixture_tool_id(),
                })
            }
        }

        let plugin = UnsupportedPlugin;
        let config = OpenToolDetailConfig {
            log_dir: None,
            diagnostics_dir: None,
        };

        let detail = run(fixture_tool_id(), &plugin, None, &config)
            .await
            .expect("run succeeds even when inspect_tool errors with Unsupported");

        assert_eq!(detail.tool_id, fixture_tool_id());
        assert_eq!(detail.detected_version, None);
        assert!(detail.search_paths.is_empty());
    }
}
