//! Regression test for the production warm-start path.
//!
//! Bug (commit 19850001, 2026-05-18): `crates/modeltap-app/src/main.rs:199`
//! short-circuited warm-start to `None` whenever `MODELTAP_CACHE_PATH` was
//! unset — i.e. production launches NEVER opened the default cache file.
//! `ls "$HOME/Library/Application Support/modeltap/"` returned "No such file
//! or directory" after a fresh binary run.
//!
//! The author of the guard appears to have written it as a test-isolation
//! seam, but `cache_path::resolve(None, None)` already does the correct
//! three-tier fallback (CLI → env → `dirs::data_dir()`). The guard was
//! suppressing the entire production code path.
//!
//! This test pins `HOME` to a tempdir (so it never pollutes the developer's
//! real HOME) and does NOT set `MODELTAP_CACHE_PATH`. After the fix, the
//! cache file MUST appear at the platform-default location:
//!   - macOS:        `$HOME/Library/Application Support/modeltap/cache.sqlite`
//!   - Linux/WSL:    `$HOME/.local/share/modeltap/cache.sqlite`
//!
//! Pre-fix: file is absent → test FAILS.
//! Post-fix: file is present, non-empty → test PASSES.

use std::time::Duration;

use assert_cmd::Command;

/// Platform-specific default cache path relative to `$HOME`.
///
/// Mirrors `dirs::data_dir()`'s platform mapping (the same crate used by
/// `crates/modeltap-app/src/adapters/cache_path.rs`). We avoid linking
/// `dirs` here so the test asserts the user-visible contract, not the
/// resolver's internals.
fn expected_cache_relative_to_home() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Library/Application Support/modeltap/cache.sqlite"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        ".local/share/modeltap/cache.sqlite"
    }
    #[cfg(windows)]
    {
        compile_error!("modeltap is WSL-only on Windows; native Windows not supported");
    }
}

#[test]
fn production_default_warm_start_opens_default_cache_file() {
    // Pin HOME to a tempdir so we don't write to the developer's real
    // `$HOME/Library/Application Support/modeltap/`. `dirs::data_dir()` on
    // macOS resolves from `$HOME`; on Linux from `$XDG_DATA_HOME` (which
    // falls back to `$HOME/.local/share`). We override both for safety.
    let home_temp = tempfile::tempdir().expect("home tempdir");
    let log_temp = tempfile::tempdir().expect("log tempdir");
    let log_dir = log_temp.path().join(".modeltap");
    std::fs::create_dir_all(&log_dir).expect("log dir");

    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("HOME", home_temp.path())
        // On Linux, XDG_DATA_HOME wins over $HOME/.local/share. Set it to
        // `$tempdir/.local/share` so the assert below holds on both
        // platforms.
        .env("XDG_DATA_HOME", home_temp.path().join(".local/share"))
        // CRITICAL: do NOT set MODELTAP_CACHE_PATH. This test is the
        // litmus for the production default path.
        .env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_LOG_DIR", &log_dir)
        .env("MODELTAP_TERM_COLS", "120")
        // Quiet down all plugin search dirs so the launch is fast and
        // deterministic — the cache file is what we're asserting on, not
        // inventory contents.
        .env("HF_HOME", "/nonexistent/no-such-hf-home")
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    cmd.arg("--quit-after-paint")
        .timeout(Duration::from_secs(10))
        .assert()
        .success();

    let expected = home_temp.path().join(expected_cache_relative_to_home());
    assert!(
        expected.exists(),
        "production warm-start did not create the default cache file at {}\n\
         (expected because MODELTAP_CACHE_PATH was unset; the production \
         resolver should have fallen through to dirs::data_dir())",
        expected.display(),
    );

    // Non-empty: the warm-start path must actually open and initialize the
    // SQLite database, not just touch an empty file.
    let metadata = std::fs::metadata(&expected).expect("stat cache.sqlite");
    assert!(
        metadata.len() > 0,
        "cache file exists but is empty at {} (len = {})",
        expected.display(),
        metadata.len(),
    );
}
