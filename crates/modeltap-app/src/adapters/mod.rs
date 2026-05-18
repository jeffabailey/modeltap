//! Composition-root adapters — the thin sync/path/env-var layer that lives
//! between the composition root and the rest of the app.
//!
//! Per architecture-design.md §4.3 (`modeltap-app::adapters::*`). Currently
//! hosts the cache-path resolver added in
//! tool-model-info-sqlite-cache step 01-04.

pub mod cache_path;
