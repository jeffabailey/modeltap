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
