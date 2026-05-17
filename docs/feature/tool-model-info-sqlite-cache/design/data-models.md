# Data Models — tool-model-info-sqlite-cache

**Wave:** DESIGN (3 of 6)
**Author:** Morgan (nw-solution-architect)
**Date:** 2026-05-17

This document specifies the SQLite schema for the `modeltap-store` cache (DDL, indexes, migration path), the in-memory mirror types exposed by the crate, and the rationale for each schema decision. The schema is **v1** (the initial migration `0001_initial.sql`); US-27 will add `cache_sha256` as `0002_add_sha256_persistence.sql` in a future release.

## Design principles

1. **Forward-only, additive, idempotent migrations (C-INFO-6).** No down migrations. New columns are nullable. Destructive changes (rename, type change) are deferred or paired with corruption-recovery rebuild.
2. **WAL journaling enabled at open time.** Concurrent reads + serialized writes per US-23 Scenarios 4-5.
3. **`(mtime, size, inode, dev)` quad on every file row.** This is the pre-mutate revalidation key; it must be stored in a way that supports an exact-match query.
4. **Tool-relevant metadata is plugin-defined.** The schema does not enumerate GGUF KVs / HF config fields; it stores a JSON blob (`metadata_kv_json`) with the plugin's selected subset.
5. **Timestamps as ISO-8601 TEXT (UTC).** Sortable, human-readable, time-zone-agnostic. SQLite's TEXT type stores them efficiently; lexicographic sort = chronological sort.
6. **SHA256 as TEXT.** Lowercase hex; nullable in v1 (US-27 makes this load-bearing). Index is partial (`WHERE sha256 IS NOT NULL`) to keep the index small.

## SQLite DDL (migration `0001_initial.sql`)

```sql
-- 0001_initial.sql
-- Initial cache schema for modeltap (per ADR-015, ADR-017).
-- This migration is applied to a fresh DB OR to any DB with PRAGMA user_version = 0.

-- Set WAL journal mode for concurrent access (per ADR-015 §"Concurrency").
-- NOTE: PRAGMA journal_mode is not part of the schema migration itself; it is set
-- at every connection open. This line documents the intent. The actual PRAGMA is
-- issued by Cache::open() before the migration runs.
-- PRAGMA journal_mode = WAL;
-- PRAGMA busy_timeout = 5000;

CREATE TABLE cache_meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT             NOT NULL
);

-- Seed required meta rows. These keys are referenced by code; missing keys are a bug.
INSERT INTO cache_meta (key, value) VALUES
    ('schema_version_label', 'v1'),
    ('created_at',           strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('last_full_reconcile_at', '');

CREATE TABLE cache_tools (
    tool_id                TEXT PRIMARY KEY NOT NULL,
    install_path           TEXT NOT NULL,
    detected_version       TEXT,                      -- Option<String>; NULL = "(not detectable)"
    plugin_version         TEXT NOT NULL,
    model_count            INTEGER NOT NULL DEFAULT 0,
    disk_usage_bytes       INTEGER NOT NULL DEFAULT 0,
    largest_model_id       TEXT,                      -- FK to cache_models, not enforced (model may have been removed since)
    last_scan_at           TEXT NOT NULL,             -- ISO-8601 UTC
    last_scan_duration_ms  INTEGER NOT NULL DEFAULT 0,
    last_error             TEXT,                      -- NULL when no error
    last_error_at          TEXT,                      -- ISO-8601 UTC; NULL when no error
    search_paths_json      TEXT NOT NULL DEFAULT '[]' -- JSON array of {path, source: "default"|"user_config"}
);

CREATE INDEX idx_cache_tools_last_scan_at
    ON cache_tools(last_scan_at);

CREATE TABLE cache_models (
    model_id                   TEXT NOT NULL,         -- plugin-specified; unique within a tool
    tool_id                    TEXT NOT NULL,
    display_name               TEXT NOT NULL,
    format                     TEXT,                  -- "GGUF v3", "Ollama manifest v2", "safetensors v2"
    quantisation               TEXT,                  -- "Q4_K_M"
    size_bytes                 INTEGER NOT NULL DEFAULT 0,
    sha256                     TEXT,                  -- lowercase hex; NULL until computed
    architecture               TEXT,
    parameters_billions        REAL,                  -- e.g. 7.24
    context_length             INTEGER,
    dedup_group_id             TEXT,                  -- NULL until grouping computed; same sha256 prefix or repo+quant
    metadata_kv_json           TEXT,                  -- JSON object; plugin-selected KVs; NULL when un-introspectable
    metadata_introspected_at   TEXT,                  -- ISO-8601 UTC; NULL when never introspected
    last_seen_at               TEXT NOT NULL,         -- ISO-8601 UTC; updated on every reconcile
    last_validated_at          TEXT,                  -- ISO-8601 UTC; updated on pre-mutate revalidation
    PRIMARY KEY (model_id, tool_id),
    FOREIGN KEY (tool_id) REFERENCES cache_tools(tool_id) ON DELETE CASCADE
);

CREATE INDEX idx_cache_models_tool_last_seen
    ON cache_models(tool_id, last_seen_at);

CREATE INDEX idx_cache_models_sha256
    ON cache_models(sha256)
    WHERE sha256 IS NOT NULL;

CREATE INDEX idx_cache_models_dedup_group
    ON cache_models(dedup_group_id)
    WHERE dedup_group_id IS NOT NULL;

CREATE TABLE cache_model_files (
    file_id        INTEGER PRIMARY KEY AUTOINCREMENT,
    model_id       TEXT    NOT NULL,
    tool_id        TEXT    NOT NULL,
    path           TEXT    NOT NULL,                  -- absolute path on the local filesystem
    size_bytes     INTEGER NOT NULL,
    mtime_epoch_ns INTEGER NOT NULL,                  -- nanoseconds since UNIX epoch; the M of the quad
    inode          INTEGER NOT NULL,                  -- the I of the quad
    dev            INTEGER NOT NULL,                  -- the D of the quad
    last_stat_at   TEXT    NOT NULL,                  -- ISO-8601 UTC of last stat() that produced these values
    FOREIGN KEY (model_id, tool_id) REFERENCES cache_models(model_id, tool_id) ON DELETE CASCADE
);

CREATE INDEX idx_cache_model_files_model
    ON cache_model_files(model_id, tool_id);

CREATE UNIQUE INDEX idx_cache_model_files_path
    ON cache_model_files(path);

-- Bump user_version to 1 at the end of the migration.
-- rusqlite_migration handles this automatically based on filename ordering; the
-- comment below documents intent for code reviewers.
-- PRAGMA user_version = 1;
```

### Column-type rationale

| Column / decision | Chosen type | Alternative | Rationale |
|---|---|---|---|
| `sha256` | TEXT (hex) | BLOB (raw bytes) | TEXT is debuggable in `sqlite3` CLI; size penalty is 64 bytes vs 32 bytes, irrelevant at our row count; SQLite stores TEXT efficiently |
| Timestamps | TEXT (ISO-8601 UTC) | INTEGER (epoch seconds/ms) | Human-readable in `sqlite3` CLI; lexicographic sort = chronological; matches `strftime('%Y-%m-%dT%H:%M:%fZ', 'now')` |
| `mtime` | INTEGER (epoch nanoseconds) | TEXT ISO-8601 | mtime is a stat-derived value compared exactly; nanosecond integer matches the underlying `Metadata::modified()` semantics and avoids ISO-8601 round-trip precision loss |
| `inode`, `dev` | INTEGER | TEXT pair | Stat returns these as u64; INTEGER (SQLite's NUMERIC affinity) handles them directly |
| `metadata_kv_json` | TEXT (JSON) | Separate KV table | Q-INFO-8 closed by DISCUSS: JSON column for v1. Tool-relevant subset is plugin-defined and not queried; a relational schema would be over-engineered. Migrate to relational if/when queries demand it |
| `search_paths_json` | TEXT (JSON) | Separate `cache_tool_search_paths` table | Same rationale; small list per tool, never queried independently |
| `dedup_group_id` | TEXT, nullable | Derived at read time | Stored to support future "show me all dedup groups" queries; nullable because grouping is computed lazily |

### Index rationale

| Index | Query supported | Cardinality |
|---|---|---|
| `idx_cache_tools_last_scan_at` | TTL eligibility check at warm-start | small (one row per tool, ~5-10 rows) |
| `idx_cache_models_tool_last_seen` | `SELECT * FROM cache_models WHERE tool_id = ?` (warm-start models-for-tool query) | medium (10-100s per tool) |
| `idx_cache_models_sha256` (partial) | dedup group computation on SHA256 match | medium; partial keeps index size proportional to hashed-models, not total-models |
| `idx_cache_models_dedup_group` (partial) | "all members of a dedup group" reads when opening unify dialog | small |
| `idx_cache_model_files_model` | files-for-model lookup at pre-mutate revalidation | small (1-N per model, typically 1-3) |
| `idx_cache_model_files_path` UNIQUE | revalidator's "is this path still known to the cache?" check; also catches accidental duplicate-file inserts | medium |

## In-memory mirror types (in `modeltap-store::types`)

These are the typed row representations the crate's public API exchanges with `modeltap-app`. Keep these types **pure data** — no methods beyond Serde derives and trivial constructors.

```rust
// types.rs (illustrative; software-crafter owns final shape)

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedTool {
    pub tool_id: ToolId,
    pub install_path: PathBuf,
    pub detected_version: Option<String>,
    pub plugin_version: String,
    pub model_count: u64,
    pub disk_usage_bytes: u64,
    pub largest_model_id: Option<ModelId>,
    pub last_scan_at: SystemTime,
    pub last_scan_duration_ms: u64,
    pub last_error: Option<String>,
    pub last_error_at: Option<SystemTime>,
    pub search_paths: Vec<SearchPathEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchPathEntry {
    pub path: PathBuf,
    pub source: SearchPathSource, // Default | UserConfig
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedModel {
    pub model_id: ModelId,
    pub tool_id: ToolId,
    pub display_name: String,
    pub format: Option<String>,
    pub quantisation: Option<String>,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub architecture: Option<String>,
    pub parameters_billions: Option<f64>,
    pub context_length: Option<u32>,
    pub dedup_group_id: Option<String>,
    pub metadata_kv: BTreeMap<String, String>,
    pub metadata_introspected_at: Option<SystemTime>,
    pub last_seen_at: SystemTime,
    pub last_validated_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedFile {
    pub model_id: ModelId,
    pub tool_id: ToolId,
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime: SystemTime,           // serialized as epoch_ns at the SQL boundary
    pub inode: u64,
    pub dev: u64,
    pub last_stat_at: SystemTime,
}

/// Result of pre-mutate revalidation. The composition root inspects this and
/// decides next steps (proceed, refresh dialog, or abort).
#[derive(Debug, Clone, PartialEq)]
pub enum ValidationResult {
    Match,
    Drift { fresh: FileStat },
    Gone,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileStat {
    pub size_bytes: u64,
    pub mtime: SystemTime,
    pub inode: u64,
    pub dev: u64,
}
```

## Migration v0 → v1 (the initial migration)

A fresh cache file has `PRAGMA user_version = 0` (SQLite default). `0001_initial.sql` creates all tables, inserts seed `cache_meta` rows, and `rusqlite_migration` bumps `user_version` to `1` at the end.

### Idempotency check

The migration is naturally idempotent because every `CREATE TABLE` and `CREATE INDEX` would fail if run twice — but `rusqlite_migration` is responsible for **not running** the migration when `user_version >= 1` already. Tests under `crates/modeltap-store/tests/migration.rs` exercise:

1. **Fresh DB:** `user_version = 0` → migration runs → `user_version = 1`.
2. **Already-migrated DB:** `user_version = 1` → migration does NOT run → no-op.
3. **Partial migration failure (simulated):** kill the SQL halfway → next open detects the partial state via missing `cache_tools` table → routes to recovery (rename to `.corrupt-<ts>`, cold-start).

### Future migrations (illustrative; not part of v1)

| File | Purpose | Triggered by |
|---|---|---|
| `0002_add_sha256_persistence.sql` | Adds `cache_sha256` table (path → content_hash + stat quad) | US-27 (Release 3, deferred) |
| `0003_add_per_tool_ttl_override.sql` | Adds `cache_tools.ttl_seconds_override INTEGER` for Q-INFO-4 optional per-tool TTL | Post-Release-2 enhancement, if user demand |

These files do not exist yet; they are listed to demonstrate that the migration framework is designed for forward extension.

## Constants in `modeltap-store`

```rust
// modeltap-store/src/migrate.rs
pub const EXPECTED_SCHEMA_VERSION: u32 = 1;
```

Bumped in lockstep with each new `migrations/NNNN_*.sql` file. The constant is the source of truth for "what version of the schema does this binary expect?"

## Storage estimates

| User profile | Models | Files | DB size (est.) |
|---|---|---|---|
| Light user | 30 | 50 | ~50 KB |
| Typical Devon | 200 | 300 | ~500 KB |
| Power user | 1000 | 1500 | ~5 MB |
| Heaviest plausible | 5000 | 8000 | ~25 MB |

The size budget per US-23 NFR is "≤1 MB typical, ≤5 MB power user." The estimates above (driven mostly by `metadata_kv_json` and `path` strings) confirm the budget holds. WAL files add ephemeral overhead during writes; SQLite checkpoints them on close.

## Backup strategy

**None in v1.** Corruption recovery (rename to `.corrupt-<ts>`) is the safety net. The cache is rebuildable from filesystem state via cold-start; no irreplaceable data lives in it.

If users request backups in a future release, the seam is: `modeltap cache export <path>` subcommand under the future `modeltap cache <subcommand>` family (deferred with US-27).
