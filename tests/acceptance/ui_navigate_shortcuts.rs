//! Consolidated UI acceptance — Q2: "Can I navigate the UI, use the shortcuts,
//! and get the expected results?"
//!
//! Two legs:
//!   A. Shortcut sweep — drive every main-view shortcut in one scripted
//!      session and assert the event loop dispatches each without crashing,
//!      navigation keeps the inventory painted, and the session quits cleanly.
//!   B. SHA256 persistence (US-27 AC-27-1, folded in here per the lean-suite
//!      decision) — across two launches with `[cache] persist_sha256 = true`,
//!      launch 1 emits `hash.computed` for the model and launch 2 (file
//!      unchanged) seeds from the cache and emits NO `hash.computed`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

struct UiFixture {
    _temp: TempDir,
    test_tool_root: PathBuf,
    cache_path: PathBuf,
    log_dir: PathBuf,
    config_path: PathBuf,
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
    // A config that opts INTO SHA256 persistence (leg B).
    let config_path = temp.path().join("config.toml");
    std::fs::write(&config_path, "[cache]\npersist_sha256 = true\n").expect("write config");
    UiFixture {
        _temp: temp,
        test_tool_root,
        cache_path,
        log_dir,
        config_path,
    }
}

/// `modeltap` headless command. `config` selects the TOML: `None` pins the
/// no-such-config path (persistence off); `Some` points at the persist config.
fn cmd(fx: &UiFixture, config: Option<&Path>) -> Command {
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
        .env("MODELTAP_ATOMIC_CHAT_DIRS", "/nonexistent/no-such-atomic-chat")
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache");
    match config {
        Some(p) => cmd.env("MODELTAP_CONFIG_PATH", p),
        None => cmd.env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml"),
    };
    cmd
}

fn read_launch_log(log_dir: &Path) -> Vec<Value> {
    let raw = std::fs::read_to_string(log_dir.join("launch.log")).unwrap_or_default();
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn has_event(events: &[Value], name: &str) -> bool {
    events
        .iter()
        .any(|e| e.get("event").and_then(|v| v.as_str()) == Some(name))
}

// ---------------------------------------------------------------------------
// Leg A — shortcut sweep
// ---------------------------------------------------------------------------

#[test]
fn ui_navigates_and_dispatches_every_shortcut_then_quits_cleanly() {
    let fx = fixture();
    // Exercise navigation (arrows + tab), the detail/help screens (i, ?), an
    // unbound key (x), and the dialog-opening destructive shortcuts (u/z/d/F)
    // — each followed by <esc> so any opened overlay closes — then quit. The
    // contract here is "every shortcut dispatches without crashing the loop";
    // destructive actions are NOT confirmed (no fs mutation).
    let script = "<down><up><tab>i<esc>?<esc>u<esc>z<esc>d<esc>F<esc>xq";
    let assert = cmd(&fx, None)
        .env("MODELTAP_HEADLESS_INPUT", script)
        .timeout(Duration::from_secs(20))
        .assert()
        .success();

    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    // The session survived every shortcut and the final frame is the main view
    // (bottom bar present). The model row is still in the inventory (no key
    // corrupted or deleted it — destructive dialogs were cancelled with Esc).
    assert!(
        stdout.contains("[q] quit"),
        "after the shortcut sweep the main-view bottom bar must still render; got:\n{stdout}"
    );

    // The model file must still exist on disk — no destructive shortcut
    // actually deleted it (all were Esc-cancelled).
    assert!(
        fx.test_tool_root.join("test-model-7b.gguf").exists(),
        "no destructive shortcut may delete the model when cancelled with Esc"
    );

    // Clean shutdown via q.
    assert!(has_event(
        &read_launch_log(&fx.log_dir),
        "launch.ended"
    ));
}

// ---------------------------------------------------------------------------
// Leg B — SHA256 persistence across launches (US-27 AC-27-1)
// ---------------------------------------------------------------------------

#[test]
fn sha256_persists_across_launches_so_unchanged_file_is_not_rehashed() {
    let fx = fixture();

    // Launch 1: fresh cache + persist on. The pool hashes the model, so a
    // hash.computed event is emitted AND the hash is written to cache_sha256.
    // `<hash-complete>` waits for the pool to finish before `q` quits.
    cmd(&fx, Some(&fx.config_path))
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let after_launch_1 = read_launch_log(&fx.log_dir);
    assert!(
        has_event(&after_launch_1, "hash.computed"),
        "launch 1 (fresh cache) must compute + emit hash.computed; events: {:?}",
        after_launch_1
            .iter()
            .filter_map(|e| e.get("event").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
    );

    // Truncate launch.log so launch 2's events are observed in isolation.
    std::fs::write(fx.log_dir.join("launch.log"), b"").expect("truncate launch.log");

    // Launch 2: same cache, file unchanged. Warm-start seeds the in-process
    // cache from cache_sha256, so the pool finds a hit and emits NO
    // hash.computed for the unchanged model (AC-27-1).
    cmd(&fx, Some(&fx.config_path))
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let after_launch_2 = read_launch_log(&fx.log_dir);
    assert!(
        !has_event(&after_launch_2, "hash.computed"),
        "launch 2 (unchanged file, persisted hash) must NOT re-hash; \
         hash.computed unexpectedly present. events: {:?}",
        after_launch_2
            .iter()
            .filter_map(|e| e.get("event").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn sha256_invalidates_and_rehashes_when_the_file_changes(/* US-27 AC-27-2 */) {
    let fx = fixture();
    let model = fx.test_tool_root.join("test-model-7b.gguf");

    // Launch 1: hash + persist the original content.
    cmd(&fx, Some(&fx.config_path))
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();
    assert!(
        has_event(&read_launch_log(&fx.log_dir), "hash.computed"),
        "launch 1 must compute the initial hash"
    );

    std::fs::write(fx.log_dir.join("launch.log"), b"").expect("truncate launch.log");

    // The file changes on disk (different length → the size element of the
    // validity quad drifts, regardless of mtime resolution). The persisted
    // hash is now stale.
    std::fs::write(
        &model,
        b"DIFFERENT content - this file changed since the cached hash was computed",
    )
    .expect("rewrite model with new content");

    // Launch 2: the quad no longer matches, so the persisted hash is NOT
    // seeded; the pool recomputes and the writeback overwrites the stale row.
    // The observable: hash.computed IS emitted again (invalidation → re-hash).
    cmd(&fx, Some(&fx.config_path))
        .env("MODELTAP_HEADLESS_INPUT", "<hash-complete>q")
        .timeout(Duration::from_secs(30))
        .assert()
        .success();

    let after_launch_2 = read_launch_log(&fx.log_dir);
    assert!(
        has_event(&after_launch_2, "hash.computed"),
        "launch 2 (file CHANGED) must invalidate the stale persisted hash and \
         re-hash; hash.computed missing. events: {:?}",
        after_launch_2
            .iter()
            .filter_map(|e| e.get("event").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
    );
}
