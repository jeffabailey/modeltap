//! Acceptance tests for US-04 (Row metadata format — indicator + size + 'also in:').
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-04 @release-1 scenarios. The 4 scenarios:
//!
//! 1. "Single-tool format-compatible model shows o" — driven through the
//!    binary with the devon-multi-tool fixture. Only Ollama is installed in
//!    the 02-01 slice (llama-cli/hf/lm-studio are still NotInstalled stubs),
//!    so every Ollama model is a single-tool registration and resolves to
//!    `Compatible` (`o` glyph). The right-pane rows must each begin with
//!    `o ` followed by the model id.
//!
//! 2. "Multi-tool model shows the * indicator and other tools" — tested
//!    indirectly through `render::row` unit tests using a synthetic Inventory
//!    that registers the same model under 2+ tools (the binary cannot yet
//!    produce that state). The acceptance assertion here is a smoke check
//!    that the `also in:` substring is reachable from the production render
//!    path: when no shared models exist (the WS slice), no `also in:` text
//!    appears in the frame. This guards against accidental leakage of the
//!    annotation when it shouldn't be visible.
//!
//! 3. "Unknown format shows ? indicator" — exercised via the unit + property
//!    tests in `render::indicator` / `render::row`. The binary cannot yet
//!    construct unparseable-format models from real fixtures.
//!
//! 4. "NO_COLOR=1 preserves indicator symbol and drops ANSI" — driven through
//!    the binary with `NO_COLOR=1`. The headless `TestBackend` already strips
//!    style attributes before printing, so the assertion here is the
//!    symbol-presence half (every row starts with `o`); the byte-level
//!    "no `\x1b[` in serialized output" half is asserted in
//!    `render::row` unit tests against the `Line` returned by `render_row`.
//!
//! Tags: @us-04 @release-1.

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
        .env("MODELTAP_TERM_COLS", "100")
        // Pin the other plugins at non-existent paths so this test isolates
        // from the developer's real Ollama / llama-cli / HF / lm-studio installs.
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env(
            "MODELTAP_GPT4ALL_DIRS",
            "/nonexistent/no-such-gpt4all",
        )
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache");
    if let Some(dir) = ollama_dir {
        cmd.env("MODELTAP_OLLAMA_DIR", dir);
    } else {
        cmd.env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama");
    }
    (cmd, log_dir_temp)
}

/// Capture the rendered frame text from stdout. Drops the trailing
/// `modeltap.session_summary.v1` JSON so the assertion target is the visible
/// frame.
fn frame_text(stdout: &str) -> String {
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| !l.starts_with(r#"{"schema":"modeltap.session_summary.v1""#))
        .collect();
    lines.join("\n")
}

/// Extract the right-pane model rows from a captured frame.
///
/// The headless renderer paints a two-pane layout with box-drawing borders.
/// Each model row of the right-pane content has shape `│<left-pane>││<right-
/// pane content>│`, i.e. four `│` separators in total. We pick the segment
/// AFTER the third `│` (the right-pane interior), strip leading/trailing
/// whitespace AND any residual box-drawing characters, then keep only rows
/// whose contents look like a model row (carry a size suffix and are not
/// chrome). Top/bottom borders that use `┌┐└┘` and contain no `│` are
/// rejected because the split yields a single segment.
fn strip_borders(s: &str) -> &str {
    s.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '│' | '┃' | '║' | '|' | '┌' | '┐' | '└' | '┘' | '─')
    })
}

fn model_rows(frame: &str) -> Vec<String> {
    frame
        .lines()
        .filter_map(|line| {
            // A model row contains at least 3 `│` separators (left-border,
            // pane-divider, right-border). Top/bottom box borders use `┌─┐`
            // / `└─┘` and have zero `│` chars; reject them.
            let segments: Vec<&str> = line.split('│').collect();
            if segments.len() < 4 {
                return None;
            }
            // The right-pane interior is the segment between the last two
            // `│` separators (the rightmost non-trailing-empty segment).
            let interior = strip_borders(segments[segments.len() - 2]);
            if interior.is_empty() {
                return None;
            }
            // Filter out chrome — header lines, summary lines, etc.
            if interior.starts_with("Models in")
                || interior.starts_with("Tools")
                || interior.starts_with("Disk:")
                || interior.starts_with("Total:")
                || interior.starts_with("Last action:")
                || interior.starts_with("Reclaimed:")
            {
                return None;
            }
            // A model row carries a size suffix; the scroll-position indicator
            // (e.g. "1/4") and empty rows are filtered by this gate.
            if interior.contains(" GB") || interior.contains(" MB") || interior.ends_with(" B") {
                Some(interior.to_string())
            } else {
                None
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario 1 (US-04.AC-1, AC-4): single-tool format-compatible model shows `o`.
//
// devon-multi-tool fixture has 4 Ollama manifests over 3 unique blobs. With
// only Ollama installed in the 02-01 slice, every model is a single-tool
// registration → `Compatible` indicator → `o ` prefix on every row.
// ---------------------------------------------------------------------------

#[test]
fn single_tool_format_compatible_model_shows_o_indicator() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    let rows = model_rows(&frame);

    assert!(
        !rows.is_empty(),
        "expected at least one model row in right pane, got frame:\n{}",
        frame
    );
    for row in &rows {
        let first_char = row.chars().next().expect("non-empty row");
        assert_eq!(
            first_char, 'o',
            "AC-1: every single-tool format-compatible row must start with 'o', got: {:?} from frame:\n{}",
            row, frame
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 2 (US-04.AC-2): No `also in:` annotation when no model is shared
// across tools (the WS slice). This guards against accidental leakage of the
// shared-model annotation when there is no actual sharing.
// ---------------------------------------------------------------------------

#[test]
fn no_also_in_annotation_when_no_cross_tool_models() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);

    assert!(
        !frame.contains("also in:"),
        "AC-2: 'also in:' annotation must not appear when no model is shared across tools, got frame:\n{}",
        frame
    );
}

// ---------------------------------------------------------------------------
// Scenario 3 (US-04.AC-1 indicator universe): every right-pane row begins with
// exactly one of `{o, *, !, ?}`. This is the structural property that the
// production frame must satisfy regardless of inventory state.
// ---------------------------------------------------------------------------

#[test]
fn every_right_pane_row_starts_with_an_indicator_glyph() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    let frame = frame_text(&stdout);
    let rows = model_rows(&frame);

    assert!(
        !rows.is_empty(),
        "expected at least one model row in right pane, got frame:\n{}",
        frame
    );
    for row in &rows {
        let first_char = row.chars().next().expect("non-empty row");
        assert!(
            matches!(first_char, 'o' | '*' | '!' | '?'),
            "AC-1: every row must start with one of {{o, *, !, ?}}, got {:?} from row {:?}",
            first_char,
            row
        );
    }
}

// ---------------------------------------------------------------------------
// Scenario 4 (US-04.AC-3 NO_COLOR): with NO_COLOR=1, indicator symbols are
// preserved. The headless TestBackend already strips style attributes when
// serializing the frame, so this scenario asserts the symbol-presence half:
// every row still starts with `o` when NO_COLOR=1 is set.
//
// The complementary assertion — that the rendered Line carries no ANSI escape
// bytes when NO_COLOR is active — lives in the render::row unit tests against
// the Line<'_> directly (unit test: no_color_active_strips_ansi_styling).
// ---------------------------------------------------------------------------

#[test]
fn no_color_env_var_preserves_indicator_symbols() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, _log_temp) = modeltap_headless(Some(&ollama_dir));

    cmd.env("NO_COLOR", "1");

    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();

    // No ANSI escape bytes in the headless stdout (TestBackend never emits
    // ANSI; this is the byte-level guard).
    assert!(
        !stdout.contains('\x1b'),
        "AC-3 NO_COLOR: stdout must contain no ANSI escape bytes, got bytes containing 0x1b"
    );

    let frame = frame_text(&stdout);
    let rows = model_rows(&frame);
    assert!(
        !rows.is_empty(),
        "expected at least one model row under NO_COLOR=1, got frame:\n{}",
        frame
    );
    for row in &rows {
        let first_char = row.chars().next().expect("non-empty row");
        assert_eq!(
            first_char, 'o',
            "AC-3: NO_COLOR=1 must preserve the indicator symbol; expected 'o', got {:?} from row {:?}",
            first_char, row
        );
    }
}
