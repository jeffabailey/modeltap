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

/// What an event records. Step 01-01 needs only Started/Ended; later steps
/// add launch.timing, launch.inventory, action.* per kpi-instrumentation.md.
pub enum RecordKind {
    LaunchStarted,
    LaunchEnded,
}

pub struct LaunchLogger {
    session_id: String,
    log_path: Option<PathBuf>,
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
        let log_path = log_dir.map(|d| d.join("launch.log"));
        let mut me = Self {
            session_id,
            log_path,
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
    }

    /// Append one JSONL event. Best-effort; failures degrade silently after
    /// the first stderr warning.
    pub fn record(&mut self, kind: RecordKind) {
        let Some(path) = self.log_path.clone() else {
            return;
        };
        let event = match kind {
            RecordKind::LaunchStarted => "launch.started",
            RecordKind::LaunchEnded => "launch.ended",
        };
        let payload = json!({
            "schema": SCHEMA,
            "ts": current_timestamp(),
            "session_id": self.session_id,
            "event": event,
            "modeltap_version": env!("CARGO_PKG_VERSION"),
            "platform": platform_triplet(),
        });
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
