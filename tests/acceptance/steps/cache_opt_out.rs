//! Step-definitions for the cache-opt-out scenarios (US-23 Scenario 6 +
//! AC-23-8 / AC-23-9 + INT-INFO-6).
//!
//! tool-model-info-sqlite-cache step 04-02. Mirrors the cache_lifecycle.rs
//! pattern: each Gherkin phrase becomes a plain Rust function over a
//! `CacheOptOutWorld` struct; the driver file invokes them in scenario order.
//!
//! The four scenarios:
//!
//!   1. "--no-cache bypasses the cache for one launch" — the CLI flag wins,
//!      Cache::open is never invoked, the cache directory is byte-identical
//!      before and after.
//!   2. "cache.enabled = false config has the same effect as --no-cache" —
//!      a config-only opt-out reaches the same DirManifest invariant.
//!   3. "--no-cache produces zero cache writes for the entire launch" — the
//!      explicit byte-precise restatement of scenario 1, asserted via the
//!      DirManifest helper.
//!   4. "modeltap --version succeeds when the cache is unreadable" — the
//!      `--version` path exits 0 even when `MODELTAP_CACHE_PATH` points at
//!      a corrupt SQLite file. Clap's auto-version handler exits before
//!      main()'s body runs, so the cache is never touched.

#![allow(dead_code)] // Step phrases are referenced by the cache_opt_out
                     // driver; the rest of the workspace doesn't import
                     // them yet.

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use assert_cmd::Command;
use modeltap_acceptance::fixtures::cache_fixtures::DevonCacheEmptyFixture;
use modeltap_acceptance::fixtures::dir_manifest::DirManifest;
use modeltap_acceptance::test_tool::TEST_MODEL_FILENAME;
use tempfile::TempDir;

/// Mutable scenario state. One per scenario.
pub struct CacheOptOutWorld {
    pub fixture: DevonCacheEmptyFixture,
    /// Holder for the per-scenario `~/.modeltap/config.toml` file. Lives in
    /// its own tempdir so `MODELTAP_CONFIG_PATH` can be set to its full
    /// path. `None` when the scenario does NOT use a config file.
    pub config_file: Option<PerScenarioConfig>,
    pub last_output: Option<Output>,
}

impl CacheOptOutWorld {
    pub fn new() -> Self {
        Self {
            fixture: DevonCacheEmptyFixture::build(),
            config_file: None,
            last_output: None,
        }
    }
}

/// Owns a tempdir holding a single `config.toml`. The scenario points
/// `MODELTAP_CONFIG_PATH` at `path()` so the modeltap binary loads our
/// fixture file instead of `$HOME/.modeltap/config.toml`.
pub struct PerScenarioConfig {
    pub dir: TempDir,
    pub path: PathBuf,
}

impl PerScenarioConfig {
    pub fn with_contents(contents: &str) -> Self {
        let dir = TempDir::new().expect("create config tempdir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, contents).expect("write config.toml");
        Self { dir, path }
    }
}

// ---------------------------------------------------------------------------
// Command builder — mirrors cache_lifecycle.rs::modeltap_command_with_test_tool
// but EVERY env var is opt-IN so each scenario can shape its own variant.
// ---------------------------------------------------------------------------

fn base_modeltap_command(world: &CacheOptOutWorld) -> Command {
    let mut cmd = Command::cargo_bin("modeltap").expect("cargo bin modeltap");
    cmd.env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "100")
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", world.fixture.test_tool_root())
        .env("MODELTAP_CACHE_PATH", world.fixture.cache_path())
        .env("MODELTAP_LOG_DIR", world.fixture.log_dir())
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache");
    // MODELTAP_CONFIG_PATH: scenario-controlled. When `config_file` is set,
    // point the binary at our fixture; otherwise pin to a non-existent path
    // so the binary's default `$HOME/.modeltap/config.toml` resolution is
    // a no-op (test isolation).
    if let Some(cfg) = world.config_file.as_ref() {
        cmd.env("MODELTAP_CONFIG_PATH", &cfg.path);
    } else {
        cmd.env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml");
    }
    cmd
}

// ---------------------------------------------------------------------------
// Given steps
// ---------------------------------------------------------------------------

/// `Given the cache file does not exist`
pub fn given_the_cache_file_does_not_exist(world: &CacheOptOutWorld) {
    let path = world.fixture.cache_path();
    assert!(
        !path.exists(),
        "precondition violated: cache.sqlite already exists at {}",
        path.display()
    );
}

/// `Given the TestTool will discover one model at the fixture path`
pub fn given_the_test_tool_will_discover_one_model(world: &CacheOptOutWorld) {
    let model_path = world.fixture.test_tool_root().join(TEST_MODEL_FILENAME);
    assert!(
        model_path.exists(),
        "TestTool's seed model must exist at {}",
        model_path.display()
    );
}

/// `Given a config file with [cache] enabled = false`
pub fn given_a_config_file_with_cache_disabled(world: &mut CacheOptOutWorld) {
    world.config_file = Some(PerScenarioConfig::with_contents(
        "[cache]\nenabled = false\n",
    ));
}

/// `Given the cache file at <path> is a corrupt SQLite header`
///
/// Overwrites the empty fixture's cache.sqlite path with 16 KB of
/// deterministic non-SQLite bytes. This mirrors `DevonCacheCorruptFixture`'s
/// payload but reuses the empty fixture so the `--version` scenario doesn't
/// need a different fixture type.
pub fn given_the_cache_file_is_corrupt(world: &CacheOptOutWorld) {
    let path = world.fixture.cache_path();
    let bytes: Vec<u8> = (0..16_384u32)
        .map(|i| ((i.wrapping_mul(2654435761)) >> 24) as u8)
        .collect();
    std::fs::write(&path, bytes).expect("seed corrupt cache.sqlite for --version scenario");
    assert!(path.exists(), "corrupt cache file must exist on disk");
}

// ---------------------------------------------------------------------------
// When steps
// ---------------------------------------------------------------------------

/// `When the user runs "modeltap --no-cache" and quits after first paint`
pub fn when_user_runs_modeltap_with_no_cache(world: &mut CacheOptOutWorld) {
    let output = base_modeltap_command(world)
        .arg("--no-cache")
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("spawn modeltap --no-cache");
    world.last_output = Some(output);
}

/// `When the user runs "modeltap" and quits after first paint`
///
/// No `--no-cache` flag; only the per-scenario `MODELTAP_CONFIG_PATH` is
/// in effect. Used by the `cache.enabled = false` config scenario to prove
/// the config alone is sufficient.
pub fn when_user_runs_modeltap_without_flag(world: &mut CacheOptOutWorld) {
    let output = base_modeltap_command(world)
        .arg("--quit-after-paint")
        .timeout(Duration::from_secs(30))
        .output()
        .expect("spawn modeltap (no --no-cache)");
    world.last_output = Some(output);
}

/// `When the user runs "modeltap --version"`
///
/// Clap's auto-version handler runs BEFORE main()'s body, so the cache is
/// never opened. Asserts the exit code only; the launch never reaches the
/// warm-start orchestrator.
pub fn when_user_runs_modeltap_version(world: &mut CacheOptOutWorld) {
    let output = base_modeltap_command(world)
        .arg("--version")
        .timeout(Duration::from_secs(10))
        .output()
        .expect("spawn modeltap --version");
    world.last_output = Some(output);
}

// ---------------------------------------------------------------------------
// Then steps
// ---------------------------------------------------------------------------

/// `Then modeltap exits successfully`
pub fn then_modeltap_exits_successfully(world: &CacheOptOutWorld) {
    let out = world
        .last_output
        .as_ref()
        .expect("a when_* step must run before this Then");
    assert!(
        out.status.success(),
        "modeltap must exit 0; got status={:?}, stderr={}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `Then the cache directory is byte-identical before and after the launch`
///
/// Re-snapshot the xdg-data/modeltap/ tree and compare to the pre-launch
/// snapshot. AC-23-8: with `--no-cache` (or `cache.enabled = false`), the
/// cache directory must contain zero new bytes — cache.sqlite, the -wal,
/// and the -shm files must all be absent (or unchanged if they were
/// pre-existing).
pub fn then_cache_directory_is_byte_identical(
    world: &CacheOptOutWorld,
    before: &DirManifest,
) {
    let cache_dir = cache_dir_for(world);
    let after = DirManifest::snapshot(&cache_dir);
    before.assert_equal(&after);
}

/// `Then no cache.sqlite-wal or cache.sqlite-shm file is present`
///
/// Strengthens the DirManifest assertion by explicitly checking the two
/// sidecar files SQLite would create on the first write. Together with the
/// DirManifest equality, this proves Cache::open was NEVER invoked: rusqlite
/// creates the -wal and -shm files lazily on first WAL-mode write, so their
/// absence after a full launch is the definitive smoking gun.
pub fn then_no_wal_or_shm_sidecar_present(world: &CacheOptOutWorld) {
    let cache_path = world.fixture.cache_path();
    let wal = with_sidecar_suffix(&cache_path, "-wal");
    let shm = with_sidecar_suffix(&cache_path, "-shm");
    assert!(
        !wal.exists(),
        "cache.sqlite-wal must not exist after a --no-cache launch; found at {}",
        wal.display()
    );
    assert!(
        !shm.exists(),
        "cache.sqlite-shm must not exist after a --no-cache launch; found at {}",
        shm.display()
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `<temp>/xdg-data/modeltap/` — the parent directory of cache.sqlite. The
/// DirManifest snapshot is taken over this directory.
pub fn cache_dir_for(world: &CacheOptOutWorld) -> PathBuf {
    world
        .fixture
        .cache_path()
        .parent()
        .expect("cache_path has a parent directory")
        .to_path_buf()
}

/// Compute the SQLite WAL/SHM sidecar path. The on-disk shape is
/// `<cache_path>-wal` / `<cache_path>-shm` (single dash, no replacement of
/// the `.sqlite` extension) — see SQLite WAL mode docs. `suffix` here is
/// `"-wal"` or `"-shm"`.
fn with_sidecar_suffix(path: &Path, suffix: &str) -> PathBuf {
    let s = path.to_string_lossy().into_owned();
    PathBuf::from(format!("{s}{suffix}"))
}
