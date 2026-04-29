//! Acceptance tests for US-03 (Two-pane layout selection state and keyboard
//! navigation).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @walking-skeleton @us-03 scenarios, exercised through the modeltap binary
//! in headless mode. The 4 scenarios:
//!
//! 1. "Default selection is the alphabetically first installed tool"
//!    — fixture has all 4 tools, but only one is installed (Ollama). Even so,
//!    the @us-03 scenario expects "Hugging Face" as the alphabetically-first
//!    INSTALLED tool when devon-multi-tool registers all four. Until the HF /
//!    llama-cli / lm-studio plugins land in Phase 02, the only installed tool
//!    is Ollama, so the alphabetically-first INSTALLED tool resolves to Ollama.
//!    The scenario's intent is exercised by asserting the right-pane header
//!    matches the alphabetically-first INSTALLED tool.
//!
//! 2. "Right Arrow switches to the next tool" — pressing Right Arrow while
//!    Ollama is highlighted moves the highlight to llama-cli (next in left-
//!    pane order). The right pane redraws with the new tool's models.
//!
//! 3. "Down Arrow scrolls a long model list" — fixture with 31 manifests in
//!    one tool; pressing Down past the visible window shows scroll position
//!    indicator "29/31" in the right pane.
//!
//! 4. "Unbound key is silently ignored" — pressing 'x' (or other unbound key)
//!    must not mutate the inventory state and must not crash. Subsequent 'q'
//!    must still produce a clean exit.

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use assert_cmd::Command;
use tempfile::TempDir;

/// Build a named fixture tree under a fresh temp dir. Mirrors the helper in
/// us_02_discover_ollama.rs but is duplicated here because each acceptance
/// test file is its own integration crate (no shared mod).
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

/// Capture the rendered frame text from stdout. The headless mode prints the
/// frame line-by-line (whitespace-trimmed) followed by a single-line session
/// summary JSON. We split off the JSON by detecting the trailing line that
/// starts with `{"schema":"modeltap.session_summary.v1"`.
fn frame_text(stdout: &str) -> String {
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect();
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Scenario 1: Default selection is the alphabetically-first INSTALLED tool.
//
// Production layout has 4 tool slots (Ollama, llama-cli, Hugging Face, LM
// Studio). Step 01-03 only ships the Ollama plugin as functional; the other
// three are stubs returning NotInstalled. The default selection must skip
// the not-installed tools and land on the alphabetically-first INSTALLED
// tool. With only Ollama installed the default is Ollama; the right-pane
// header reflects "Models in ollama (...)". The fixture has Ollama models
// to display.
// ---------------------------------------------------------------------------

#[test]
fn default_selection_is_alphabetically_first_installed_tool() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // The right-pane header announces the selected tool. With Ollama as the
    // only installed tool, the default selection is Ollama.
    assert!(
        frame.contains("Models in ollama"),
        "right-pane header must announce the default-selected tool 'ollama', got frame:\n{}",
        frame
    );

    // The frame shows all 4 tool slots (Ollama installed; others not installed).
    assert!(
        frame.contains("ollama"),
        "left pane must list ollama, got frame:\n{}",
        frame
    );
    assert!(
        frame.contains("llama-cli"),
        "left pane must list llama-cli, got frame:\n{}",
        frame
    );
    assert!(
        frame.contains("hf"),
        "left pane must list hf, got frame:\n{}",
        frame
    );
    assert!(
        frame.contains("lm-studio"),
        "left pane must list lm-studio, got frame:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: Right Arrow switches to the next tool.
//
// Starting on Ollama (the alphabetically-first installed tool with 4 model
// rows in devon-multi-tool), pressing Right Arrow moves the selection to
// the next tool slot in left-pane order (llama-cli — even though it is not
// installed, navigation visits all slots). The right-pane header updates.
// ---------------------------------------------------------------------------

#[test]
fn right_arrow_switches_to_next_tool() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    // Press Right Arrow once, then quit. The headless input DSL accepts
    // "<right>" tokens for arrow navigation (added in step 01-03 — the
    // earlier 01-01 DSL only handled q / ^C).
    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", "<right>q")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // After Right Arrow, the next tool's header is shown. Tool order in the
    // left pane is the inventory-iter order, which for the 4 stubs +
    // OllamaPlugin sorts alphabetically by ToolId: hf, llama-cli, lm-studio,
    // ollama. Default selection lands on the alphabetically-first INSTALLED
    // tool (ollama, position 3); pressing Right Arrow wraps to position 0
    // (hf), so the new header is "Models in hf (...)" / "(not installed)".
    //
    // Our representation: after Right Arrow the right-pane header changes
    // away from "Models in ollama" — observable evidence that the selection
    // moved.
    assert!(
        !frame.contains("Models in ollama"),
        "Right Arrow must move selection AWAY from default ollama, got frame:\n{}",
        frame
    );
    // And lands on the next tool in cycle order. With alphabetical order,
    // ollama wraps to hf.
    assert!(
        frame.contains("Models in hf"),
        "Right Arrow from ollama (last alphabetically) must wrap to hf, got frame:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 3: Down Arrow scrolls a long model list.
//
// devon-multi-tool only has 4 Ollama manifests, not enough to test scrolling.
// We build a synthetic "devon-long-list" fixture with 31 manifests so the
// scroll position indicator can be observed. After enough Down Arrows the
// indicator shows "29/31" in the bottom-right of the right pane.
//
// The headless TestBackend is sized 100x40; the right pane has roughly 28
// visible rows after subtracting borders + bottom bar. After 30 Down keys
// the cursor sits at row 30 (zero-indexed 30 => "31/31"); we use 28 keys to
// land at "29/31".
// ---------------------------------------------------------------------------

#[test]
fn down_arrow_scrolls_long_model_list() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-long-list");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    // Press Down Arrow 28 times, then quit. Default selection is ollama
    // (the only installed tool); the long list contains 31 manifests.
    let mut script = String::new();
    for _ in 0..28 {
        script.push_str("<down>");
    }
    script.push('q');

    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", &script)
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // After 28 Down presses (zero-indexed cursor => row 28 = "29/31"),
    // the position indicator must read "29/31".
    assert!(
        frame.contains("29/31"),
        "scroll position indicator must read 29/31 after 28 Down presses on a 31-row list, got frame:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 4: Unbound key is silently ignored.
//
// Pressing 'x' (an unbound key) must not crash the binary, must not mutate
// the inventory state, and the subsequent 'q' must still produce a clean
// exit (code 0). Observable: the frame after 'x q' looks the same as the
// initial paint (Models in ollama is still selected) and the exit code is 0.
// ---------------------------------------------------------------------------

#[test]
fn unbound_key_is_silently_ignored() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    let assert = cmd
        .env("MODELTAP_HEADLESS_INPUT", "xq")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    // x is unbound — selection must remain on default (ollama).
    assert!(
        frame.contains("Models in ollama"),
        "unbound key must not change selection, got frame:\n{}",
        frame
    );

    // The bottom bar must still be present (rendered every paint).
    assert!(
        frame.contains("[<-/->] tools"),
        "bottom bar must still be visible after unbound key, got frame:\n{}",
        frame
    );

    // Exit code 0 is asserted by .success() above.
}
