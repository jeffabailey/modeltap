//! modeltap-app library surface.
//!
//! Most of `modeltap-app` is the binary `main()` (in `main.rs`); a small
//! library surface is exposed here so integration tests can call refresh /
//! action helpers directly. The bin re-uses these via `crate::*` paths.
//!
//! Library-only modules (no plugin linkage, no JSONL writer) — kept thin so
//! pulling them into a test binary does not drag the whole composition
//! root.

#![forbid(unsafe_code)]

// tool-model-info-sqlite-cache step 01-04:
// `adapters` hosts the cache-path resolver; `orchestration` hosts the
// warm-start orchestrator. Both are exposed via the library half so
// integration tests can drive them without spawning the binary.
pub mod adapters;
// tool-model-info-sqlite-cache step 04-02: app-level configuration loader
// reads `[cache] enabled` from `~/.modeltap/config.toml` (or the
// `MODELTAP_CONFIG_PATH` test override). The composition root resolves the
// CLI `--no-cache` flag against this config — flag wins when both set.
pub mod config;
pub mod orchestration;

pub mod hash_pool;
pub mod hash_pool_wiring;
pub mod inventory_build;
pub mod lsof_adapter;
pub mod platform;
pub mod plugin_isolation;
pub mod reclassify;
pub mod refresh;
// Registry is exposed via the library half so integration tests can drive
// the `MODELTAP_TEST_PLUGINS` seam (tool-model-info-sqlite-cache step 01-03)
// directly via `modeltap_app::registry::collect_plugins` rather than spawning
// the binary. main.rs imports through `modeltap_app::registry` instead of
// declaring its own `mod registry;`.
pub mod registry;
pub mod sha256_cache;
