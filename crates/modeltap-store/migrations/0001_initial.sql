-- 0001_initial.sql
-- Initial cache schema for modeltap (per ADR-015, ADR-017 and data-models.md).
-- This migration is applied to a fresh DB OR to any DB with PRAGMA user_version = 0.
--
-- PRAGMA journal_mode = WAL and PRAGMA busy_timeout = 5000 are NOT part of the
-- migration itself; they are connection-level settings issued by Cache::open()
-- before the migrator runs. See open.rs.
--
-- rusqlite_migration sets PRAGMA user_version = 1 automatically after the SQL
-- below applies cleanly.

CREATE TABLE cache_meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT             NOT NULL
);

-- Seed required meta rows. Missing keys are a bug; callers may assume presence.
INSERT INTO cache_meta (key, value) VALUES
    ('schema_version_label',     'v1'),
    ('created_at',               strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    ('last_full_reconcile_at',   '');

CREATE TABLE cache_tools (
    tool_id                TEXT PRIMARY KEY NOT NULL,
    install_path           TEXT NOT NULL,
    detected_version       TEXT,
    plugin_version         TEXT NOT NULL,
    model_count            INTEGER NOT NULL DEFAULT 0,
    disk_usage_bytes       INTEGER NOT NULL DEFAULT 0,
    largest_model_id       TEXT,
    last_scan_at           TEXT NOT NULL,
    last_scan_duration_ms  INTEGER NOT NULL DEFAULT 0,
    last_error             TEXT,
    last_error_at          TEXT,
    search_paths_json      TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX idx_cache_tools_last_scan_at
    ON cache_tools(last_scan_at);

CREATE TABLE cache_models (
    model_id                   TEXT NOT NULL,
    tool_id                    TEXT NOT NULL,
    display_name               TEXT NOT NULL,
    format                     TEXT,
    quantisation               TEXT,
    size_bytes                 INTEGER NOT NULL DEFAULT 0,
    sha256                     TEXT,
    architecture               TEXT,
    parameters_billions        REAL,
    context_length             INTEGER,
    dedup_group_id             TEXT,
    metadata_kv_json           TEXT,
    metadata_introspected_at   TEXT,
    last_seen_at               TEXT NOT NULL,
    last_validated_at          TEXT,
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
    path           TEXT    NOT NULL,
    size_bytes     INTEGER NOT NULL,
    mtime_epoch_ns INTEGER NOT NULL,
    inode          INTEGER NOT NULL,
    dev            INTEGER NOT NULL,
    last_stat_at   TEXT    NOT NULL,
    FOREIGN KEY (model_id, tool_id) REFERENCES cache_models(model_id, tool_id) ON DELETE CASCADE
);

CREATE INDEX idx_cache_model_files_model
    ON cache_model_files(model_id, tool_id);

CREATE UNIQUE INDEX idx_cache_model_files_path
    ON cache_model_files(path);
