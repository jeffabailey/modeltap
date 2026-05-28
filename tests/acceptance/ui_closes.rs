//! Consolidated UI acceptance — Q3: "Can I close the UI?"
//!
//! `q` exits cleanly (code 0) and emits `launch.ended`; Ctrl+C exits 130 and
//! must NOT emit `launch.ended` (the KPI invariant). Drives the real binary
//! headless with scripted input.

use std::path::PathBuf;
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

struct UiFixture {
    _temp: TempDir,
    test_tool_root: PathBuf,
    cache_path: PathBuf,
    log_dir: PathBuf,
}

fn fixture() -> UiFixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let test_tool_root = temp.path().join("test-tool/models");
    std::fs::create_dir_all(&test_tool_root).expect("create test-tool root");
    std::fs::write(
        test_tool_root.join("test-model-7b.gguf"),
        b"GGUF test model bytes - deterministic content for hashing",
    )
    .expect("write test model");
    let log_dir = temp.path().join("logs");
    std::fs::create_dir_all(&log_dir).expect("create log dir");
    let cache_path = temp.path().join("xdg-data/modeltap/cache.sqlite");
    std::fs::create_dir_all(cache_path.parent().unwrap()).expect("create cache parent");
    UiFixture {
        _temp: temp,
        test_tool_root,
        cache_path,
        log_dir,
    }
}

fn cmd(fx: &UiFixture) -> Command {
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "100")
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", &fx.test_tool_root)
        .env("MODELTAP_CACHE_PATH", &fx.cache_path)
        .env("MODELTAP_LOG_DIR", &fx.log_dir)
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache");
    cmd
}

fn read_launch_log(log_dir: &std::path::Path) -> Vec<Value> {
    let path = log_dir.join("launch.log");
    let raw = std::fs::read_to_string(&path).unwrap_or_default();
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[test]
fn ui_closes_cleanly_on_q() {
    let fx = fixture();
    cmd(&fx)
        .env("MODELTAP_HEADLESS_INPUT", "q")
        .timeout(Duration::from_secs(15))
        .assert()
        .success();

    let events = read_launch_log(&fx.log_dir);
    let last = events.last().expect("at least one launch.log event");
    assert_eq!(
        last.get("event").and_then(|v| v.as_str()),
        Some("launch.ended"),
        "last event on q-quit must be launch.ended; got {last:?}"
    );
}

#[test]
fn ui_closes_on_ctrl_c_with_code_130_and_no_launch_ended() {
    let fx = fixture();
    let assert = cmd(&fx)
        .env("MODELTAP_HEADLESS_INPUT", "^C")
        .timeout(Duration::from_secs(15))
        .assert()
        .failure();

    assert_eq!(
        assert.get_output().status.code(),
        Some(130),
        "Ctrl+C must exit 130 (POSIX 128+SIGINT)"
    );

    let events = read_launch_log(&fx.log_dir);
    for ev in &events {
        assert_ne!(
            ev.get("event").and_then(|v| v.as_str()),
            Some("launch.ended"),
            "launch.ended must NOT be emitted on Ctrl+C; found {ev:?}"
        );
    }
}
