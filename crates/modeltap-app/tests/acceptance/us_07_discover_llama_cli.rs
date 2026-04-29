//! Acceptance tests for US-07 (Discover llama-cli loose .gguf models).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! lines 349–379. The llama-cli plugin walks `~/llms/` and `~/models/` by
//! default, plus any extra `[plugins.llama-cli] search_paths` from
//! `~/.modeltap/config.toml`. Each `.gguf` file is parsed JUST FAR ENOUGH to
//! recover `general.architecture` and the quantization label
//! (`general.file_type`); corrupt files are surfaced with `[format: corrupt]`
//! rather than silently dropped.
//!
//! Behaviors covered (US-07 acceptance criteria):
//! - AC-1 — default search paths `~/llms/` and `~/models/` are scanned
//! - AC-2 — `[plugins.llama-cli] search_paths` extras from config are honored
//! - AC-3 — header parsing extracts the quantization label
//! - AC-4 — corrupt/truncated files are listed with `[format: corrupt]`,
//!   discovery does NOT crash and the other models still appear
//!
//! Each test enters through the modeltap binary driving port. Assertions are
//! made against the launch.log JSONL events (the driven port boundary).

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// Build a named fixture tree under a fresh temp dir. Mirrors the helper in
/// `us_02_discover_ollama.rs` so the two suites stay in sync.
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
    (temp, target)
}

/// Construct a `modeltap` headless command with all four plugin env-vars
/// pinned at non-existent paths, then let the caller override the ones the
/// test cares about. This isolates the test from any real Ollama / HF / etc.
/// directories under the developer's `$HOME`.
fn modeltap_headless() -> (Command, TempDir) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "100")
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        // Defensive defaults for plugins that already read other env vars or
        // will read them later. The llama-cli plugin under test reads
        // MODELTAP_LLAMACLI_DIRS (colon-separated). Tests opt in by setting it.
        .env("MODELTAP_LLAMACLI_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    (cmd, log_dir_temp)
}

fn read_launch_log(log_dir: &Path) -> Vec<Value> {
    let path = log_dir.join("launch.log");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read launch.log at {}: {}", path.display(), e));
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect()
}

fn find_event<'a>(events: &'a [Value], name: &str) -> Option<&'a Value> {
    events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some(name))
}

/// Read the on-disk `models.log` (one JSONL line per discovered model). The
/// composition root writes this so acceptance tests can assert per-model
/// metadata (id, format, display label) without the TUI being involved.
fn read_models_log(log_dir: &Path) -> Vec<Value> {
    let path = log_dir.join("models.log");
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("each models.log line is JSON"))
        .collect()
}

// ---------------------------------------------------------------------------
// AC-1 — Default search paths (~/llms, ~/models) are scanned.
// ---------------------------------------------------------------------------

#[test]
fn default_search_paths_are_scanned() {
    let (_temp, fixture_root) = build_fixture("devon-llama-cli");
    let llms = fixture_root.join("llms");
    let models = fixture_root.join("models");
    // Devon's default search paths, redirected at the fixture's two roots.
    let dirs = format!("{}:{}", llms.display(), models.display());

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_LLAMACLI_DIRS", &dirs);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);

    let inv = find_event(&events, "launch.inventory")
        .unwrap_or_else(|| panic!("no launch.inventory in:\n{:?}", events));
    let total = inv
        .get("total_models")
        .and_then(|v| v.as_u64())
        .expect("total_models is a number");
    // Fixture has 3 valid GGUF files (2 in llms/, 1 in models/) plus 1 corrupt
    // file. AC-4 requires the corrupt entry to ALSO be reported (with
    // [format: corrupt]) — so total is 4.
    assert_eq!(
        total, 4,
        "fixture has 4 .gguf files (3 valid + 1 corrupt); got {}\nevents: {:#?}",
        total, events
    );

    // Per-tool models.log must include all 4 entries from llama-cli.
    let models_log = read_models_log(&log_dir);
    let llama_cli_models: Vec<&Value> = models_log
        .iter()
        .filter(|m| m.get("tool").and_then(|v| v.as_str()) == Some("llama-cli"))
        .collect();
    assert_eq!(
        llama_cli_models.len(),
        4,
        "llama-cli must surface 4 models in models.log; got {}",
        llama_cli_models.len()
    );

    // The two valid llms/ entries are present.
    let ids: Vec<String> = llama_cli_models
        .iter()
        .filter_map(|m| {
            m.get("id_in_tool")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();
    assert!(
        ids.iter().any(|i| i.contains("mistral-7b-q4")),
        "expected a mistral-7b-q4 model id, got {:?}",
        ids
    );
    assert!(
        ids.iter().any(|i| i.contains("qwen-1_5b")),
        "expected a qwen-1_5b model id (proves ~/models/ default was scanned), got {:?}",
        ids
    );
}

// ---------------------------------------------------------------------------
// AC-2 — Configured additional search path is honored.
// ---------------------------------------------------------------------------

#[test]
fn configured_additional_search_path_is_honored() {
    let (_temp_extra, extra_root) = build_fixture("devon-llama-cli-extra");
    let extra_dir = extra_root.join("data").join("models");
    assert!(
        extra_dir.join("extra.gguf").exists(),
        "fixture must contain extra.gguf"
    );

    // Write a config TOML in a separate tempdir and point MODELTAP_CONFIG_PATH
    // at it. The default search paths are NOT redirected (they remain pointing
    // at /nonexistent/no-such-llama-cli) — this test proves the TOML's
    // search_paths is the ONLY source of models for llama-cli.
    let cfg_temp = tempfile::tempdir().expect("cfg tempdir");
    let cfg_path = cfg_temp.path().join("config.toml");
    let toml = format!(
        "[plugins.llama-cli]\nsearch_paths = [\"{}\"]\n",
        extra_dir.display()
    );
    std::fs::write(&cfg_path, toml).expect("write config.toml");

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_CONFIG_PATH", &cfg_path)
        // Defaults stay at the non-existent path so any "extra.gguf" found
        // came from the configured TOML, not a default.
        .env("MODELTAP_LLAMACLI_DIRS", "/nonexistent/no-such-llama-cli");

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let llama_cli_models: Vec<&Value> = models_log
        .iter()
        .filter(|m| m.get("tool").and_then(|v| v.as_str()) == Some("llama-cli"))
        .collect();
    assert!(
        !llama_cli_models.is_empty(),
        "AC-2: at least one model must be found from the configured search_path; got 0"
    );
    let ids: Vec<String> = llama_cli_models
        .iter()
        .filter_map(|m| {
            m.get("id_in_tool")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();
    assert!(
        ids.iter().any(|i| i.contains("extra")),
        "AC-2: 'extra.gguf' must appear in llama-cli models, got {:?}",
        ids
    );
}

// ---------------------------------------------------------------------------
// AC-3 — GGUF header parsing extracts the quantization label.
// ---------------------------------------------------------------------------

#[test]
fn gguf_header_parsing_extracts_quantization_label() {
    let (_temp, fixture_root) = build_fixture("devon-llama-cli");
    let llms = fixture_root.join("llms");
    let dirs = llms.display().to_string();

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_LLAMACLI_DIRS", &dirs);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);

    // Find the llama-3-8b-q4_K_M.gguf entry. It was synthesized with
    // file_type = 15 ("Q4_K_M" in the GGUF spec). The plugin's header parser
    // must surface that label in the display label.
    let llama3 = models_log
        .iter()
        .filter(|m| m.get("tool").and_then(|v| v.as_str()) == Some("llama-cli"))
        .find(|m| {
            m.get("id_in_tool")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s.contains("llama-3-8b-q4_K_M"))
        })
        .unwrap_or_else(|| {
            panic!(
                "llama-3-8b-q4_K_M.gguf must be in models.log: {:#?}",
                models_log
            )
        });

    let display_label = llama3
        .get("display_label")
        .and_then(|v| v.as_str())
        .expect("display_label string");
    assert!(
        display_label.to_ascii_lowercase().contains("q4_k_m"),
        "AC-3: display_label must include the quantization label 'Q4_K_M' (case-insensitive); got {:?}",
        display_label
    );
}

// ---------------------------------------------------------------------------
// AC-4 — Corrupt GGUF flagged but does not crash discovery.
// ---------------------------------------------------------------------------

#[test]
fn corrupt_gguf_flagged_but_does_not_crash_discovery() {
    let (_temp, fixture_root) = build_fixture("devon-llama-cli");
    let llms = fixture_root.join("llms");
    let models = fixture_root.join("models");
    let dirs = format!("{}:{}", llms.display(), models.display());

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_LLAMACLI_DIRS", &dirs);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);

    // The corrupt entry must be present with format = "Corrupt" — NOT
    // silently dropped.
    let corrupt = models_log
        .iter()
        .find(|m| {
            m.get("tool").and_then(|v| v.as_str()) == Some("llama-cli")
                && m.get("id_in_tool")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.contains("corrupt"))
        })
        .unwrap_or_else(|| panic!("corrupt.gguf must appear in models.log:\n{:#?}", models_log));

    let format = corrupt
        .get("format")
        .and_then(|v| v.as_str())
        .expect("format string");
    // Per ADR / data-models: Corrupt is a ModelStatus variant; we serialize
    // the plugin's view of "format" as a string (e.g., "Gguf", "Corrupt").
    // Either the format itself is `Corrupt` or the status reports Corrupt.
    let status = corrupt.get("status").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        format.eq_ignore_ascii_case("corrupt") || status.to_ascii_lowercase().contains("corrupt"),
        "AC-4: corrupt.gguf must be flagged with [format: corrupt] OR ModelStatus::Corrupt; got format={:?} status={:?}",
        format,
        status
    );

    // The other 3 valid models must still be present (discovery did not crash).
    let llama_cli_count = models_log
        .iter()
        .filter(|m| m.get("tool").and_then(|v| v.as_str()) == Some("llama-cli"))
        .count();
    assert_eq!(
        llama_cli_count, 4,
        "AC-4: corrupt entry must NOT abort discovery; expected 4 entries (3 valid + 1 corrupt), got {}",
        llama_cli_count
    );
}
