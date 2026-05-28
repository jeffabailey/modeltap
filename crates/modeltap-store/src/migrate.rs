//! Schema migration runner (per ADR-017).
//!
//! Wraps `rusqlite_migration::Migrations`. Migrations are embedded at
//! compile-time via `include_str!`; the runner applies each in filename
//! order until `PRAGMA user_version` matches `EXPECTED_SCHEMA_VERSION`.

use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

use crate::error::CacheError;

/// The schema version this binary expects. Bumped in lockstep with each
/// `migrations/NNNN_*.sql` file. The cache is opened with this value as the
/// migration target; if the on-disk `user_version` is lower, the migrator
/// runs forward; if it is higher (downgrade), the recovery path renames the
/// file and starts fresh (lands in a follow-up step).
pub const EXPECTED_SCHEMA_VERSION: u32 = 2;

/// Embedded v1 migration. `include_str!` resolves at compile time; the file
/// must be present in the published crate sources.
const MIGRATION_0001_INITIAL: &str = include_str!("../migrations/0001_initial.sql");

/// Embedded v2 migration — adds the file-level `cache_sha256` table for US-27
/// SHA256 persistence (ADR-018). Purely additive over v1.
const MIGRATION_0002_SHA256: &str = include_str!("../migrations/0002_add_sha256_persistence.sql");

/// Build the `rusqlite_migration::Migrations` chain for this crate's
/// embedded SQL. Returns a fresh `Migrations` each call so the caller owns
/// it for the duration of one connection. Order is load-bearing: each `M::up`
/// advances `user_version` by one, so 0001 MUST precede 0002.
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(MIGRATION_0001_INITIAL),
        M::up(MIGRATION_0002_SHA256),
    ])
}

/// Run migrations forward to `EXPECTED_SCHEMA_VERSION` on the given
/// connection. Idempotent: if `user_version` already equals
/// `EXPECTED_SCHEMA_VERSION` the migrator is a no-op.
pub(crate) fn migrate_to_latest(conn: &mut Connection) -> Result<(), CacheError> {
    migrations()
        .to_latest(conn)
        .map_err(|source| CacheError::Migration {
            detail: source.to_string(),
        })?;
    Ok(())
}
