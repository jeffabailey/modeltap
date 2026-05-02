//! Step 01-05 unit tests — render::row dedup-glyph column.
//!
//! Asserts that `render_row_basic` (and `render_row`) emit a fixed-width
//! dedup-glyph column between the existing compatibility-glyph column
//! (position 0) and the model-id column. The column is one of `?/~/-/=/#`,
//! optionally followed by `!` for the `Failed` variant.
//!
//! Layout contract:
//!   <compat-glyph><sp><dedup-glyph>[<!>]<sp><id>  <size>...
//!
//! - Position 0: compatibility glyph (unchanged from US-04: `o`/`*`/`!`/`?`).
//! - Position 1: separating space (unchanged).
//! - Position 2: dedup glyph (`?`/`~`/`-`/`=`/`#`).
//! - Position 3: either `!` decorator (Failed only) OR space.
//! - Position 4: separating space (only present when position 3 was `!`).
//!
//! Test budget: 6 distinct behaviors (5 glyphs + Failed decorator) +
//! 1 NO_COLOR invariance behavior = 7 tests max. We use 7.
//!
//! Each test enters through the pure `render_row_basic` driving port and
//! asserts on the returned `Line`'s plain text. No mocks, no I/O.

use modeltap_core::domain::dedup_glyph::DedupGlyph;
use modeltap_core::domain::RowIndicator;
use modeltap_tui::render::row::render_row_basic;
use ratatui::style::Style;
use ratatui::text::Line;

fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<Vec<_>>()
        .join("")
}

// ----------------------------------------------------------------------------
// Behavior 1-5: each DedupGlyph variant renders to its canonical character at
// position 2 (after `<compat-glyph><sp>`).
// ----------------------------------------------------------------------------

#[test]
fn pending_glyph_renders_question_mark_at_dedup_column() {
    let line = render_row_basic(
        "mistral:7b",
        4_500_000_000,
        RowIndicator::Compatible,
        &[],
        DedupGlyph::Pending,
        false,
    );
    let text = line_text(&line);
    let third_char = text.chars().nth(2).expect("position 2 must exist");
    assert_eq!(
        third_char, '?',
        "Pending must render `?` at position 2, got {:?}",
        text
    );
}

#[test]
fn hashing_glyph_renders_tilde_at_dedup_column() {
    let line = render_row_basic(
        "mistral:7b",
        4_500_000_000,
        RowIndicator::Compatible,
        &[],
        DedupGlyph::Hashing,
        false,
    );
    let text = line_text(&line);
    let third_char = text.chars().nth(2).expect("position 2 must exist");
    assert_eq!(
        third_char, '~',
        "Hashing must render `~` at position 2, got {:?}",
        text
    );
}

#[test]
fn unique_glyph_renders_dash_at_dedup_column() {
    let line = render_row_basic(
        "mistral:7b",
        4_500_000_000,
        RowIndicator::Compatible,
        &[],
        DedupGlyph::Unique,
        false,
    );
    let text = line_text(&line);
    let third_char = text.chars().nth(2).expect("position 2 must exist");
    assert_eq!(
        third_char, '-',
        "Unique must render `-` at position 2, got {:?}",
        text
    );
    // Unique must NOT carry the `!` decorator — the next char must be a space.
    let fourth_char = text.chars().nth(3).expect("position 3 must exist");
    assert_eq!(
        fourth_char, ' ',
        "Unique must render space (not `!`) at position 3, got {:?}",
        text
    );
}

#[test]
fn dedup_able_glyph_renders_equals_at_dedup_column() {
    let line = render_row_basic(
        "mistral:7b",
        4_500_000_000,
        RowIndicator::Shared,
        &[],
        DedupGlyph::DedupAble,
        false,
    );
    let text = line_text(&line);
    let third_char = text.chars().nth(2).expect("position 2 must exist");
    assert_eq!(
        third_char, '=',
        "DedupAble must render `=` at position 2, got {:?}",
        text
    );
}

#[test]
fn already_unified_glyph_renders_hash_at_dedup_column() {
    let line = render_row_basic(
        "mistral:7b",
        4_500_000_000,
        RowIndicator::Shared,
        &[],
        DedupGlyph::AlreadyUnified,
        false,
    );
    let text = line_text(&line);
    let third_char = text.chars().nth(2).expect("position 2 must exist");
    assert_eq!(
        third_char, '#',
        "AlreadyUnified must render `#` at position 2, got {:?}",
        text
    );
}

// ----------------------------------------------------------------------------
// Behavior 6: Failed renders `-` at position 2 AND `!` decorator at position 3.
// ----------------------------------------------------------------------------

#[test]
fn failed_glyph_renders_dash_with_bang_decorator() {
    let line = render_row_basic(
        "mistral:7b",
        4_500_000_000,
        RowIndicator::Compatible,
        &[],
        DedupGlyph::Failed,
        false,
    );
    let text = line_text(&line);
    let third_char = text.chars().nth(2).expect("position 2 must exist");
    let fourth_char = text.chars().nth(3).expect("position 3 must exist");
    assert_eq!(
        third_char, '-',
        "Failed must render `-` at position 2, got {:?}",
        text
    );
    assert_eq!(
        fourth_char, '!',
        "Failed must render `!` decorator at position 3, got {:?}",
        text
    );
}

// ----------------------------------------------------------------------------
// Behavior 7: NO_COLOR invariance — every dedup glyph (including the colored
// ones `=` and `#`) remains visible as plain text under no_color=true. No
// span carries an fg color.
// ----------------------------------------------------------------------------

#[test]
fn dedup_glyphs_remain_visible_under_no_color_with_no_styled_spans() {
    for (glyph, expected) in [
        (DedupGlyph::Pending, '?'),
        (DedupGlyph::Hashing, '~'),
        (DedupGlyph::Unique, '-'),
        (DedupGlyph::Failed, '-'),
        (DedupGlyph::DedupAble, '='),
        (DedupGlyph::AlreadyUnified, '#'),
    ] {
        let line = render_row_basic(
            "mistral:7b",
            4_500_000_000,
            RowIndicator::Compatible,
            &[],
            glyph,
            true, // no_color
        );
        let text = line_text(&line);
        let third_char = text.chars().nth(2).expect("position 2 must exist");
        assert_eq!(
            third_char, expected,
            "{:?} must render {:?} at position 2 even under no_color=true; got {:?}",
            glyph, expected, text
        );
        // Under no_color, every span must be Style::default() — no fg color
        // escape will be emitted by ratatui.
        for span in &line.spans {
            assert_eq!(
                span.style,
                Style::default(),
                "{:?}: span {:?} carries non-default style under no_color=true",
                glyph,
                span.content
            );
        }
    }
}
