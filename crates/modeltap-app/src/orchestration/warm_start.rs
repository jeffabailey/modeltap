//! Warm-start orchestrator (AC-25-* happy paths).
//!
//! Top-of-launch routine that:
//!   (a) jumps to cold-start (returns `Inventory::empty()`) when the cache
//!       is disabled by config (`cache.enabled = false` or `--no-cache`),
//!   (b) opens the cache via `Cache::open` wrapped in
//!       `tokio::task::spawn_blocking` (architecture-design.md §7.1 — sync
//!       rusqlite at the edge of async land),
//!   (c) on `OpenedFresh` returns `Inventory::empty()` (cold-start will
//!       populate),
//!   (d) on `OpenedExisting` / `OpenedAfterMigration` reads `cache.tools()`
//!       + `cache.models_for_tool(_)` for each tool (also via
//!       `spawn_blocking`) and builds an `Inventory`,
//!   (e) emits a JSONL `launch.warm_paint_ms` event measured from
//!       `run_start` to inventory-ready (warm path only — fresh and
//!       disabled paths emit no warm-paint event; cold-start emits its own
//!       `launch.first_paint_ms` later).
//!
//! Full TTL eligibility, per-tool warm-vs-cold mix, and recovery banner are
//! deferred to phase 04. Step 01-04 wires only the happy paths.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use modeltap_core::logic::compatibility::{Inventory, InventoryEntry};
use modeltap_core::types::{ContentHash, DiscoveredModel, DisplayLabel, Format, ModelStatus, ToolId};
use modeltap_store::types::CachedModel;
use modeltap_store::{Cache, CacheError, CacheOpenResult};
use serde_json::json;
use thiserror::Error;

use crate::config::DEFAULT_TOOL_TTL_SECONDS;

/// Inputs into the warm-start path.
#[derive(Debug, Clone)]
pub struct WarmStartConfig {
    /// Honors `--no-cache` and `[cache] enabled = false` (AC-25-7,
    /// AC-23-8/9). When false, warm-start short-circuits to
    /// `WarmStartSource::Disabled` and the launch falls through to
    /// cold-start.
    pub cache_enabled: bool,

    /// Where to emit `launch.warm_paint_ms`. Mirrors `MODELTAP_LOG_DIR` from
    /// the binary's `observability::LaunchLogger`. `None` disables JSONL
    /// emission (the warm-start path remains functional — tests assert no
    /// event is written when the dir is absent).
    pub log_dir: Option<PathBuf>,

    /// Per-tool TTL eligibility window in seconds (step 04-03 / US-25
    /// AC-25-2 + AC-25-4). A cached tool row whose `last_scan_at >= now -
    /// tool_ttl_seconds` paints from cache; older rows are returned as
    /// `stale_tool_ids` for the downstream cold-scan dispatcher. Defaults to
    /// `DEFAULT_TOOL_TTL_SECONDS` (24h) so step-01-04 callers that never
    /// set it inherit the documented value.
    pub tool_ttl_seconds: u64,

    /// Reference instant for the TTL comparison. Taken as a parameter
    /// instead of `SystemTime::now()` so the orchestrator stays
    /// deterministic under test. Production callers pass
    /// `SystemTime::now()`.
    pub now: SystemTime,
}

impl Default for WarmStartConfig {
    fn default() -> Self {
        Self {
            cache_enabled: false,
            log_dir: None,
            tool_ttl_seconds: DEFAULT_TOOL_TTL_SECONDS,
            now: SystemTime::now(),
        }
    }
}

/// Where the inventory came from. The composition root branches on this:
/// `Existing` paints from cache + dispatches a background reconcile;
/// `Fresh`, `Disabled`, and `AfterRecovery` fall through to cold-start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarmStartSource {
    /// `cache_enabled = false` — cold-start owns the launch entirely.
    Disabled,
    /// Cache file did not exist before this call; an empty schema was
    /// created. Cold-start populates it on first reconcile.
    Fresh,
    /// Cache existed and is at the expected schema version.
    Existing,
    /// Cache existed at an older schema and was migrated forward.
    AfterMigration { from: u32, to: u32 },
}

/// Result of the warm-start path. The composition root inspects `source`
/// to decide whether to paint immediately or proceed to cold-start.
#[derive(Debug)]
pub struct WarmStartResult {
    pub inventory: Inventory,
    pub source: WarmStartSource,
    /// Step 04-03: tool_ids whose cached row failed the per-tool TTL gate
    /// (or returned a transient I/O error during read). The caller
    /// dispatches a per-tool cold-scan for each; tools whose models DID
    /// paint from cache are absent from this list. Always empty on the
    /// `Disabled` / `Fresh` paths (no cache to age out from).
    pub stale_tool_ids: Vec<ToolId>,
}

#[derive(Debug, Error)]
pub enum WarmStartError {
    #[error("cache I/O failed")]
    Cache(#[from] CacheError),

    /// `spawn_blocking` itself failed (panic or runtime shutdown). Treat
    /// as a cold-start signal at the call site.
    #[error("blocking task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

/// Run the warm-start path. Always returns a `WarmStartResult` on success;
/// callers branch on `source` to decide whether to skip cold-start. On
/// error, callers MUST fall back to cold-start (AC-23-11 / C-INFO-2: cache
/// failure never prevents the inventory view).
pub async fn run(
    config: &WarmStartConfig,
    cache_path: &Path,
) -> Result<WarmStartResult, WarmStartError> {
    let run_start = Instant::now();

    if !config.cache_enabled {
        return Ok(WarmStartResult {
            inventory: Inventory::default(),
            source: WarmStartSource::Disabled,
            stale_tool_ids: Vec::new(),
        });
    }

    let path_for_open = cache_path.to_path_buf();
    let open_result = tokio::task::spawn_blocking(move || Cache::open(&path_for_open)).await??;

    let (cache, source) = match open_result {
        CacheOpenResult::OpenedFresh(_) => {
            // Empty schema — nothing to paint. Cold-start populates.
            return Ok(WarmStartResult {
                inventory: Inventory::default(),
                source: WarmStartSource::Fresh,
                stale_tool_ids: Vec::new(),
            });
        }
        CacheOpenResult::OpenedExisting(c) => (c, WarmStartSource::Existing),
        CacheOpenResult::OpenedAfterMigration { from, to, cache } => {
            (cache, WarmStartSource::AfterMigration { from, to })
        }
        CacheOpenResult::OpenedAfterRecovery { .. } => {
            // Recovery path is wired in step 01-05+. For now, treat as a
            // fresh empty cache so the cold-start populates.
            return Ok(WarmStartResult {
                inventory: Inventory::default(),
                source: WarmStartSource::Fresh,
                stale_tool_ids: Vec::new(),
            });
        }
    };

    // Per-tool TTL eligibility partition (step 04-03 / AC-25-2 / AC-25-4):
    // fresh tools paint from cache; stale tools fall through to cold-start.
    //
    // Transient I/O fallback (AC-25-7): a per-tool read error (CacheError::Io
    // / CacheError::Sqlite mid-read) does NOT abort warm-start — that tool
    // is treated as stale so cold-start picks it up. Only a `cache.tools()`
    // failure (cannot enumerate at all) bubbles up; cache_enabled=true with
    // an unreadable top-level tools list is what the C-INFO-2 outer
    // fallback at the call site handles.
    let ttl_seconds = config.tool_ttl_seconds;
    let now = config.now;
    let partition =
        tokio::task::spawn_blocking(move || -> Result<WarmPartition, CacheError> {
            let tools = cache.tools()?;
            let mut entries: Vec<InventoryEntry> = Vec::new();
            let mut stale: Vec<ToolId> = Vec::new();
            for tool in &tools {
                let eligible = match cache.ttl_eligible(&tool.tool_id, ttl_seconds, now) {
                    Ok(b) => b,
                    Err(CacheError::Io { .. }) | Err(CacheError::Sqlite(_)) => {
                        // Transient read failure for this tool — treat as
                        // stale; cold-start will own the row.
                        stale.push(tool.tool_id);
                        continue;
                    }
                    Err(other) => return Err(other),
                };
                if !eligible {
                    stale.push(tool.tool_id);
                    continue;
                }
                match cache.models_for_tool(&tool.tool_id) {
                    Ok(models) => {
                        for m in models {
                            entries.push(inventory_entry_from_cached(m));
                        }
                    }
                    Err(CacheError::Io { .. }) | Err(CacheError::Sqlite(_)) => {
                        // Tool was TTL-fresh but the model rows could not
                        // be read — fall through to cold-start for this
                        // tool. Drop any entries already accumulated for
                        // the tool (none yet, because the inner loop only
                        // appends on `Ok`).
                        stale.push(tool.tool_id);
                    }
                    Err(other) => return Err(other),
                }
            }
            Ok(WarmPartition {
                inventory: Inventory { entries },
                stale_tool_ids: stale,
            })
        })
        .await??;

    let elapsed_ms = run_start.elapsed().as_millis() as u64;
    emit_warm_paint_event(config.log_dir.as_deref(), elapsed_ms);

    Ok(WarmStartResult {
        inventory: partition.inventory,
        source,
        stale_tool_ids: partition.stale_tool_ids,
    })
}

/// Internal carrier for the `spawn_blocking` closure — keeps the
/// (inventory, stale-list) pair together so the outer task does not need
/// to thread two values out by hand.
struct WarmPartition {
    inventory: Inventory,
    stale_tool_ids: Vec<ToolId>,
}

/// Project a `CachedModel` row back into the cross-plugin `InventoryEntry`
/// the engine consumes. `content_hash` is `None` until SHA256 cache
/// hydration (phase 04) lifts the hex string back into a `ContentHash`.
fn inventory_entry_from_cached(m: CachedModel) -> InventoryEntry {
    let format = parse_format(m.format.as_deref());
    let size_bytes = m.size_bytes;
    let display_label = DisplayLabel(m.display_name.clone());
    let id_in_tool = m.model_id.clone();

    InventoryEntry {
        tool: m.tool_id,
        model: DiscoveredModel {
            id_in_tool,
            display_label,
            format,
            size_bytes,
            // The cache row's path is not stored on `CachedModel` — file
            // rows live in `cache_model_files` (phase 04). Use an empty
            // path; the compatibility engine does not consult `on_disk_path`.
            on_disk_path: PathBuf::new(),
            status: ModelStatus::Healthy,
        },
        content_hash: m.sha256.and_then(parse_content_hash),
    }
}

fn parse_format(label: Option<&str>) -> Format {
    match label.map(|s| s.to_ascii_lowercase()) {
        Some(ref s) if s == "gguf" => Format::Gguf,
        Some(ref s) if s == "safetensors" => Format::Safetensors,
        Some(ref s) if s == "bin" => Format::Bin,
        Some(ref s) if s == "awq" => Format::Awq,
        Some(ref s) if s == "gptq" => Format::Gptq,
        Some(ref s) if s == "ollamablob" || s == "ollama_blob" => Format::OllamaBlob,
        Some(ref s) if s == "mlx" => Format::Mlx,
        _ => Format::Other,
    }
}

fn parse_content_hash(hex: String) -> Option<ContentHash> {
    // ContentHash is a thin newtype over [u8; 32] (ADR-002). Step 01-04
    // does not require revivable hashes — the compatibility engine treats
    // `None` as "not computed yet". Leave wiring to phase 04.
    let _ = hex;
    None
}

/// Append a single `launch.warm_paint_ms` JSONL line to `<log_dir>/launch.log`.
/// Best-effort; failures are swallowed so an unwritable log dir never blocks
/// the launch.
fn emit_warm_paint_event(log_dir: Option<&Path>, duration_ms: u64) {
    let Some(dir) = log_dir else {
        return;
    };
    let path = dir.join("launch.log");
    let envelope = json!({
        "schema": "modeltap.launch.v1",
        "event": "launch.warm_paint_ms",
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
