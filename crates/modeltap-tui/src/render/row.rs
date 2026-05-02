//! Pure render fn for one right-pane model row (US-04 + US-U3).
//!
//! Canonical row format (after step 01-05 inserts the dedup-glyph column):
//!
//! ```text
//! <compat> <dedup>  <id>  <size>            // Compatible / FormatLocked / Unknown
//! <compat> <dedup>  <id>  <size>  also in: <other tools>   // Shared
//! <compat> -!       <id>  <size>            // Failed dedup decorator
//! ```
//!
//! - `<compat>` is one of `{o, *, !, ?}` (US-04 compatibility indicator).
//! - `<dedup>` is one of `{?, ~, -, =, #}` (US-U3 dedup-state glyph). The
//!   `Failed` variant adds a `!` decorator immediately after the `-`. See
//!   `modeltap_core::domain::dedup_glyph::DedupGlyph`.
//!
//! Color rules are paired with both glyphs (see `render::indicator` and
//! `dedup_glyph_style` below). Pure function — takes data, returns a ratatui
//! `Line`. No I/O, no env reads (NO_COLOR is queried by the caller via
//! `render::colors::no_color_active()` and passed in).
//!
//! ## Format-field rendering
//!
//! When the indicator is `Unknown`, the row renders `[format: ?]` after the
//! size column to surface the unparseability per US-04.AC requirements. The
//! `?` in `[format: ?]` is unrelated to the new dedup-glyph column at
//! position 2; the two columns are independent concerns.

use modeltap_core::domain::dedup_glyph::DedupGlyph;
use modeltap_core::domain::RowIndicator;
use modeltap_core::{DiscoveredModel, Format, ToolId};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::render::indicator::{indicator_glyph, indicator_style};

/// The text of the dedup column for a given `DedupGlyph`. Returns either a
/// single-character glyph (`?`/`~`/`-`/`=`/`#`) or `-!` for the `Failed`
/// variant where the trailing `!` is the conservative-when-uncertain decorator
/// (BR-3) — the row was unique-by-default because the hash failed.
fn dedup_glyph_text(glyph: DedupGlyph) -> &'static str {
    match glyph {
        DedupGlyph::Pending => "?",
        DedupGlyph::Hashing => "~",
        DedupGlyph::Unique => "-",
        DedupGlyph::Failed => "-!",
        DedupGlyph::DedupAble => "=",
        DedupGlyph::AlreadyUnified => "#",
    }
}

/// Color rules for the dedup-glyph column. Paired with the glyph so the row
/// is distinguishable by glyph alone (NFR-6, NO_COLOR contract). Under
/// `no_color = true` every variant returns `Style::default()` so no ANSI
/// color escape is emitted; the glyph itself is the sole carrier of state.
fn dedup_glyph_style(glyph: DedupGlyph, no_color: bool) -> Style {
    if no_color {
        return Style::default();
    }
    match glyph {
        // The two cross-tool-share variants get a hint of color in addition
        // to their distinctive glyph; the others stay neutral.
        DedupGlyph::DedupAble => Style::default().fg(Color::Cyan),
        DedupGlyph::AlreadyUnified => Style::default().fg(Color::Green),
        DedupGlyph::Pending | DedupGlyph::Hashing | DedupGlyph::Unique | DedupGlyph::Failed => {
            Style::default()
        }
    }
}

/// Append the styled dedup-glyph column (and its trailing space) to `spans`.
/// Layout: `<dedup-glyph>[<!>]<sp>`. The `!` decorator is bundled inside the
/// styled glyph span so a single fg-color setting covers both characters.
fn push_dedup_column<'a>(spans: &mut Vec<Span<'a>>, dedup: DedupGlyph, no_color: bool) {
    let dedup_text = dedup_glyph_text(dedup);
    let dedup_style = dedup_glyph_style(dedup, no_color);
    spans.push(Span::styled(dedup_text.to_string(), dedup_style));
    spans.push(Span::raw(" "));
}

/// Render one model row as a ratatui `Line`.
///
/// Layout: `<compat-glyph><sp><dedup-glyph>[<!>]<sp><id>  <size>` with an
/// optional `  also in: …` suffix when `indicator == Shared`. The first
/// character is always the compatibility-indicator glyph (unchanged from
/// US-04); the dedup glyph appears at position 2.
///
/// `also_in` lists OTHER tools that also have this model — pass an empty
/// slice for non-Shared indicators.
///
/// `dedup`: the per-row dedup state glyph (computed by
/// `modeltap_core::logic::dedup::compute_dedup_glyph` at render-data
/// assembly time).
///
/// `no_color`: when true, every span is `Style::default()` (no color escapes
/// will be emitted by ratatui). Per the WCAG contract both glyphs are always
/// present, regardless of `no_color`.
pub fn render_row<'a>(
    model: &'a DiscoveredModel,
    indicator: RowIndicator,
    also_in: &[ToolId],
    dedup: DedupGlyph,
    no_color: bool,
) -> Line<'a> {
    let glyph = indicator_glyph(indicator);
    let glyph_style = indicator_style(indicator, no_color);

    let size = format_size(model.size_bytes);

    let mut spans: Vec<Span<'a>> = Vec::with_capacity(8);
    // Compatibility-indicator glyph (styled).
    spans.push(Span::styled(glyph.to_string(), glyph_style));
    // Single space separator after the compat glyph.
    spans.push(Span::raw(" "));
    // Dedup glyph + optional `!` decorator + trailing space.
    push_dedup_column(&mut spans, dedup, no_color);
    // Model id.
    spans.push(Span::raw(model.id_in_tool.clone()));
    // Two-space gap then size.
    spans.push(Span::raw(format!("  {}", size)));
    // `[format: ?]` for Unknown rows so the unparseability is visible without
    // relying on color alone.
    if matches!(indicator, RowIndicator::Unknown) || matches!(model.format, Format::Other) {
        spans.push(Span::raw("  [format: ?]"));
    }
    // `also in: <comma-separated other tools>` for Shared rows.
    if matches!(indicator, RowIndicator::Shared) && !also_in.is_empty() {
        let names: Vec<&str> = also_in.iter().map(|t| t.0).collect();
        spans.push(Span::raw(format!("  also in: {}", names.join(", "))));
    }

    Line::from(spans)
}

/// Render-row variant that takes a flat (id, size) pair instead of a full
/// `DiscoveredModel`. The right pane uses this in 02-01 because the view-
/// model `ToolView` only carries id+size; 02-05 will plumb the full
/// `DiscoveredModel` through and `render_row_basic` can be deleted in favor
/// of `render_row` everywhere.
pub fn render_row_basic<'a>(
    id_in_tool: &'a str,
    size_bytes: u64,
    indicator: RowIndicator,
    also_in: &[ToolId],
    dedup: DedupGlyph,
    no_color: bool,
) -> Line<'a> {
    let glyph = indicator_glyph(indicator);
    let glyph_style = indicator_style(indicator, no_color);

    let size = format_size(size_bytes);

    let mut spans: Vec<Span<'a>> = Vec::with_capacity(8);
    spans.push(Span::styled(glyph.to_string(), glyph_style));
    spans.push(Span::raw(" "));
    push_dedup_column(&mut spans, dedup, no_color);
    spans.push(Span::raw(id_in_tool.to_string()));
    spans.push(Span::raw(format!("  {}", size)));
    if matches!(indicator, RowIndicator::Unknown) {
        spans.push(Span::raw("  [format: ?]"));
    }
    if matches!(indicator, RowIndicator::Shared) && !also_in.is_empty() {
        let names: Vec<&str> = also_in.iter().map(|t| t.0).collect();
        spans.push(Span::raw(format!("  also in: {}", names.join(", "))));
    }

    Line::from(spans)
}

/// Display-formatter for byte counts. Identical to `right_pane::format_size`;
/// kept here so the row module is self-contained.
fn format_size(bytes: u64) -> String {
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

/// Serialize a Line into its plain-text content (concatenation of span
/// contents). Used by tests for byte-level assertions; not used in production
/// rendering.
#[cfg(test)]
fn line_text(line: &Line<'_>) -> String {
    line.spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_core::types::{DisplayLabel, ModelStatus};
    use ratatui::style::Style;
    use std::path::PathBuf;

    fn model(id: &str, format: Format, size_bytes: u64) -> DiscoveredModel {
        DiscoveredModel {
            id_in_tool: id.to_string(),
            on_disk_path: PathBuf::from("/tmp/x"),
            size_bytes,
            format,
            display_label: DisplayLabel::from(id),
            status: ModelStatus::Healthy,
        }
    }

    // -----------------------------------------------------------------------
    // First character is the indicator glyph (US-04.AC-1).
    // -----------------------------------------------------------------------

    #[test]
    fn render_row_first_character_is_o_for_compatible() {
        let m = model("mistral:7b", Format::Gguf, 4_500_000_000);
        let line = render_row(
            &m,
            RowIndicator::Compatible,
            &[],
            DedupGlyph::Pending,
            false,
        );
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('o'), "got: {:?}", text);
    }

    #[test]
    fn render_row_first_character_is_star_for_shared() {
        let m = model("mistral:7b", Format::Gguf, 4_500_000_000);
        let others = vec![ToolId("llama-cli")];
        let line = render_row(
            &m,
            RowIndicator::Shared,
            &others,
            DedupGlyph::Pending,
            false,
        );
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('*'), "got: {:?}", text);
    }

    #[test]
    fn render_row_first_character_is_bang_for_format_locked() {
        let m = model("mistral:7b", Format::OllamaBlob, 4_500_000_000);
        let line = render_row(
            &m,
            RowIndicator::FormatLocked,
            &[],
            DedupGlyph::Pending,
            false,
        );
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('!'), "got: {:?}", text);
    }

    #[test]
    fn render_row_first_character_is_question_for_unknown() {
        let m = model("mystery", Format::Other, 4_500_000_000);
        let line = render_row(&m, RowIndicator::Unknown, &[], DedupGlyph::Pending, false);
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('?'), "got: {:?}", text);
    }

    // -----------------------------------------------------------------------
    // Size and id appear in the row (basic schema check).
    // -----------------------------------------------------------------------

    #[test]
    fn render_row_includes_id_and_size() {
        let m = model("mistral:7b-q4", Format::Gguf, 4_500_000_000);
        let line = render_row(
            &m,
            RowIndicator::Compatible,
            &[],
            DedupGlyph::Pending,
            false,
        );
        let text = line_text(&line);
        assert!(text.contains("mistral:7b-q4"), "got: {:?}", text);
        assert!(text.contains("4.5 GB"), "got: {:?}", text);
    }

    // -----------------------------------------------------------------------
    // `also in: …` annotation appears only for Shared rows (US-04.AC-2).
    // -----------------------------------------------------------------------

    #[test]
    fn render_row_shared_includes_also_in_with_comma_separated_other_tools() {
        let m = model("mistral:7b", Format::Gguf, 4_500_000_000);
        let others = vec![ToolId("llama-cli"), ToolId("hf")];
        let line = render_row(
            &m,
            RowIndicator::Shared,
            &others,
            DedupGlyph::Pending,
            false,
        );
        let text = line_text(&line);
        assert!(text.contains("also in: llama-cli, hf"), "got: {:?}", text);
    }

    #[test]
    fn render_row_compatible_does_not_include_also_in() {
        let m = model("mistral:7b", Format::Gguf, 4_500_000_000);
        let line = render_row(
            &m,
            RowIndicator::Compatible,
            &[],
            DedupGlyph::Pending,
            false,
        );
        let text = line_text(&line);
        assert!(!text.contains("also in:"), "got: {:?}", text);
    }

    #[test]
    fn render_row_shared_with_empty_also_in_does_not_include_annotation() {
        // Defensive: classifier could in principle return Shared with an
        // empty other-tools list (it wouldn't, but the render fn must not
        // crash and must not emit a dangling "also in: " label).
        let m = model("mistral:7b", Format::Gguf, 4_500_000_000);
        let line = render_row(&m, RowIndicator::Shared, &[], DedupGlyph::Pending, false);
        let text = line_text(&line);
        assert!(!text.contains("also in:"), "got: {:?}", text);
    }

    // -----------------------------------------------------------------------
    // Unknown row carries `[format: ?]`.
    // -----------------------------------------------------------------------

    #[test]
    fn render_row_unknown_includes_format_question_field() {
        let m = model("mystery", Format::Other, 1_000_000_000);
        let line = render_row(&m, RowIndicator::Unknown, &[], DedupGlyph::Pending, false);
        let text = line_text(&line);
        assert!(text.contains("[format: ?]"), "got: {:?}", text);
    }

    // -----------------------------------------------------------------------
    // NO_COLOR: render_row with no_color=true produces a Line whose styles
    // are all Style::default() — no fg color set anywhere.
    // -----------------------------------------------------------------------

    #[test]
    fn render_row_no_color_produces_no_styled_spans() {
        // FormatLocked would normally be red; under no_color it must be plain.
        let m = model("mistral:7b", Format::Gguf, 4_500_000_000);
        let line = render_row(
            &m,
            RowIndicator::FormatLocked,
            &[],
            DedupGlyph::Pending,
            true,
        );
        for span in &line.spans {
            assert_eq!(
                span.style,
                Style::default(),
                "span {:?} carries non-default style under no_color=true",
                span.content
            );
        }
    }

    #[test]
    fn render_row_no_color_unknown_glyph_is_present_but_unstyled() {
        let m = model("mystery", Format::Other, 1_000_000_000);
        let line = render_row(&m, RowIndicator::Unknown, &[], DedupGlyph::Pending, true);
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('?'));
        for span in &line.spans {
            assert_eq!(span.style, Style::default());
        }
    }

    // -----------------------------------------------------------------------
    // Property: every (indicator, model) combination produces a row whose
    // first character is in {o, *, !, ?}.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // render_row_basic — flat (id, size) variant used by right_pane in 02-01.
    // -----------------------------------------------------------------------

    #[test]
    fn render_row_basic_compatible_starts_with_o_and_contains_id_size() {
        let line = render_row_basic(
            "mistral:7b",
            4_500_000_000,
            RowIndicator::Compatible,
            &[],
            DedupGlyph::Pending,
            false,
        );
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('o'));
        assert!(text.contains("mistral:7b"));
        assert!(text.contains("4.5 GB"));
        assert!(!text.contains("also in:"));
    }

    #[test]
    fn render_row_basic_shared_includes_also_in() {
        let others = vec![ToolId("llama-cli"), ToolId("hf")];
        let line = render_row_basic(
            "mistral:7b",
            4_500_000_000,
            RowIndicator::Shared,
            &others,
            DedupGlyph::Pending,
            false,
        );
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('*'));
        assert!(text.contains("also in: llama-cli, hf"));
    }

    #[test]
    fn render_row_basic_unknown_includes_format_question_field() {
        let line = render_row_basic(
            "mystery",
            1_000_000_000,
            RowIndicator::Unknown,
            &[],
            DedupGlyph::Pending,
            false,
        );
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('?'));
        assert!(text.contains("[format: ?]"));
    }

    #[test]
    fn property_first_char_always_in_indicator_universe() {
        let formats = [
            Format::Gguf,
            Format::Safetensors,
            Format::Bin,
            Format::Awq,
            Format::Gptq,
            Format::OllamaBlob,
            Format::Mlx,
            Format::Other,
        ];
        let indicators = [
            RowIndicator::Compatible,
            RowIndicator::Shared,
            RowIndicator::FormatLocked,
            RowIndicator::Unknown,
        ];
        let sizes = [0_u64, 1, 1_000_000, 1_000_000_000, 4_500_000_000];
        let ids = ["a", "x:y-z", "really-long-model-id-with-tag:q4_K_M"];
        let dedups = [
            DedupGlyph::Pending,
            DedupGlyph::Hashing,
            DedupGlyph::Unique,
            DedupGlyph::Failed,
            DedupGlyph::DedupAble,
            DedupGlyph::AlreadyUnified,
        ];
        for fmt in formats {
            for ind in indicators {
                for size in sizes {
                    for id in ids {
                        for &no_color in &[false, true] {
                            for dedup in dedups {
                                let m = model(id, fmt, size);
                                let others = vec![ToolId("llama-cli")];
                                let line = render_row(&m, ind, &others, dedup, no_color);
                                let text = line_text(&line);
                                let first = text.chars().next().expect("non-empty");
                                assert!(
                                    matches!(first, 'o' | '*' | '!' | '?'),
                                    "first char {:?} not in {{o,*,!,?}} for fmt={:?} ind={:?} size={} id={:?} no_color={} dedup={:?}",
                                    first, fmt, ind, size, id, no_color, dedup
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
