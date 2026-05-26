//! Acceptance tests for US-15 (Discover LM Studio models).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! lines 612–634. The LM Studio plugin walks BOTH default path conventions:
//!
//!   `~/.cache/lm-studio/models/`  — newer convention (LM Studio 0.3.x+).
//!   `~/.lmstudio/models/`         — older convention (some installs still here).
//!
//! Plus an optional `[plugins.lm-studio] search_paths` override via
//! `~/.modeltap/config.toml` (mirror llama-cli's resolution from US-07).
//!
//! `accepted_formats()` returns `[Format::Gguf]` for v1 — MLX is out of
//! scope per intake C3 / ADR-004.
//!
//! Behaviors covered (US-15 acceptance criteria):
//! - AC-1 — `~/.cache/lm-studio/models/` is scanned (new convention).
//! - AC-2 — `~/.lmstudio/models/` is scanned when the new path is absent.
//! - AC-3 — format inferred from filename suffix (`.gguf`).
//! - AC-4 — "not installed" (neither path exists) distinguished from
//!   "error" (path exists but unreadable).
//!
//! Each test enters through the modeltap binary driving port. Assertions are
//! made against the launch.log + models.log JSONL events (driven port
//! boundary).

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// Build a named fixture tree under a fresh temp dir. Mirrors the helper
/// in `us_07_discover_llama_cli.rs` and `us_12_discover_hf.rs`.
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
/// pinned at non-existent paths so this test isolates from the developer's
/// real `$HOME`. The LM Studio plugin reads `MODELTAP_LMSTUDIO_DIRS` for
/// its search-path override; tests opt in by setting it.
fn modeltap_headless() -> (Command, TempDir) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("create log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_CACHE_PATH", log_dir.join("cache.sqlite"))
        .env("MODELTAP_TERM_COLS", "100")
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache")
        // The LM Studio plugin reads MODELTAP_LMSTUDIO_DIRS (colon-separated)
        // for its search-path override. Default to a non-existent path so
        // each test must explicitly opt in by overriding.
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all");
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

fn lm_studio_models(models_log: &[Value]) -> Vec<&Value> {
    models_log
        .iter()
        .filter(|m| m.get("tool").and_then(|v| v.as_str()) == Some("lm-studio"))
        .collect()
}

// ---------------------------------------------------------------------------
// AC-1 — New default path `~/.cache/lm-studio/models/` is scanned.
// ---------------------------------------------------------------------------

#[test]
fn lm_studio_cache_is_discovered() {
    let (_temp, fixture_root) = build_fixture("devon-lm-studio");
    // Point the plugin at the fixture's `.cache/lm-studio/models/` tree.
    let models_dir = fixture_root.join(".cache").join("lm-studio").join("models");
    assert!(
        models_dir.exists(),
        "fixture must contain .cache/lm-studio/models"
    );

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_LMSTUDIO_DIRS", &models_dir);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let lm = lm_studio_models(&models_log);

    // Fixture has 3 valid GGUFs across three org/repo subdirs.
    assert_eq!(
        lm.len(),
        3,
        "AC-1: lm-studio plugin must surface 3 models from .cache/lm-studio/models; got {}\nentries: {:#?}",
        lm.len(),
        lm
    );

    // All entries must report Format::Gguf (MLX out of scope per intake C3).
    for m in &lm {
        let format = m.get("format").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(
            format, "Gguf",
            "AC-3 — every LM Studio entry must have format=Gguf in v1; got {:?} for {:?}",
            format, m
        );
    }
}

// ---------------------------------------------------------------------------
// AC-2 — Older default path `~/.lmstudio/models/` is honored.
// ---------------------------------------------------------------------------

#[test]
fn older_lm_studio_path_is_honored() {
    let (_temp, fixture_root) = build_fixture("devon-lm-studio-older");
    let older_dir = fixture_root.join(".lmstudio").join("models");
    assert!(
        older_dir.exists(),
        "fixture must contain .lmstudio/models (older convention)"
    );

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_LMSTUDIO_DIRS", &older_dir);

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let models_log = read_models_log(&log_dir);
    let lm = lm_studio_models(&models_log);

    // Fixture has 1 entry (Hermes-2-Pro under QuantFactory/).
    assert_eq!(
        lm.len(),
        1,
        "AC-2: older path must yield 1 model; got {}\nentries: {:#?}",
        lm.len(),
        lm
    );
    let id = lm[0]
        .get("id_in_tool")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        id.contains("hermes-2-pro") || id.contains("Hermes-2-Pro"),
        "AC-2: older-path model id must mention hermes-2-pro; got {:?}",
        id
    );
}

// ---------------------------------------------------------------------------
// AC-4 — "Not installed" (neither path exists) shows benign message.
// ---------------------------------------------------------------------------

#[test]
fn lm_studio_not_installed_shows_benign_message() {
    // Both default paths missing → plugin reports NotInstalled; the launch
    // inventory's tool_errors must NOT list lm-studio as an error (NotInstalled
    // is a benign state, not an error per US-02 AC-4 and US-15 AC-4).
    let (mut cmd, log_temp) = modeltap_headless();
    // MODELTAP_LMSTUDIO_DIRS already pinned at a non-existent path by
    // modeltap_headless(). Don't override.

    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let log_dir = log_temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);

    let inv = find_event(&events, "launch.inventory")
        .unwrap_or_else(|| panic!("no launch.inventory in:\n{:?}", events));
    let tool_errors = inv
        .get("tool_errors")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("tool_errors must be present and an array; got {:?}", inv));

    // tool_errors entries are tool names that surfaced an Error variant.
    // NotInstalled (the benign state) MUST NOT appear in tool_errors.
    let lm_in_errors = tool_errors.iter().any(|e| e.as_str() == Some("lm-studio"));
    assert!(
        !lm_in_errors,
        "AC-4: 'lm-studio' must NOT appear in launch.inventory.tool_errors when both default paths are missing (NotInstalled is benign, not an error). Got tool_errors={:?}",
        tool_errors
    );

    // Models.log must contain zero lm-studio entries (nothing was discovered).
    let models_log = read_models_log(&log_dir);
    let lm = lm_studio_models(&models_log);
    assert_eq!(
        lm.len(),
        0,
        "AC-4: 0 models when not installed; got {}\nentries: {:#?}",
        lm.len(),
        lm
    );
}

// ---------------------------------------------------------------------------
// AC-4 — "Error" (path exists, unreadable) is distinguished from "Not installed".
// ---------------------------------------------------------------------------
//
// Permission-denied paths are surfaced as a tool error in launch.inventory's
// tool_errors list, NOT as NotInstalled. This proves AC-4's distinction.
#[cfg(unix)]
#[test]
fn lm_studio_unreadable_path_is_reported_as_error() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let models_dir = temp.path().join("models");
    fs::create_dir_all(&models_dir).expect("create models dir");
    // Make the directory unreadable (mode 000). Tests must restore mode for
    // cleanup — TempDir's drop will fail otherwise.
    fs::set_permissions(&models_dir, fs::Permissions::from_mode(0o000)).expect("set perms 000");

    let (mut cmd, log_temp) = modeltap_headless();
    cmd.env("MODELTAP_LMSTUDIO_DIRS", &models_dir);

    let result = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();
    drop(result);

    let log_dir = log_temp.path().join(".modeltap");
    let events = read_launch_log(&log_dir);
    let inv = find_event(&events, "launch.inventory")
        .unwrap_or_else(|| panic!("no launch.inventory in:\n{:?}", events));
    let tool_errors = inv
        .get("tool_errors")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("tool_errors must be present and an array; got {:?}", inv));
    let lm_in_errors = tool_errors.iter().any(|e| e.as_str() == Some("lm-studio"));
    assert!(
        lm_in_errors,
        "AC-4: 'lm-studio' MUST appear in tool_errors when the configured path \
         exists but is unreadable (this is the 'error' state, not 'not installed'). \
         Got tool_errors={:?}",
        tool_errors
    );

    // Restore perms so TempDir drop can clean up.
    let _ = fs::set_permissions(&models_dir, fs::Permissions::from_mode(0o755));
}
