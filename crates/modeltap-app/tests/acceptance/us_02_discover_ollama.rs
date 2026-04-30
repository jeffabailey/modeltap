//! Acceptance tests for US-02 (Discover Ollama models).
//!
//! Per `docs/feature/modeltap-tui/distill/acceptance-test-plan.md` §3, the
//! Ollama plugin is driven against a fixture tree under
//! `tests/fixtures/.build/<name>/` (built by `tests/fixtures/build.sh`).
//! The binary in headless mode points the Ollama plugin at this tree via the
//! `MODELTAP_OLLAMA_DIR` env var (the test seam declared in §3).
//!
//! Behaviors covered (from US-02 acceptance criteria):
//! - AC-1 — discovery returns models with id, size, on-disk path
//! - AC-2 — total disk usage deduplicates blobs (manifests sharing a blob
//!   count once)
//! - AC-3 — missing Ollama directory → ToolStatus::NotInstalled (no crash)
//! - AC-4 — unreadable Ollama directory → ToolStatus::Error (no crash)
//! - AC-6 — JSONL `launch.timing` event includes `plugin_timings_ms.ollama`
//!
//! Each test enters through the modeltap binary driving port. Assertions are
//! made against the launch.log JSONL events (the driven port boundary) and
//! the captured stdout summary.

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// Build a named fixture tree under a fresh temp dir. Returns the temp dir
/// (kept alive by the caller) and the path to use for `MODELTAP_OLLAMA_DIR`.
///
/// The Ollama plugin's discovery root is the `.ollama/models/` directory; the
/// builder lays out `<temp>/.ollama/models/...`.
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

    // Per the env-var contract, MODELTAP_OLLAMA_DIR points at the
    // `.ollama/models/` directory (the discovery root).
    let ollama_dir = target.join(".ollama").join("models");
    (temp, ollama_dir)
}

fn modeltap_headless_with_ollama(ollama_dir: Option<&Path>) -> (Command, TempDir) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "100")
        // Pin the other plugins at non-existent paths so the test isolates
        // from any real Ollama / llama-cli / HF / lm-studio installs on the
        // developer's machine.
        .env("MODELTAP_LLAMACLI_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache");
    if let Some(dir) = ollama_dir {
        cmd.env("MODELTAP_OLLAMA_DIR", dir);
    } else {
        // Explicit "no Ollama installed": override to a path that does not
        // exist so the production code's `~/.ollama/` lookup does not
        // accidentally find a real installation on the developer's machine.
        cmd.env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama");
    }
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

// ---------------------------------------------------------------------------
// AC-1 + AC-2 — Devon's Ollama models are discovered with correct sizes,
// and the per-blob deduplication makes the total equal the sum of unique
// blob sizes (NOT the sum of manifest sizes).
// ---------------------------------------------------------------------------

#[test]
fn devon_ollama_models_are_discovered_with_dedup_size_total() {
    let (_temp_fix, ollama_dir) = build_fixture("devon-multi-tool");
    let (mut cmd, log_temp) = modeltap_headless_with_ollama(Some(&ollama_dir));

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);

    // launch.inventory must report 4 manifest-models from the fixture.
    let inv = find_event(&events, "launch.inventory")
        .unwrap_or_else(|| panic!("no launch.inventory event in:\n{:?}", events));
    let total = inv
        .get("total_models")
        .and_then(|v| v.as_u64())
        .expect("total_models is a number");
    assert_eq!(
        total, 4,
        "fixture has 4 manifest entries (llama3, mistral, codellama:13b-q4_K_M, codellama:13b-instruct-q4_K_M); got {}",
        total
    );

    // Per AC-2: the codellama manifests share one blob (3.7 GB). Total must
    // count that blob ONCE, not twice.
    //
    //   blob_llama:    4_700_000_000
    //   blob_mistral:  4_400_000_000
    //   blob_codellama 3_700_000_000  (shared by 2 manifests; counted once)
    //   ------------------------------
    //   total:        12_800_000_000
    let total_bytes = inv
        .get("total_disk_usage_bytes")
        .and_then(|v| v.as_u64())
        .expect("total_disk_usage_bytes is a number");
    let expected_unique = 4_700_000_000_u64 + 4_400_000_000 + 3_700_000_000;
    assert_eq!(
        total_bytes, expected_unique,
        "AC-2 violated: total_disk_usage_bytes must dedup shared blobs ({} bytes), got {}",
        expected_unique, total_bytes
    );
}

// ---------------------------------------------------------------------------
// AC-3 — Missing Ollama directory is handled as not-installed, not as error.
// ---------------------------------------------------------------------------

#[test]
fn missing_ollama_directory_is_handled_as_not_installed() {
    let (mut cmd, log_temp) = modeltap_headless_with_ollama(None); // pointed at /nonexistent
    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);

    let inv = find_event(&events, "launch.inventory")
        .expect("launch.inventory must be emitted even when no models are found");
    let total = inv
        .get("total_models")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(total, 0, "missing Ollama dir must report 0 models");

    // The launch.timing event must still appear (the plugin ran and reported
    // NotInstalled fast — its timing entry is present but small).
    let timing = find_event(&events, "launch.timing")
        .expect("launch.timing must be emitted even on not-installed");
    let plugin_timings = timing
        .get("plugin_timings_ms")
        .and_then(|v| v.as_object())
        .expect("plugin_timings_ms object");
    assert!(
        plugin_timings.contains_key("ollama"),
        "plugin_timings_ms must include 'ollama' key per AC-6, got {:?}",
        plugin_timings
    );
}

// ---------------------------------------------------------------------------
// AC-4 — Unreadable Ollama directory does not crash modeltap.
// ---------------------------------------------------------------------------

#[test]
fn unreadable_ollama_directory_does_not_crash() {
    if !cfg!(unix) {
        eprintln!("skipping: unreadable-dir scenario is Unix-only");
        return;
    }
    // Skip when running as root: chmod 0000 has no effect on root and the
    // test would falsely pass with "models discovered".
    #[cfg(unix)]
    {
        let uid = StdCommand::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(1);
        if uid == 0 {
            eprintln!("skipping: cannot test permission-denied as root");
            return;
        }
    }

    let (temp, ollama_dir) = build_fixture("devon-permission-denied");
    let (mut cmd, log_temp) = modeltap_headless_with_ollama(Some(&ollama_dir));

    let result = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    // Restore mode so the tempdir can be cleaned up. The fixture builder
    // chmod 0000's the manifests dir; we need to reverse before drop.
    let manifests = ollama_dir.join("manifests");
    set_mode(&manifests, 0o700);
    drop(temp);

    // launch.inventory must be emitted (the binary did not crash) and
    // total_models must be 0 (the plugin reported an error, not a count).
    let log_dir = log_temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let inv = find_event(&events, "launch.inventory")
        .expect("launch.inventory must be emitted even on plugin error");
    let total = inv
        .get("total_models")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(total, 0, "errored plugin must not contribute models");

    // Diagnostic must be visible to the user. Per AC-4, the user-facing
    // signal is the left-pane "(error)" annotation; the JSONL evidence is
    // a "tool_status" hint in launch.inventory naming the failed plugin.
    // Our representation: launch.inventory.tool_errors must list "ollama".
    let tool_errors = inv
        .get("tool_errors")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        tool_errors.iter().any(|e| e.as_str() == Some("ollama")),
        "AC-4: launch.inventory.tool_errors must include 'ollama' on permission-denied, got {:?}",
        tool_errors
    );

    // Still exited 0 and produced the bottom bar (the binary did not crash).
    let stdout = String::from_utf8_lossy(&result.get_output().stdout).to_string();
    assert!(
        stdout.contains("[<-/->] tools"),
        "stdout must still contain bottom bar (no crash):\n{}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// AC-6 — JSONL launch.timing includes plugin_timings_ms.ollama
// (Already partly checked in AC-3 test; this one asserts the schema on
// the happy path with real models.)
// ---------------------------------------------------------------------------

#[test]
fn launch_timing_event_records_ollama_plugin_timing() {
    let (_temp, ollama_dir) = build_fixture("devon-only-ollama");
    let (mut cmd, log_temp) = modeltap_headless_with_ollama(Some(&ollama_dir));

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let timing = find_event(&events, "launch.timing").expect("launch.timing event must be emitted");
    assert_eq!(
        timing.get("schema").and_then(|v| v.as_str()),
        Some("modeltap.launch.v1"),
        "schema mismatch on launch.timing"
    );
    let plugin_timings = timing
        .get("plugin_timings_ms")
        .and_then(|v| v.as_object())
        .expect("plugin_timings_ms object");
    let ollama_ms = plugin_timings
        .get("ollama")
        .and_then(|v| v.as_u64())
        .expect("plugin_timings_ms.ollama present and numeric");
    // K3 budget for the Ollama plugin alone is 200 ms on a typical install;
    // we relax to 5000 in the test to absorb CI variability.
    assert!(
        ollama_ms < 5000,
        "ollama discovery took {} ms, expected < 5000 in test environment",
        ollama_ms
    );
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(mode);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}
