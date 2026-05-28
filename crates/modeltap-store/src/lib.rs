//! modeltap-store — SQLite-backed inventory cache (per ADR-015 / ADR-017).
//!
//! Step 01-02 of `tool-model-info-sqlite-cache`. Owns:
//!
//! - `Cache::open(path)` / `Cache::open_in_memory()` — opens (or creates) the
//!   SQLite file at `path` with WAL + busy_timeout=5000 and runs migrations
//!   forward to [`EXPECTED_SCHEMA_VERSION`].
//! - The minimum CRUD surface for `cache_tools` (`write_tool`, `tools`) and
//!   `cache_models` (`write_models`, `models_for_tool`). Full repo surface
//!   (files, meta, revalidation) lands in Phase 04.
//! - The embedded v1 migration at `migrations/0001_initial.sql`.
//!
//! Architecture rules:
//! - R7: only `modeltap-app` may depend on this crate.
//! - R8: this crate MUST NOT depend on `tokio` or `ratatui` (the cache is
//!   sync; the composition root bridges via `tokio::task::spawn_blocking`).

#![forbid(unsafe_code)]

mod error;
mod migrate;
mod open;
pub mod recovery;
mod repo;
mod revalidate;

pub mod types;

pub use error::CacheError;
pub use migrate::EXPECTED_SCHEMA_VERSION;
pub use open::{Cache, CacheOpenResult};
pub use recovery::RecoveryReason;
pub use revalidate::stat_file_quad;
