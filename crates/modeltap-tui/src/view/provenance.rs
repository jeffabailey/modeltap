//! Pure `format_provenance(now, last_scan_at) -> String` helper for the
//! summary-bar provenance line (US-25, step 05-03).
//!
//! Per CM-D §9 of
//! `docs/feature/tool-model-info-sqlite-cache/distill/acceptance-test-plan.md`
//! this is a pure-function helper: `now` is a parameter (NOT
//! `SystemTime::now()` inside the body) so the function is deterministic
//! under test and stays free of `unsafe { SYSTEM_TIME }` patterns.
//!
//! ## Return shape
//!
//! - `None` last_scan_at -> `"never reconciled"` (no cache yet)
//! - `< 5` seconds since last scan -> `"just now"`
//! - `< 60` seconds -> `"<N> sec ago"`
//! - `< 60` minutes -> `"<N> min ago"`
//! - `< 24` hours -> `"<N> hours ago"`
//! - `>= 24` hours -> `"<N> days ago"`
//!
//! `last_scan_at > now` (clock skew) is folded into `"just now"` — the
//! function is monotonic in the elapsed-direction so a future timestamp is
//! interpreted as zero elapsed. Saturating arithmetic everywhere — never
//! panics on `SystemTime` math.

use std::time::{Duration, SystemTime};

/// Threshold under which we render `"just now"`. AC-24-7 budgets the
/// orchestrator round-trip at ≤ 1000 ms, so a 5-second window is generous
/// enough to absorb the entire dispatch + redraw cycle while still feeling
/// fresh to the user.
const JUST_NOW_THRESHOLD: Duration = Duration::from_secs(5);

/// Format the freshness suffix for the summary-bar provenance line.
///
/// Pure: `now` is a parameter, NOT `SystemTime::now()` inside the body.
/// Tests pass synthetic instants; the production callers in
/// `render::summary_bar::render` pass `SystemTime::now()` once per frame.
///
/// Returns one of:
///   - `"never reconciled"`     when `last_scan_at` is `None`
///   - `"just now"`             when elapsed < 5 sec (or clock skew)
///   - `"<N> sec ago"`          when 5 sec ≤ elapsed < 60 sec
///   - `"<N> min ago"`          when 60 sec ≤ elapsed < 60 min
///   - `"<N> hours ago"`        when 60 min ≤ elapsed < 24 hours
///   - `"<N> days ago"`         when elapsed ≥ 24 hours
pub fn format_provenance(now: SystemTime, last_scan_at: Option<SystemTime>) -> String {
    let Some(scanned_at) = last_scan_at else {
        return "never reconciled".to_string();
    };
    // Saturating: clock-skew (last_scan_at > now) collapses to zero elapsed.
    let elapsed = now.duration_since(scanned_at).unwrap_or(Duration::ZERO);
    if elapsed < JUST_NOW_THRESHOLD {
        return "just now".to_string();
    }
    let secs = elapsed.as_secs();
    if secs < 60 {
        return format!("{} sec ago", secs);
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{} min ago", mins);
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{} hours ago", hours);
    }
    let days = hours / 24;
    format!("{} days ago", days)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Anchor instant used across the tests so `now` is deterministic. The
    /// exact value is irrelevant — only `now - last_scan_at` matters.
    fn anchor_now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    #[test]
    fn format_provenance_returns_never_when_last_scan_is_none() {
        assert_eq!(format_provenance(anchor_now(), None), "never reconciled");
    }

    #[test]
    fn format_provenance_returns_just_now_for_recent_scans() {
        let now = anchor_now();
        // 0 sec, 1 sec, 4 sec: all "just now" (below the 5-sec threshold).
        for offset_secs in [0u64, 1, 4] {
            let scanned = now - Duration::from_secs(offset_secs);
            assert_eq!(
                format_provenance(now, Some(scanned)),
                "just now",
                "offset {} sec should be 'just now'",
                offset_secs
            );
        }
    }

    #[test]
    fn format_provenance_returns_just_now_under_clock_skew() {
        // last_scan_at > now (clock went backward, NTP correction) is folded
        // into "just now" via saturating duration_since.
        let now = anchor_now();
        let future_scan = now + Duration::from_secs(60);
        assert_eq!(format_provenance(now, Some(future_scan)), "just now");
    }

    #[test]
    fn format_provenance_returns_sec_ago_under_one_minute() {
        let now = anchor_now();
        // Walk every 5-second boundary up to 59 sec — all should render
        // "<N> sec ago" with the exact elapsed second count.
        for offset_secs in [5u64, 10, 30, 45, 59] {
            let scanned = now - Duration::from_secs(offset_secs);
            assert_eq!(
                format_provenance(now, Some(scanned)),
                format!("{} sec ago", offset_secs),
                "offset {} sec mismatch",
                offset_secs
            );
        }
    }

    #[test]
    fn format_provenance_returns_min_ago_under_one_hour() {
        let now = anchor_now();
        // 60 sec = 1 min; 14 min, 59 min — all under the 1-hour boundary.
        let cases: &[(u64, &str)] = &[
            (60, "1 min ago"),
            (14 * 60, "14 min ago"),
            (59 * 60, "59 min ago"),
        ];
        for (offset_secs, expected) in cases {
            let scanned = now - Duration::from_secs(*offset_secs);
            assert_eq!(
                format_provenance(now, Some(scanned)),
                *expected,
                "offset {} sec mismatch",
                offset_secs
            );
        }
    }

    #[test]
    fn format_provenance_returns_hours_ago_under_one_day() {
        let now = anchor_now();
        // 60 min = 1 hour; 5 hours; 23 hours — all under the 24-hour boundary.
        let cases: &[(u64, &str)] = &[
            (60 * 60, "1 hours ago"),
            (5 * 60 * 60, "5 hours ago"),
            (23 * 60 * 60, "23 hours ago"),
        ];
        for (offset_secs, expected) in cases {
            let scanned = now - Duration::from_secs(*offset_secs);
            assert_eq!(
                format_provenance(now, Some(scanned)),
                *expected,
                "offset {} sec mismatch",
                offset_secs
            );
        }
    }

    #[test]
    fn format_provenance_returns_days_ago_past_one_day() {
        let now = anchor_now();
        let cases: &[(u64, &str)] = &[
            (24 * 60 * 60, "1 days ago"),
            (3 * 24 * 60 * 60, "3 days ago"),
            (30 * 24 * 60 * 60, "30 days ago"),
        ];
        for (offset_secs, expected) in cases {
            let scanned = now - Duration::from_secs(*offset_secs);
            assert_eq!(
                format_provenance(now, Some(scanned)),
                *expected,
                "offset {} sec mismatch",
                offset_secs
            );
        }
    }
}
