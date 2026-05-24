//! Cache opt-out acceptance scenarios (US-23 Scenario 6 + AC-23-8 / AC-23-9
//! + INT-INFO-6).
//!
//! tool-model-info-sqlite-cache step 04-02 — Phase 04 opt-out path.
//!
//! Four scenarios, each a plain `#[test]` per the project's cucumber-driver
//! convention (no cucumber-rs macro machinery). The step-phrase
//! implementations live in `steps/cache_opt_out.rs`; this driver wires them
//! in scenario order.
//!
//! Strategy B (real I/O against fixture-populated temp dirs) per
//! `docs/feature/tool-model-info-sqlite-cache/distill/wave-decisions.md` §D5.
//! Every scenario spawns the real `modeltap` binary via
//! `assert_cmd::Command::cargo_bin` against an isolated tempdir.

#[path = "steps/cache_opt_out.rs"]
mod cache_opt_out;

use cache_opt_out::*;
use modeltap_acceptance::fixtures::dir_manifest::DirManifest;

// ---------------------------------------------------------------------------
// Scenario 1: "--no-cache bypasses the cache for one launch"
// ---------------------------------------------------------------------------
//
// AC-23-8: with the `--no-cache` flag, the cache directory contains zero
// new bytes after the launch. The launch must still succeed (cache failure
// path never prevents launch — C-INFO-2), so we assert exit 0 AND DirManifest
// equality.
#[test]
fn no_cache_flag_bypasses_the_cache_for_one_launch() {
    let mut world = CacheOptOutWorld::new();

    // Given the cache file does not exist
    given_the_cache_file_does_not_exist(&world);
    // Given the TestTool will discover one model
    given_the_test_tool_will_discover_one_model(&world);

    // Snapshot the cache directory BEFORE the launch. The fixture's
    // `build()` creates `xdg-data/modeltap/` (empty); the snapshot
    // captures that empty state so the post-launch comparison detects
    // any cache.sqlite (or -wal / -shm) write.
    let before = DirManifest::snapshot(&cache_dir_for(&world));

    // When the user runs "modeltap --no-cache"
    when_user_runs_modeltap_with_no_cache(&mut world);

    // Then modeltap exits successfully
    then_modeltap_exits_successfully(&world);
    // And the cache directory is byte-identical before and after
    then_cache_directory_is_byte_identical(&world, &before);
    // And no cache.sqlite-wal / cache.sqlite-shm sidecars are present
    then_no_wal_or_shm_sidecar_present(&world);
}

// ---------------------------------------------------------------------------
// Scenario 2: "cache.enabled = false config has the same effect as --no-cache"
// ---------------------------------------------------------------------------
//
// AC-23-9: setting `[cache] enabled = false` in `~/.modeltap/config.toml`
// (here pinned via `MODELTAP_CONFIG_PATH`) reaches the same opt-out state
// as the CLI flag — no flag is passed in this scenario.
#[test]
fn cache_enabled_false_config_has_same_effect_as_no_cache() {
    let mut world = CacheOptOutWorld::new();

    given_the_cache_file_does_not_exist(&world);
    given_the_test_tool_will_discover_one_model(&world);
    // Given a config file with `[cache] enabled = false`
    given_a_config_file_with_cache_disabled(&mut world);

    let before = DirManifest::snapshot(&cache_dir_for(&world));

    // When the user runs "modeltap" (NO --no-cache flag — the config alone
    // is the opt-out lever).
    when_user_runs_modeltap_without_flag(&mut world);

    then_modeltap_exits_successfully(&world);
    then_cache_directory_is_byte_identical(&world, &before);
    then_no_wal_or_shm_sidecar_present(&world);
}

// ---------------------------------------------------------------------------
// Scenario 3: "--no-cache produces zero cache writes for the entire launch"
// ---------------------------------------------------------------------------
//
// Explicit restatement of AC-23-8 as a single byte-precise invariant: even
// the LATE writes (background reconcile-writeback, model-detail cache
// updates) must NOT touch the cache directory. The scenario lets the
// launch complete its full headless cycle (no `--quit-after-paint` early
// exit) before re-snapshotting.
//
// In Phase 04 the headless harness already exits after one frame, so this
// is equivalent to scenario 1 in observable behaviour — the test exists to
// document the contract explicitly so a future regression that defers cache
// writes to a background task would still trip it.
#[test]
fn no_cache_produces_zero_cache_writes_for_entire_launch() {
    let mut world = CacheOptOutWorld::new();

    given_the_cache_file_does_not_exist(&world);
    given_the_test_tool_will_discover_one_model(&world);

    let before = DirManifest::snapshot(&cache_dir_for(&world));

    when_user_runs_modeltap_with_no_cache(&mut world);

    then_modeltap_exits_successfully(&world);
    // Belt-and-braces: assert the cache.sqlite file itself is absent (not
    // just unchanged). With the `before` snapshot of an empty xdg-data dir,
    // DirManifest equality implies absence; the explicit check guards
    // against a future fixture change that pre-creates the file.
    let cache_path = world.fixture.cache_path();
    assert!(
        !cache_path.exists(),
        "cache.sqlite must not exist after a --no-cache launch; found at {}",
        cache_path.display()
    );
    then_cache_directory_is_byte_identical(&world, &before);
    then_no_wal_or_shm_sidecar_present(&world);
}

// ---------------------------------------------------------------------------
// Scenario 4: "modeltap --version succeeds when the cache is unreadable"
// ---------------------------------------------------------------------------
//
// INT-INFO-6: `modeltap --version` exits 0 even when MODELTAP_CACHE_PATH
// points at a corrupt SQLite file. Clap's auto-version handler runs BEFORE
// `main()`'s body, so the cache is never touched. The fixture pre-installs
// a corrupt header so that ANY downstream cache-open attempt would fail —
// proving the path doesn't reach Cache::open.
#[test]
fn modeltap_version_succeeds_when_cache_is_unreadable() {
    let mut world = CacheOptOutWorld::new();

    // Given the cache file is corrupt
    given_the_cache_file_is_corrupt(&world);

    // When the user runs "modeltap --version"
    when_user_runs_modeltap_version(&mut world);

    // Then modeltap exits successfully (and clap printed the version string
    // to stdout — assert_cmd captures the status code; the version string
    // shape is verified by the existing release_process tests, so we don't
    // duplicate that assertion here).
    then_modeltap_exits_successfully(&world);
}
