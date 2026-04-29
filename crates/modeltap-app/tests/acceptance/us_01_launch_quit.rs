//! Acceptance tests for US-01 (TUI launches and quits cleanly).
//!
//! Per `docs/feature/modeltap-tui/distill/acceptance-test-plan.md` §1, US-01
//! uses `assert_cmd` to drive the real `modeltap` binary in headless mode.
//! These tests are the @walking-skeleton @us-01 scenarios from
//! `features/master-acceptance.feature` translated to Rust.
//!
//! Headless contract (per acceptance-test-plan.md §4):
//! - `MODELTAP_HEADLESS=1` → no raw mode; TestBackend rendering; on quit emit
//!   a single JSON object to stdout describing the session.
//! - `MODELTAP_TERM_COLS=<N>` → override TestBackend width (also used by the
//!   terminal-too-narrow guard so we can simulate a narrow terminal in CI).
//! - `MODELTAP_LOG_DIR=<path>` → override `~/.modeltap/` for log isolation.
//!
//! These tests are deliberately framework-light: cucumber-rs lands in
//! step 01-02 once the test surface is broader. For the WS scaffold a small
//! set of `#[test]` functions is sufficient.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Helper: build a `Command` for the modeltap binary with a per-scenario
/// log directory and headless mode pre-configured. Returns both the command
/// and the temp dir guard (so the directory survives until the test ends).
fn modeltap_headless() -> (Command, TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let log_dir = temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir);
    (cmd, temp)
}

fn read_launch_log(log_dir: &std::path::Path) -> Vec<serde_json::Value> {
    let path = log_dir.join("launch.log");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read launch.log at {}: {}", path.display(), e));
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect()
}

/// Scenario: Devon launches modeltap and sees the two-pane layout.
///
/// In headless mode the binary paints once, prints a session-summary JSON to
/// stdout, then exits cleanly. We assert: exit 0, the captured frame contains
/// the bottom-bar text (US-01 AC-6), and `launch.started` was emitted.
#[test]
fn devon_launches_and_sees_two_pane_layout() {
    let (mut cmd, temp) = modeltap_headless();
    let started = Instant::now();
    let assert = cmd
        .arg("--quit-after-paint")
        .env("MODELTAP_TERM_COLS", "100")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();
    let elapsed = started.elapsed();

    // US-01 AC-1: cold start → first paint < 1 second on a workstation.
    // In CI we relax to 3 s to absorb runner variability; the K3 bench job
    // is the strict gate.
    assert!(
        elapsed < Duration::from_secs(3),
        "first paint took {:?}, expected < 3 s",
        elapsed
    );

    // The headless mode writes the rendered frame to stdout. Bottom bar must
    // appear (US-01 AC-6 + US-08 AC-1).
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stdout.contains("[<-/->] tools"),
        "stdout does not contain bottom-bar text:\n{}",
        stdout
    );
    assert!(stdout.contains("[q] quit"), "stdout missing [q] quit");

    // launch.started JSONL event was emitted (kpi-instrumentation §2.1).
    let log_dir = temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let first = events.first().expect("at least one log event");
    assert_eq!(
        first.get("event").and_then(|v| v.as_str()),
        Some("launch.started"),
        "first log event must be launch.started, got {:?}",
        first
    );
    assert_eq!(
        first.get("schema").and_then(|v| v.as_str()),
        Some("modeltap.launch.v1"),
        "schema mismatch on launch.started"
    );
    assert!(
        first.get("session_id").and_then(|v| v.as_str()).is_some(),
        "launch.started must carry a session_id"
    );
}

/// Scenario: Devon quits with q.
///
/// When MODELTAP_HEADLESS_INPUT contains "q", the binary processes one quit
/// keystroke and exits 0. The launch.ended event must follow.
#[test]
fn devon_quits_with_q() {
    let (mut cmd, temp) = modeltap_headless();
    cmd.env("MODELTAP_HEADLESS_INPUT", "q")
        .env("MODELTAP_TERM_COLS", "100")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();

    let log_dir = temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let last = events.last().expect("at least one event");
    assert_eq!(
        last.get("event").and_then(|v| v.as_str()),
        Some("launch.ended"),
        "last event on q-quit must be launch.ended, got {:?}",
        last
    );
}

/// Scenario: Devon quits with Ctrl+C.
///
/// Headless surrogate: `MODELTAP_HEADLESS_INPUT=^C` triggers the same code
/// path as a SIGINT signal handler would. Exit code 130 (per POSIX 128+SIGINT).
/// Per the master-acceptance "launch.ended NOT emitted on Ctrl+C" KPI
/// invariant, the launch.ended event must NOT appear.
#[test]
fn devon_quits_with_ctrl_c() {
    let (mut cmd, temp) = modeltap_headless();
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", "^C")
        .env("MODELTAP_TERM_COLS", "100")
        .timeout(Duration::from_secs(5))
        .assert()
        .failure();

    let code = assert.get_output().status.code();
    assert_eq!(
        code,
        Some(130),
        "Ctrl+C exit code must be 130 (POSIX 128+SIGINT), got {:?}",
        code
    );

    // launch.ended must NOT be present.
    let log_dir = temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    for ev in &events {
        assert_ne!(
            ev.get("event").and_then(|v| v.as_str()),
            Some("launch.ended"),
            "launch.ended must NOT be emitted on Ctrl+C, found in: {:?}",
            ev
        );
    }
}

/// Scenario: Terminal too narrow refuses to start.
///
/// 60-column terminal → exit code 2; usage error on stderr; no partial TUI
/// rendered (no frame markers in stdout).
#[test]
fn terminal_too_narrow_refuses_to_start() {
    let (mut cmd, _temp) = modeltap_headless();
    let assert = cmd
        .env("MODELTAP_TERM_COLS", "60")
        .timeout(Duration::from_secs(5))
        .assert()
        .failure();

    let code = assert.get_output().status.code();
    assert_eq!(code, Some(2), "narrow terminal must exit 2, got {:?}", code);

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let expected = "Terminal too narrow: need at least 80 columns, found 60";
    assert!(
        stderr.contains(expected),
        "stderr must contain {:?}, got:\n{}",
        expected,
        stderr
    );

    // No partial TUI rendered: stdout has no bottom-bar text.
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        !stdout.contains("[<-/->] tools"),
        "no TUI should be rendered when terminal is too narrow, got:\n{}",
        stdout
    );
}

/// Scenario: Modeltap log directory is unwritable.
///
/// Per intake Q7 + ADR-003: logs are operational only. An unwritable log dir
/// must not crash modeltap; render the TUI, warn to stderr, exit 0.
#[test]
fn unwritable_log_dir_does_not_crash() {
    let temp = tempfile::tempdir().expect("tempdir");
    let log_dir = temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    // Make the log dir read-only (mode 0500) so the appender cannot create
    // launch.log within it.
    set_mode(&log_dir, 0o500);

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    let assert = cmd
        .env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "100")
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(5))
        .assert()
        .success();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        predicates::str::contains("warning: cannot write launch log").eval(stderr.as_str()),
        "stderr must contain unwritable-log warning, got:\n{}",
        stderr
    );

    // Restore mode so the tempdir can be cleaned up.
    set_mode(&log_dir, 0o700);
    drop(temp);
}

#[cfg(unix)]
fn set_mode(path: &PathBuf, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path).expect("stat").permissions();
    perms.set_mode(mode);
    std::fs::set_permissions(path, perms).expect("chmod");
}

#[cfg(not(unix))]
fn set_mode(_path: &PathBuf, _mode: u32) {
    // No-op on non-Unix; the unwritable-dir scenario is Unix-only in v1.
}
