//! Integration tests for warm-start orchestration and cache-path resolver
//! (tool-model-info-sqlite-cache step 01-04).
//!
//! Covers AC-23-1 (cache path resolution) and AC-25-1/AC-25-4 happy paths
//! for the warm-start path. The corruption / recovery / TTL surfaces land
//! in subsequent steps (01-05+).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::SystemTime;

use modeltap_app::adapters::cache_path;
use modeltap_app::orchestration::warm_start::{self, WarmStartConfig, WarmStartSource};
use modeltap_core::types::ToolId;
use modeltap_store::types::{CachedModel, CachedTool};
use modeltap_store::Cache;
use tempfile::TempDir;

const TEST_TOOL: ToolId = ToolId("warm-test-tool");

// ---------------------------------------------------------------------------
// cache_path::resolve
// ---------------------------------------------------------------------------

#[test]
fn cache_path_resolves_env_var_when_set() {
    let env_override = OsString::from("/tmp/modeltap-test-warm/cache.sqlite");
    let resolved =
        cache_path::resolve(None, Some(env_override.as_os_str())).expect("env override resolves");
    assert_eq!(
        resolved,
        PathBuf::from("/tmp/modeltap-test-warm/cache.sqlite")
    );
}

#[test]
fn cache_path_prefers_cli_override_over_env() {
    let cli = PathBuf::from("/tmp/modeltap-cli/cache.sqlite");
    let env_override = OsString::from("/tmp/modeltap-env/cache.sqlite");
    let resolved = cache_path::resolve(Some(cli.as_path()), Some(env_override.as_os_str()))
        .expect("cli override resolves");
    assert_eq!(resolved, cli);
}

#[test]
fn cache_path_falls_back_to_data_dir_when_no_overrides() {
    let resolved = cache_path::resolve(None, None).expect("default resolves");
    // The exact value depends on $HOME on the test host; we only assert the
    // tail matches the documented default. `dirs::data_dir()` returns
    // `$HOME/Library/Application Support` on macOS and `$XDG_DATA_HOME` (or
    // `$HOME/.local/share`) on Linux — both end with the same `modeltap/cache.sqlite`
    // suffix when we join.
    let suffix = resolved
        .iter()
        .rev()
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<PathBuf>();
    assert_eq!(suffix, PathBuf::from("modeltap").join("cache.sqlite"));
}

// ---------------------------------------------------------------------------
// warm_start::run
// ---------------------------------------------------------------------------

fn new_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
}

#[test]
fn warm_start_with_disabled_cache_returns_empty_inventory_and_no_warm_paint_event() {
    let tmp = TempDir::new().expect("tempdir");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).expect("logdir");
    let cache_path = tmp.path().join("cache.sqlite");

    let config = WarmStartConfig {
        cache_enabled: false,
        log_dir: Some(log_dir.clone()),
    };

    let rt = new_runtime();
    let result = rt
        .block_on(warm_start::run(&config, &cache_path))
        .expect("warm_start returns ok");

    assert!(matches!(result.source, WarmStartSource::Disabled));
    assert!(result.inventory.entries.is_empty());
    // No warm_paint_ms event must be emitted when the cache is disabled.
    assert!(!log_contains_warm_paint(&log_dir.join("launch.log")));
}

#[test]
fn warm_start_on_fresh_cache_returns_empty_inventory_and_no_warm_paint_event() {
    let tmp = TempDir::new().expect("tempdir");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).expect("logdir");
    // Note: cache file does NOT exist beforehand — this is the fresh path.
    let cache_path = tmp.path().join("cache.sqlite");

    let config = WarmStartConfig {
        cache_enabled: true,
        log_dir: Some(log_dir.clone()),
    };

    let rt = new_runtime();
    let result = rt
        .block_on(warm_start::run(&config, &cache_path))
        .expect("warm_start returns ok");

    assert!(matches!(result.source, WarmStartSource::Fresh));
    assert!(result.inventory.entries.is_empty());
    assert!(!log_contains_warm_paint(&log_dir.join("launch.log")));
}

#[test]
fn warm_start_on_existing_cache_returns_built_inventory_and_emits_warm_paint_event() {
    let tmp = TempDir::new().expect("tempdir");
    let log_dir = tmp.path().join("logs");
    std::fs::create_dir_all(&log_dir).expect("logdir");
    let cache_path = tmp.path().join("cache.sqlite");

    // Seed the cache with one tool and one model.
    {
        let cache = match Cache::open(&cache_path).expect("seed open") {
            modeltap_store::CacheOpenResult::OpenedFresh(c) => c,
            other => panic!("expected OpenedFresh on seed, got {:?}", other),
        };
        cache.write_tool(&seeded_tool()).expect("seed tool");
        cache
            .write_models(&TEST_TOOL, &[seeded_model()])
            .expect("seed model");
    }

    let config = WarmStartConfig {
        cache_enabled: true,
        log_dir: Some(log_dir.clone()),
    };

    let rt = new_runtime();
    let result = rt
        .block_on(warm_start::run(&config, &cache_path))
        .expect("warm_start returns ok");

    assert!(matches!(result.source, WarmStartSource::Existing));
    assert_eq!(
        result.inventory.entries.len(),
        1,
        "exactly one inventory entry from one cached model"
    );
    assert_eq!(result.inventory.entries[0].tool, TEST_TOOL);
    assert!(log_contains_warm_paint(&log_dir.join("launch.log")));
}

fn seeded_tool() -> CachedTool {
    CachedTool {
        tool_id: TEST_TOOL,
        install_path: PathBuf::from("/tmp/warm-test/install"),
        detected_version: Some("v9.9.9".to_string()),
        plugin_version: "0.0.0".to_string(),
        model_count: 1,
        disk_usage_bytes: 1024,
        largest_model_id: Some("seed-model".to_string()),
        last_scan_at: SystemTime::now(),
        last_scan_duration_ms: 10,
        last_error: None,
        last_error_at: None,
        search_paths: Vec::new(),
    }
}

fn seeded_model() -> CachedModel {
    CachedModel {
        model_id: "seed-model".to_string(),
        tool_id: TEST_TOOL,
        display_name: "Seeded warm-start model".to_string(),
        format: Some("gguf".to_string()),
        quantisation: Some("Q4_K_M".to_string()),
        size_bytes: 1024,
        sha256: None,
        architecture: None,
        parameters_billions: None,
        context_length: None,
        dedup_group_id: None,
        metadata_kv: BTreeMap::new(),
        metadata_introspected_at: None,
        last_seen_at: SystemTime::now(),
        last_validated_at: None,
    }
}

fn log_contains_warm_paint(path: &std::path::Path) -> bool {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return false;
    };
    contents
        .lines()
        .any(|line| line.contains("launch.warm_paint_ms"))
}
