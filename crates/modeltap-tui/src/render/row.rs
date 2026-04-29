//! Pure render fn for one right-pane model row (US-04).
//!
//! Canonical row format:
//!
//! ```text
//! <indicator> <id>  <size>            // Compatible / FormatLocked / Unknown
//! <indicator> <id>  <size>  also in: <other tools>   // Shared
//! ```
//!
//! Where `<indicator>` is one of `{o, *, !, ?}` and color rules are paired
//! with the glyph (see `render::indicator`). Pure function — takes data,
//! returns a ratatui `Line`. No I/O, no env reads (NO_COLOR is queried by
//! the caller via `render::colors::no_color_active()` and passed in).
//!
//! ## Format-field rendering
//!
//! When the indicator is `Unknown`, the row renders `[format: ?]` after the
//! size column to surface the unparseability per US-04.AC requirements. For
//! the other indicators the format is omitted from the row text in 02-01;
//! 02-05 will introduce a richer format field.

use modeltap_core::domain::RowIndicator;
use modeltap_core::{DiscoveredModel, Format, ToolId};
use ratatui::text::{Line, Span};

use crate::render::indicator::{indicator_glyph, indicator_style};

/// Render one model row as a ratatui `Line`. The first character is always
/// the indicator glyph; the second character is a space; the rest of the
/// line is `<id>  <size>` with an optional `  also in: …` suffix when
/// `indicator == Shared`.
///
/// `also_in` lists OTHER tools that also have this model — pass an empty
/// slice for non-Shared indicators.
///
/// `no_color`: when true, every span is `Style::default()` (no color escapes
/// will be emitted by ratatui). Per the WCAG contract the indicator GLYPH is
/// always present, regardless of `no_color`.
pub fn render_row<'a>(
    model: &'a DiscoveredModel,
    indicator: RowIndicator,
    also_in: &[ToolId],
    no_color: bool,
) -> Line<'a> {
    let glyph = indicator_glyph(indicator);
    let glyph_style = indicator_style(indicator, no_color);

    let size = format_size(model.size_bytes);

    let mut spans: Vec<Span<'a>> = Vec::with_capacity(6);
    // Indicator glyph (styled).
    spans.push(Span::styled(glyph.to_string(), glyph_style));
    // Single space separator after the glyph.
    spans.push(Span::raw(" "));
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
    no_color: bool,
) -> Line<'a> {
    let glyph = indicator_glyph(indicator);
    let glyph_style = indicator_style(indicator, no_color);

    let size = format_size(size_bytes);

    let mut spans: Vec<Span<'a>> = Vec::with_capacity(6);
    spans.push(Span::styled(glyph.to_string(), glyph_style));
    spans.push(Span::raw(" "));
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
        let line = render_row(&m, RowIndicator::Compatible, &[], false);
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('o'), "got: {:?}", text);
    }

    #[test]
    fn render_row_first_character_is_star_for_shared() {
        let m = model("mistral:7b", Format::Gguf, 4_500_000_000);
        let others = vec![ToolId("llama-cli")];
        let line = render_row(&m, RowIndicator::Shared, &others, false);
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('*'), "got: {:?}", text);
    }

    #[test]
    fn render_row_first_character_is_bang_for_format_locked() {
        let m = model("mistral:7b", Format::OllamaBlob, 4_500_000_000);
        let line = render_row(&m, RowIndicator::FormatLocked, &[], false);
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('!'), "got: {:?}", text);
    }

    #[test]
    fn render_row_first_character_is_question_for_unknown() {
        let m = model("mystery", Format::Other, 4_500_000_000);
        let line = render_row(&m, RowIndicator::Unknown, &[], false);
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('?'), "got: {:?}", text);
    }

    // -----------------------------------------------------------------------
    // Size and id appear in the row (basic schema check).
    // -----------------------------------------------------------------------

    #[test]
    fn render_row_includes_id_and_size() {
        let m = model("mistral:7b-q4", Format::Gguf, 4_500_000_000);
        let line = render_row(&m, RowIndicator::Compatible, &[], false);
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
        let line = render_row(&m, RowIndicator::Shared, &others, false);
        let text = line_text(&line);
        assert!(text.contains("also in: llama-cli, hf"), "got: {:?}", text);
    }

    #[test]
    fn render_row_compatible_does_not_include_also_in() {
        let m = model("mistral:7b", Format::Gguf, 4_500_000_000);
        let line = render_row(&m, RowIndicator::Compatible, &[], false);
        let text = line_text(&line);
        assert!(!text.contains("also in:"), "got: {:?}", text);
    }

    #[test]
    fn render_row_shared_with_empty_also_in_does_not_include_annotation() {
        // Defensive: classifier could in principle return Shared with an
        // empty other-tools list (it wouldn't, but the render fn must not
        // crash and must not emit a dangling "also in: " label).
        let m = model("mistral:7b", Format::Gguf, 4_500_000_000);
        let line = render_row(&m, RowIndicator::Shared, &[], false);
        let text = line_text(&line);
        assert!(!text.contains("also in:"), "got: {:?}", text);
    }

    // -----------------------------------------------------------------------
    // Unknown row carries `[format: ?]`.
    // -----------------------------------------------------------------------

    #[test]
    fn render_row_unknown_includes_format_question_field() {
        let m = model("mystery", Format::Other, 1_000_000_000);
        let line = render_row(&m, RowIndicator::Unknown, &[], false);
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
        let line = render_row(&m, RowIndicator::FormatLocked, &[], true);
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
        let line = render_row(&m, RowIndicator::Unknown, &[], true);
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
            false,
        );
        let text = line_text(&line);
        assert_eq!(text.chars().next(), Some('*'));
        assert!(text.contains("also in: llama-cli, hf"));
    }

    #[test]
    fn render_row_basic_unknown_includes_format_question_field() {
        let line = render_row_basic("mystery", 1_000_000_000, RowIndicator::Unknown, &[], false);
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
        for fmt in formats {
            for ind in indicators {
                for size in sizes {
                    for id in ids {
                        for &no_color in &[false, true] {
                            let m = model(id, fmt, size);
                            let others = vec![ToolId("llama-cli")];
                            let line = render_row(&m, ind, &others, no_color);
                            let text = line_text(&line);
                            let first = text.chars().next().expect("non-empty");
                            assert!(
                                matches!(first, 'o' | '*' | '!' | '?'),
                                "first char {:?} not in {{o,*,!,?}} for fmt={:?} ind={:?} size={} id={:?} no_color={}",
                                first, fmt, ind, size, id, no_color
                            );
                        }
                    }
                }
            }
        }
    }
}
