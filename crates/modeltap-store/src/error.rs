//! Error model for modeltap-store.
//!
//! Per ADR-007: `thiserror` in domain crates. Each variant maps to a
//! caller-actionable failure mode; the composition root (`modeltap-app`)
//! decides how to surface it (recovery banner, log line, abort).

use std::io;
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CacheError {
    /// SQLite returned an error opening, querying, or writing.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// The migrator failed mid-run. The connection is left in whatever state
    /// rusqlite_migration left it; recovery (rename + cold-start) is the
    /// caller's responsibility.
    #[error("migration failed: {detail}")]
    Migration { detail: String },

    /// Failed to create the parent directory of the cache path, or to open
    /// the file itself.
    #[error("io error at {path:?}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// A row had a malformed value (e.g., a non-parseable timestamp, an
    /// invalid JSON column). Surfaces as a recovery trigger upstream.
    #[error("malformed row in {table}: {detail}")]
    MalformedRow { table: &'static str, detail: String },
}
