// xtask::changelog — pure changelog-section extraction.
//
// Per DESIGN component-boundaries.md §2.2:
//   pub fn extract_section(changelog: &str, version: &Version) -> Result<String, ChangelogError>;
//
// Pure function: takes the full CHANGELOG.md text and a Version, returns the
// body of the matching `## [X.Y.Z]` section. No I/O — file reading is the
// caller's responsibility (the CLI dispatcher in `main.rs`, via
// `xtask::fs_adapter::read_to_string`).
//
// Implemented in DELIVER step 01-03 (Walking Skeleton, US-05 — PUBLISH).

use crate::cargo_toml::Version;

#[derive(Debug)]
pub enum ChangelogError {
    /// No `## [<version>]` heading found in the changelog text.
    /// The CLI dispatcher formats this as `"CHANGELOG.md has no [X.Y.Z] section"`
    /// (exact wording required by walking-skeleton.feature US-05 failure scenario).
    SectionNotFound,
}

impl std::fmt::Display for ChangelogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangelogError::SectionNotFound => write!(f, "section not found"),
        }
    }
}

impl std::error::Error for ChangelogError {}

/// Extract the body of `## [<version>]` from the changelog text.
///
/// Algorithm:
/// 1. Locate the line `## [<version>]` (anchored at line start). Tolerates an
///    optional trailing `- YYYY-MM-DD` suffix per keep-a-changelog convention.
/// 2. Capture every line UNTIL the next `## [` heading (any version) or EOF.
/// 3. Strip leading/trailing blank lines from the captured body, preserving
///    internal blank lines.
/// 4. Return the body. The trailing newline is normalised so the output file
///    ends with exactly one `\n`.
///
/// Returns `Err(SectionNotFound)` when no matching heading exists.
pub fn extract_section(changelog_text: &str, version: &Version) -> Result<String, ChangelogError> {
    let target_heading_prefix = format!("## [{version}]");
    let mut lines = changelog_text.lines();

    // 1. Skip lines until we find the target heading.
    let mut found = false;
    for line in lines.by_ref() {
        if is_matching_heading(line, &target_heading_prefix) {
            found = true;
            break;
        }
    }

    if !found {
        return Err(ChangelogError::SectionNotFound);
    }

    // 2. Collect body lines until the next `## [` heading or EOF.
    let mut body: Vec<&str> = Vec::new();
    for line in lines {
        if is_any_section_heading(line) {
            break;
        }
        body.push(line);
    }

    // 3. Trim leading and trailing blank lines, preserving internal blanks.
    let start = body
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(body.len());
    let end = body
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    let trimmed = &body[start..end];

    // 4. Join with `\n` and append a single trailing newline.
    let mut out = trimmed.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

/// Does `line` match `"## [<version>]"`, optionally followed by `" - YYYY-MM-DD"`?
///
/// We accept the heading exactly as written by keep-a-changelog tooling:
///   - `## [0.1.0]`
///   - `## [0.1.0] - 2026-05-03`
///
/// Trailing whitespace is tolerated. Anything else after the closing bracket
/// (other than the optional date) is rejected so we don't accidentally match
/// `## [0.1.0-rc1]` when looking for `## [0.1.0]`.
fn is_matching_heading(line: &str, target_prefix: &str) -> bool {
    let line = line.trim_end();
    if !line.starts_with(target_prefix) {
        return false;
    }
    let rest = &line[target_prefix.len()..];
    if rest.is_empty() {
        return true;
    }
    // Accept `<sp>-<sp>YYYY-MM-DD` suffix (keep-a-changelog).
    is_iso_date_suffix(rest)
}

/// Does `line` look like ANY `## [...]` section heading? Used to detect the
/// boundary at the START of the next section.
fn is_any_section_heading(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("## [")
}

/// Is `s` of the form ` - YYYY-MM-DD` (with possible trailing whitespace)?
///
/// We don't pull in `regex` for this — a hand-rolled check keeps the dependency
/// graph lean and the parse intent obvious.
fn is_iso_date_suffix(s: &str) -> bool {
    let s = s.trim_end();
    let bytes = s.as_bytes();
    // Expected: " - YYYY-MM-DD" = 13 bytes.
    if bytes.len() != 13 {
        return false;
    }
    if &bytes[0..3] != b" - " {
        return false;
    }
    // Date portion: YYYY-MM-DD.
    let date = &bytes[3..];
    let is_digit = |b: u8| b.is_ascii_digit();
    is_digit(date[0])
        && is_digit(date[1])
        && is_digit(date[2])
        && is_digit(date[3])
        && date[4] == b'-'
        && is_digit(date[5])
        && is_digit(date[6])
        && date[7] == b'-'
        && is_digit(date[8])
        && is_digit(date[9])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().expect("test fixture must be valid semver")
    }

    const TWO_SECTIONS: &str = "\
# Changelog

## [0.1.0] - 2026-04-01

Initial public release.

- feat: first thing
- feat: second thing

## [0.0.1-rc1] - 2026-05-03

Walking-skeleton release-candidate.

- chore: bootstrap pipeline
";

    #[test]
    fn extract_section_returns_body_of_matching_section() {
        let body = extract_section(TWO_SECTIONS, &v("0.0.1-rc1")).expect("section present");

        assert!(
            body.contains("Walking-skeleton release-candidate."),
            "body should contain matched section text, got: {body:?}"
        );
        assert!(
            body.contains("chore: bootstrap pipeline"),
            "body should contain matched bullet, got: {body:?}"
        );
    }

    #[test]
    fn extract_section_does_not_leak_adjacent_section_content() {
        let body = extract_section(TWO_SECTIONS, &v("0.0.1-rc1")).expect("section present");

        assert!(
            !body.contains("Initial public release."),
            "body must not include adjacent section text, got: {body:?}"
        );
        assert!(
            !body.contains("first thing"),
            "body must not include adjacent section bullet, got: {body:?}"
        );
        assert!(
            !body.contains("## ["),
            "body must not include any section heading, got: {body:?}"
        );
    }

    #[test]
    fn extract_section_tolerates_heading_without_trailing_date() {
        let text = "\
## [0.1.0]

Body text.
";
        let body = extract_section(text, &v("0.1.0")).expect("section present");
        assert_eq!(body, "Body text.\n");
    }

    #[test]
    fn extract_section_returns_section_not_found_when_version_absent() {
        let err =
            extract_section(TWO_SECTIONS, &v("0.2.0")).expect_err("missing section must fail");
        assert!(matches!(err, ChangelogError::SectionNotFound));
    }

    #[test]
    fn extract_section_distinguishes_versions_with_overlapping_prefixes() {
        // Asking for `0.1.0` must not match `0.1.0-rc1` and vice-versa.
        let text = "\
## [0.1.0-rc1]

Pre-release body.

## [0.1.0]

Final body.
";
        let final_body = extract_section(text, &v("0.1.0")).expect("0.1.0 present");
        assert!(final_body.contains("Final body."));
        assert!(!final_body.contains("Pre-release body."));

        let rc_body = extract_section(text, &v("0.1.0-rc1")).expect("0.1.0-rc1 present");
        assert!(rc_body.contains("Pre-release body."));
        assert!(!rc_body.contains("Final body."));
    }

    #[test]
    fn extract_section_preserves_internal_blank_lines_but_trims_outer_blanks() {
        let text = "\
## [0.1.0] - 2026-04-01



First paragraph.

Second paragraph.



## [0.0.1] - 2026-03-01

Older.
";
        let body = extract_section(text, &v("0.1.0")).expect("section present");
        // Outer blanks trimmed.
        assert!(
            body.starts_with("First paragraph."),
            "body must start at first non-blank, got: {body:?}"
        );
        assert!(
            body.trim_end().ends_with("Second paragraph."),
            "body must end at last non-blank line, got: {body:?}"
        );
        // Internal blank between paragraphs preserved.
        assert!(
            body.contains("First paragraph.\n\nSecond paragraph."),
            "internal blank line must be preserved, got: {body:?}"
        );
    }

    // -------------------------------------------------------------------------
    // Mutation-coverage: ChangelogError Display.
    //
    // Pins exact text so the following mutant is killed:
    //   - <impl Display for ChangelogError>::fmt -> Ok(Default::default())
    // -------------------------------------------------------------------------

    #[test]
    fn display_for_section_not_found_uses_exact_phrasing() {
        let err = ChangelogError::SectionNotFound;
        assert_eq!(format!("{err}"), "section not found");
    }

    // -------------------------------------------------------------------------
    // Mutation-coverage: is_iso_date_suffix is a 10-clause `&&` chain over
    // a fixed-width " - YYYY-MM-DD" suffix. We pin TRUE and FALSE cases so
    // that:
    //   - replace fn -> bool with true is killed by ANY false-returning case
    //   - each of the 8 `&& -> ||` mutants is killed by an input where exactly
    //     one byte at the mutation's right-hand clause is wrong; flipping the
    //     adjacent `&&` to `||` then masks the wrongness and FLIPS the result.
    //
    // The 8 mutated `&&` positions sit between:
    //   (1) date[0] | (2) date[1] | (3) date[2] | (4) date[3]
    //   (5) date[4]==b'-' | (6) date[5] | (7) date[6] | (8) date[7]==b'-'
    //   (9) date[8] | (10) date[9]
    //
    // For each `&& -> ||` mutation, an input where the RIGHT-hand clause is
    // false while everything to its left is true causes the mutant to return
    // true (incorrectly) while the correct function returns false. We
    // construct one such input per position.
    // -------------------------------------------------------------------------

    #[test]
    fn is_iso_date_suffix_true_for_canonical_date() {
        // Baseline: a perfectly-formed " - YYYY-MM-DD" must return true. This
        // alone proves the function is not the constant `false`.
        assert!(is_iso_date_suffix(" - 2026-05-03"));
        assert!(is_iso_date_suffix(" - 0000-00-00"));
        assert!(is_iso_date_suffix(" - 9999-12-31"));
    }

    #[test]
    fn is_iso_date_suffix_false_when_length_or_prefix_wrong() {
        // Kills: replace fn -> bool with true (any false-returning case).
        assert!(!is_iso_date_suffix(""));
        assert!(!is_iso_date_suffix(" - 2026-05-0")); // 12 bytes, too short
        assert!(!is_iso_date_suffix(" - 2026-05-031")); // 14 bytes, too long
        assert!(!is_iso_date_suffix("- - 026-05-03")); // 13 bytes, wrong prefix
        assert!(!is_iso_date_suffix("X- 2026-05-03")); // 13 bytes, prefix[0]
        assert!(!is_iso_date_suffix(" X 2026-05-03")); // 13 bytes, prefix[1]
        assert!(!is_iso_date_suffix(" -X2026-05-03")); // 13 bytes, prefix[2]
    }

    /// Each row holds a `(label, suffix)` where `suffix` is exactly 13 bytes
    /// AND only ONE position differs from a valid date. The correct function
    /// returns `false` for every row; flipping the corresponding `&&` to `||`
    /// in `is_iso_date_suffix` causes the mutant to return `true`.
    #[test]
    fn is_iso_date_suffix_false_for_one_bad_byte_at_each_position() {
        // date offsets within `s` after the 3-byte " - " prefix: 0..=9.
        let cases = [
            ("date[0]=X", " - X026-05-03"), // && between date[0] and date[1]
            ("date[1]=X", " - 2X26-05-03"), // && between date[1] and date[2]
            ("date[2]=X", " - 20X6-05-03"), // && between date[2] and date[3]
            ("date[3]=X", " - 202X-05-03"), // && between date[3] and date[4]==-
            ("date[4]=X", " - 2026X05-03"), // && between date[4]==- and date[5]
            ("date[5]=X", " - 2026-X5-03"), // && between date[5] and date[6]
            ("date[6]=X", " - 2026-0X-03"), // && between date[6] and date[7]==-
            ("date[7]=X", " - 2026-05X03"), // && between date[7]==- and date[8]
            ("date[8]=X", " - 2026-05-X3"), // && between date[8] and date[9]
            ("date[9]=X", " - 2026-05-0X"), // last clause; mutating its && is N/A
        ];
        for (label, suffix) in cases {
            assert_eq!(
                suffix.len(),
                13,
                "test fixture {label} must be exactly 13 bytes"
            );
            assert!(
                !is_iso_date_suffix(suffix),
                "{label}: input {suffix:?} has one bad byte and must return false"
            );
        }
    }

    #[test]
    fn extract_section_handles_section_at_eof_with_no_following_heading() {
        let text = "\
## [0.1.0] - 2026-04-01

Only section.
";
        let body = extract_section(text, &v("0.1.0")).expect("section present");
        assert_eq!(body, "Only section.\n");
    }
}
