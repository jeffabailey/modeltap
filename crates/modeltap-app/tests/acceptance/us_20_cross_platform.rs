//! Acceptance tests for US-20 (Cross-platform CI matrix + WSL/Windows handling;
//! Phase 04 exit gate / Release 3 exit gate).
//!
//! Per `docs/feature/modeltap-tui/distill/features/master-acceptance.feature`
//! @us-20 scenarios. Three scenarios drive the `modeltap` binary under
//! different `MODELTAP_FORCE_PLATFORM` overrides — a Scenario Outline over
//! the supported targets:
//!
//! 1. **Discovery uses per-OS default paths** — Scenario Outline over
//!    macos-aarch64, linux-x86_64, linux-aarch64. With `MODELTAP_FORCE_PLATFORM`
//!    set to each variant the binary launches headlessly and emits a
//!    `launch.inventory` event whose `tools_registered` list matches the
//!    canonical four-plugin set. The platform override is read from
//!    `MODELTAP_FORCE_PLATFORM` and surfaces no Windows refusal.
//!
//! 2. **WSL is treated as Linux** — `MODELTAP_FORCE_PLATFORM=linux-x86_64`
//!    behaves identically to native Linux. WSL has no special handling.
//!
//! 3. **Native Windows binary refuses to run with clear message** —
//!    `MODELTAP_FORCE_PLATFORM=windows-x86_64` causes the binary to exit 64
//!    with the WSL guidance on stderr (no TUI rendered, no log file written).
//!
//! Tags: @us-20 @release-3 @cross-platform.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

/// Build the `modeltap` headless command with a per-test log dir and a forced
/// platform override. The platform override is the contract under test —
/// `current_platform()` must read this env var when it is set, regardless of
/// the actual host OS, so a single CI job exercises every supported variant.
fn modeltap_with_platform(platform: &str) -> (Command, TempDir, PathBuf) {
    let log_dir_temp = tempfile::tempdir().expect("tempdir for log");
    let log_dir = log_dir_temp.path().join(".modeltap");
    fs::create_dir_all(&log_dir).expect("create log dir");
    let log_file = log_dir.join("launch.log");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        .env("MODELTAP_FORCE_PLATFORM", platform)
        // Quiet down all plugin search dirs so each scenario runs fast and
        // cannot accidentally pick up the test runner's $HOME.
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LLAMACLI_DIRS", "/nonexistent/no-such-llama")
        .env("HF_HOME", "/nonexistent/no-such-hf")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    (cmd, log_dir_temp, log_file)
}

fn read_jsonl_events(log_file: &std::path::Path) -> Vec<Value> {
    let content = fs::read_to_string(log_file).unwrap_or_default();
    content
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario 1 (Outline): Discovery uses per-OS default paths.
// Outlined over the three supported Unix-like variants. Each must launch
// successfully and emit a `launch.inventory` event listing the canonical
// four-plugin set (atomic-chat-fixture is gated by an opt-in env var so it
// is NOT in the registered list under default test invocation; this test
// asserts the four production plugins are present).
// ---------------------------------------------------------------------------

fn discovery_runs_on_platform(platform: &str) {
    let (mut cmd, _temp, log_file) = modeltap_with_platform(platform);
    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    let events = read_jsonl_events(&log_file);
    let inventory = events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("launch.inventory"))
        .unwrap_or_else(|| panic!("must emit launch.inventory on {platform}, events={events:?}"));

    let registered = inventory
        .get("tools_registered")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("launch.inventory must carry tools_registered, got {inventory}"));
    let names: Vec<String> = registered
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();

    for expected in ["hf", "llama-cli", "lm-studio", "ollama"] {
        assert!(
            names.iter().any(|n| n == expected),
            "AC-1 ({platform}): tools_registered must contain {expected}, got {names:?}"
        );
    }
}

#[test]
fn discovery_uses_per_os_default_paths_macos_aarch64() {
    discovery_runs_on_platform("macos-aarch64");
}

#[test]
fn discovery_uses_per_os_default_paths_linux_x86_64() {
    discovery_runs_on_platform("linux-x86_64");
}

#[test]
fn discovery_uses_per_os_default_paths_linux_aarch64() {
    discovery_runs_on_platform("linux-aarch64");
}

// ---------------------------------------------------------------------------
// Scenario 2: WSL is treated as Linux.
// `linux-x86_64` is the canonical WSL platform string. This scenario asserts
// no special-case branching for WSL — the binary launches the same way it
// would on bare-metal Linux. We exercise this by launching with
// `MODELTAP_FORCE_PLATFORM=linux-x86_64` and asserting clean exit + no
// Windows refusal text on stderr.
// ---------------------------------------------------------------------------

#[test]
fn wsl_is_treated_as_linux() {
    let (mut cmd, _temp, _log_file) = modeltap_with_platform("linux-x86_64");
    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(20))
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    assert!(
        !stderr.contains("Windows is supported only via WSL"),
        "AC-2: WSL path must NOT print the Windows refusal, got stderr:\n{stderr}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 3: Native Windows binary refuses to run with clear message.
// `MODELTAP_FORCE_PLATFORM=windows-x86_64` simulates a native Windows host.
// The binary must exit with code 64 and print the documented WSL-guidance
// message on stderr. No TUI is rendered.
// ---------------------------------------------------------------------------

#[test]
fn native_windows_binary_refuses_to_run_with_clear_message() {
    let (mut cmd, _temp, _log_file) = modeltap_with_platform("windows-x86_64");
    let assert = cmd
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .failure();

    let code = assert.get_output().status.code();
    assert_eq!(
        code,
        Some(64),
        "AC-3: native Windows binary must exit 64, got {code:?}"
    );

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).to_string();
    let expected_msg = "Windows is supported only via WSL";
    let expected_url = "https://learn.microsoft.com/windows/wsl/install";
    assert!(
        stderr.contains(expected_msg),
        "AC-3: stderr must contain {expected_msg:?}, got:\n{stderr}"
    );
    assert!(
        stderr.contains(expected_url),
        "AC-3: stderr must contain WSL install URL {expected_url:?}, got:\n{stderr}"
    );
}
