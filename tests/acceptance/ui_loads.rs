//! Consolidated UI acceptance — Q1: "Can I load the UI?"
//!
//! One of the three lean UI-lifecycle acceptance tests that replace the
//! per-feature cache/detail acceptance binaries (the detailed logic lives in
//! crate-level integration/unit tests; these three prove the UI itself works).
//!
//! Drives the real `modeltap` binary headless with the in-process TestTool
//! plugin so the inventory is deterministic (one model), then asserts the
//! two-pane layout + bottom bar + the model row all painted on first frame.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use tempfile::TempDir;

/// Build a hermetic fixture: a tempdir containing one TestTool model file, an
/// (absent) cache path, and a log dir. Returns the guard + the three paths.
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
    // The TestTool reports any file under its root; give it realistic bytes so
    // the background hash pool has something to hash.
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

/// A `modeltap` command wired for headless TestTool-only discovery. Every real
/// plugin is pinned at a non-existent path so the test is hermetic.
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

#[test]
fn ui_loads_two_pane_layout_with_inventory() {
    let fx = fixture();
    let started = Instant::now();
    let assert = cmd(&fx)
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(15))
        .assert()
        .success();
    let elapsed = started.elapsed();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();

    // Two-pane layout + bottom bar painted on the first frame (US-01 AC-6).
    assert!(
        stdout.contains("[<-/->] tools"),
        "first frame must contain the bottom-bar nav hint; got:\n{stdout}"
    );
    assert!(
        stdout.contains("[q] quit"),
        "bottom bar must offer [q] quit"
    );
    // The TestTool's model row painted in the right pane.
    assert!(
        stdout.contains("test-model-7b"),
        "right pane must show the TestTool model row; got:\n{stdout}"
    );

    // Cold start to first paint is fast (US-01 AC-1; relaxed for CI/Gatekeeper).
    assert!(
        elapsed < Duration::from_secs(10),
        "first paint took {elapsed:?}, expected well under 10s"
    );
}
