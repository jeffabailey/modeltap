-- 0002_add_sha256_persistence.sql
-- Adds the file-level SHA256 persistence table for US-27 (Release 3).
-- Per ADR-018 §"Release 3 (US-27, deferred — opt-in)" and ADR-017.
--
-- PURELY ADDITIVE: this migration creates one new table + one index. It does
-- NOT alter cache_models, cache_tools, or cache_model_files. The model-level
-- cache_models.sha256 column (Tier 2, shipped in 0001) remains the denormalized
-- warm-paint fast path; this table is the file-level source of truth (Tier 3).
--
-- rusqlite_migration sets PRAGMA user_version = 2 automatically after this
-- applies cleanly.

-- File-level content hash keyed by absolute path. The (mtime_epoch_ns,
-- size_bytes, inode, dev) quad is the validity fingerprint: a SHA256 entry is
-- only trusted when a fresh stat of the path still matches all four fields
-- (FileStat::matches). content_hash is lowercase hex. computed_at is ISO-8601
-- UTC and drives the "dedup key computed N days ago" provenance line.
CREATE TABLE cache_sha256 (
    path           TEXT PRIMARY KEY NOT NULL,
    mtime_epoch_ns INTEGER NOT NULL,
    size_bytes     INTEGER NOT NULL,
    inode          INTEGER NOT NULL,
    dev            INTEGER NOT NULL,
    content_hash   TEXT    NOT NULL,
    computed_at    TEXT    NOT NULL
);

-- Hardlink dedup means the same (inode, dev) may map to multiple paths after a
-- unify. This index lets the SHA256 lookup short-circuit by physical identity
-- without a full table scan when resolving a hardlinked path.
CREATE INDEX idx_cache_sha256_inode_dev
    ON cache_sha256(inode, dev);
