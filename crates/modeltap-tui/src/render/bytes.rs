//! Human-readable byte-count formatting.
//!
//! Shared by every dialog/pane that surfaces a "Reclaim:" / "Disk:" /
//! "size" line, plus by the `actions::unify::dry_run` orchestrator (whose
//! preview lines must match the dialog's "Reclaim:" line character-for-
//! character per US-14).
//!
//! Decimal SI units (1 GB = 10^9 B) — chosen because users compare modeltap
//! totals to the disk-usage numbers shown by `df -h` / Finder / Nautilus /
//! Windows Explorer, all of which use SI for free-space reporting.

/// Format a byte count as a one-decimal-place SI unit string.
///
/// - `>= 1_000_000_000` → "X.X GB"
/// - `>= 1_000_000` → "X.X MB"
/// - else → "N B"
pub fn format_bytes(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const MB: u64 = 1_000_000;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format a byte count ALWAYS in GB with one decimal place. Used by the
/// folder-delete mixed-mode dialog and post-action banner so reclaim and
/// retain values share a unit and align column-wise, even when the retain
/// total is sub-GB (e.g. a single 808 MB shared file rendered as "0.8 GB"
/// alongside a 13.2 GB reclaim). Decimal SI (1 GB == 10^9 B) to stay
/// consistent with `format_bytes`.
pub fn format_gb(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    format!("{:.1} GB", bytes as f64 / GB as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_1mb_renders_in_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999_999), "999999 B");
    }

    #[test]
    fn between_1mb_and_1gb_renders_in_mb_with_one_decimal() {
        assert_eq!(format_bytes(1_000_000), "1.0 MB");
        assert_eq!(format_bytes(2_500_000), "2.5 MB");
    }

    #[test]
    fn over_1gb_renders_in_gb_with_one_decimal() {
        assert_eq!(format_bytes(1_000_000_000), "1.0 GB");
        assert_eq!(format_bytes(4_400_000_000), "4.4 GB");
    }
}
