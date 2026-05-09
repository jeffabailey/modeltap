//! Bottom bar: shortcut line driven by `keymap::SHORTCUT_TABLE` (US-08).
//!
//! Render is pure: takes a `BarContext` (current screen, selection state,
//! NO_COLOR active flag) and returns a `Line<'static>` that can be
//! `frame.render_widget`'d. The bar always occupies exactly one row
//! (US-08 AC-1).
//!
//! Shortcut visibility is driven by `Shortcut.sections`: only entries whose
//! sections contain the currently-active `BarSection` are rendered. Inside
//! a section, each shortcut is either **active** (default DIM styling — the
//! whole bar is muted relative to the panes) or **unavailable** (extra
//! `Modifier::DIM` so the visual contrast against active shortcuts remains).
//! Unavailable shortcuts are NOT removed (US-08 AC-2): the user sees what's
//! possible elsewhere.
//!
//! Per ADR-006 the view layer is pure: this module reads inputs and writes
//! ratatui widgets; no I/O, no mutation, no env reads (NO_COLOR is hoisted
//! into `BarContext` by the caller).
//!
//! ## Source of truth
//!
//! All shortcut labels come from `keymap::SHORTCUT_TABLE`. No string
//! literals appear in this module that would duplicate a SHORTCUT_TABLE
//! entry; the architecture-lint test in `tests/architecture.rs` enforces
//! this across the whole render tree.

use modeltap_core::logic::unification_status::UnificationStatus;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app_state::{AppState, FocusPane, Screen};
use crate::keymap::{up_down_bar_label, BarSection, Shortcut, SHORTCUT_TABLE};
use crate::screens::detail::DetailScreenState;

/// Pure inputs the bar render fn needs. Constructed once per frame from
/// `AppState`; passing a `BarContext` (instead of `&AppState` directly)
/// keeps the render fn unit-testable without constructing a full AppState
/// for every test variation.
#[derive(Debug, Clone)]
pub struct BarContext<'a> {
    pub section: BarSection,
    /// True when the currently-selected tool has at least one model. Drives
    /// availability of `[z] zap tool` and `[u] unify` on the main bar.
    pub current_tool_has_models: bool,
    /// `Some(state)` when on the detail screen — drives availability of
    /// `[u] unify` (dimmed for SINGLE TOOL).
    pub detail: Option<&'a DetailScreenState>,
    /// True when `state.refresh_failed_tools` is non-empty (US-11.AC-2).
    /// Drives visibility of the `[r] retry` shortcut: the bar omits the
    /// entry entirely when this is false (no failure to retry).
    pub has_refresh_failures: bool,
    /// Which pane currently has keyboard focus. Drives the focus-aware
    /// Up/Down row label so the bar tells the truth ("tools" vs "models")
    /// in both focus states.
    pub focus: FocusPane,
}

impl<'a> BarContext<'a> {
    /// Derive the rendering context from an `AppState`. Keeps the bar
    /// render fn pure with respect to AppState shape changes.
    pub fn for_state(state: &'a AppState) -> Self {
        let section = match &state.current_screen {
            Screen::Main => BarSection::Main,
            Screen::Detail(_) => BarSection::Detail,
            Screen::Help { .. } => BarSection::Help,
        };
        let detail = match &state.current_screen {
            Screen::Detail(d) => Some(d),
            _ => None,
        };
        let current_tool_has_models = state
            .current_tool()
            .map(|t| !t.model_ids.is_empty())
            .unwrap_or(false);
        let has_refresh_failures = !state.refresh_failed_tools.is_empty();
        Self {
            section,
            current_tool_has_models,
            detail,
            has_refresh_failures,
            focus: state.focus,
        }
    }
}

/// Top-level frame entry point: render the bar widget into `area`.
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let ctx = BarContext::for_state(state);
    let line = render_bottom_bar(&ctx, no_color_active());
    frame.render_widget(Paragraph::new(line), area);
}

/// Pure: build the bar line from `SHORTCUT_TABLE` filtered by section.
/// Unavailable entries get an extra `Modifier::DIM`. This is the function
/// unit-tested at port granularity (US-08 B2..B5).
pub fn render_bottom_bar(ctx: &BarContext<'_>, _no_color: bool) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let active = Style::default().add_modifier(Modifier::DIM);
    let unavailable = Style::default()
        .add_modifier(Modifier::DIM)
        .add_modifier(Modifier::CROSSED_OUT);

    let mut first = true;
    for entry in SHORTCUT_TABLE {
        if !entry.sections.contains(&ctx.section) {
            continue;
        }
        // [r] retry is conditionally visible — omit entirely when no
        // refresh failures are pending (US-11.AC-2).
        if is_retry_entry(entry) && !ctx.has_refresh_failures {
            continue;
        }
        if !first {
            spans.push(Span::styled("  ", active));
        }
        first = false;

        let style = if is_available(entry, ctx) {
            active
        } else {
            unavailable
        };
        // Focus-aware Up/Down label: the SHORTCUT_TABLE Up row carries the
        // legacy "[up/down] models" label (authored when only the right pane
        // accepted Up/Down). With focus-aware dispatch, the same key now
        // navigates tools when the left pane has focus — substitute the
        // truthful per-focus label here so the bar matches dispatch reality.
        let label = if entry.key.code == crossterm::event::KeyCode::Up
            && entry.key.modifiers == crossterm::event::KeyModifiers::NONE
        {
            up_down_bar_label(ctx.focus)
        } else {
            entry.label
        };
        spans.push(Span::styled(label, style));
    }
    Line::from(spans)
}

/// Convert a rendered bar `Line` to plain text (concatenation of spans).
/// Used by the `int_6_invariant` property test and by acceptance tests
/// that match against `frame` strings.
pub fn bar_to_plain_string(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

// ---------------------------------------------------------------------------
// Availability predicate — pure
// ---------------------------------------------------------------------------

/// True when the shortcut is applicable to the current context. Returning
/// false dims the shortcut without removing it (US-08 AC-2).
fn is_available(entry: &Shortcut, ctx: &BarContext<'_>) -> bool {
    match ctx.section {
        BarSection::Main => is_available_main(entry, ctx),
        BarSection::Detail => is_available_detail(entry, ctx),
        // Help and Dialog bars are always available (small set; nothing to dim).
        BarSection::Help | BarSection::Dialog => true,
    }
}

fn is_available_main(entry: &Shortcut, ctx: &BarContext<'_>) -> bool {
    use crossterm::event::KeyCode;
    match entry.key.code {
        // [z] zap tool requires the current tool to have models. Empty tools
        // get the dialog as a benign nothing-to-zap, but the shortcut is
        // unavailable for the current selection per US-08.AC-2.
        KeyCode::Char('z') => ctx.current_tool_has_models,
        // [u] unify on main is unavailable when no model row is selected /
        // current tool is empty (you can't unify nothing). Future detail-
        // screen unify is the primary path.
        KeyCode::Char('u') => ctx.current_tool_has_models,
        // [d] delete-from-one is unavailable on the Unified virtual column
        // (current_tool() is None → current_tool_has_models is false) and
        // when the real tool is empty. Mirrors the production gating in
        // `interactive::lift_delete_one_in_main` so the bar matches what the
        // keystroke will actually do (RCA: fix-delete-one-hang Root Cause C).
        KeyCode::Char('d') => ctx.current_tool_has_models,
        // Everything else (arrows, ?, q) is always applicable on main.
        _ => true,
    }
}

fn is_available_detail(entry: &Shortcut, ctx: &BarContext<'_>) -> bool {
    use crossterm::event::KeyCode;
    let detail = match ctx.detail {
        Some(d) => d,
        None => return true,
    };
    match entry.key.code {
        // [u] unify is dimmed for SINGLE TOOL detail (nothing to unify).
        KeyCode::Char('u') => !matches!(detail.status(), UnificationStatus::SingleTool),
        // [d] delete-from-one is dimmed for SINGLE TOOL detail (deleting the
        // only registration would orphan the model).
        KeyCode::Char('d') => !matches!(detail.status(), UnificationStatus::SingleTool),
        _ => true,
    }
}

fn no_color_active() -> bool {
    crate::render::colors::no_color_active()
}

/// True when this entry is the `[r] retry` shortcut (US-11.AC-2). Identified
/// by KeyCode rather than label-string-equality so a future label tweak
/// (e.g. localization) cannot drift from the dispatch contract.
fn is_retry_entry(entry: &Shortcut) -> bool {
    use crossterm::event::KeyCode;
    matches!(entry.key.code, KeyCode::Char('r'))
        && entry.key.modifiers == crossterm::event::KeyModifiers::NONE
}
