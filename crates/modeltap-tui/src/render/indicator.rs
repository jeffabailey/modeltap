//! Render-side helpers for the row-indicator glyph and color (US-04.AC-1, AC-3).
//!
//! Pure functions over `RowIndicator`. The classifier itself lives in
//! `modeltap_core::domain::indicator`; this module only converts the enum to
//! a glyph + a ratatui `Style`.
//!
//! Color rules (paired with the glyph so NO_COLOR-compliant — WCAG):
//!   - `Compatible` (`o`) → neutral
//!   - `Shared` (`*`)     → neutral
//!   - `FormatLocked` (`!`) → red
//!   - `Unknown` (`?`)    → yellow
//!
//! When `no_color = true`, all variants return a plain `Style::default()` so
//! no ANSI color escapes are emitted; the glyph remains the sole carrier of
//! the distinction.

use modeltap_core::domain::RowIndicator;
use ratatui::style::{Color, Style};

/// The single character rendered as the row's first column.
pub fn indicator_glyph(ind: RowIndicator) -> char {
    match ind {
        RowIndicator::Compatible => 'o',
        RowIndicator::Shared => '*',
        RowIndicator::FormatLocked => '!',
        RowIndicator::Unknown => '?',
    }
}

/// The ratatui `Style` applied to the glyph. With `no_color`, every variant
/// returns `Style::default()` so the only visible distinction is the glyph
/// itself (NO_COLOR + WCAG color-independence contract — US-04.AC-3).
pub fn indicator_style(ind: RowIndicator, no_color: bool) -> Style {
    if no_color {
        return Style::default();
    }
    match ind {
        RowIndicator::Compatible | RowIndicator::Shared => Style::default(),
        RowIndicator::FormatLocked => Style::default().fg(Color::Red),
        RowIndicator::Unknown => Style::default().fg(Color::Yellow),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // indicator_glyph — exhaustive enum-to-char mapping.
    // -----------------------------------------------------------------------

    #[test]
    fn indicator_glyph_maps_compatible_to_o() {
        assert_eq!(indicator_glyph(RowIndicator::Compatible), 'o');
    }

    #[test]
    fn indicator_glyph_maps_shared_to_star() {
        assert_eq!(indicator_glyph(RowIndicator::Shared), '*');
    }

    #[test]
    fn indicator_glyph_maps_format_locked_to_bang() {
        assert_eq!(indicator_glyph(RowIndicator::FormatLocked), '!');
    }

    #[test]
    fn indicator_glyph_maps_unknown_to_question() {
        assert_eq!(indicator_glyph(RowIndicator::Unknown), '?');
    }

    // -----------------------------------------------------------------------
    // indicator_style — color applied only when no_color = false; ! red, ? yellow.
    // -----------------------------------------------------------------------

    #[test]
    fn indicator_style_format_locked_is_red_when_color_allowed() {
        let s = indicator_style(RowIndicator::FormatLocked, false);
        assert_eq!(s.fg, Some(Color::Red));
    }

    #[test]
    fn indicator_style_unknown_is_yellow_when_color_allowed() {
        let s = indicator_style(RowIndicator::Unknown, false);
        assert_eq!(s.fg, Some(Color::Yellow));
    }

    #[test]
    fn indicator_style_compatible_is_neutral_when_color_allowed() {
        let s = indicator_style(RowIndicator::Compatible, false);
        assert_eq!(s.fg, None);
    }

    #[test]
    fn indicator_style_shared_is_neutral_when_color_allowed() {
        let s = indicator_style(RowIndicator::Shared, false);
        assert_eq!(s.fg, None);
    }

    #[test]
    fn indicator_style_no_color_strips_all_colors() {
        // Every variant returns Style::default() under no_color = true — even
        // the variants that would otherwise be red/yellow.
        for ind in [
            RowIndicator::Compatible,
            RowIndicator::Shared,
            RowIndicator::FormatLocked,
            RowIndicator::Unknown,
        ] {
            let s = indicator_style(ind, true);
            assert_eq!(
                s.fg, None,
                "{:?} must have no fg color when no_color=true",
                ind
            );
        }
    }
}
