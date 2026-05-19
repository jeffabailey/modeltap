//! Per-tool detail screen (US-21). Pure view-model + pure render fn.
//!
//! Pressing Enter on a left-pane row in the main view opens this screen. It
//! shows (per AC-21-2):
//!
//! - Discovery root (the tool's install path)
//! - Version — `"(not detectable)"` when the plugin returned `None` (AC-21-3)
//! - Search paths — each tagged "(default)" or "(user config)" (AC-21-5)
//! - Model count
//! - Disk usage
//! - Largest model
//! - Last scan (ISO timestamp; the "(N min ago)" suffix lands later)
//! - Scan duration (ms)
//! - Last error — `"(none)"` when absent (AC-21-4)
//! - Plugin version
//!
//! The bottom bar (rendered by the shared `bottom_bar` module) carries
//! `[Esc] back`, `[r] refresh this tool`, `[?] help` (AC-21-8).
//!
//! Per ADR-006 the view layer is pure: this module reads
//! `&ToolDetailScreenState` and writes ratatui widgets. No I/O, no mutation.
//! The orchestrator (`modeltap-app::orchestration::open_tool_detail`) composes
//! cache + `inspect_tool()` results into a `ToolDetail`, then dispatches
//! `Msg::ToolDetailReady(detail)` which `update` lifts into the screen.

use std::time::SystemTime;

use modeltap_core::domain::inspect::{SearchPathSource, ToolDetail};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app_state::AppState;
use crate::render::bottom_bar::{render_bottom_bar, BarContext};
use crate::render::bytes::format_bytes;

/// Pure state for the tool-detail screen. Constructed by `update()` from
/// `Msg::ToolDetailReady(detail)`. The `Box<ToolDetailScreenState>` indirection
/// in `Screen::ToolDetail` keeps the `Screen` enum compact even though
/// `ToolDetail` carries ~10 owned fields.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDetailScreenState {
    pub detail: ToolDetail,
}

impl ToolDetailScreenState {
    pub fn new(detail: ToolDetail) -> Self {
        Self { detail }
    }
}

// ---------------------------------------------------------------------------
// Sentinel constants — load-bearing for AC-21-3 / AC-21-4 assertions.
// ---------------------------------------------------------------------------

/// Rendered in the `Version:` field when `detected_version` is `None`
/// (AC-21-3). Public so step definitions can assert against the literal
/// without duplicating the string.
pub const VERSION_NOT_DETECTABLE: &str = "(not detectable)";

/// Rendered in the `Last error:` field when `last_error` is `None`
/// (AC-21-4). Public for the same reason as `VERSION_NOT_DETECTABLE`.
pub const LAST_ERROR_NONE: &str = "(none)";

/// Search-path provenance labels (AC-21-5). Public so step definitions
/// can grep for them.
pub const SEARCH_PATH_DEFAULT_LABEL: &str = "(default)";
pub const SEARCH_PATH_USER_CONFIG_LABEL: &str = "(user config)";

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

/// Render the tool detail screen into `area`. Vertical layout:
///
/// ```text
/// ┌─ Tool: <tool_id> ─────────────────────────────────┐
/// │ Discovery root: <install_path>                    │
/// │ Version:        <detected_version OR "(not...)">  │
/// │ Plugin version: <plugin_version>                  │
/// │ Model count:    <n>                               │
/// │ Disk usage:     <bytes>                           │
/// │ Largest model:  <id>                              │
/// │ Last scan:      <iso8601>                         │
/// │ Scan duration:  <n> ms                            │
/// │ Last error:     <text OR "(none)">                │
/// │                                                   │
/// │ Search paths:                                     │
/// │   <path1> (default)                               │
/// │   <path2> (user config)                           │
/// └───────────────────────────────────────────────────┘
/// (bottom bar generated from SHORTCUT_TABLE — `[Esc] back`,
///  `[r] refresh this tool`, `[?] help`)
/// ```
pub fn render(frame: &mut Frame<'_>, area: Rect, screen: &ToolDetailScreenState, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    let title = format!(" Tool: {} ", screen.detail.tool_id.0);
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(chunks[0]);
    frame.render_widget(block, chunks[0]);

    let body_lines = build_body_lines(&screen.detail);
    let paragraph = Paragraph::new(body_lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);

    // Bottom bar — sourced from SHORTCUT_TABLE so the labels and dispatch
    // can never drift. The `BarContext` for `Screen::ToolDetail` reuses the
    // `Detail` section because the AC-21-8 shortcuts match the existing
    // detail-screen set verbatim (`[Esc] back`, `[r]`, `[?] help`).
    let ctx = BarContext::for_state(app);
    let bar = render_bottom_bar(&ctx, crate::render::colors::no_color_active());
    frame.render_widget(Paragraph::new(bar), chunks[1]);
}

/// Pure helper — build the rendered text lines for a `ToolDetail`. Exposed
/// for unit tests so we can assert substrings without spinning up a
/// ratatui frame.
pub fn build_body_lines(detail: &ToolDetail) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(format!(
        "Discovery root: {}",
        detail.install_path.display()
    )));
    lines.push(Line::from(format!(
        "Version:        {}",
        format_version(detail.detected_version.as_deref())
    )));
    lines.push(Line::from(format!(
        "Plugin version: {}",
        detail.plugin_version
    )));
    lines.push(Line::from(format!(
        "Model count:    {}",
        detail.model_count
    )));
    lines.push(Line::from(format!(
        "Disk usage:     {}",
        format_bytes(detail.disk_usage_bytes)
    )));
    lines.push(Line::from(format!(
        "Largest model:  {}",
        format_largest_model(detail.largest_model.as_ref())
    )));
    lines.push(Line::from(format!(
        "Last scan:      {}",
        format_timestamp(detail.last_scan_at.as_ref())
    )));
    lines.push(Line::from(format!(
        "Scan duration:  {}",
        format_scan_duration_ms(detail.last_scan_duration_ms)
    )));
    lines.push(Line::from(format!(
        "Last error:     {}",
        format_last_error(detail.last_error.as_deref(), detail.last_error_at.as_ref(),)
    )));
    lines.push(Line::from(""));
    lines.push(Line::from("Search paths:"));
    if detail.search_paths.is_empty() {
        lines.push(Line::from("  (none)"));
    } else {
        for entry in &detail.search_paths {
            lines.push(Line::from(format!(
                "  {} {}",
                entry.path.display(),
                source_label(entry.source)
            )));
        }
    }
    lines
}

fn format_version(detected: Option<&str>) -> String {
    match detected {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => VERSION_NOT_DETECTABLE.to_string(),
    }
}

fn format_largest_model(largest: Option<&modeltap_core::domain::inspect::ModelId>) -> String {
    match largest {
        Some(id) => id.0.clone(),
        None => "(none)".to_string(),
    }
}

fn format_scan_duration_ms(duration: Option<u64>) -> String {
    match duration {
        Some(ms) => format!("{} ms", ms),
        None => "(unknown)".to_string(),
    }
}

fn format_last_error(error: Option<&str>, error_at: Option<&SystemTime>) -> String {
    match error {
        Some(msg) => match error_at {
            Some(t) => format!("{} ({})", msg, format_system_time(t)),
            None => msg.to_string(),
        },
        None => LAST_ERROR_NONE.to_string(),
    }
}

fn format_timestamp(t: Option<&SystemTime>) -> String {
    match t {
        Some(time) => format_system_time(time),
        None => "(never)".to_string(),
    }
}

fn format_system_time(t: &SystemTime) -> String {
    // Hand-rolled ISO-8601 UTC formatter at second resolution. Avoids
    // adding a `time`/`chrono` dep to modeltap-tui — the detail screen
    // doesn't need calendar arithmetic; it just needs a stable, sortable,
    // human-readable string to surface alongside `last_error` and
    // `last_scan_at`. The full timestamp formatter lives in
    // `modeltap-store::repo::tools::format_iso8601_utc` for the cache; this
    // module needs only the second-precision form.
    let Ok(duration) = t.duration_since(std::time::UNIX_EPOCH) else {
        return "(invalid time)".to_string();
    };
    let total_secs = duration.as_secs() as i64;
    let (year, month, day, hour, minute, second) = unix_seconds_to_ymdhms(total_secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Convert a UNIX timestamp in seconds (UTC) into `(year, month, day, hour,
/// minute, second)`. Pure arithmetic — no allocation, no dependency. Handles
/// leap years via the standard Gregorian rule. Output range is 1970..=9999;
/// stamps outside that range clamp to the boundary (the detail screen does
/// not depend on extreme historic / future stamps).
fn unix_seconds_to_ymdhms(seconds: i64) -> (i32, u8, u8, u8, u8, u8) {
    const SECONDS_PER_DAY: i64 = 86_400;
    let days = seconds.div_euclid(SECONDS_PER_DAY);
    let secs_today = seconds.rem_euclid(SECONDS_PER_DAY);
    let hour = (secs_today / 3600) as u8;
    let minute = ((secs_today % 3600) / 60) as u8;
    let second = (secs_today % 60) as u8;

    // Civil-date algorithm (Howard Hinnant, public-domain). Converts the day
    // count since 1970-01-01 into Gregorian (year, month, day) with leap-year
    // handling. The math is constant-time and produces correct results for
    // any 64-bit input within the supported Gregorian range.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m: u8 = if mp < 10 {
        (mp + 3) as u8
    } else {
        (mp - 9) as u8
    };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m, d, hour, minute, second)
}

fn source_label(source: SearchPathSource) -> &'static str {
    match source {
        SearchPathSource::Default => SEARCH_PATH_DEFAULT_LABEL,
        SearchPathSource::UserConfig => SEARCH_PATH_USER_CONFIG_LABEL,
    }
}

/// Borrowed view useful to callers that want to grep an entire rendered
/// detail screen for a substring. Pure helper used by `tests/acceptance/`
/// step assertions that operate on a captured frame OR on the in-process
/// rendered text.
pub fn render_to_plain_string(detail: &ToolDetail) -> String {
    let mut out = String::new();
    for line in build_body_lines(detail) {
        for span in &line.spans {
            out.push_str(&span.content);
        }
        out.push('\n');
    }
    out
}

/// Sentinel-grepping convenience: true when the rendered detail screen
/// for `detail` contains every field label AC-21-2 demands. Public so
/// the step definition `then_the_rest_of_the_detail_screen_renders_normally`
/// has a single point of truth.
#[allow(dead_code)] // Phase 02 uses this from step-defs; kept here so the
                    // assertion surface lives next to the render fn.
pub fn rendered_screen_contains_all_required_labels(detail: &ToolDetail) -> bool {
    let txt = render_to_plain_string(detail);
    REQUIRED_FIELD_LABELS
        .iter()
        .all(|label| txt.contains(label))
}

/// The nine field labels AC-21-2 mandates appear on the detail screen.
/// Public so step-defs can iterate; kept in lock-step with `build_body_lines`
/// by the `tests/lint.rs` architecture lint.
pub const REQUIRED_FIELD_LABELS: &[&str] = &[
    "Discovery root:",
    "Version:",
    "Search paths:",
    "Model count:",
    "Disk usage:",
    "Largest model:",
    "Last scan:",
    "Plugin version:",
    "Last error:",
];

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_core::domain::inspect::{ModelId, SearchPathEntry, ToolDetail};
    use modeltap_core::ToolId;
    use std::path::PathBuf;
    use std::time::Duration;

    fn empty_detail(tool_id: ToolId) -> ToolDetail {
        ToolDetail {
            tool_id,
            install_path: PathBuf::from("/opt/test-tool"),
            detected_version: None,
            plugin_version: "modeltap-plugin-test 0.0.0".to_string(),
            search_paths: Vec::new(),
            model_count: 0,
            disk_usage_bytes: 0,
            largest_model: None,
            last_scan_at: None,
            last_scan_duration_ms: None,
            last_error: None,
            last_error_at: None,
        }
    }

    /// RED_UNIT — AC-21-3: when `detected_version` is None the Version field
    /// renders as `"(not detectable)"`, not as a stale or false value.
    #[test]
    fn version_field_renders_not_detectable_when_detected_version_is_none() {
        let detail = empty_detail(ToolId("test-tool"));
        let txt = render_to_plain_string(&detail);
        assert!(
            txt.contains(&format!("Version:        {}", VERSION_NOT_DETECTABLE)),
            "Version field must render as `(not detectable)` when detected_version is None — got:\n{txt}"
        );
    }

    /// RED_UNIT — AC-21-3: explicit Some(value) renders the value verbatim.
    #[test]
    fn version_field_renders_provided_value_when_detected_version_is_some() {
        let mut detail = empty_detail(ToolId("ollama"));
        detail.detected_version = Some("0.6.4".to_string());
        let txt = render_to_plain_string(&detail);
        assert!(
            txt.contains("Version:        0.6.4"),
            "Version field must render the detected_version verbatim — got:\n{txt}"
        );
    }

    /// RED_UNIT — AC-21-4: when `last_error` is None the field reads `(none)`.
    #[test]
    fn last_error_field_reads_none_when_absent() {
        let detail = empty_detail(ToolId("test-tool"));
        let txt = render_to_plain_string(&detail);
        assert!(
            txt.contains(&format!("Last error:     {}", LAST_ERROR_NONE)),
            "Last error field must read `(none)` when last_error is None — got:\n{txt}"
        );
    }

    /// RED_UNIT — AC-21-4: when `last_error` is Some the text appears in the
    /// rendered field along with the timestamp.
    #[test]
    fn last_error_field_renders_text_with_timestamp_when_present() {
        let mut detail = empty_detail(ToolId("ollama"));
        detail.last_error = Some("permission denied reading ~/.ollama/models/".to_string());
        detail.last_error_at = Some(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000));
        let txt = render_to_plain_string(&detail);
        assert!(
            txt.contains("permission denied reading"),
            "rendered text must contain the error message — got:\n{txt}"
        );
        // The timestamp formatter uses an ISO-8601 UTC shape; assert the year
        // prefix so the assertion is stable across timezone-dependent
        // formatters.
        assert!(
            txt.contains("2023-"),
            "rendered text must contain an ISO-formatted timestamp — got:\n{txt}"
        );
    }

    /// RED_UNIT — AC-21-5: search-path entries are tagged "(default)" /
    /// "(user config)" per `SearchPathSource`.
    #[test]
    fn search_paths_section_labels_default_and_user_config() {
        let mut detail = empty_detail(ToolId("llama-cli"));
        detail.search_paths = vec![
            SearchPathEntry {
                path: PathBuf::from("/home/devon/llms"),
                source: SearchPathSource::Default,
            },
            SearchPathEntry {
                path: PathBuf::from("/data/models"),
                source: SearchPathSource::UserConfig,
            },
        ];
        let txt = render_to_plain_string(&detail);
        assert!(
            txt.contains("/home/devon/llms (default)"),
            "default-source path must be tagged `(default)` — got:\n{txt}"
        );
        assert!(
            txt.contains("/data/models (user config)"),
            "user-config-source path must be tagged `(user config)` — got:\n{txt}"
        );
    }

    /// RED_UNIT — AC-21-2: all nine required field labels render.
    #[test]
    fn all_required_field_labels_present_in_render() {
        let detail = empty_detail(ToolId("test-tool"));
        assert!(
            rendered_screen_contains_all_required_labels(&detail),
            "render must include every AC-21-2 field label"
        );
    }

    /// RED_UNIT — AC-21-2: model count + disk usage + largest model render
    /// from the cache-sourced fields.
    #[test]
    fn cache_sourced_fields_render_into_the_detail_screen() {
        let mut detail = empty_detail(ToolId("ollama"));
        detail.model_count = 12;
        detail.disk_usage_bytes = 47_300_000_000;
        detail.largest_model = Some(ModelId("llama3:70b-instruct-q4_K_M".to_string()));
        let txt = render_to_plain_string(&detail);
        assert!(
            txt.contains("Model count:    12"),
            "Model count must render the cache value — got:\n{txt}"
        );
        assert!(
            txt.contains("47.3 GB"),
            "Disk usage must render via format_bytes — got:\n{txt}"
        );
        assert!(
            txt.contains("llama3:70b-instruct-q4_K_M"),
            "Largest model must render the model_id — got:\n{txt}"
        );
    }
}
