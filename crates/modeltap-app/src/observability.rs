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
    ///
    /// `tools_registered` (US-18 AC-7) — alphabetically-sorted list of every
    /// plugin's `Tool::name()`. Riley's release dashboards consume this field
    /// to know which plugin set is deployed in a given build. Sorted so a
    /// build-over-build diff is stable.
    LaunchInventory {
        total_models: u64,
        total_disk_usage_bytes: u64,
        dedupable_count: u64,
        format_locked_count: u64,
        tool_errors: Vec<String>,
        tools_registered: Vec<String>,
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
    /// Result of a confirmed single-model delete-from-one action (US-05b,
    /// step 03-06; ADR-009). Per the privacy rule: NO model names, NO paths,
    /// NO usernames — only tool name, aggregate byte count, was-shared
    /// classification, outcome string. The `was_shared` discriminator lets
    /// observability distinguish low-friction (`true` -> y/n confirm) from
    /// typed-id (`false` -> Unique mode) destructive paths without leaking
    /// the model id itself.
    ActionZapOne {
        tool: String,
        bytes_reclaimed: u64,
        was_shared: bool,
        outcome: &'static str,
    },
    /// Result of a confirmed unify action (US-10). Per the privacy rule
    /// (`kpi-instrumentation.md` §"action.unify"): NO model names, NO paths,
    /// NO hash values — only tool ids + aggregate byte counts. The
    /// `model_dedup_key_kind` discriminator records WHICH dedup-key family
    /// produced the unify (sha256 vs hf-hub-id+quant) without disclosing
    /// the value.
    ActionUnify {
        model_dedup_key_kind: &'static str,
        tools_unified: Vec<String>,
        bytes_reclaimed: u64,
        outcome: &'static str,
        /// US-19: count of cross-fs targets the user chose to skip. NOT
        /// failures — explicit user choice. Always present (0 in the
        /// same-fs fast path); makes the JSONL schema stable for the K-set.
        cross_fs_targets_skipped: u64,
        /// US-19: count of cross-fs targets the user chose to byte-copy.
        /// Counted as success in `tools_unified` but reclaim is zero.
        cross_fs_targets_copied: u64,
    },
    /// Result of a dry-run preview of a unify action (US-14). Distinct from
    /// `ActionUnify` so K1/K5 instrumentation can distinguish previewed-vs-
    /// executed actions; emitted with `outcome="previewed"` and never
    /// written to disk (no `bytes_reclaimed`, only `bytes_would_reclaim`).
    /// Per the privacy rule (`kpi-instrumentation.md` §"action.unify"): NO
    /// model names, NO paths, NO hash values.
    ActionUnifyDryRun {
        model_dedup_key_kind: &'static str,
        tools_to_unify: Vec<String>,
        bytes_would_reclaim: u64,
        cross_fs_targets: u64,
        outcome: &'static str,
    },
    /// Result of a confirmed folder-group-bulk-delete action (US-05c,
    /// step 01-05; ADR-010). Per the privacy rule (`kpi-instrumentation.md`
    /// §"Privacy"): NO on-disk paths, NO blob hex digests. The `folder_path`
    /// field is the canonical `<author>/<repo>` identifier the user typed
    /// at the confirmation prompt — a logical identifier, NOT a filesystem
    /// path. `outcome` is one of `"success"`, `"partial"`, `"failed"`,
    /// `"cancelled_mismatch"` (typed-confirm byte-mismatch — step 02-01),
    /// or `"cancelled_escape"` (Esc pressed during the dialog — step 02-01).
    /// `outcomes_count` is the size of the `Vec<DeleteOutcome>` returned by
    /// `Tool::delete_folder`; ALWAYS 0 on the cancel paths because the plugin
    /// is never called (step 02-01, M6 @property invariant).
    /// `keystroke_count` (step 06-01, K-FGD-2 / D3): total input events the
    /// folder-confirm dialog observed from open-to-decision — printable
    /// chars + Backspace + Ctrl+W. Shift+F is excluded (it transitions
    /// FROM main view TO dialog state). Always emitted on EVERY path
    /// (success, cancel, refusal) so the M6 @property test cannot pass
    /// vacuously. May be 0 on pre-flight refusal paths (the dialog never
    /// opened) — that is the correct semantic value, not a missing field.
    ActionFolderDelete {
        tool: String,
        folder_path: String,
        files_total: u64,
        files_removed: u64,
        bytes_reclaimed: u64,
        bytes_retained: u64,
        outcomes_count: u64,
        keystroke_count: u64,
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
    /// Pre-mutate revalidation invocation (Step 05-02 part 2/2 — K5 gate).
    /// Emitted once per `orchestration::revalidate::pre_mutate` call. Fields
    /// per dispatch spec: tool, model, outcome ("proceed"|"drift"|"gone"|
    /// "store_error"), duration_ms. NO paths, NO blob hex digests — `tool`
    /// is the registered plugin id (e.g., `"hf"`); `model` is the
    /// plugin-supplied `id_in_tool` string (logical identifier, not a
    /// filesystem path). The K5 invariant ("cache must never enable a
    /// stale-data destructive action") is enforced at every destructive
    /// entry point; this event lets observability see WHEN the gate ran,
    /// WHAT it decided, and HOW LONG the revalidation took (input to
    /// future K-INFO budgets on destructive-action latency).
    RevalidateInvoked {
        tool: String,
        model: String,
        outcome: &'static str,
        duration_ms: u64,
    },
    /// Plugin `inspect_model` invocation initiated by the pre-mutate
    /// revalidator (Step 05-04 — US-26 AC-26-6). Emitted once per call to
    /// `orchestration::revalidate::re_introspect_after_drift` so observability
    /// can correlate `revalidate.invoked outcome=drift` with the downstream
    /// re-introspect that recomputes `cache_models.size_bytes` and
    /// `metadata_kv_json` for the drifted file. `source` is always
    /// `"pre_mutate_drift"` in v1 — the schema reserves the field for future
    /// inspect-trigger sources (interactive Refresh, scheduled rescan, etc.).
    /// Per the privacy rule: `tool` is the registered plugin id, `model` is
    /// the plugin-supplied id_in_tool — no paths, no hashes.
    InspectInvoked {
        tool: String,
        model: String,
        source: &'static str,
        duration_ms: u64,
    },
    /// Auto-refresh trigger issued by the pre-mutate revalidator on a
    /// `Gone` outcome (Step 05-04 — US-26 AC-26-7). Emitted by
    /// `orchestration::revalidate::auto_refresh_after_gone` immediately
    /// before the per-tool reconcile is enqueued. `source` is always
    /// `"pre_mutate_gone"` in v1 — the schema reserves the field for future
    /// auto-refresh triggers. Per the privacy rule: only the registered
    /// plugin id leaks; no model name, no path of the vanished file.
    RefreshTool {
        tool: String,
        source: &'static str,
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
                tools_registered,
            } => {
                let mut env = self.base_envelope("launch.inventory");
                env["total_models"] = json!(total_models);
                env["total_disk_usage_bytes"] = json!(total_disk_usage_bytes);
                env["dedupable_count"] = json!(dedupable_count);
                env["format_locked_count"] = json!(format_locked_count);
                env["tool_errors"] = json!(tool_errors);
                env["tools_registered"] = json!(tools_registered);
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
            RecordKind::ActionZapOne {
                tool,
                bytes_reclaimed,
                was_shared,
                outcome,
            } => {
                let mut env = self.base_envelope("action.zap_one");
                env["tool"] = json!(tool);
                env["bytes_reclaimed"] = json!(bytes_reclaimed);
                env["was_shared"] = json!(was_shared);
                env["outcome"] = json!(outcome);
                env
            }
            RecordKind::ActionUnify {
                model_dedup_key_kind,
                tools_unified,
                bytes_reclaimed,
                outcome,
                cross_fs_targets_skipped,
                cross_fs_targets_copied,
            } => {
                let mut env = self.base_envelope("action.unify");
                env["model_dedup_key_kind"] = json!(model_dedup_key_kind);
                env["tools_unified"] = json!(tools_unified);
                env["bytes_reclaimed"] = json!(bytes_reclaimed);
                env["outcome"] = json!(outcome);
                env["cross_fs_targets_skipped"] = json!(cross_fs_targets_skipped);
                env["cross_fs_targets_copied"] = json!(cross_fs_targets_copied);
                env
            }
            RecordKind::ActionUnifyDryRun {
                model_dedup_key_kind,
                tools_to_unify,
                bytes_would_reclaim,
                cross_fs_targets,
                outcome,
            } => {
                let mut env = self.base_envelope("action.unify_dry_run");
                env["model_dedup_key_kind"] = json!(model_dedup_key_kind);
                env["tools_to_unify"] = json!(tools_to_unify);
                env["bytes_would_reclaim"] = json!(bytes_would_reclaim);
                env["cross_fs_targets"] = json!(cross_fs_targets);
                env["outcome"] = json!(outcome);
                env
            }
            RecordKind::ActionFolderDelete {
                tool,
                folder_path,
                files_total,
                files_removed,
                bytes_reclaimed,
                bytes_retained,
                outcomes_count,
                keystroke_count,
                outcome,
            } => {
                let mut env = self.base_envelope("action.folder_delete");
                env["tool"] = json!(tool);
                env["folder_path"] = json!(folder_path);
                env["files_total"] = json!(files_total);
                env["files_removed"] = json!(files_removed);
                env["bytes_reclaimed"] = json!(bytes_reclaimed);
                env["bytes_retained"] = json!(bytes_retained);
                env["outcomes_count"] = json!(outcomes_count);
                env["keystroke_count"] = json!(keystroke_count);
                env["outcome"] = json!(outcome);
                env
            }
            RecordKind::RevalidateInvoked {
                tool,
                model,
                outcome,
                duration_ms,
            } => {
                let mut env = self.base_envelope("revalidate.invoked");
                env["tool"] = json!(tool);
                env["model"] = json!(model);
                env["outcome"] = json!(outcome);
                env["duration_ms"] = json!(duration_ms);
                env
            }
            RecordKind::InspectInvoked {
                tool,
                model,
                source,
                duration_ms,
            } => {
                let mut env = self.base_envelope("inspect.invoked");
                env["tool"] = json!(tool);
                env["model"] = json!(model);
                env["source"] = json!(source);
                env["duration_ms"] = json!(duration_ms);
                env
            }
            RecordKind::RefreshTool { tool, source } => {
                let mut env = self.base_envelope("refresh.tool");
                env["tool"] = json!(tool);
                env["source"] = json!(source);
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
