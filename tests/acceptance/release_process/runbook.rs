// Acceptance tests for the maintainer-facing RELEASING.md runbook.
//
// Step: 03-03 (Phase 3 — hands-off automation, US-13).
// Source scenarios: docs/feature/release-process-homebrew-github/distill/
//                   features/hands-off-automation.feature, US-13:
//   - "Runbook exists at repo root within the line budget"
//   - "Runbook contains the per-release log table"
//   - "Runbook documents the operational safety notes"
//
// Strategy: pure-file structural assertions on the in-tree `RELEASING.md`.
// The file is hand-authored markdown — there is no production code path to
// exercise here. The "driving port" for these scenarios IS the markdown
// document; asserting against its parsed event stream is the port-to-port
// check at this scope (per nw-tdd-methodology §"Pure domain functions ARE
// their own driving ports").
//
// Why pulldown-cmark: a pure regex/line-count check would over-fit on
// formatting (e.g., trailing-blank-line semantics, indented list items in
// fenced blocks). A CommonMark event stream lets us count "real" numbered
// list items (List(Some(1)) at the document level) and discover table
// columns via TableHead/TableCell events independent of pipe alignment.

use std::path::PathBuf;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

// =============================================================================
// Fixture helpers — locate the in-repo RELEASING.md at the workspace root.
// =============================================================================

/// Absolute path to `RELEASING.md` at the workspace root.
///
/// Resolved at compile time from this crate's `CARGO_MANIFEST_DIR` (the
/// `tests/` crate), then `..` to the workspace root.
fn releasing_md_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p.push("RELEASING.md");
    p
}

/// Read RELEASING.md to a String. Panics with a clear diagnostic if missing
/// (the first scenario asserts existence; if reading fails, that scenario
/// fails first — keep the other scenarios' panic messages diagnostic).
fn read_releasing_md() -> String {
    let path = releasing_md_path();
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("RELEASING.md must exist at {}: {e}", path.display()))
}

/// Parse RELEASING.md events with tables enabled (the per-release log table
/// is a GFM table, not vanilla CommonMark).
fn parse_events(text: &str) -> Vec<Event<'_>> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    Parser::new_ext(text, opts).collect()
}

/// Count document-level numbered list ITEMS (not lists). A single `1. … 2.
/// … 3.` ordered list contributes 3 items, not 1. Nested list items inside
/// fenced code blocks do not count (pulldown-cmark exposes them only as
/// Text events inside CodeBlock).
fn count_numbered_steps(events: &[Event]) -> usize {
    let mut depth_in_ordered_list: i32 = 0;
    let mut count = 0usize;
    for ev in events {
        match ev {
            Event::Start(Tag::List(Some(_))) => depth_in_ordered_list += 1,
            Event::End(TagEnd::List(true)) => depth_in_ordered_list -= 1,
            Event::Start(Tag::Item) if depth_in_ordered_list > 0 => count += 1,
            _ => {}
        }
    }
    count
}

/// Extract the column header texts from the FIRST table in the document.
/// Returns an empty Vec if the document has no table.
fn first_table_columns(events: &[Event]) -> Vec<String> {
    let mut in_head = false;
    let mut in_cell = false;
    let mut current = String::new();
    let mut cols: Vec<String> = Vec::new();
    for ev in events {
        match ev {
            Event::Start(Tag::TableHead) => in_head = true,
            Event::End(TagEnd::TableHead) => break,
            Event::Start(Tag::TableCell) if in_head => {
                in_cell = true;
                current.clear();
            }
            Event::End(TagEnd::TableCell) if in_head => {
                cols.push(std::mem::take(&mut current));
                in_cell = false;
            }
            Event::Text(t) if in_cell => current.push_str(t.as_ref()),
            Event::Code(t) if in_cell => current.push_str(t.as_ref()),
            _ => {}
        }
    }
    cols.into_iter().map(normalise_column_name).collect()
}

/// Normalise a column header for comparison: lowercase, trim, collapse
/// internal whitespace runs, and replace spaces with hyphens. Lets the
/// runbook author use "Tag pushed at" or "tag-pushed-at" interchangeably.
fn normalise_column_name(s: String) -> String {
    let lower = s.trim().to_lowercase();
    let collapsed: String = lower.split_whitespace().collect::<Vec<_>>().join("-");
    collapsed
}

/// Collect the lowercase, trimmed text of every heading at the given level.
fn headings_at(events: &[Event], level: HeadingLevel) -> Vec<String> {
    let mut in_heading = false;
    let mut current = String::new();
    let mut out = Vec::new();
    for ev in events {
        match ev {
            Event::Start(Tag::Heading { level: lvl, .. }) if *lvl == level => {
                in_heading = true;
                current.clear();
            }
            Event::End(TagEnd::Heading(lvl)) if *lvl == level => {
                in_heading = false;
                out.push(current.trim().to_lowercase());
            }
            Event::Text(t) if in_heading => current.push_str(t.as_ref()),
            Event::Code(t) if in_heading => current.push_str(t.as_ref()),
            _ => {}
        }
    }
    out
}

/// Returns true iff ANY heading at level H1, H2, or H3 (case-insensitively)
/// CONTAINS the given needle. Lets the author choose between "## First-time
/// setup" and "### First-Time Setup" without breaking the test.
fn has_section_header(events: &[Event], needle: &str) -> bool {
    let needle = needle.to_lowercase();
    [HeadingLevel::H1, HeadingLevel::H2, HeadingLevel::H3]
        .into_iter()
        .flat_map(|lvl| headings_at(events, lvl))
        .any(|h| h.contains(&needle))
}

// =============================================================================
// Scenario 1 — "Runbook exists at repo root within the line budget"
//   AC: file exists, ≤ 80 lines, ≤ 10 numbered steps.
// =============================================================================

#[test]
fn runbook_exists_at_repo_root_within_line_budget() {
    let path = releasing_md_path();
    assert!(
        path.exists(),
        "RELEASING.md must exist at workspace root: {}",
        path.display()
    );

    let text = read_releasing_md();

    // Line count: count terminator-delimited lines so a trailing blank line
    // does not cost an extra line. `lines()` already discards the trailing
    // newline.
    let line_count = text.lines().count();
    assert!(
        line_count <= 80,
        "RELEASING.md must have at most 80 lines per US-13.AC; got {line_count}"
    );

    // Numbered-step count: parse as CommonMark and count document-level
    // ordered-list ITEMS (not lists). Multiple ordered lists are summed.
    let events = parse_events(&text);
    let steps = count_numbered_steps(&events);
    assert!(
        steps <= 10,
        "RELEASING.md must have at most 10 numbered steps per US-13.AC; got {steps}"
    );
    // At least one numbered step must exist — an empty runbook would
    // technically satisfy "at most 10" but defeats the purpose of a runbook.
    assert!(
        steps >= 1,
        "RELEASING.md must contain at least one numbered step; got {steps}"
    );
}

// =============================================================================
// Scenario 2 — "Runbook contains the per-release log table"
//   AC: a markdown table with columns: version, tag-pushed-at,
//       release-published-at, tap-merged-at, time-to-tap, platforms-verified,
//       provenance-verified, notes.
// =============================================================================

#[test]
fn runbook_contains_release_log_table_with_required_columns() {
    let text = read_releasing_md();
    let events = parse_events(&text);
    let cols = first_table_columns(&events);

    assert!(
        !cols.is_empty(),
        "RELEASING.md must contain at least one markdown table (the release log); found none"
    );

    let required = [
        "version",
        "tag-pushed-at",
        "release-published-at",
        "tap-merged-at",
        "time-to-tap",
        "platforms-verified",
        "provenance-verified",
        "notes",
    ];
    for col in required {
        assert!(
            cols.iter().any(|c| c == col),
            "RELEASING.md release-log table must declare column {col:?}; got columns={cols:?}"
        );
    }
}

// =============================================================================
// Scenario 3 — "Runbook documents the operational safety notes"
//   AC: documents GH_TAP_TOKEN rotation procedure, manual-edit-clobber
//       trade-off, macOS Gatekeeper xattr workaround.
//
// Plus US-13.AC fold-in: First-time-setup section MUST cover create tap
// repo, set GH_TAP_TOKEN, configure tap branch protection. We assert the
// section's PRESENCE here (the body text is verified by content keywords
// in the same scan).
// =============================================================================

#[test]
fn runbook_documents_operational_safety_notes_and_first_time_setup() {
    let text = read_releasing_md();
    let lower = text.to_lowercase();
    let events = parse_events(&text);

    // Operational safety notes — content keywords:
    assert!(
        lower.contains("gh_tap_token") && lower.contains("rotat"),
        "RELEASING.md must document the GH_TAP_TOKEN rotation procedure (look for 'GH_TAP_TOKEN' \
         and 'rotat')"
    );
    assert!(
        lower.contains("clobber") || lower.contains("overwritten") || lower.contains("overwrite"),
        "RELEASING.md must document the manual-edit-clobber trade-off on bump branches"
    );
    assert!(
        lower.contains("xattr") && lower.contains("com.apple.quarantine"),
        "RELEASING.md must document the macOS Gatekeeper `xattr -dr com.apple.quarantine` \
         workaround"
    );

    // First-time setup — section presence + content keywords:
    assert!(
        has_section_header(&events, "first-time setup"),
        "RELEASING.md must contain a 'First-time setup' section header (H1/H2/H3)"
    );
    assert!(
        lower.contains("create") && lower.contains("tap repo"),
        "First-time setup must document creating the tap repo"
    );
    assert!(
        lower.contains("gh_tap_token"),
        "First-time setup must document setting GH_TAP_TOKEN"
    );
    assert!(
        lower.contains("branch protection"),
        "First-time setup must document configuring tap branch protection"
    );
}
