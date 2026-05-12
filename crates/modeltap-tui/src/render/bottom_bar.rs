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
use modeltap_core::ToolId;
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
    /// The currently-selected real tool, when one is selected (synthetic
    /// `[All Unified]` slots produce `None`). Drives the US-05c AC-5 / AC-18
    /// dim of `[F] folder-delete` for non-HF active tools so the user never
    /// sees Shift+F as an available action when it cannot do anything.
    pub active_tool: Option<ToolId>,
    /// Maximum render width (in columns) for the bar, or `None` for
    /// unconstrained rendering. The production caller sets this from
    /// `area.width` so the bar can drop the lowest-priority entry
    /// (`[F] folder-delete`) when the full set of Main shortcuts would
    /// overflow and push `[q] quit` off the visible row. Unit tests that
    /// construct the context via `BarContext::for_state` keep `None`, so
    /// the historical "always show every applicable shortcut" contract
    /// holds when callers do not opt into width-aware filtering.
    pub max_width: Option<u16>,
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
        let active_tool = state.current_tool().map(|t| t.tool);
        Self {
            section,
            current_tool_has_models,
            detail,
            has_refresh_failures,
            focus: state.focus,
            active_tool,
            max_width: None,
        }
    }
}

/// Top-level frame entry point: render the bar widget into `area`.
///
/// Threads `area.width` into the `BarContext` so the bar can omit the
/// lowest-priority shortcut (`[F] folder-delete`) when the full Main set
/// would overflow at the terminal width — keeping `[q] quit` visible on
/// 100-col headless terminals (US-01 / INT-FGD-8 regression gate).
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let mut ctx = BarContext::for_state(state);
    ctx.max_width = Some(area.width);
    let line = render_bottom_bar(&ctx, no_color_active());
    frame.render_widget(Paragraph::new(line), area);
}

/// Pure: build the bar line from `SHORTCUT_TABLE` filtered by section.
/// Unavailable entries get an extra `Modifier::DIM`. This is the function
/// unit-tested at port granularity (US-08 B2..B5).
///
/// When `ctx.max_width` is `Some(w)` and the rendered Main bar would exceed
/// `w` columns, the lowest-priority entry (`[F] folder-delete`) is omitted
/// so the higher-priority `[?] help` and `[q] quit` shortcuts remain
/// visible. Callers that want every applicable shortcut regardless of
/// width leave `max_width = None` (the default from `for_state`).
pub fn render_bottom_bar(ctx: &BarContext<'_>, _no_color: bool) -> Line<'static> {
    let drop_folder_delete = should_drop_folder_delete(ctx);
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
        // Width-aware drop: omit [F] folder-delete entirely when the full
        // bar would not fit in the available terminal width. The Main bar
        // is the only section dense enough to overflow at 100 cols; Detail
        // / Help bars are shorter and unaffected.
        if drop_folder_delete && is_folder_delete_entry(entry) {
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

/// True when the bar would overflow `ctx.max_width` AND dropping
/// `[F] folder-delete` is the appropriate relief valve. The drop is
/// scoped to the Main section since the Detail and Help bars do not
/// include the entry; the Help section already always fits.
fn should_drop_folder_delete(ctx: &BarContext<'_>) -> bool {
    let max = match ctx.max_width {
        Some(w) => w,
        None => return false,
    };
    if ctx.section != BarSection::Main {
        return false;
    }
    full_bar_width(ctx) > max as usize
}

/// Compute the plain-text width the bar WOULD render at given the current
/// context — i.e. the sum of every applicable label plus the 2-char
/// separators between them. Used solely by `should_drop_folder_delete`
/// to decide whether to omit `[F] folder-delete`.
fn full_bar_width(ctx: &BarContext<'_>) -> usize {
    let mut total = 0usize;
    let mut first = true;
    for entry in SHORTCUT_TABLE {
        if !entry.sections.contains(&ctx.section) {
            continue;
        }
        if is_retry_entry(entry) && !ctx.has_refresh_failures {
            continue;
        }
        let label_len = if entry.key.code == crossterm::event::KeyCode::Up
            && entry.key.modifiers == crossterm::event::KeyModifiers::NONE
        {
            up_down_bar_label(ctx.focus).len()
        } else {
            entry.label.len()
        };
        if !first {
            total += 2; // "  " separator
        }
        first = false;
        total += label_len;
    }
    total
}

/// True when this entry is the `[F] folder-delete` shortcut. Identified by
/// KeyCode + SHIFT modifier (same shape as the AC-5 guard in `keymap`) so
/// the width-aware drop and the dispatch guard share the same predicate
/// shape — a future label rename would not silently bypass the drop.
fn is_folder_delete_entry(entry: &Shortcut) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    entry.key.code == KeyCode::Char('F') && entry.key.modifiers.contains(KeyModifiers::SHIFT)
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
    use crossterm::event::{KeyCode, KeyModifiers};
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
        // [F] folder-delete is HF-only (US-05c AC-5 / AC-18). Dim the entry
        // for any non-HF active tool AND for the synthetic [All Unified]
        // slot (`active_tool == None`). Mirrors the AC-5 guard in
        // `keymap::dispatch_with_active_tool` so the dim is paired with the
        // dispatch behavior — no color-only signalling (WCAG: the
        // CROSSED_OUT modifier carries the affordance for NO_COLOR users).
        KeyCode::Char('F') if entry.key.modifiers.contains(KeyModifiers::SHIFT) => {
            matches!(&ctx.active_tool, Some(t) if t == &ToolId("hf"))
        }
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
