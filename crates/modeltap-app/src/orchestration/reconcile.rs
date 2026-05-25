//! Background reconcile orchestrator (Phase 05 step 05-01 / US-24 + US-26).
//!
//! Composes the discovered plugin output with the cached `cache_models` /
//! `cache_tools` rows and writes the merged inventory back atomically per
//! tool. Drives the user-visible silent-ack indicator (AC-26-4) and the
//! last-known-good preservation contract (AC-26-3): a per-tool write failure
//! never mutates the cache and never blocks other tools from completing.
//!
//! ## Scope semantics
//!
//! `ReconcileScope::All` reconciles every registered plugin. Dispatched
//! automatically after warm-paint and (in 05-03) on `[Shift+R]`.
//! `ReconcileScope::Tool(tool_id)` reconciles a single plugin. Dispatched
//! (in 05-03) on `[r]` from the main view's left-pane cursor.
//!
//! ## I/O policy
//!
//! Every rusqlite call goes through `tokio::task::spawn_blocking`
//! (architecture-design.md §7.1). Every `Tool::discover()` is wrapped in
//! `AssertUnwindSafe(...).catch_unwind()` so a plugin panic is isolated as
//! a per-tool `ToolFailed` event rather than unwinding into the orchestrator
//! and tearing down the TUI (the same pattern `open_tool_detail` uses for
//! `inspect_tool`).
//!
//! ## Stream shape
//!
//! `run(...)` returns a `BoxStream<ReconcileEvent>` driven by an internal
//! `tokio::sync::mpsc::unbounded_channel`. The caller consumes the stream
//! and dispatches one `Msg::ReconcileCompleted` / `Msg::ReconcileFailed`
//! per tool completion into the pure update loop. `ReconcileEvent::AllCompleted`
//! marks the end of the stream so the caller can stop polling.
//!
//! Wiring for the manual-refresh hotkeys lands in step 05-03; this module
//! exposes the orchestrator that those hotkeys will invoke.

use std::collections::BTreeMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::SystemTime;

use futures_util::{stream, FutureExt, Stream, StreamExt};
use modeltap_core::logic::inventory_diff::{compute_inventory_diff, InventoryDiff, ModelSignature};
use modeltap_core::{DiscoverError, Tool, ToolId};
use modeltap_store::types::{CachedModel, CachedTool};
use modeltap_store::{Cache, CacheError, CacheOpenResult};
use thiserror::Error;

/// Filename appended under the diagnostics directory when a per-tool
/// reconcile fails. Mirrors `open_tool_detail::DIAGNOSTICS_LOG_FILENAME` so
/// triage tooling reads one file regardless of which orchestrator wrote the
/// line. Public so step-definitions and the composition root can reference
/// the literal without duplicating the string.
pub const DIAGNOSTICS_LOG_FILENAME: &str = "diagnostics.log";

/// What to reconcile. `All` is the default after warm-paint and the
/// `[Shift+R]` semantic; `Tool(id)` is the per-tool refresh on `[r]`
/// (05-03 wiring).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileScope {
    All,
    Tool(ToolId),
}

/// One event the orchestrator emits across the lifetime of a single
/// `run(...)` call. The event-loop consumer translates each into the
/// corresponding `Msg::ReconcileCompleted` / `Msg::ReconcileFailed`
/// dispatch into the pure update.
#[derive(Debug, Clone, PartialEq)]
pub enum ReconcileEvent {
    /// A per-tool reconcile started. Fired before `discover()` is invoked
    /// so the future per-tool spinner indicator can paint immediately.
    ToolStarted { tool: ToolId },
    /// The per-tool atomic write succeeded. `diff` carries the
    /// added/removed/modified summary; `diff.is_empty()` means nothing
    /// drifted and the silent-ack indicator MUST stay dark.
    ToolCompleted { tool: ToolId, diff: InventoryDiff },
    /// The per-tool reconcile failed. The cache stayed at last-known-good
    /// (AC-26-3); the orchestrator already appended a `reconcile_failed`
    /// line to `<diagnostics_dir>/diagnostics.log` before this event was
    /// sent.
    ToolFailed { tool: ToolId, reason: String },
    /// Sentinel emitted after every per-tool event. Marks the end of the
    /// stream so the consumer can stop polling — `futures_util::Stream`'s
    /// `Poll::Ready(None)` follows on the next poll.
    AllCompleted,
}

/// Inputs into the reconcile orchestrator. `cache_path = None` short-
/// circuits the orchestrator entirely (no `Cache::open`, no writes) — the
/// composition root passes `None` when `--no-cache` / `[cache] enabled =
/// false` is in effect. In that mode the stream emits a single
/// `AllCompleted` event and ends.
///
/// `Default` is hand-rolled because `SystemTime` does not implement
/// `Default`; the unit-test short-circuit (cache_path = None) needs a cheap
/// zero-value config and `UNIX_EPOCH` is the obvious deterministic default.
#[derive(Debug, Clone)]
pub struct ReconcileConfig {
    /// Resolved absolute path to the SQLite cache. `None` disables the
    /// reconcile entirely.
    pub cache_path: Option<PathBuf>,
    /// Best-effort observability sink for `reconcile_failed` lines. `None`
    /// silently drops the line (the per-tool event still fires).
    pub diagnostics_dir: Option<PathBuf>,
    /// Reference instant for cache row timestamps. Taken as a parameter so
    /// the orchestrator stays deterministic under test. Production callers
    /// pass `SystemTime::now()`.
    pub now: SystemTime,
}

impl Default for ReconcileConfig {
    fn default() -> Self {
        Self {
            cache_path: None,
            diagnostics_dir: None,
            now: SystemTime::UNIX_EPOCH,
        }
    }
}

#[derive(Debug, Error)]
pub enum ReconcileError {
    #[error("cache I/O failed: {0}")]
    Cache(#[from] CacheError),
    #[error("blocking task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("plugin not registered: {0}")]
    UnknownPlugin(String),
}

/// Plugin handle the orchestrator consumes. The same `Arc<dyn Tool>`
/// instances the composition root already constructs at startup — passing
/// the slice by reference would force a `'static` lifetime on the spawned
/// per-tool tasks; an `Arc<dyn Tool>` clones cheaply and survives across
/// the `spawn_blocking` boundary.
pub type PluginHandle = Arc<dyn Tool + Send + Sync>;

/// Run the background reconcile and return an event stream.
///
/// `plugins` is the registered plugin set. For `Scope::All` every plugin
/// is reconciled in sequence (parallelism could be added later, but the
/// per-tool `BEGIN IMMEDIATE` already serialises the SQLite writer half so
/// sequential dispatch keeps the wait-time metric clean). For
/// `Scope::Tool(id)` only that one plugin is reconciled — an unknown id
/// emits a single `ToolFailed` event with reason `UnknownPlugin`.
///
/// The returned stream is `Pin<Box<...>>` so it can cross await points
/// inside the event loop without leaking concrete future types into the
/// caller's signature (matches the same shape `futures_util` returns from
/// `BoxStream`).
pub fn run(
    scope: ReconcileScope,
    plugins: Vec<PluginHandle>,
    config: ReconcileConfig,
) -> Pin<Box<dyn Stream<Item = ReconcileEvent> + Send>> {
    // Cache-disabled short-circuit: emit AllCompleted and end. The
    // composition root NEVER calls us in this state when wired correctly,
    // but the defensive branch keeps the orchestrator robust if a future
    // call site forgets the gate.
    let Some(cache_path) = config.cache_path.clone() else {
        return Box::pin(stream::once(async { ReconcileEvent::AllCompleted }));
    };

    let selected: Vec<PluginHandle> = match &scope {
        ReconcileScope::All => plugins,
        ReconcileScope::Tool(target) => plugins
            .into_iter()
            .filter(|p| p.name().0 == target.0)
            .collect(),
    };

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<ReconcileEvent>();
    let diagnostics_dir = config.diagnostics_dir.clone();
    let now = config.now;

    // Drive every per-tool reconcile from one async task. Per-tool I/O
    // already hops into `spawn_blocking` for the SQLite half; the plugin
    // `discover()` is async and runs on the current runtime. Sequential
    // dispatch keeps the BEGIN IMMEDIATE serial behaviour predictable.
    tokio::spawn(async move {
        // Handle unknown-plugin Scope::Tool — emit one ToolFailed before
        // AllCompleted so the caller knows the dispatch was attempted.
        if let ReconcileScope::Tool(target) = &scope {
            if selected.is_empty() {
                let _ = tx.send(ReconcileEvent::ToolFailed {
                    tool: *target,
                    reason: format!("plugin {} not registered", target.0),
                });
                let _ = tx.send(ReconcileEvent::AllCompleted);
                return;
            }
        }

        for plugin in &selected {
            let tool_id = plugin.name();
            let _ = tx.send(ReconcileEvent::ToolStarted { tool: tool_id });

            // Step 1 — discover() with panic isolation. Production plugins
            // are trusted but a future third-party plugin (or a regression
            // in an in-tree one) should NOT unwind into the orchestrator
            // and tear down the TUI. AssertUnwindSafe is sound here for
            // the same reason as `open_tool_detail`: we discard the plugin
            // reference if it panics during this call (we do not re-invoke
            // discover() on the same plugin instance in the same run).
            let discover_fut = AssertUnwindSafe(plugin.discover()).catch_unwind();
            let discovered = match discover_fut.await {
                Ok(Ok(models)) => models,
                Ok(Err(DiscoverError::NotInstalled)) => {
                    // NotInstalled is not a failure — the tool simply has
                    // nothing to reconcile. Emit ToolCompleted with an
                    // empty diff so the caller's no-op path runs.
                    let _ = tx.send(ReconcileEvent::ToolCompleted {
                        tool: tool_id,
                        diff: InventoryDiff {
                            tool_id,
                            added_models: Vec::new(),
                            removed_models: Vec::new(),
                            modified_models: Vec::new(),
                        },
                    });
                    continue;
                }
                Ok(Err(err)) => {
                    write_diagnostics_failed_line(
                        diagnostics_dir.as_deref(),
                        tool_id,
                        &format!("discover_error: {err}"),
                    );
                    let _ = tx.send(ReconcileEvent::ToolFailed {
                        tool: tool_id,
                        reason: err.to_string(),
                    });
                    continue;
                }
                Err(panic_payload) => {
                    let message = format_panic_payload(panic_payload);
                    write_diagnostics_failed_line(
                        diagnostics_dir.as_deref(),
                        tool_id,
                        &format!("plugin_panic: {message}"),
                    );
                    let _ = tx.send(ReconcileEvent::ToolFailed {
                        tool: tool_id,
                        reason: format!("plugin panic: {message}"),
                    });
                    continue;
                }
            };

            // Step 2 — fetch cached signature + write atomically. Both
            // halves run inside the SAME `spawn_blocking` so the cache
            // handle stays on one thread and the BEGIN IMMEDIATE wait is
            // measured against a fresh read of the cached rows. The
            // closure returns the diff so the orchestrator can emit it.
            let cache_path_owned = cache_path.clone();
            let plugin_version = env!("CARGO_PKG_VERSION").to_string();
            let discovered_for_blocking = discovered.clone();
            let join = tokio::task::spawn_blocking(move || -> Result<InventoryDiff, CacheError> {
                let cache = open_cache(&cache_path_owned)?;
                let cached_rows = cache.models_for_tool(&tool_id)?;
                let diff = compute_inventory_diff(
                    tool_id,
                    &cached_rows
                        .iter()
                        .map(cached_to_signature)
                        .collect::<Vec<_>>(),
                    &discovered_for_blocking
                        .iter()
                        .map(discovered_to_signature)
                        .collect::<Vec<_>>(),
                );

                let (cached_tool, cached_models) =
                    project_to_cache_rows(tool_id, &discovered_for_blocking, &plugin_version, now);
                cache.atomic_reconcile_write(&cached_tool, &cached_models)?;
                Ok(diff)
            })
            .await;

            match join {
                Ok(Ok(diff)) => {
                    let _ = tx.send(ReconcileEvent::ToolCompleted {
                        tool: tool_id,
                        diff,
                    });
                }
                Ok(Err(e)) => {
                    write_diagnostics_failed_line(
                        diagnostics_dir.as_deref(),
                        tool_id,
                        &format!("cache_write_error: {e}"),
                    );
                    let _ = tx.send(ReconcileEvent::ToolFailed {
                        tool: tool_id,
                        reason: e.to_string(),
                    });
                }
                Err(e) => {
                    write_diagnostics_failed_line(
                        diagnostics_dir.as_deref(),
                        tool_id,
                        &format!("join_error: {e}"),
                    );
                    let _ = tx.send(ReconcileEvent::ToolFailed {
                        tool: tool_id,
                        reason: format!("blocking task failed: {e}"),
                    });
                }
            }
        }

        let _ = tx.send(ReconcileEvent::AllCompleted);
    });

    // Bridge `UnboundedReceiver` into a `Stream` without adding
    // `tokio-stream` as a dep. `stream::unfold` is in `futures_util` which
    // we already use elsewhere (open_tool_detail panic-isolation).
    Box::pin(stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| (event, rx))
    }))
}

/// Convenience: drain the full event stream into a Vec. Used by tests and
/// by the composition root when it needs every event before continuing
/// (the typical post-warm-paint dispatch). The caller can equivalently
/// `while let Some(ev) = stream.next().await` for incremental dispatch.
pub async fn collect_all(
    mut stream: Pin<Box<dyn Stream<Item = ReconcileEvent> + Send>>,
) -> Vec<ReconcileEvent> {
    let mut out = Vec::new();
    while let Some(event) = stream.next().await {
        out.push(event);
        if matches!(out.last(), Some(ReconcileEvent::AllCompleted)) {
            break;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn open_cache(path: &Path) -> Result<Cache, CacheError> {
    let opened = Cache::open(path)?;
    Ok(match opened {
        CacheOpenResult::OpenedFresh(c) => c,
        CacheOpenResult::OpenedExisting(c) => c,
        CacheOpenResult::OpenedAfterMigration { cache, .. } => cache,
        CacheOpenResult::OpenedAfterRecovery { cache, .. } => cache,
    })
}

fn cached_to_signature(m: &CachedModel) -> ModelSignature {
    ModelSignature {
        model_id: m.model_id.clone(),
        size_bytes: m.size_bytes,
        sha256: m.sha256.clone(),
    }
}

fn discovered_to_signature(m: &modeltap_core::DiscoveredModel) -> ModelSignature {
    ModelSignature {
        model_id: m.id_in_tool.clone(),
        size_bytes: m.size_bytes,
        // discover() never produces a sha256 — the hash pool does that
        // asynchronously and writes back through a separate path.
        sha256: None,
    }
}

/// Project the fresh `DiscoveredModel` list back into the cache row shape.
/// Mirrors the projection in `main::reconcile_writeback` so both the
/// post-cold writeback and the background reconcile produce byte-identical
/// rows for a given input. INT-INFO-3 holds because
/// `tool.disk_usage_bytes = sum(models.size_bytes)` here by construction
/// (within 1-byte rounding because every accumulator is u64).
fn project_to_cache_rows(
    tool_id: ToolId,
    discovered: &[modeltap_core::DiscoveredModel],
    plugin_version: &str,
    now: SystemTime,
) -> (CachedTool, Vec<CachedModel>) {
    let total_bytes: u64 = discovered.iter().map(|m| m.size_bytes).sum();
    let largest_id = discovered
        .iter()
        .max_by_key(|m| m.size_bytes)
        .map(|m| m.id_in_tool.clone());

    let cached_tool = CachedTool {
        tool_id,
        install_path: std::path::PathBuf::new(),
        detected_version: None,
        plugin_version: plugin_version.to_string(),
        model_count: discovered.len() as u64,
        disk_usage_bytes: total_bytes,
        largest_model_id: largest_id,
        last_scan_at: now,
        last_scan_duration_ms: 0,
        last_error: None,
        last_error_at: None,
        search_paths: Vec::new(),
    };

    let cached_models: Vec<CachedModel> = discovered
        .iter()
        .map(|m| CachedModel {
            model_id: m.id_in_tool.clone(),
            tool_id,
            display_name: m.display_label.0.clone(),
            format: Some(format_label_for_cache(m.format).to_string()),
            quantisation: None,
            size_bytes: m.size_bytes,
            sha256: None,
            architecture: None,
            parameters_billions: None,
            context_length: None,
            dedup_group_id: None,
            metadata_kv: BTreeMap::new(),
            metadata_introspected_at: None,
            last_seen_at: now,
            last_validated_at: None,
        })
        .collect();

    (cached_tool, cached_models)
}

/// Cache-side stringification of `Format`. Lower-snake to match the
/// existing `warm_start::parse_format` round-trip.
fn format_label_for_cache(f: modeltap_core::Format) -> &'static str {
    match f {
        modeltap_core::Format::Gguf => "gguf",
        modeltap_core::Format::Safetensors => "safetensors",
        modeltap_core::Format::Bin => "bin",
        modeltap_core::Format::Awq => "awq",
        modeltap_core::Format::Gptq => "gptq",
        modeltap_core::Format::OllamaBlob => "ollamablob",
        modeltap_core::Format::Mlx => "mlx",
        modeltap_core::Format::Other => "other",
    }
}

fn format_panic_payload(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    "<non-string panic payload>".to_string()
}

/// Append a single `reconcile_failed tool=<id> reason=<text>` line to
/// `<diagnostics_dir>/diagnostics.log` (AC-26-3). Best-effort: a missing
/// diagnostics dir or unwritable file is swallowed so an observability
/// failure never compounds into a second user-visible failure. Multi-line
/// reasons are flattened with `\\n` so the line-oriented format stays
/// readable.
fn write_diagnostics_failed_line(diagnostics_dir: Option<&Path>, tool_id: ToolId, reason: &str) {
    use std::fs::OpenOptions;
    use std::io::Write;

    let Some(dir) = diagnostics_dir else {
        return;
    };
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(DIAGNOSTICS_LOG_FILENAME);
    let sanitised = reason.replace('\n', "\\n");
    let mut line = format!("reconcile_failed tool={} reason={}", tool_id.0, sanitised);
    line.push('\n');
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use modeltap_core::{
        DeleteError, DeleteOutcome, DiscoveredModel, DisplayLabel, Format, LinkError, LinkOutcome,
        ModelMeta, ModelStatus,
    };

    fn fixture_tool_id() -> ToolId {
        ToolId("test-tool")
    }

    fn fixture_discovered(id: &str, size: u64) -> DiscoveredModel {
        DiscoveredModel {
            id_in_tool: id.to_string(),
            on_disk_path: PathBuf::from("/test/path"),
            size_bytes: size,
            format: Format::Gguf,
            display_label: DisplayLabel(id.to_string()),
            status: ModelStatus::Healthy,
        }
    }

    struct StubPlugin {
        tool_id: ToolId,
        models: Vec<DiscoveredModel>,
        panics_on_discover: bool,
    }

    #[async_trait]
    impl Tool for StubPlugin {
        fn name(&self) -> ToolId {
            self.tool_id
        }
        fn accepted_formats(&self) -> &'static [Format] {
            &[Format::Gguf]
        }
        async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
            if self.panics_on_discover {
                panic!("stub plugin panic for test");
            }
            Ok(self.models.clone())
        }
        async fn link(&self, _src: &Path, _model: &ModelMeta) -> Result<LinkOutcome, LinkError> {
            Err(LinkError::NotYetImplemented("test stub".to_string()))
        }
        async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
            Err(DeleteError::Unsupported { tool: self.tool_id })
        }
        async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
            Err(DeleteError::Unsupported { tool: self.tool_id })
        }
    }

    fn config_with_cache(cache_path: PathBuf) -> ReconcileConfig {
        ReconcileConfig {
            cache_path: Some(cache_path),
            diagnostics_dir: None,
            now: SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000),
        }
    }

    /// RED_UNIT — cache_path = None short-circuits to AllCompleted.
    #[tokio::test]
    async fn run_with_no_cache_path_emits_only_all_completed() {
        let config = ReconcileConfig::default();
        let stream = run(ReconcileScope::All, Vec::new(), config);
        let events = collect_all(stream).await;
        assert_eq!(events, vec![ReconcileEvent::AllCompleted]);
    }

    /// RED_UNIT — empty plugin list emits exactly one AllCompleted.
    #[tokio::test]
    async fn run_with_empty_plugins_emits_only_all_completed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_path = tmp.path().join("cache.sqlite");
        let config = config_with_cache(cache_path);
        let stream = run(ReconcileScope::All, Vec::new(), config);
        let events = collect_all(stream).await;
        assert_eq!(events, vec![ReconcileEvent::AllCompleted]);
    }

    /// RED_UNIT — Scope::Tool with no matching plugin emits ToolFailed +
    /// AllCompleted.
    #[tokio::test]
    async fn run_with_unknown_tool_scope_emits_tool_failed_then_all_completed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_path = tmp.path().join("cache.sqlite");
        let config = config_with_cache(cache_path);
        let stream = run(
            ReconcileScope::Tool(ToolId("does-not-exist")),
            Vec::new(),
            config,
        );
        let events = collect_all(stream).await;
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0], ReconcileEvent::ToolFailed { .. }));
        assert_eq!(events[1], ReconcileEvent::AllCompleted);
    }

    /// RED_UNIT — successful per-tool reconcile emits ToolStarted +
    /// ToolCompleted with an added-models diff (cache was empty), then
    /// AllCompleted.
    #[tokio::test]
    async fn run_succeeds_and_emits_diff_for_fresh_tool() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_path = tmp.path().join("cache.sqlite");
        let plugin: PluginHandle = Arc::new(StubPlugin {
            tool_id: fixture_tool_id(),
            models: vec![fixture_discovered("m1", 100)],
            panics_on_discover: false,
        });
        let config = config_with_cache(cache_path);
        let stream = run(ReconcileScope::All, vec![plugin], config);
        let events = collect_all(stream).await;

        assert!(matches!(
            events[0],
            ReconcileEvent::ToolStarted { tool } if tool == fixture_tool_id()
        ));
        match &events[1] {
            ReconcileEvent::ToolCompleted { tool, diff } => {
                assert_eq!(*tool, fixture_tool_id());
                assert_eq!(diff.added_models, vec!["m1".to_string()]);
                assert!(diff.removed_models.is_empty());
            }
            other => panic!("expected ToolCompleted, got {other:?}"),
        }
        assert_eq!(events.last(), Some(&ReconcileEvent::AllCompleted));
    }

    /// RED_UNIT — plugin panic during discover() is isolated as a
    /// ToolFailed event; the orchestrator continues to AllCompleted.
    #[tokio::test]
    async fn run_isolates_plugin_panic_as_tool_failed() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache_path = tmp.path().join("cache.sqlite");
        let plugin: PluginHandle = Arc::new(StubPlugin {
            tool_id: fixture_tool_id(),
            models: Vec::new(),
            panics_on_discover: true,
        });
        let config = config_with_cache(cache_path);
        let stream = run(ReconcileScope::All, vec![plugin], config);
        let events = collect_all(stream).await;

        assert!(events.iter().any(|e| matches!(
            e,
            ReconcileEvent::ToolFailed { tool, .. } if *tool == fixture_tool_id()
        )));
        assert_eq!(events.last(), Some(&ReconcileEvent::AllCompleted));
    }

    /// RED_UNIT — atomic_reconcile_write delivers INT-INFO-3: the total
    /// disk_usage_bytes equals the sum of per-model size_bytes (1-byte
    /// rounding, all u64). This proves the row projection upholds the
    /// invariant that the renderer later asserts.
    #[test]
    fn project_to_cache_rows_preserves_disk_usage_sum_invariant() {
        let discovered = vec![
            fixture_discovered("a", 100),
            fixture_discovered("b", 250),
            fixture_discovered("c", 500),
        ];
        let (tool_row, model_rows) = project_to_cache_rows(
            fixture_tool_id(),
            &discovered,
            "0.0.0",
            SystemTime::UNIX_EPOCH,
        );
        let sum_of_models: u64 = model_rows.iter().map(|m| m.size_bytes).sum();
        assert_eq!(
            tool_row.disk_usage_bytes, sum_of_models,
            "INT-INFO-3: total.disk_usage == sum(model.size_bytes)"
        );
    }
}
