//! JSONL launch log writer (per `docs/feature/modeltap-tui/devops/kpi-instrumentation.md`).
//!
//! Step 01-01 emits exactly two event types:
//! - `launch.started` — first event in every session, carries session_id +
//!   modeltap_version + platform.
//! - `launch.ended` — clean-quit exit only (NOT on Ctrl+C).
//!
//! Per intake Q7 + ADR-003 (stateless guarantee), an unwritable log dir must
//! NOT crash the binary. The writer degrades gracefully: it warns once on
//! stderr and discards subsequent events.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;
use ulid::Ulid;

const SCHEMA: &str = "modeltap.launch.v1";

/// What an event records. Step 01-01 needs only Started/Ended; step 01-02
/// adds launch.timing + launch.inventory per kpi-instrumentation.md §3.
///
/// All variants share the `Launch` prefix because every event in this enum
/// is a `launch.*` JSONL event per the v1 schema. The shared prefix is part
/// of the schema vocabulary, not a code smell.
#[allow(clippy::enum_variant_names)]
pub enum RecordKind {
    LaunchStarted,
    LaunchEnded,
    /// Per-plugin discovery timings. The K3 KPI is computed from this event's
    /// `process_start_to_first_paint_ms` field; for now we emit a minimal
    /// shape with `plugin_timings_ms`. Other K3 fields land when the
    /// production loop is built in 01-03.
    LaunchTiming {
        plugin_timings_ms: Vec<(String, u64)>,
        full_inventory_ms: u64,
        model_count: u64,
    },
    /// Cross-tool inventory summary. Per the acceptance test plan + AC-3 the
    /// event is emitted EVEN when totals are zero and EVEN when one or more
    /// plugins errored. Schema is intentionally a superset of the v1 spec
    /// (see kpi-instrumentation.md §3) — extra fields are forward-compatible.
    LaunchInventory {
        total_models: u64,
        total_disk_usage_bytes: u64,
        dedupable_count: u64,
        format_locked_count: u64,
        tool_errors: Vec<String>,
    },
    /// Result of a confirmed zap-all action (US-05). Per the privacy rule
    /// (`kpi-instrumentation.md` §"Privacy"): NO model names, NO paths, NO
    /// usernames — only tool name + aggregate counts.
    ActionZapAll {
        tool: String,
        models_removed: u64,
        bytes_reclaimed: u64,
        outcome: &'static str,
    },
    /// One entry per discovered model. Written to a separate `models.log`
    /// file (NOT `launch.log`) so per-model metadata stays out of the
    /// privacy-sensitive launch event stream. Used by acceptance tests to
    /// assert per-model `format`, `display_label`, `status` without going
    /// through the TUI.
    ///
    /// This is internal-tooling-only — the user is opted-in via the existing
    /// log dir, and the file lives next to launch.log under the same
    /// MODELTAP_LOG_DIR.
    DiscoveredModel {
        tool: String,
        id_in_tool: String,
        display_label: String,
        format: &'static str,
        status: &'static str,
        size_bytes: u64,
    },
}

pub struct LaunchLogger {
    session_id: String,
    log_path: Option<PathBuf>,
    /// Path for per-model JSONL entries. Lives next to `launch.log` under
    /// the same `MODELTAP_LOG_DIR`. Optional because the log dir may be
    /// unwritable; we silently no-op model writes in that case.
    models_log_path: Option<PathBuf>,
    /// True once we've warned to stderr about an unwritable log dir; prevents
    /// repeated warnings if multiple writes fail.
    warned_unwritable: bool,
}

impl LaunchLogger {
    /// Construct a logger for the given log directory. If `log_dir` is None
    /// or unwritable, the logger silently no-ops every record() call after
    /// emitting one warning to stderr (per AC-7).
    pub fn open(log_dir: Option<PathBuf>) -> Self {
        let session_id = Ulid::new().to_string();
        let log_path = log_dir.as_ref().map(|d| d.join("launch.log"));
        let models_log_path = log_dir.as_ref().map(|d| d.join("models.log"));
        let mut me = Self {
            session_id,
            log_path,
            models_log_path,
            warned_unwritable: false,
        };
        // Probe writability up-front so we can emit the warning once before
        // discovery starts. Tests assert the warning text on stderr.
        if let Some(path) = me.log_path.clone() {
            if !is_writable(&path) {
                me.warn_and_disable(&path);
            }
        }
        me
    }

    /// Emit the user-facing "cannot write launch log" warning to stderr (once)
    /// and disable further writes. Centralizes the formatting so the warning
    /// text matches in both the up-front writability probe and the
    /// post-failure path inside `record()`.
    fn warn_and_disable(&mut self, path: &Path) {
        let dir = path
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| path.display().to_string());
        eprintln!("warning: cannot write launch log to {}", dir);
        self.warned_unwritable = true;
        self.log_path = None;
        // Don't try to write per-model entries either if the dir is dead.
        self.models_log_path = None;
    }

    /// Append one JSONL event. Best-effort; failures degrade silently after
    /// the first stderr warning.
    pub fn record(&mut self, kind: RecordKind) {
        // `DiscoveredModel` writes to a separate file; everything else writes
        // to launch.log. Branch up front so the launch.log path computation
        // does not need to gate the per-model write.
        if let RecordKind::DiscoveredModel {
            tool,
            id_in_tool,
            display_label,
            format,
            status,
            size_bytes,
        } = kind
        {
            self.write_model_entry(
                &tool,
                &id_in_tool,
                &display_label,
                format,
                status,
                size_bytes,
            );
            return;
        }

        let Some(path) = self.log_path.clone() else {
            return;
        };
        let payload = match kind {
            RecordKind::LaunchStarted => self.base_envelope("launch.started"),
            RecordKind::LaunchEnded => self.base_envelope("launch.ended"),
            RecordKind::LaunchTiming {
                plugin_timings_ms,
                full_inventory_ms,
                model_count,
            } => {
                let mut env = self.base_envelope("launch.timing");
                let timings_obj: serde_json::Map<String, serde_json::Value> = plugin_timings_ms
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::from(v)))
                    .collect();
                env["plugin_timings_ms"] = serde_json::Value::Object(timings_obj);
                env["full_inventory_ms"] = json!(full_inventory_ms);
                env["model_count"] = json!(model_count);
                env
            }
            RecordKind::LaunchInventory {
                total_models,
                total_disk_usage_bytes,
                dedupable_count,
                format_locked_count,
                tool_errors,
            } => {
                let mut env = self.base_envelope("launch.inventory");
                env["total_models"] = json!(total_models);
                env["total_disk_usage_bytes"] = json!(total_disk_usage_bytes);
                env["dedupable_count"] = json!(dedupable_count);
                env["format_locked_count"] = json!(format_locked_count);
                env["tool_errors"] = json!(tool_errors);
                env
            }
            RecordKind::ActionZapAll {
                tool,
                models_removed,
                bytes_reclaimed,
                outcome,
            } => {
                let mut env = self.base_envelope("action.zap_all");
                env["tool"] = json!(tool);
                env["models_removed"] = json!(models_removed);
                env["bytes_reclaimed"] = json!(bytes_reclaimed);
                env["outcome"] = json!(outcome);
                env
            }
            RecordKind::DiscoveredModel { .. } => unreachable!("handled above"),
        };
        let mut serialized = payload.to_string();
        serialized.push('\n');
        let result = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(serialized.as_bytes()));
        if result.is_err() && !self.warned_unwritable {
            self.warn_and_disable(&path);
        }
    }

    fn write_model_entry(
        &mut self,
        tool: &str,
        id_in_tool: &str,
        display_label: &str,
        format: &'static str,
        status: &'static str,
        size_bytes: u64,
    ) {
        let Some(path) = self.models_log_path.clone() else {
            return;
        };
        let env = json!({
            "schema": "modeltap.models.v1",
            "ts": current_timestamp(),
            "session_id": self.session_id,
            "tool": tool,
            "id_in_tool": id_in_tool,
            "display_label": display_label,
            "format": format,
            "status": status,
            "size_bytes": size_bytes,
        });
        let mut serialized = env.to_string();
        serialized.push('\n');
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(serialized.as_bytes()));
        // Per-model entries are a tooling convenience; if writing fails we
        // silently drop. The user already saw a warning if launch.log failed.
    }

    fn base_envelope(&self, event: &str) -> serde_json::Value {
        json!({
            "schema": SCHEMA,
            "ts": current_timestamp(),
            "session_id": self.session_id,
            "event": event,
            "modeltap_version": env!("CARGO_PKG_VERSION"),
            "platform": platform_triplet(),
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.log_path.as_deref()
    }
}

fn is_writable(path: &Path) -> bool {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .is_ok()
}

fn current_timestamp() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}

fn platform_triplet() -> String {
    format!(
        "{}-{}",
        match std::env::consts::OS {
            "macos" => "macos",
            "linux" => "linux",
            other => other,
        },
        std::env::consts::ARCH
    )
}
