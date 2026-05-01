//! `DedupGlyph` — the dedup-state column glyph rendered next to each row in
//! the right pane.
//!
//! This is the *dedup* glyph and is intentionally separate from
//! `domain::indicator::RowIndicator`, which is the *compatibility* glyph
//! (`o`/`*`/`!`/`?`). Two enums avoid a single overloaded one (per
//! `docs/feature/cross-tool-model-unify/design/component-boundaries.md`).
//!
//! Pure data — no logic, no I/O. The classifier that decides which glyph a
//! row gets lives in `logic::dedup` and lands in step 01-02.
//!
//! Source of truth for the variant shape:
//! `docs/feature/cross-tool-model-unify/design/data-models.md`.

use serde::Serialize;

/// The dedup-state column glyph for a single right-pane row.
///
/// Variants are ordered to read top-down through the lifecycle of a row's
/// dedup state — pre-hash → hashing → terminal classification.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize)]
pub enum DedupGlyph {
    /// `?` — no hash yet, no worker assigned.
    Pending,
    /// `~` — a worker is currently hashing this file.
    Hashing,
    /// `-` (without decorator) — unique content; no peer has a matching hash.
    Unique,
    /// `-` with `!` decorator — hashing failed (read error, IO error).
    /// Conservative-when-uncertain (BR-3): treated as Unique for action purposes.
    Failed,
    /// `=` — ≥2 separate inodes share the same SHA256.
    DedupAble,
    /// `#` — ≥2 paths already share one inode.
    AlreadyUnified,
}
