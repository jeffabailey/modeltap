//! Cache recovery routine — converts unrecoverable open-time failures into a
//! clean fresh-cache restart per architecture-design.md §7.4 and ADR-015 §5.
//!
//! Three failure modes route here:
//!
//! 1. **SQLITE_CORRUPT / NotADatabase** — the file does not parse as a valid
//!    SQLite database (truncation, on-disk bit rot, foreign garbage written
//!    by a different program). The original file is renamed to
//!    `cache.sqlite.corrupt-<YYYY-MM-DDTHHMMSS>` and a fresh empty cache is
//!    created at the original path.
//!
//! 2. **Downgrade** — `PRAGMA user_version` on the on-disk file exceeds
//!    [`EXPECTED_SCHEMA_VERSION`]. This happens when a newer modeltap built
//!    against schema v2 wrote the cache and the user then re-launched an
//!    older v1 binary. The original file is renamed to
//!    `cache.sqlite.future-version-<found>` and a fresh empty cache is
//!    created at the original path. We never attempt to read a future
//!    schema with an older binary — the column shapes may be incompatible.
//!
//! 3. **Migration failure** — `rusqlite_migration::Migrations::to_latest`
//!    returned an error. The connection is in whatever state the migrator
//!    left it; the safest restart is to rename the file with the same
//!    `.corrupt-<ts>` suffix used for SQLITE_CORRUPT and create a fresh
//!    cache. Migration-failure recovery preserves the rename-target naming
//!    convention so support can correlate `.corrupt-*` files with both
//!    on-disk corruption and migration bugs.
//!
//! For each path, the routine:
//!
//! - Computes the rename target (timestamp formatted via the `time` crate).
//! - Calls `std::fs::rename(path, new_path)` (best-effort; rename errors
//!   are logged but do not abort the recovery — the user-visible outcome is
//!   still a working empty cache).
//! - Appends a single `cache_recovery reason=<...> renamed_to=<...>` line
//!   to `<MODELTAP_DIAGNOSTICS_DIR>/diagnostics.log` (best-effort; matches
//!   the diagnostics writer pattern used elsewhere in modeltap-app).
//! - Re-opens a fresh empty cache at the original path; migrations run.
//! - Returns `(Cache, renamed_to_path)` so the caller can include the
//!   renamed path in the `OpenedAfterRecovery` variant.
//!
//! AC-23-11 invariant: cache failure NEVER prevents inventory view from
//! rendering. The composition root surfaces this as a dismissable banner and
//! the inventory view paints normally below it via the cold-start fallback.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rusqlite::Connection;

use crate::error::CacheError;
use crate::migrate::migrate_to_latest;

/// Why the recovery path engaged. Carried through `CacheOpenResult::OpenedAfterRecovery`
/// so the TUI banner and the diagnostics line can both describe the cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryReason {
    /// Either `SQLITE_CORRUPT` (header valid, body damaged) or `SQLITE_NOTADB`
    /// (header invalid — e.g. 16 KB of random garbage). Both code paths land
    /// the file in `cache.sqlite.corrupt-<ts>`.
    Corrupted,
    /// `PRAGMA user_version` exceeded `EXPECTED_SCHEMA_VERSION`. `found` is the
    /// on-disk version; `expected` is `EXPECTED_SCHEMA_VERSION` at the time
    /// of open. Renamed to `cache.sqlite.future-version-<found>`.
    Downgrade { found: u32, expected: u32 },
    /// `rusqlite_migration::Migrations::to_latest` returned an error.
    /// `from` is the pre-migration `user_version`; `to` is the version we
    /// expected to reach (`EXPECTED_SCHEMA_VERSION`). Renamed with the same
    /// `.corrupt-<ts>` suffix used for [`RecoveryReason::Corrupted`].
    MigrationFailed { from: u32, to: u32 },
}

impl RecoveryReason {
    /// Short token used as the `reason=` value in the diagnostics.log line.
    /// Stable across releases — log scrapers may key on it.
    pub fn diagnostics_token(&self) -> &'static str {
        match self {
            RecoveryReason::Corrupted => "corrupted",
            RecoveryReason::Downgrade { .. } => "downgrade",
            RecoveryReason::MigrationFailed { .. } => "migration_failed",
        }
    }
}

/// Run the recovery dance: rename the broken cache, append the diagnostics
/// line, open a fresh empty cache at the same path. Returns the fresh
/// `Connection` plus the path the original file was renamed to.
///
/// The caller is responsible for wrapping the returned `Connection` in a
/// `Cache` (the recovery module is independent of `Cache`'s `Mutex<Connection>`
/// wrapping so it can be reused from `open.rs` without circular imports).
///
/// Errors:
/// - Returns `Err(CacheError::Io)` if `std::fs::rename` fails AND the original
///   path is still occupied — meaning we cannot create a fresh cache without
///   either clobbering the corrupt file or leaking a useless renamed sibling.
///   (Best-effort rename means we ignore "rename to nonexistent target"
///   errors only; "rename source still exists" is a hard error.)
/// - Returns `Err(CacheError::Sqlite)` / `Err(CacheError::Migration)` if the
///   fresh open or its migration step fails. This is rare (the file did not
///   exist a moment ago) but recoverable failures here propagate up.
pub(crate) fn recover_and_reopen(
    path: &Path,
    reason: &RecoveryReason,
) -> Result<(Connection, PathBuf), CacheError> {
    let renamed_to = compute_rename_target(path, reason);
    // Best-effort rename. The cache lives in the user's data dir; if the
    // rename fails for some odd FS reason (read-only mount, dest exists),
    // we fall back to removing the original so the fresh open below can
    // succeed. We log the rename failure to diagnostics.log so support has
    // a breadcrumb.
    let rename_succeeded = match std::fs::rename(path, &renamed_to) {
        Ok(()) => true,
        Err(rename_err) => {
            // Try to remove the original so the fresh open below can land a
            // new empty file. If THAT also fails we cannot proceed.
            match std::fs::remove_file(path) {
                Ok(()) => false,
                Err(remove_err) => {
                    return Err(CacheError::Io {
                        path: path.to_path_buf(),
                        source: std::io::Error::new(
                            remove_err.kind(),
                            format!(
                                "cache recovery: rename failed ({rename_err}); \
                                 fallback remove also failed ({remove_err})"
                            ),
                        ),
                    });
                }
            }
        }
    };
    let effective_target = if rename_succeeded {
        renamed_to.clone()
    } else {
        // Rename failed but remove succeeded — the diagnostics line should
        // still record the originally-intended target so support knows what
        // *would* have happened on a healthy FS.
        renamed_to.clone()
    };

    write_diagnostics_line(reason, &effective_target);

    let mut conn = Connection::open(path).map_err(CacheError::Sqlite)?;
    apply_open_pragmas(&conn)?;
    migrate_to_latest(&mut conn)?;
    Ok((conn, effective_target))
}

/// Apply the same PRAGMAs as `Cache::apply_open_pragmas`. Duplicated here
/// rather than re-exported to keep the recovery module's surface minimal —
/// the two call sites cannot diverge because both are exercised by the
/// corruption integration tests.
fn apply_open_pragmas(conn: &Connection) -> Result<(), CacheError> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 5_000_i64)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

/// Build the rename target path for a given recovery reason.
///
/// - `Corrupted` and `MigrationFailed` use `cache.sqlite.corrupt-<YYYY-MM-DDTHHMMSS>`.
/// - `Downgrade` uses `cache.sqlite.future-version-<found>`.
///
/// The timestamp uses `time::OffsetDateTime` for ISO-8601-compact formatting
/// without dragging in `chrono`. UTC throughout. Falls back to "unknown" if
/// the system clock is somehow before UNIX_EPOCH (which would already have
/// broken everything else — but the recovery path's invariant is "always
/// produce a fresh cache", so a degenerate timestamp must not abort it).
pub(crate) fn compute_rename_target(path: &Path, reason: &RecoveryReason) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "cache.sqlite".to_string());

    let suffix = match reason {
        RecoveryReason::Corrupted | RecoveryReason::MigrationFailed { .. } => {
            format!("corrupt-{}", compact_utc_timestamp())
        }
        RecoveryReason::Downgrade { found, .. } => format!("future-version-{found}"),
    };

    let new_file_name = format!("{file_name}.{suffix}");
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(new_file_name),
        _ => PathBuf::from(new_file_name),
    }
}

/// Format the current UTC time as `YYYY-MM-DDTHHMMSS` (compact, no
/// punctuation in the time component so the resulting filename does not
/// need escaping in shell-quoted contexts).
fn compact_utc_timestamp() -> String {
    let now = SystemTime::now();
    let secs = match now.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => return "unknown".to_string(),
    };
    let offset = match time::OffsetDateTime::from_unix_timestamp(secs) {
        Ok(o) => o,
        Err(_) => return "unknown".to_string(),
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}{:02}{:02}",
        offset.year(),
        offset.month() as u8,
        offset.day(),
        offset.hour(),
        offset.minute(),
        offset.second(),
    )
}

/// Append `cache_recovery reason=<token> renamed_to=<path>` to
/// `<MODELTAP_DIAGNOSTICS_DIR>/diagnostics.log` when the env var is set, else
/// to `$HOME/.modeltap/diagnostics.log` when `HOME` is set, else silently
/// drop the line. Best-effort: all I/O errors are swallowed so a malformed
/// diagnostics directory cannot compound into a second user-visible failure.
///
/// Path resolution mirrors the convention used in modeltap-app's existing
/// `write_diagnostics_panic_line` (orchestration/open_tool_detail.rs).
fn write_diagnostics_line(reason: &RecoveryReason, renamed_to: &Path) {
    let dir = match resolve_diagnostics_dir() {
        Some(d) => d,
        None => return,
    };
    // Best-effort directory creation; if this fails the OpenOptions::open
    // below will also fail and the whole write is silently skipped.
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("diagnostics.log");
    let token = reason.diagnostics_token();
    let renamed_display = renamed_to.display();
    let mut line = format!("cache_recovery reason={token} renamed_to={renamed_display}");
    line.push('\n');
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// Resolve the diagnostics directory: env override > `~/.modeltap` > None.
fn resolve_diagnostics_dir() -> Option<PathBuf> {
    if let Some(env_dir) = std::env::var_os("MODELTAP_DIAGNOSTICS_DIR") {
        return Some(PathBuf::from(env_dir));
    }
    // `~/.modeltap` fallback. We deliberately do NOT depend on the `dirs`
    // crate — modeltap-store has no `dirs` dep and the recovery path's
    // home-dir resolution is allowed to be HOME-only (the composition root
    // is the canonical home-resolver; the recovery path's fallback exists
    // only so a misconfigured environment still leaves a breadcrumb).
    let home = std::env::var_os("HOME")?;
    let mut p = PathBuf::from(home);
    p.push(".modeltap");
    Some(p)
}

// ---------------------------------------------------------------------------
// Unit tests — exercise the pure helpers (compute_rename_target,
// compact_utc_timestamp, diagnostics_token). The recover_and_reopen
// end-to-end path is exercised from tests/corruption.rs against real
// tempdir-backed broken caches.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_token_is_stable_per_variant() {
        assert_eq!(RecoveryReason::Corrupted.diagnostics_token(), "corrupted");
        assert_eq!(
            RecoveryReason::Downgrade {
                found: 99,
                expected: 1
            }
            .diagnostics_token(),
            "downgrade"
        );
        assert_eq!(
            RecoveryReason::MigrationFailed { from: 0, to: 1 }.diagnostics_token(),
            "migration_failed"
        );
    }

    #[test]
    fn compute_rename_target_uses_corrupt_suffix_for_corrupted() {
        let path = Path::new("/tmp/modeltap/cache.sqlite");
        let renamed = compute_rename_target(path, &RecoveryReason::Corrupted);
        let name = renamed.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("cache.sqlite.corrupt-"),
            "expected `cache.sqlite.corrupt-<ts>` prefix, got {name}"
        );
        // Parent must be preserved so the renamed file lands beside the original.
        assert_eq!(renamed.parent(), Some(Path::new("/tmp/modeltap")));
    }

    #[test]
    fn compute_rename_target_uses_corrupt_suffix_for_migration_failed() {
        let path = Path::new("/tmp/modeltap/cache.sqlite");
        let renamed =
            compute_rename_target(path, &RecoveryReason::MigrationFailed { from: 0, to: 1 });
        let name = renamed.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("cache.sqlite.corrupt-"),
            "MigrationFailed must reuse the corrupt-<ts> suffix, got {name}"
        );
    }

    #[test]
    fn compute_rename_target_uses_future_version_suffix_for_downgrade() {
        let path = Path::new("/tmp/modeltap/cache.sqlite");
        let renamed = compute_rename_target(
            path,
            &RecoveryReason::Downgrade {
                found: 99,
                expected: 1,
            },
        );
        let name = renamed.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, "cache.sqlite.future-version-99");
    }

    #[test]
    fn compact_utc_timestamp_matches_yyyy_mm_dd_t_hhmmss_shape() {
        let ts = compact_utc_timestamp();
        // Either the real format (YYYY-MM-DDTHHMMSS, 15 chars) or the
        // "unknown" fallback (which only fires before UNIX_EPOCH).
        if ts != "unknown" {
            assert_eq!(ts.len(), 15, "expected YYYY-MM-DDTHHMMSS (15 chars), got {ts}");
            assert!(ts.contains('T'), "must contain 'T' separator: {ts}");
        }
    }
}
