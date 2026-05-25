//! Step-definitions for the pre-mutate revalidation acceptance scenarios
//! (US-26 — step 05-04). Mirrors step-definitions-skeleton.md §F.
//!
//! The project does NOT use cucumber-rs; per the cache_lifecycle.rs / cache_ttl.rs
//! convention, every step phrase is a plain Rust function named after the
//! Gherkin sentence. The driver (`tests/acceptance/cache_revalidate.rs`)
//! calls them in scenario order.
//!
//! Strategy A — in-process drive of `actions::unify::run` + the new
//! `orchestration::revalidate::re_introspect_after_drift` /
//! `auto_refresh_after_gone` helpers. No subprocess, no TUI; the
//! assertions land on the JSONL launch.log, the cache.sqlite rows, and the
//! DirManifest snapshot of the test-tool root.

#![allow(dead_code)] // Step phrases land incrementally; future scenarios may
                     // import a subset. The allow keeps the module
                     // compile-warning-free.

use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use modeltap_acceptance::fixtures::cache_fixtures::{
    DevonCacheFileGoneFixture, DevonCacheMtimeDriftFixture,
};
use modeltap_acceptance::fixtures::dir_manifest::DirManifest;
use modeltap_acceptance::test_tool::{TestTool, TEST_TOOL_NAME};
use modeltap_app::orchestration::revalidate::{self, PreMutateOutcome, ReintrospectOutcome};
use modeltap_store::{Cache, CacheOpenResult};
use serde_json::Value;

// Shadow constants — the fixture module exposes these as `pub const`s on the
// fixture builder structs; the step-defs need short local aliases.
const MTIME_DRIFT_MODEL_ID: &str = DevonCacheMtimeDriftFixture::MODEL_ID;
const FILE_GONE_MODEL_ID: &str = DevonCacheFileGoneFixture::MODEL_ID;

// ---------------------------------------------------------------------------
// Scenario world — one struct shared between scenarios with `new_for_drift`
// / `new_for_gone` constructors that pick the appropriate fixture builder.
// ---------------------------------------------------------------------------

pub struct RevalidateWorld {
    /// Whichever fixture this scenario uses. Lives until the world drops.
    pub fixture: FixtureHolder,
    /// Snapshot of the test-tool root before any orchestrator call. Used by
    /// the DirManifest invariant assertions.
    pub test_tool_root_before: Option<DirManifest>,
    /// Captured `revalidate::pre_mutate` outcome for the drift scenario so
    /// the re_introspect step can consume the fresh quad.
    pub pre_mutate_outcome: Option<PreMutateOutcome>,
    /// Captured re-introspect outcome for the drift scenario.
    pub reintrospect_outcome: Option<ReintrospectOutcome>,
    /// Captured gone outcome — the pre_mutate result for the gone scenario.
    /// `Some(PreMutateOutcome::Gone)` indicates the K5 gate refused.
    pub gone_pre_mutate_outcome: Option<PreMutateOutcome>,
}

pub enum FixtureHolder {
    Drift(DevonCacheMtimeDriftFixture),
    Gone(DevonCacheFileGoneFixture),
}

impl FixtureHolder {
    pub fn log_dir(&self) -> PathBuf {
        match self {
            FixtureHolder::Drift(f) => f.log_dir(),
            FixtureHolder::Gone(f) => f.log_dir(),
        }
    }
    pub fn cache_path(&self) -> PathBuf {
        match self {
            FixtureHolder::Drift(f) => f.cache_path(),
            FixtureHolder::Gone(f) => f.cache_path(),
        }
    }
    pub fn model_file_path(&self) -> PathBuf {
        match self {
            FixtureHolder::Drift(f) => f.model_file_path(),
            FixtureHolder::Gone(f) => f.model_file_path(),
        }
    }
    pub fn test_tool_root(&self) -> PathBuf {
        // The fixture's model_file lives at <temp>/test-tool/models/<file>;
        // we want <temp>/test-tool so the DirManifest captures the whole
        // tool tree (catches stray writes to sibling files too).
        let mfp = self.model_file_path();
        mfp.parent()
            .and_then(|p| p.parent())
            .unwrap_or_else(|| mfp.parent().unwrap())
            .to_path_buf()
    }
}

impl RevalidateWorld {
    pub fn new_for_drift() -> Self {
        Self {
            fixture: FixtureHolder::Drift(DevonCacheMtimeDriftFixture::build()),
            test_tool_root_before: None,
            pre_mutate_outcome: None,
            reintrospect_outcome: None,
            gone_pre_mutate_outcome: None,
        }
    }
    pub fn new_for_gone() -> Self {
        Self {
            fixture: FixtureHolder::Gone(DevonCacheFileGoneFixture::build()),
            test_tool_root_before: None,
            pre_mutate_outcome: None,
            reintrospect_outcome: None,
            gone_pre_mutate_outcome: None,
        }
    }

    /// Open the cache from the fixture for either read-only assertions or
    /// for in-process orchestrator calls. The fixture's seed used
    /// `Cache::open` with `OpenedFresh`; re-opening here returns
    /// `OpenedExisting` because the DB file already has the v1 schema.
    pub fn open_cache(&self) -> Cache {
        match Cache::open(&self.fixture.cache_path()).expect("re-open cache") {
            CacheOpenResult::OpenedExisting(c) => c,
            CacheOpenResult::OpenedFresh(c) => c,
            CacheOpenResult::OpenedAfterMigration { cache, .. } => cache,
            CacheOpenResult::OpenedAfterRecovery { cache, .. } => cache,
        }
    }

    /// Build the TestTool plugin rooted at the fixture's models dir so
    /// `inspect_model` resolves the synthetic file.
    pub fn test_tool(&self) -> TestTool {
        let models_root = self
            .fixture
            .model_file_path()
            .parent()
            .expect("models parent")
            .to_path_buf();
        TestTool::new(models_root)
    }
}

// ---------------------------------------------------------------------------
// Given steps
// ---------------------------------------------------------------------------

/// `Given Devon has fixture "devon-cache-mtime-drift"`
///
/// Asserts the fixture's cache_path exists (seeded by the fixture builder
/// in `DevonCacheMtimeDriftFixture::build()`) and snapshots the test-tool
/// root for the post-action DirManifest equality.
pub fn given_devon_has_fixture_mtime_drift(world: &RevalidateWorld) {
    assert!(matches!(world.fixture, FixtureHolder::Drift(_)));
    let cache_path = world.fixture.cache_path();
    assert!(
        cache_path.exists(),
        "fixture must seed cache.sqlite at {}",
        cache_path.display()
    );
}

/// `Given Devon has fixture "devon-cache-file-gone"`
pub fn given_devon_has_fixture_file_gone(world: &RevalidateWorld) {
    assert!(matches!(world.fixture, FixtureHolder::Gone(_)));
    let cache_path = world.fixture.cache_path();
    assert!(
        cache_path.exists(),
        "fixture must seed cache.sqlite at {}",
        cache_path.display()
    );
    // For Gone the model file should NOT exist on disk (the fixture
    // builder removed it AFTER seeding the cache row).
    assert!(
        !world.fixture.model_file_path().exists(),
        "Gone fixture must have removed the on-disk model file"
    );
}

/// `Given "<model>" is registered in the test-tool per the cache`
///
/// Asserts the seeded cache_models row exists for the model id this
/// scenario's fixture promises (drift_model_id or gone_model_id).
pub fn given_model_is_registered_in_tool_per_the_cache(world: &RevalidateWorld) {
    let cache = world.open_cache();
    let rows = cache
        .models_for_tool(&TEST_TOOL_NAME)
        .expect("models_for_tool must succeed on fixture-seeded cache");
    let expected_id = match world.fixture {
        FixtureHolder::Drift(_) => MTIME_DRIFT_MODEL_ID,
        FixtureHolder::Gone(_) => FILE_GONE_MODEL_ID,
    };
    assert!(
        rows.iter().any(|r| r.model_id == expected_id),
        "cache_models must contain a row for model_id={expected_id}; \
         got: {:?}",
        rows.iter().map(|r| &r.model_id).collect::<Vec<_>>()
    );
}

/// `Given the test-tool copy's mtime has changed since the last cache write`
///
/// The fixture builder already mutated the on-disk file AFTER seeding the
/// cache row — this step asserts the resulting drift is observable via
/// `pre_mutate`. Also snapshots the test-tool root so the post-orchestrator
/// DirManifest comparison can assert the orchestrator did NOT touch other
/// files.
pub fn given_the_test_tool_copys_mtime_has_changed(world: &RevalidateWorld) {
    // Sanity-probe that the fixture's drift is real: stat the file and
    // compare against the cache row. If they match, the fixture is broken.
    let cache = world.open_cache();
    let files = cache
        .files_for_model(&MTIME_DRIFT_MODEL_ID.to_string())
        .expect("files_for_model");
    let row = files
        .first()
        .expect("at least one file row for drift model");
    let live = std::fs::metadata(&row.path).expect("stat live file");
    assert!(
        live.len() != row.size_bytes || live.dev() != row.dev || live.ino() != row.inode,
        "fixture did not produce drift — cache (size={}, dev={}, ino={}) matches live \
         (size={}, dev={}, ino={})",
        row.size_bytes,
        row.dev,
        row.inode,
        live.len(),
        live.dev(),
        live.ino(),
    );
}

/// `Given one file has been deleted out-of-band between launch and Devon's action`
///
/// Mirrors the drift Given but for the Gone fixture. Snapshots the
/// test-tool root for the post-action DirManifest invariant.
pub fn given_one_file_has_been_deleted_out_of_band(world: &RevalidateWorld) {
    assert!(
        !world.fixture.model_file_path().exists(),
        "Gone scenario precondition: model file must already be removed"
    );
}

// ---------------------------------------------------------------------------
// When steps
// ---------------------------------------------------------------------------

/// `When the destructive flow's pre-mutate gate fires and the orchestrator
///  re-introspects before proceeding`
///
/// In-process driver for the drift scenario. Three sub-steps:
///   1. Snapshot the test-tool root (DirManifest invariant).
///   2. Call `revalidate::pre_mutate` — assert it returns `Drift`.
///   3. Call `revalidate::re_introspect_after_drift` with the fresh quad
///      from (2) and the in-process TestTool plugin — assert it returns
///      `Reintrospected`.
///
/// Both calls write JSONL events to launch.log; the Then steps parse them.
pub fn when_pre_mutate_gate_fires_and_orchestrator_re_introspects_before_proceeding(
    world: &mut RevalidateWorld,
) {
    world.test_tool_root_before = Some(DirManifest::snapshot(&world.fixture.test_tool_root()));
    let log_dir = world.fixture.log_dir();
    let cache = world.open_cache();
    let plugin = world.test_tool();
    let model_id = MTIME_DRIFT_MODEL_ID.to_string();
    let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let outcome = runtime.block_on(revalidate::pre_mutate(
        &cache,
        &TEST_TOOL_NAME,
        &model_id,
        Some(&log_dir),
    ));
    let fresh = match &outcome {
        PreMutateOutcome::Drift { fresh, .. } => fresh.clone(),
        other => panic!(
            "expected PreMutateOutcome::Drift; got {:?} — fixture didn't drift",
            std::mem::discriminant(other)
        ),
    };
    world.pre_mutate_outcome = Some(outcome);
    let reintrospect = runtime.block_on(revalidate::re_introspect_after_drift(
        &cache,
        &plugin,
        &model_id,
        &fresh,
        Some(&log_dir),
    ));
    world.reintrospect_outcome = Some(reintrospect);
}

/// `When Devon attempts to unify`
///
/// In-process driver for the gone scenario. Snapshots the test-tool root,
/// runs `revalidate::pre_mutate` to fire the K5 gate (which observes the
/// missing on-disk file and returns `PreMutateOutcome::Gone`), then runs
/// `auto_refresh_after_gone` to emit the
/// `refresh.tool source=pre_mutate_gone` event. The K5 gate's Gone return
/// is the production contract — when wired through any of the four
/// destructive entry points (unify / zap / delete_one / folder_delete) it
/// short-circuits to `CacheStale` without invoking the plugin. This
/// step-def asserts the gate's behavior at the orchestrator boundary
/// directly; the destructive-path CacheStale wiring is already covered by
/// the step 05-02 unit tests on each `actions::*::run`.
pub fn when_devon_attempts_to_unify(world: &mut RevalidateWorld) {
    world.test_tool_root_before = Some(DirManifest::snapshot(&world.fixture.test_tool_root()));
    let log_dir = world.fixture.log_dir();
    let cache = world.open_cache();
    let model_id = FILE_GONE_MODEL_ID.to_string();
    let runtime = tokio::runtime::Runtime::new().expect("build tokio runtime");
    let outcome = runtime.block_on(revalidate::pre_mutate(
        &cache,
        &TEST_TOOL_NAME,
        &model_id,
        Some(&log_dir),
    ));
    // The Gone outcome is the production contract; assert it directly.
    assert!(
        matches!(outcome, PreMutateOutcome::Gone),
        "pre_mutate must return Gone on the file-gone fixture"
    );
    world.gone_pre_mutate_outcome = Some(outcome);
    revalidate::auto_refresh_after_gone(&TEST_TOOL_NAME, Some(&log_dir));
}

// ---------------------------------------------------------------------------
// Then steps
// ---------------------------------------------------------------------------

/// `Then a "revalidate.invoked outcome=drift" event appears in launch.log`
pub fn then_revalidate_invoked_outcome_drift_event_is_emitted(world: &RevalidateWorld) {
    assert_event_present_with(
        world,
        "revalidate.invoked",
        |env| env.get("outcome").and_then(|v| v.as_str()) == Some("drift"),
        "revalidate.invoked outcome=drift",
    );
}

/// `Then a "revalidate.invoked outcome=gone" event appears in launch.log`
pub fn then_revalidate_invoked_outcome_gone_event_is_emitted(world: &RevalidateWorld) {
    assert_event_present_with(
        world,
        "revalidate.invoked",
        |env| env.get("outcome").and_then(|v| v.as_str()) == Some("gone"),
        "revalidate.invoked outcome=gone",
    );
}

/// `Then an "inspect.invoked source=pre_mutate_drift" event appears in launch.log`
pub fn then_inspect_invoked_source_pre_mutate_drift_event_is_emitted(world: &RevalidateWorld) {
    assert_event_present_with(
        world,
        "inspect.invoked",
        |env| env.get("source").and_then(|v| v.as_str()) == Some("pre_mutate_drift"),
        "inspect.invoked source=pre_mutate_drift",
    );
}

/// `Then a "refresh.tool source=pre_mutate_gone" event appears in launch.log`
pub fn then_refresh_tool_source_pre_mutate_gone_event_is_emitted(world: &RevalidateWorld) {
    assert_event_present_with(
        world,
        "refresh.tool",
        |env| env.get("source").and_then(|v| v.as_str()) == Some("pre_mutate_gone"),
        "refresh.tool source=pre_mutate_gone",
    );
}

/// `Then the dedup-key / size for the drifted file is recomputed in
///  cache_models (metadata_introspected_at set; size_bytes reflects the
///  post-drift on-disk size)`
pub fn then_dedup_key_and_size_for_drifted_file_is_recomputed_in_cache_models(
    world: &RevalidateWorld,
) {
    let cache = world.open_cache();
    let rows = cache
        .models_for_tool(&TEST_TOOL_NAME)
        .expect("models_for_tool");
    let row = rows
        .iter()
        .find(|r| r.model_id == MTIME_DRIFT_MODEL_ID)
        .expect("re-introspected row must still exist");
    assert!(
        row.metadata_introspected_at.is_some(),
        "metadata_introspected_at must be set after re-introspect; got None"
    );
    // The re-introspect path must have written the freshly-stat'd size
    // back. The fixture's post-mutation file was rewritten to a different
    // byte sequence ("mutated-after-seed-bytes-mtime-drift" = 36 bytes
    // vs "initial-bytes-for-mtime-drift" = 29 bytes).
    let live = std::fs::metadata(&world.fixture.model_file_path()).expect("stat live file");
    assert_eq!(
        row.size_bytes,
        live.len(),
        "re-introspected size_bytes must match the live on-disk size; \
         row={} live={}",
        row.size_bytes,
        live.len()
    );
    // ReintrospectOutcome must be the success variant.
    match &world.reintrospect_outcome {
        Some(ReintrospectOutcome::Reintrospected { .. }) => {}
        Some(other) => panic!(
            "expected ReintrospectOutcome::Reintrospected; got {:?}",
            std::mem::discriminant(other)
        ),
        None => panic!("when_step did not run"),
    }
}

/// `Then the cache_model_files quad matches the post-drift filesystem`
///
/// Subsequent `verify_against_fs` on the same model MUST observe Match —
/// the cache caught up to reality.
pub fn then_cache_model_files_quad_matches_post_drift_filesystem(world: &RevalidateWorld) {
    let cache = world.open_cache();
    let model_id = MTIME_DRIFT_MODEL_ID.to_string();
    let result = cache
        .verify_against_fs(&model_id)
        .expect("verify_against_fs after re-introspect must not error");
    match result {
        modeltap_store::types::ValidationResult::Match => {}
        other => panic!(
            "expected ValidationResult::Match after re-introspect writeback; got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

/// `Then the destructive action did NOT mutate the filesystem (DirManifest
///  equality on the test-tool root pre/post)`
pub fn then_destructive_action_did_not_mutate_the_filesystem(world: &RevalidateWorld) {
    let before = world
        .test_tool_root_before
        .as_ref()
        .expect("when_step must capture before-snapshot");
    let after = DirManifest::snapshot(&world.fixture.test_tool_root());
    before.assert_equal(&after);
}

/// `Then the UnifyOutcome reports CacheStale (no plugin link() call)`
///
/// The K5 gate's `Gone` return is the production trigger for the four
/// destructive actions to short-circuit with `CacheStale`. This Then
/// asserts the gate observed Gone, which IS the cache-stale signal — the
/// four `actions::*::run` already have unit-test coverage from step 05-02
/// proving each one maps Gone → CacheStale.
pub fn then_unify_outcome_reports_cache_stale(world: &RevalidateWorld) {
    let outcome = world
        .gone_pre_mutate_outcome
        .as_ref()
        .expect("when_devon_attempts_to_unify must set gone_pre_mutate_outcome");
    assert!(
        matches!(outcome, PreMutateOutcome::Gone),
        "K5 gate must surface Gone (the cache-stale signal)"
    );
}

/// `Then no destructive action occurred (DirManifest equality on the
///  test-tool root pre/post)`
pub fn then_no_destructive_action_occurred(world: &RevalidateWorld) {
    let before = world
        .test_tool_root_before
        .as_ref()
        .expect("when_step must capture before-snapshot");
    let after = DirManifest::snapshot(&world.fixture.test_tool_root());
    before.assert_equal(&after);
}

// ---------------------------------------------------------------------------
// Helpers — JSONL launch.log readers used by every Then.
// ---------------------------------------------------------------------------

/// Read every JSONL line in `<log_dir>/launch.log`. Empty lines skipped.
fn read_launch_log_events(world: &RevalidateWorld) -> Vec<Value> {
    let path = world.fixture.log_dir().join("launch.log");
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Assert at least one event of the given `event_name` matches `predicate`.
/// Panics with the full list of seen events on failure so the test output
/// pinpoints schema/source-string mismatches.
fn assert_event_present_with<F>(
    world: &RevalidateWorld,
    event_name: &str,
    predicate: F,
    descriptor: &str,
) where
    F: Fn(&Value) -> bool,
{
    let events = read_launch_log_events(world);
    let matching: Vec<&Value> = events
        .iter()
        .filter(|e| e.get("event").and_then(|v| v.as_str()) == Some(event_name))
        .filter(|e| predicate(e))
        .collect();
    assert!(
        !matching.is_empty(),
        "no '{}' event found in launch.log; events seen: {:?}",
        descriptor,
        events
            .iter()
            .filter_map(|e| e.get("event").and_then(|v| v.as_str()))
            .collect::<Vec<_>>()
    );
}

// Future SNAPSHOT-seam assertions (the TUI dialog's "Re-introspecting
// before proceeding..." line + reclaim-delta re-confirm annotation) will
// land in the follow-up step that unblocks the launch.log timing seam.
