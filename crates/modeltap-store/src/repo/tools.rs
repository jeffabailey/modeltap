//! ToolsRepo — `cache_tools` reads and writes.
//!
//! Step 01-02 minimum: `write_tool` (UPSERT one row) and `tools()` (read all
//! rows). The richer query surface (per-tool TTL eligibility, partial column
//! updates) lands in Phase 04.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::params;

use modeltap_core::types::ToolId;

use crate::error::CacheError;
use crate::open::Cache;
use crate::types::{CachedTool, SearchPathEntry, SearchPathSource};

impl Cache {
    /// Upsert one `cache_tools` row. Idempotent: re-running with an identical
    /// `CachedTool` is a no-op at the row level (values overwrite themselves).
    pub fn write_tool(&self, tool: &CachedTool) -> Result<(), CacheError> {
        let install_path = path_to_db_text(&tool.install_path);
        let last_scan_at = format_iso8601_utc(&tool.last_scan_at)?;
        let last_error_at = tool
            .last_error_at
            .as_ref()
            .map(format_iso8601_utc)
            .transpose()?;
        let search_paths_json =
            serde_json::to_string(&tool.search_paths).map_err(|e| CacheError::MalformedRow {
                table: "cache_tools",
                detail: format!("serialize search_paths_json: {e}"),
            })?;

        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO cache_tools (
                    tool_id, install_path, detected_version, plugin_version,
                    model_count, disk_usage_bytes, largest_model_id,
                    last_scan_at, last_scan_duration_ms,
                    last_error, last_error_at, search_paths_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT(tool_id) DO UPDATE SET
                    install_path = excluded.install_path,
                    detected_version = excluded.detected_version,
                    plugin_version = excluded.plugin_version,
                    model_count = excluded.model_count,
                    disk_usage_bytes = excluded.disk_usage_bytes,
                    largest_model_id = excluded.largest_model_id,
                    last_scan_at = excluded.last_scan_at,
                    last_scan_duration_ms = excluded.last_scan_duration_ms,
                    last_error = excluded.last_error,
                    last_error_at = excluded.last_error_at,
                    search_paths_json = excluded.search_paths_json",
                params![
                    tool.tool_id.0,
                    install_path,
                    tool.detected_version,
                    tool.plugin_version,
                    tool.model_count as i64,
                    tool.disk_usage_bytes as i64,
                    tool.largest_model_id,
                    last_scan_at,
                    tool.last_scan_duration_ms as i64,
                    tool.last_error,
                    last_error_at,
                    search_paths_json,
                ],
            )?;
            Ok(())
        })
    }

    /// Read all `cache_tools` rows. Order is unspecified (callers sort).
    pub fn tools(&self) -> Result<Vec<CachedTool>, CacheError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT tool_id, install_path, detected_version, plugin_version,
                        model_count, disk_usage_bytes, largest_model_id,
                        last_scan_at, last_scan_duration_ms,
                        last_error, last_error_at, search_paths_json
                 FROM cache_tools",
            )?;
            let rows = stmt
                .query_map([], |row| {
                    Ok(RawToolRow {
                        tool_id: row.get(0)?,
                        install_path: row.get(1)?,
                        detected_version: row.get(2)?,
                        plugin_version: row.get(3)?,
                        model_count: row.get(4)?,
                        disk_usage_bytes: row.get(5)?,
                        largest_model_id: row.get(6)?,
                        last_scan_at: row.get(7)?,
                        last_scan_duration_ms: row.get(8)?,
                        last_error: row.get(9)?,
                        last_error_at: row.get(10)?,
                        search_paths_json: row.get(11)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            rows.into_iter().map(hydrate_tool).collect()
        })
    }
}

struct RawToolRow {
    tool_id: String,
    install_path: String,
    detected_version: Option<String>,
    plugin_version: String,
    model_count: i64,
    disk_usage_bytes: i64,
    largest_model_id: Option<String>,
    last_scan_at: String,
    last_scan_duration_ms: i64,
    last_error: Option<String>,
    last_error_at: Option<String>,
    search_paths_json: String,
}

fn hydrate_tool(raw: RawToolRow) -> Result<CachedTool, CacheError> {
    let tool_id = leak_tool_id(&raw.tool_id);
    let last_scan_at = parse_iso8601_utc(&raw.last_scan_at, "cache_tools.last_scan_at")?;
    let last_error_at = raw
        .last_error_at
        .as_deref()
        .map(|s| parse_iso8601_utc(s, "cache_tools.last_error_at"))
        .transpose()?;
    let search_paths: Vec<SearchPathEntry> =
        serde_json::from_str(&raw.search_paths_json).map_err(|e| CacheError::MalformedRow {
            table: "cache_tools",
            detail: format!("parse search_paths_json: {e}"),
        })?;

    // Defensive: serde_json round-trip of an empty array is `"[]"`, which
    // parses to an empty Vec — that matches the column DEFAULT.
    let _ = SearchPathSource::Default;

    Ok(CachedTool {
        tool_id,
        install_path: PathBuf::from(raw.install_path),
        detected_version: raw.detected_version,
        plugin_version: raw.plugin_version,
        model_count: raw.model_count as u64,
        disk_usage_bytes: raw.disk_usage_bytes as u64,
        largest_model_id: raw.largest_model_id,
        last_scan_at,
        last_scan_duration_ms: raw.last_scan_duration_ms as u64,
        last_error: raw.last_error,
        last_error_at,
        search_paths,
    })
}

/// ToolId wraps `&'static str`. Tool ids are short, stable, and bounded by
/// the registered plugin set; deliberately leaking each unique tool_id text
/// once is the simplest way to satisfy the `'static` requirement without
/// adding an Arc-backed interner. The leak is bounded by the number of
/// distinct plugin tool_ids in the registry (<10 in practice).
fn leak_tool_id(s: &str) -> ToolId {
    // Avoid duplicate leaks for the same string by checking against the
    // small interner below. This is shared with models.rs.
    ToolId(crate::repo::intern::intern_tool_id(s))
}

/// Convert a SystemTime to ISO-8601 UTC with fractional seconds, matching
/// the column convention (`strftime('%Y-%m-%dT%H:%M:%fZ', 'now')`).
pub(crate) fn format_iso8601_utc(t: &SystemTime) -> Result<String, CacheError> {
    let duration = t
        .duration_since(UNIX_EPOCH)
        .map_err(|e| CacheError::MalformedRow {
            table: "timestamp",
            detail: format!("system time before UNIX_EPOCH: {e}"),
        })?;
    let secs = duration.as_secs();
    let millis = duration.subsec_millis() as u16;

    // Hand-rolled ISO-8601 formatter to avoid pulling in chrono. The output
    // matches SQLite's strftime('%Y-%m-%dT%H:%M:%fZ', ...) — seconds with a
    // 3-digit fractional component and a trailing 'Z'.
    let offset = time::OffsetDateTime::from_unix_timestamp(secs as i64).map_err(|e| {
        CacheError::MalformedRow {
            table: "timestamp",
            detail: format!("invalid unix timestamp {secs}: {e}"),
        }
    })?;
    let (year, month, day) = (offset.year(), offset.month() as u8, offset.day());
    let (hour, minute, second) = (offset.hour(), offset.minute(), offset.second());

    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z"
    ))
}

/// Parse an ISO-8601 UTC string produced by `format_iso8601_utc` (or by
/// SQLite's `strftime('%Y-%m-%dT%H:%M:%fZ', ...)`).
pub(crate) fn parse_iso8601_utc(s: &str, column: &'static str) -> Result<SystemTime, CacheError> {
    // Accept both the 3-digit fractional form ("2026-05-17T12:34:56.789Z")
    // and the no-fractional form ("2026-05-17T12:34:56Z") to be forgiving
    // of older rows. The 3-digit form is what we write.
    let (datepart, timepart) = s.split_once('T').ok_or_else(|| CacheError::MalformedRow {
        table: column,
        detail: format!("no 'T' separator: {s}"),
    })?;
    let timepart = timepart
        .strip_suffix('Z')
        .ok_or_else(|| CacheError::MalformedRow {
            table: column,
            detail: format!("no trailing 'Z': {s}"),
        })?;

    let date_parts: Vec<&str> = datepart.split('-').collect();
    if date_parts.len() != 3 {
        return Err(CacheError::MalformedRow {
            table: column,
            detail: format!("invalid date: {datepart}"),
        });
    }
    let year: i32 = date_parts[0]
        .parse()
        .map_err(|e| CacheError::MalformedRow {
            table: column,
            detail: format!("year: {e}"),
        })?;
    let month: u8 = date_parts[1]
        .parse()
        .map_err(|e| CacheError::MalformedRow {
            table: column,
            detail: format!("month: {e}"),
        })?;
    let day: u8 = date_parts[2]
        .parse()
        .map_err(|e| CacheError::MalformedRow {
            table: column,
            detail: format!("day: {e}"),
        })?;

    let (hms, frac_ms) = match timepart.split_once('.') {
        Some((hms, frac)) => {
            let frac_value: u32 = frac.parse().map_err(|e| CacheError::MalformedRow {
                table: column,
                detail: format!("fractional millis: {e}"),
            })?;
            (hms, frac_value)
        }
        None => (timepart, 0),
    };
    let time_parts: Vec<&str> = hms.split(':').collect();
    if time_parts.len() != 3 {
        return Err(CacheError::MalformedRow {
            table: column,
            detail: format!("invalid time: {hms}"),
        });
    }
    let hour: u8 = time_parts[0]
        .parse()
        .map_err(|e| CacheError::MalformedRow {
            table: column,
            detail: format!("hour: {e}"),
        })?;
    let minute: u8 = time_parts[1]
        .parse()
        .map_err(|e| CacheError::MalformedRow {
            table: column,
            detail: format!("minute: {e}"),
        })?;
    let second: u8 = time_parts[2]
        .parse()
        .map_err(|e| CacheError::MalformedRow {
            table: column,
            detail: format!("second: {e}"),
        })?;

    let month_enum = time::Month::try_from(month).map_err(|e| CacheError::MalformedRow {
        table: column,
        detail: format!("month enum: {e}"),
    })?;
    let date = time::Date::from_calendar_date(year, month_enum, day).map_err(|e| {
        CacheError::MalformedRow {
            table: column,
            detail: format!("date: {e}"),
        }
    })?;
    let time_value =
        time::Time::from_hms(hour, minute, second).map_err(|e| CacheError::MalformedRow {
            table: column,
            detail: format!("time: {e}"),
        })?;
    let datetime = time::PrimitiveDateTime::new(date, time_value).assume_utc();
    let unix = datetime.unix_timestamp();
    if unix < 0 {
        return Err(CacheError::MalformedRow {
            table: column,
            detail: format!("pre-epoch unix timestamp {unix}"),
        });
    }
    Ok(UNIX_EPOCH + Duration::from_secs(unix as u64) + Duration::from_millis(frac_ms as u64))
}

/// Render a path to its UTF-8 string form for storage. Lossy paths (non-
/// UTF-8 bytes on Unix) are stored as their lossy projection — the cache
/// is not authoritative on mutate, and unify/zap operations always re-stat
/// the path against the live filesystem.
fn path_to_db_text(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}
