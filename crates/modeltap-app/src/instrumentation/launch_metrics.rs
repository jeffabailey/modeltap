//! Launch-metrics JSONL facade — emits the four `launch.*` duration events
//! the cache-state-model + integration-checkpoints acceptance suites read
//! from `<log_dir>/launch.log` (acceptance-test-plan.md §5, outcome-kpis.md
//! K3a / K3b / K-INFO-1 / K-INFO-7).
//!
//! Step 04-05 (closes Phase 04) collapses the per-boundary `emit_*_event`
//! helpers previously inlined in `warm_start.rs` + `main.rs` (each one a
//! 16-line `OpenOptions::new().create(true).append(true)…` snippet) into a
//! single facade so future events do not silently fork the line format. The
//! existing `launch.warm_paint_ms` + `cache.write_wait_ms` callers (steps
//! 01-04 and 04-04) keep their public envelopes byte-identical: the facade
//! writes the SAME `{"schema":"modeltap.launch.v1","event":...,...}\n` line
//! shape and the SAME best-effort fail-silent semantics so an unwritable log
//! dir never blocks the launch (C-INFO-2 + AC-23-11).
//!
//! The four events emitted by this module:
//!
//! - `launch.cache_open_ms` — `Cache::open` + `tools()` + per-tool
//!   `models_for_tool(_)` round-trip from the warm-start orchestrator. K-INFO-7
//!   budget: ≤ 100 ms p90.
//! - `launch.warm_paint_ms` — cache-painted inventory first hits the TUI
//!   buffer. K-INFO-1 / K3a budget: ≤ 150 ms p90 (debug-build envelope; the
//!   release-build target is ≤ 100 ms per outcome-kpis.md §K-INFO-1).
//! - `launch.first_paint_ms` — cold-start skeleton paint (warm-start
//!   disabled OR returned `Disabled` / `Fresh`). K3b budget: ≤ 150 ms p90.
//! - `launch.full_inventory_paint_ms` — cold-scan inventory completes its
//!   full discovery pass. K3b budget: ≤ 1150 ms p90.
//!
//! All four are emitted with the same `duration_ms` field name so the
//! cucumber driver (`tests/acceptance/cache_kpi.rs`) can parse them with one
//! helper. The `cache.write_wait_ms` event (step 04-04, written from
//! `main::emit_cache_write_wait_event`) uses `wait_ms` instead and is NOT
//! routed through this facade — its contract is "wait time at the
//! `BEGIN IMMEDIATE` boundary", not a paint duration, so the field-name
//! divergence is intentional.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;

/// Shared JSONL schema string for every launch.* event the facade emits.
/// Matches the existing `launch.warm_paint_ms` + `cache.write_wait_ms`
/// callers byte-for-byte so consumers (acceptance suites, future log
/// shippers) keep one stable schema identifier.
pub const LAUNCH_LOG_SCHEMA: &str = "modeltap.launch.v1";

/// Filename of the launch log inside `<log_dir>`. Centralised so a future
/// rename does not require grepping the codebase for the literal string.
pub const LAUNCH_LOG_FILENAME: &str = "launch.log";

/// Facade over the launch.log JSONL writer. Holds an optional `log_dir` so
/// the call site can construct one instance per launch and pass it to the
/// orchestrators by reference — when `log_dir` is `None`, every `record_*`
/// call is a no-op (matches the existing `emit_warm_paint_event` semantics
/// from step 01-04).
///
/// `Clone` is derived so the composition root can hand one instance to the
/// warm-start path and the cold-start path without ref-counting boilerplate
/// (the inner `Option<PathBuf>` is the only owned state and `PathBuf` clones
/// cheaply enough for a once-per-launch use).
#[derive(Debug, Clone, Default)]
pub struct LaunchMetrics {
    log_dir: Option<PathBuf>,
}

impl LaunchMetrics {
    /// Build a metrics facade pointed at `log_dir`. `None` disables emission
    /// (every `record_*` call returns immediately) — mirrors the production
    /// behaviour when `MODELTAP_LOG_DIR` is unset.
    pub fn new(log_dir: Option<PathBuf>) -> Self {
        Self { log_dir }
    }

    /// Return the resolved log dir (for callers that need to compose paths
    /// to sibling files — e.g., models.log). `None` when emission is
    /// disabled.
    pub fn log_dir(&self) -> Option<&Path> {
        self.log_dir.as_deref()
    }

    /// Emit `launch.cache_open_ms`. Called from the warm-start orchestrator
    /// after `Cache::open` + the per-tool read round-trip completes. K-INFO-7
    /// budget: ≤ 100 ms p90.
    pub fn record_cache_open(&self, duration_ms: u64) {
        self.write_event("launch.cache_open_ms", duration_ms);
    }

    /// Emit `launch.warm_paint_ms`. Called from the warm-start orchestrator
    /// after the cache-painted inventory is materialised. K-INFO-1 / K3a
    /// budget: ≤ 150 ms p90 (debug); ≤ 100 ms (release).
    pub fn record_warm_paint(&self, duration_ms: u64) {
        self.write_event("launch.warm_paint_ms", duration_ms);
    }

    /// Emit `launch.first_paint_ms`. Called from the composition root on the
    /// cold-start path after the empty/skeleton AppState has been built and
    /// is ready to hand to the TUI loop. K3b budget: ≤ 150 ms p90.
    pub fn record_first_paint(&self, duration_ms: u64) {
        self.write_event("launch.first_paint_ms", duration_ms);
    }

    /// Emit `launch.full_inventory_paint_ms`. Called from the composition
    /// root after the cold-scan discovery pass populates the full inventory.
    /// K3b budget: ≤ 1150 ms p90.
    pub fn record_full_inventory_paint(&self, duration_ms: u64) {
        self.write_event("launch.full_inventory_paint_ms", duration_ms);
    }

    /// Best-effort JSONL append. Identical line shape to the pre-facade
    /// `emit_warm_paint_event` helper from step 01-04. Failures are swallowed
    /// (an unwritable log dir never blocks the launch).
    fn write_event(&self, event: &str, duration_ms: u64) {
        let Some(dir) = self.log_dir.as_deref() else {
            return;
        };
        let path = dir.join(LAUNCH_LOG_FILENAME);
        let envelope = json!({
            "schema": LAUNCH_LOG_SCHEMA,
            "event": event,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tempfile::TempDir;

    fn read_log_lines(dir: &Path) -> Vec<Value> {
        let path = dir.join(LAUNCH_LOG_FILENAME);
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        raw.lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    #[test]
    fn no_log_dir_disables_emission() {
        let metrics = LaunchMetrics::new(None);
        // Should not panic, should not create any file.
        metrics.record_cache_open(42);
        metrics.record_warm_paint(100);
        metrics.record_first_paint(50);
        metrics.record_full_inventory_paint(900);
    }

    #[test]
    fn four_record_methods_each_emit_their_own_event() {
        let tmp = TempDir::new().expect("tempdir");
        let metrics = LaunchMetrics::new(Some(tmp.path().to_path_buf()));
        metrics.record_cache_open(10);
        metrics.record_warm_paint(20);
        metrics.record_first_paint(30);
        metrics.record_full_inventory_paint(40);

        let lines = read_log_lines(tmp.path());
        assert_eq!(lines.len(), 4, "one line per record_*");
        let events: Vec<&str> = lines
            .iter()
            .filter_map(|v| v.get("event").and_then(|e| e.as_str()))
            .collect();
        assert_eq!(
            events,
            vec![
                "launch.cache_open_ms",
                "launch.warm_paint_ms",
                "launch.first_paint_ms",
                "launch.full_inventory_paint_ms",
            ]
        );
        // duration_ms values round-trip
        let durations: Vec<u64> = lines
            .iter()
            .filter_map(|v| v.get("duration_ms").and_then(|d| d.as_u64()))
            .collect();
        assert_eq!(durations, vec![10, 20, 30, 40]);
        // Every line carries the schema identifier
        for line in &lines {
            assert_eq!(
                line.get("schema").and_then(|s| s.as_str()),
                Some(LAUNCH_LOG_SCHEMA)
            );
        }
    }

    #[test]
    fn multiple_records_append_in_order() {
        let tmp = TempDir::new().expect("tempdir");
        let metrics = LaunchMetrics::new(Some(tmp.path().to_path_buf()));
        for n in 0..5u64 {
            metrics.record_warm_paint(n);
        }
        let lines = read_log_lines(tmp.path());
        assert_eq!(lines.len(), 5);
        for (i, line) in lines.iter().enumerate() {
            assert_eq!(
                line.get("duration_ms").and_then(|d| d.as_u64()),
                Some(i as u64)
            );
        }
    }
}
