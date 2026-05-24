//! Step-definitions for the per-tool TTL acceptance scenarios (US-25
//! AC-25-2 / AC-25-4, AC-23-1, AC-25-7).
//!
//! tool-model-info-sqlite-cache step 04-03. Each Gherkin phrase becomes a
//! plain Rust function over a `CacheTtlWorld` struct; the driver file invokes
//! them in scenario order.
//!
//! Strategy A (in-process direct invocation of `warm_start::run` and
//! `cache_path::resolve`) — no subprocess. The fixture seeds a real
//! cache.sqlite on disk and the orchestrator reads it through the same code
//! path the modeltap binary uses; the test process owns the tokio runtime.

#![allow(dead_code)] // Step phrases are referenced by the cache_ttl driver;
                     // the rest of the workspace doesn't import them yet.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use modeltap_acceptance::fixtures::cache_fixtures::DevonCacheStaleToolFixture;
use modeltap_app::adapters::cache_path;
use modeltap_app::orchestration::warm_start::{self, WarmStartConfig, WarmStartResult, WarmStartSource};
use modeltap_core::types::ToolId;
use rusqlite::Connection;

// ---------------------------------------------------------------------------
// World
// ---------------------------------------------------------------------------

/// Per-scenario mutable state. Owns the fixture (so the tempdir lives until
/// the scenario asserts), the `WarmStartConfig` shape, and the captured
/// `WarmStartResult`.
pub struct CacheTtlWorld {
    pub fixture: DevonCacheStaleToolFixture,
    pub tool_ttl_seconds: u64,
    pub last_result: Option<WarmStartResult>,
}

impl CacheTtlWorld {
    pub fn new() -> Self {
        Self {
            fixture: DevonCacheStaleToolFixture::build(),
            // Sentinel: each scenario MUST call given_tool_ttl_is_24_hours
            // (or another setter) before when_warm_start_runs. A 0 here
            // would make every row stale unconditionally — explicit
            // failure if the scenario forgets the precondition.
            tool_ttl_seconds: 0,
            last_result: None,
        }
    }

    fn result(&self) -> &WarmStartResult {
        self.last_result
            .as_ref()
            .expect("a when_* step must run before this Then")
    }
}

// ---------------------------------------------------------------------------
// Given steps — scenario 1 + 3
// ---------------------------------------------------------------------------

/// `Given the devon-cache-stale-tool fixture is seeded`
///
/// The fixture's `build()` already populated the cache file. This step is a
/// belt-and-braces existence check that catches a future regression in the
/// fixture (e.g., the file being moved or renamed).
pub fn given_the_stale_tool_fixture_is_seeded(world: &CacheTtlWorld) {
    let cache_path = world.fixture.cache_path();
    assert!(
        cache_path.exists(),
        "fixture must pre-install cache.sqlite at {}",
        cache_path.display()
    );
}

/// `Given the tool_ttl_seconds is 24 hours`
pub fn given_tool_ttl_is_24_hours(world: &mut CacheTtlWorld) {
    world.tool_ttl_seconds = 24 * 3600;
}

/// `Given the cache_models table is dropped (simulating a transient I/O failure)`
///
/// Opens the seeded cache.sqlite via rusqlite directly (the same path the
/// CACHE seam helper uses) and drops the `cache_models` table. Subsequent
/// `models_for_tool` calls return `CacheError::Sqlite(no such table)`. This
/// is the in-process surrogate for a disk-level transient I/O failure that
/// the fallback gate in `warm_start::run` must tolerate.
pub fn given_cache_models_table_is_dropped(world: &mut CacheTtlWorld) {
    let path = world.fixture.cache_path();
    let conn = Connection::open(&path).expect("open cache for table drop");
    conn.execute("DROP TABLE cache_models", [])
        .expect("drop cache_models");
    drop(conn);
}

// ---------------------------------------------------------------------------
// Given steps — scenario 2 (production path resolution)
// ---------------------------------------------------------------------------

/// `Given neither MODELTAP_CACHE_PATH nor a CLI override is set`
///
/// Asserts the precondition only — the actual env-var pinning happens
/// inside `when_cache_path_resolve_runs_with_pinned_home` so the override
/// applies only to that resolver call.
pub fn given_no_cache_overrides_are_set() {
    // No-op assertion: scenario 2's resolver call passes `None, None`
    // explicitly. The scenario name documents the precondition.
}

// ---------------------------------------------------------------------------
// When steps
// ---------------------------------------------------------------------------

fn new_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

/// `When warm-start runs against the seeded cache`
///
/// Invokes `warm_start::run` directly with `cache_enabled = true` and the
/// scenario-configured `tool_ttl_seconds`. `now` is taken at the call site
/// — the fixture's relative timestamps (25h / 2h / 1h ago) shift with this
/// reference instant.
pub fn when_warm_start_runs(world: &mut CacheTtlWorld) {
    let rt = new_runtime();
    let config = WarmStartConfig {
        cache_enabled: true,
        log_dir: None,
        tool_ttl_seconds: world.tool_ttl_seconds,
        now: SystemTime::now(),
    };
    let result = rt
        .block_on(warm_start::run(&config, &world.fixture.cache_path()))
        .expect("warm_start returns ok");
    world.last_result = Some(result);
}

/// `When cache_path::resolve runs with HOME pinned to a tempdir`
///
/// Pins `HOME` (and on Linux `XDG_DATA_HOME`) to a freshly-created tempdir,
/// then invokes `resolve(None, None)` so the resolver falls through to
/// `dirs::data_dir()`. Returns the resolved path AND the tempdir's path
/// for the assertion step. Env-var pinning is process-wide; the test
/// restores the previous values when the returned guard drops.
pub fn when_cache_path_resolve_runs_with_pinned_home() -> ResolvedPath {
    use tempfile::TempDir;

    let home = TempDir::new().expect("home tempdir");
    let xdg = home.path().join("xdg-data");
    std::fs::create_dir_all(&xdg).expect("create xdg-data");

    // SAFETY: env vars are process-wide; running tests in parallel risks
    // interleaving. Acceptance tests live in their own [[test]] block
    // (one binary per file) so the test harness's parallel runner does
    // NOT interleave THIS test with other env-mutating tests. The guard
    // restores prior values to be a courteous neighbour.
    let _home_guard = EnvVarGuard::set("HOME", home.path());
    let _xdg_guard = EnvVarGuard::set("XDG_DATA_HOME", &xdg);
    // Also unset the cache-path env override so the resolver hits the
    // fallback branch even if the dev's shell has it set.
    let _cache_guard = EnvVarGuard::unset("MODELTAP_CACHE_PATH");

    let resolved = cache_path::resolve(None, None).expect("resolver returns ok");

    ResolvedPath {
        resolved,
        home: home.path().to_path_buf(),
        xdg_data_home: xdg,
        _home_dir: home,
    }
}

/// Pinned-resolver output. Holds the `TempDir` so it lives until the
/// scenario asserts (env-var guards drop in reverse order BEFORE the
/// tempdir, so `HOME` is restored before the dir is removed).
pub struct ResolvedPath {
    pub resolved: PathBuf,
    pub home: PathBuf,
    pub xdg_data_home: PathBuf,
    _home_dir: tempfile::TempDir,
}

// ---------------------------------------------------------------------------
// Then steps — scenario 1
// ---------------------------------------------------------------------------

/// `Then warm-start returns source = Existing`
pub fn then_warm_start_returns_existing_source(world: &CacheTtlWorld) {
    let result = world.result();
    assert!(
        matches!(result.source, WarmStartSource::Existing),
        "expected WarmStartSource::Existing, got {:?}",
        result.source
    );
}

/// `Then the inventory contains the fresh tools' models`
///
/// Two fresh tools (llama-cli at 2h, hf at 1h) each contributed one model
/// row; the inventory must contain exactly those two entries.
pub fn then_inventory_contains_fresh_tools_models(world: &CacheTtlWorld) {
    let result = world.result();
    let painted_tools: Vec<&str> = result
        .inventory
        .entries
        .iter()
        .map(|e| e.tool.0)
        .collect();
    assert!(
        painted_tools
            .iter()
            .any(|t| *t == DevonCacheStaleToolFixture::FRESH_TOOL_ID_LLAMA_CLI),
        "llama-cli (fresh, 2h) MUST paint; entries: {painted_tools:?}"
    );
    assert!(
        painted_tools
            .iter()
            .any(|t| *t == DevonCacheStaleToolFixture::FRESH_TOOL_ID_HF),
        "hf (fresh, 1h) MUST paint; entries: {painted_tools:?}"
    );
    // The stale tool must NOT contribute inventory rows.
    assert!(
        !painted_tools
            .iter()
            .any(|t| *t == DevonCacheStaleToolFixture::STALE_TOOL_ID),
        "ollama (stale, 25h) MUST NOT paint; entries: {painted_tools:?}"
    );
}

/// `Then the stale tool appears in stale_tool_ids`
pub fn then_stale_tool_appears_in_stale_tool_ids(world: &CacheTtlWorld) {
    let result = world.result();
    let stale: Vec<&str> = result.stale_tool_ids.iter().map(|t| t.0).collect();
    assert!(
        stale
            .iter()
            .any(|t| *t == DevonCacheStaleToolFixture::STALE_TOOL_ID),
        "ollama (25h, stale w.r.t. 24h TTL) MUST appear in stale_tool_ids; got {stale:?}"
    );
}

/// `Then the fresh tools are absent from stale_tool_ids`
pub fn then_fresh_tools_absent_from_stale_tool_ids(world: &CacheTtlWorld) {
    let result = world.result();
    let stale: Vec<&str> = result.stale_tool_ids.iter().map(|t| t.0).collect();
    assert!(
        !stale
            .iter()
            .any(|t| *t == DevonCacheStaleToolFixture::FRESH_TOOL_ID_LLAMA_CLI),
        "llama-cli (fresh, 2h) MUST NOT appear in stale_tool_ids; got {stale:?}"
    );
    assert!(
        !stale
            .iter()
            .any(|t| *t == DevonCacheStaleToolFixture::FRESH_TOOL_ID_HF),
        "hf (fresh, 1h) MUST NOT appear in stale_tool_ids; got {stale:?}"
    );
}

// ---------------------------------------------------------------------------
// Then steps — scenario 2
// ---------------------------------------------------------------------------

/// `Then the resolved path matches the platform default`
///
/// On macOS, `dirs::data_dir()` returns `$HOME/Library/Application Support`;
/// on Linux, `$XDG_DATA_HOME` (or `$HOME/.local/share` when unset). Either
/// way the path ends with `modeltap/cache.sqlite`. The test pins both
/// `HOME` and `XDG_DATA_HOME` so the assertion is portable: the parent of
/// the resolved file MUST live INSIDE the pinned home tree (proving the
/// resolver consulted the pinned env, not some stale absolute value).
pub fn then_resolved_path_matches_platform_default(out: &ResolvedPath) {
    let tail: PathBuf = out
        .resolved
        .iter()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    assert_eq!(
        tail,
        PathBuf::from("modeltap").join("cache.sqlite"),
        "resolved path must end in modeltap/cache.sqlite; got {}",
        out.resolved.display()
    );
    // Containment check: the resolved path must descend from the pinned
    // HOME tempdir on every supported platform.
    assert!(
        out.resolved.starts_with(&out.home),
        "resolved path {} must live under pinned HOME {}",
        out.resolved.display(),
        out.home.display()
    );
    // Platform-specific tail. macOS resolves `dirs::data_dir()` to
    // `$HOME/Library/Application Support`; Linux honours `XDG_DATA_HOME`.
    #[cfg(target_os = "macos")]
    {
        let appsupport = out.home.join("Library").join("Application Support");
        assert!(
            out.resolved.starts_with(&appsupport),
            "macOS: resolved path {} must descend from {}",
            out.resolved.display(),
            appsupport.display()
        );
    }
    #[cfg(target_os = "linux")]
    {
        assert!(
            out.resolved.starts_with(&out.xdg_data_home),
            "Linux: resolved path {} must descend from XDG_DATA_HOME {}",
            out.resolved.display(),
            out.xdg_data_home.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Then steps — scenario 3
// ---------------------------------------------------------------------------

/// `Then the inventory is empty`
pub fn then_inventory_is_empty(world: &CacheTtlWorld) {
    let result = world.result();
    assert!(
        result.inventory.entries.is_empty(),
        "inventory must be empty when every models_for_tool failed; got {} entries",
        result.inventory.entries.len()
    );
}

/// `Then all three tools appear in stale_tool_ids`
pub fn then_all_three_tools_appear_in_stale_tool_ids(world: &CacheTtlWorld) {
    let result = world.result();
    let stale: Vec<&str> = result.stale_tool_ids.iter().map(|t| t.0).collect();
    for expected in [
        DevonCacheStaleToolFixture::STALE_TOOL_ID,
        DevonCacheStaleToolFixture::FRESH_TOOL_ID_LLAMA_CLI,
        DevonCacheStaleToolFixture::FRESH_TOOL_ID_HF,
    ] {
        assert!(
            stale.iter().any(|t| *t == expected),
            "tool {expected} MUST appear in stale_tool_ids on a transient I/O failure; got {stale:?}"
        );
    }
}

/// `Then warm-start did not return an error`
///
/// Scenario-3 sanity check: the `when_warm_start_runs` step `expect()`s
/// the Result, so reaching the Then phase already proves no error. The
/// explicit check documents the invariant for the reader.
pub fn then_warm_start_did_not_error(world: &CacheTtlWorld) {
    assert!(
        world.last_result.is_some(),
        "warm_start::run MUST return Ok on transient I/O failure (AC-25-7)"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Restore-on-drop guard for a process-wide env var. Mirrors the pattern
/// other acceptance tests use to keep env-var mutations from leaking into
/// sibling scenarios.
pub struct EnvVarGuard {
    key: String,
    previous: Option<std::ffi::OsString>,
}

impl EnvVarGuard {
    /// SAFETY: env vars are process-global. Tests using this guard must
    /// not run in parallel with other tests that touch the same key. The
    /// modeltap-acceptance crate's [[test]] blocks each compile to a
    /// separate binary; cargo-test's `--test-threads` defaults to one
    /// thread per test binary for these.
    pub fn set(key: &str, value: &std::path::Path) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: see struct doc.
        unsafe {
            std::env::set_var(key, value.as_os_str());
        }
        Self {
            key: key.to_string(),
            previous,
        }
    }

    pub fn unset(key: &str) -> Self {
        let previous = std::env::var_os(key);
        // SAFETY: see struct doc.
        unsafe {
            std::env::remove_var(key);
        }
        Self {
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        // SAFETY: see struct doc on EnvVarGuard.
        unsafe {
            match self.previous.take() {
                Some(val) => std::env::set_var(&self.key, &val),
                None => std::env::remove_var(&self.key),
            }
        }
    }
}

// Documented but unused-by-tests today: keeps the `Duration` import live
// for future TTL boundary scenarios in step 04-04.
#[allow(dead_code)]
const _DURATION_PIN: Duration = Duration::from_secs(0);

// Pin ToolId import so a future "stale tool id type smoke" step
// compiles without a fresh `use` line.
#[allow(dead_code)]
fn _tool_id_pin() -> ToolId {
    ToolId("pin")
}
