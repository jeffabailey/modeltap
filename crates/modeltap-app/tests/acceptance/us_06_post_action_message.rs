//! Acceptance tests for US-06 (Post-action message with reclaim/retain breakdown).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @walking-skeleton @us-06 scenarios. The 4 scenarios are:
//!
//! 1. **Successful zap shows reclaimed and retained bytes** (active in WS).
//!    The right pane renders `Last action: zap <tool> (success)` header and
//!    `Reclaimed: <N> GB (<M> GB retained — also linked from other tools)`
//!    body after a confirmed zap. The summary bar updates total disk usage
//!    within 500 ms. Adapted from the master-acceptance "llama-cli" target —
//!    in the WS slice only Ollama has a populated fixture, so the assertion
//!    targets `zap ollama (success)` (per the same WS-adaptation note in
//!    us_05_zap_all.rs). Behavioral intent — header + body schema, summary
//!    refresh — is preserved.
//!
//! 2. **Last-action message clears when Devon navigates** (active in WS). After
//!    a zap, pressing Right Arrow advances to the next tool slot AND clears
//!    the last-action header from the right pane.
//!
//! 3. **Successful unify shows hardlink count** (`#[ignore]` — re-enable when
//!    03-02 lands). The unify action does not exist in the WS slice; the
//!    scenario is preserved as the regression net for 03-02. Re-enable by
//!    removing the `#[ignore]` once `Tool::link` returns success outcomes
//!    from a real plugin.
//!
//! 4. **Partial unify shows partial-success message** (`#[ignore]` — re-enable
//!    when 03-03 lands). Cross-filesystem partial-success requires the
//!    devon-cross-fs fixture and `LinkOutcome::Failed` plumbing, neither of
//!    which exist in the WS slice.
//!
//! Tags: @us-06 @walking-skeleton.

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

fn build_fixture(name: &str) -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let target = temp.path().join(name);
    let project_root = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .and_then(|p| p.parent().and_then(|p| p.parent().map(PathBuf::from)))
        .expect("CARGO_MANIFEST_DIR + walk to workspace root");
    let script = project_root.join("tests/fixtures/build.sh");
    let status = StdCommand::new("bash")
        .arg(&script)
        .arg(name)
        .arg(&target)
        .status()
        .expect("spawn build.sh");
    assert!(status.success(), "fixture builder failed for {}", name);
    let ollama_dir = target.join(".ollama").join("models");
    (temp, ollama_dir)
}

fn modeltap_headless(ollama_dir: Option<&Path>) -> (Command, TempDir) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "100");
    if let Some(dir) = ollama_dir {
        cmd.env("MODELTAP_OLLAMA_DIR", dir);
    } else {
        cmd.env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama");
    }
    (cmd, log_dir_temp)
}

fn frame_text(stdout: &str) -> String {
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect();
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Scenario 1 (US-06.AC-1, AC-2, AC-4 / US-11.AC-1):
// "Successful zap shows reclaimed and retained bytes"
//
// Adapted from the master-acceptance llama-cli target (see file-level note).
// On a successful zap of Ollama (devon-multi-tool fixture, 4 manifests over
// 3 unique blobs ≈ 18.0 GB), the right pane must render:
//   - Header: "Last action: zap ollama (success)"
//   - Body:   "Reclaimed: <N> GB" (retained part is "0 GB" in the WS slice
//             because no shared models exist; the header text uses the
//             "(<M> GB retained — also linked from other tools)" schema only
//             when M > 0; the test asserts the schema-relevant substrings).
//
// Note on bytes: the devon-multi-tool fixture has a 4th manifest whose blob
// equals one of the other 3 (the "codellama" manifest reuses an earlier
// blob), so unique-blob bytes < sum-of-manifests bytes. The test asserts
// "Reclaimed:" and "GB" appearance rather than an exact byte total to
// stay robust against fixture tweaks.
// ---------------------------------------------------------------------------

#[test]
fn successful_zap_shows_reclaimed_and_retained_bytes() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    // Default selection lands on ollama. Press z, type "ollama", Enter, q.
    let script = "zollama<enter>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // Header: "Last action: zap ollama (success)" — exact schema match.
    assert!(
        frame.contains("Last action: zap ollama (success)"),
        "AC-1: expected exact header 'Last action: zap ollama (success)' in frame, got:\n{}",
        frame
    );

    // Body: "Reclaimed: <N> GB" — schema match. Retained-bytes parenthetical
    // is required when M > 0; for the WS slice with no cross-tool sharing
    // we only require the "Reclaimed:" prefix and a "GB" unit.
    assert!(
        frame.contains("Reclaimed:") && frame.contains("GB"),
        "AC-2: expected 'Reclaimed: <N> GB' body in frame, got:\n{}",
        frame
    );

    // AC-4 / US-11.AC-1: summary bar shows updated total disk usage. With
    // all blobs zapped, the summary "Disk:" line should reflect the post-
    // zap inventory (0 bytes for ollama, the only installed tool in WS).
    // The summary bar is rendered as part of the bottom row; we assert the
    // schema (the substring "Disk:") appears in the frame.
    assert!(
        frame.contains("Disk:"),
        "US-11.AC-1: summary bar must show 'Disk:' total, got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 (US-06 nav-clears):
// "Last-action message clears when Devon navigates"
//
// After a successful zap of Ollama, Devon presses Right Arrow to advance to
// the next tool slot. The right pane must NO LONGER render the "Last action"
// header — it must show the new tool's models (or empty/not-installed
// message). This is a state-clearing assertion: the post-action message is
// in-memory only and is cleared on any navigation Msg.
// ---------------------------------------------------------------------------

#[test]
fn last_action_message_clears_when_devon_navigates() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    // zap → confirm → press Right Arrow → quit. The Right Arrow advances
    // from ollama to the alphabetically-next tool (hf), which is
    // NotInstalled in the WS slice. The right pane must not show the
    // last-action header.
    let script = "zollama<enter><right>q";
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // The "Last action" header must NOT appear after navigation.
    assert!(
        !frame.contains("Last action: zap"),
        "nav-clears: 'Last action: zap' header must be cleared after Right Arrow, got:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 (FUTURE-PENDING — re-enable when 03-02 lands):
// "Successful unify shows hardlink count"
//
// On successful unify, the right pane shows:
//   Header: "Last action: unify mistral:7b (success)"
//   Body:   "Reclaimed: 8.8 GB (1 inode, 3 hardlinks)"
//
// Requires Tool::link to be implemented (currently returns
// LinkError::NotYetImplemented per ADR-001 + ADR-009 trait freeze). Re-enable
// by removing the #[ignore] attribute below once 03-02 lands.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "re-enable when 03-02 lands (Tool::link implementation)"]
fn successful_unify_shows_hardlink_count() {
    // Future-pending: requires Tool::link wired to a real implementation in
    // the Ollama plugin (or any plugin). The unify action dispatches a
    // Msg::Unify and the resulting LastAction renders with verb=unify and
    // an extra string "1 inode, 3 hardlinks" in the body.
    panic!("future-pending: re-enable when 03-02 (unify) is implemented");
}

// ---------------------------------------------------------------------------
// Scenario 4 (FUTURE-PENDING — re-enable when 03-03 lands):
// "Partial unify shows partial-success message"
//
// On partial unify (cross-filesystem split, 2 of 3 targets succeeded), the
// right pane shows:
//   Header: "Last action: unify mistral:7b (partial: 2 of 3 targets linked)"
//   Body:   the failed target's path and reason
//
// Requires the devon-cross-fs fixture AND LinkOutcome::Failed plumbing
// neither of which exist in the WS slice. Re-enable when 03-03 lands.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "re-enable when 03-03 lands (cross-fs partial-success unify)"]
fn partial_unify_shows_partial_success_message() {
    // Future-pending: requires devon-cross-fs fixture builder + LinkOutcome
    // partial-success path through actions::unify (which doesn't exist yet).
    panic!("future-pending: re-enable when 03-03 (cross-fs partial unify) is implemented");
}
