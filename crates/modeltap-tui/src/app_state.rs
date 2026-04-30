//! `AppState` — the Elm-style view-model for the TUI (per ADR-006).
//!
//! Pure data. No I/O. Constructed once at startup from the discovered
//! `Inventory`; updated by `update::update()`. The view function in
//! `render::*` reads `&AppState` and writes ratatui widgets.

use std::collections::BTreeSet;

use modeltap_core::domain::last_action::LastAction;
use modeltap_core::{ToolId, ToolStatus};

use crate::dialogs::cross_fs_choice::CrossFsChoiceDialog;
pub use crate::dialogs::cross_fs_choice::{CrossFsChoice, CrossFsDecision, CrossFsMode};
use crate::dialogs::unify_confirm::UnifyDialogState;
use crate::dialogs::zap_confirm::ZapConfirmState;
use crate::screens::detail::DetailScreenState;

/// Top-level screen the TUI is currently displaying. The `view()` function in
/// `layout.rs` dispatches on this enum to the appropriate render path:
///
/// - `Main` — the two-pane discovery view (left: tools, right: rows).
/// - `Detail(state)` — the per-model detail screen (US-13).
/// - `Help { previous }` — the layered help overlay (US-08). Pressing `?`
///   from any screen wraps the current screen in `Help` so closing returns
///   to the exact prior state (selection, scroll, dialog, detail). The
///   `Box` is required because `Screen` is recursive through this variant.
///
/// Per ADR-006, screen state is pure data inside `AppState`. Screen
/// transitions are dispatched by `Msg::OpenDetail(...)` / `Msg::CloseDetail`
/// / `Msg::ToggleHelp`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// The default two-pane discovery view.
    Main,
    /// The per-model detail screen (US-13).
    Detail(DetailScreenState),
    /// The layered help overlay (US-08). `previous` is the screen `?` was
    /// pressed on; `Msg::ToggleHelp` (or Esc) restores it.
    Help { previous: Box<Screen> },
}

/// Which pane currently has focus. Tab toggles between them.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FocusPane {
    Left,
    Right,
}

/// One tool's view-projection: what the left pane displays and what the right
/// pane uses to render rows when the tool is selected. Step 01-03 keeps the
/// shape minimal — id strings + size bytes — because the row-detail rendering
/// (indicators, format labels, dedup status) lands in subsequent steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolView {
    pub tool: ToolId,
    pub status: ToolStatus,
    pub model_ids: Vec<String>,
    pub model_sizes_bytes: Vec<u64>,
}

impl ToolView {
    /// Total apparent size of this tool's models, in bytes. Equal to the sum
    /// of `model_sizes_bytes`; named accessor for left-pane display.
    pub fn total_bytes(&self) -> u64 {
        self.model_sizes_bytes.iter().sum()
    }

    /// True if `discover()` reported this tool as installed AND returned
    /// results. The default-selection algorithm picks the alphabetically-first
    /// such tool.
    pub fn is_installed(&self) -> bool {
        matches!(self.status, ToolStatus::Ok)
    }
}

/// The pure view-model. Cloned per Elm `update` call. Per ADR-006, a few KB
/// of allocation per keystroke is negligible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    /// Tool slots in left-pane render order. Sorted alphabetically by
    /// `tool.0` at construction time so navigation order is deterministic
    /// across plugin-registry orderings.
    pub tools: Vec<ToolView>,

    /// Index into `tools` of the currently-selected tool. Always a valid
    /// index — invariant enforced by all constructors and updates.
    pub selected_tool: usize,

    /// Index into `tools[selected_tool].model_ids` of the highlighted row.
    /// Reset to 0 when the selected tool changes.
    pub selected_row: usize,

    /// First visible row in the right pane. Advances when `selected_row`
    /// would otherwise scroll off the bottom of the visible window.
    pub scroll_offset: usize,

    /// Number of right-pane rows visible at once. Set by the renderer based
    /// on terminal height; defaults to 28 (the @us-03 scenario value).
    pub visible_rows: usize,

    /// Which pane has keyboard focus. Tab toggles.
    pub focus: FocusPane,

    /// True once the user has asked to quit. Composition root tears down the
    /// terminal and exits with `exit_code`.
    pub should_quit: bool,

    /// Exit code for `should_quit`. 0 = clean quit, 130 = SIGINT.
    pub exit_code: i32,

    /// `Some(...)` when the zap-confirmation dialog is open (US-05). All
    /// dialog state mutation flows through `update()`; the render layer reads
    /// this field and overlays a centered modal when set.
    pub zap_dialog: Option<ZapConfirmState>,

    /// `Some(...)` when the unify-confirmation dialog is open (US-10). Same
    /// rules as `zap_dialog` — mutation through `update()`, render overlay
    /// when set.
    pub unify_dialog: Option<UnifyDialogState>,

    /// `Some(...)` when the US-19 cross-filesystem fallback dialog is open.
    /// Opened by `Msg::OpenCrossFsDialog(plan)` and closed by any of
    /// `Msg::CrossFsSkip` / `Msg::CrossFsCopy` / `Msg::CrossFsCancel`. Mutually
    /// exclusive with `unify_dialog` — the orchestrator opens this dialog
    /// INSTEAD of the regular unify dialog when the plan has 1+ cross-fs target.
    pub cross_fs_dialog: Option<CrossFsChoiceDialog>,

    /// Structured last-action banner for the right pane (US-06). Set by
    /// `Msg::SetLastAction(...)` after an action effect completes; cleared
    /// on any navigation Msg (`SelectNextTool`, `SelectPrevTool`,
    /// `SelectNextRow`, `SelectPrevRow`). The render layer in
    /// `render::last_action` formats this into header + body lines.
    ///
    /// In-memory only (per intake Q7) — lost on restart. No persistent state.
    pub last_action: Option<LastAction>,

    /// Top-level screen currently displayed. Default `Screen::Main`. Set to
    /// `Screen::Detail(...)` by `Msg::OpenDetail` and reset to `Screen::Main`
    /// by `Msg::CloseDetail` (US-13).
    pub current_screen: Screen,

    /// Tools whose most-recent `refresh_tool_incremental` returned Err
    /// (US-11.AC-2). When non-empty, the summary bar appends a
    /// `(refresh failed)` indicator and the bottom bar exposes the `[r] retry`
    /// shortcut. Cleared per-tool on a successful `Msg::RefreshSucceeded`.
    /// `BTreeSet` for deterministic iteration order in render code.
    pub refresh_failed_tools: BTreeSet<ToolId>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            tools: Vec::new(),
            selected_tool: 0,
            selected_row: 0,
            scroll_offset: 0,
            visible_rows: 28,
            focus: FocusPane::Left,
            should_quit: false,
            exit_code: 0,
            zap_dialog: None,
            unify_dialog: None,
            cross_fs_dialog: None,
            last_action: None,
            current_screen: Screen::Main,
            refresh_failed_tools: BTreeSet::new(),
        }
    }
}

impl AppState {
    /// Construct an `AppState` from the discovered tool views. Sorts the
    /// tools alphabetically by `ToolId` and lands the default selection on
    /// the alphabetically-first INSTALLED tool. If no tool is installed,
    /// the selection falls back to index 0 so the right pane has something
    /// to render (an empty / not-installed message).
    pub fn new_with_default_selection(mut tools: Vec<ToolView>) -> Self {
        tools.sort_by(|a, b| a.tool.0.cmp(b.tool.0));
        let selected_tool = tools.iter().position(ToolView::is_installed).unwrap_or(0);
        Self {
            tools,
            selected_tool,
            selected_row: 0,
            scroll_offset: 0,
            visible_rows: 28,
            focus: FocusPane::Left,
            should_quit: false,
            exit_code: 0,
            zap_dialog: None,
            unify_dialog: None,
            cross_fs_dialog: None,
            last_action: None,
            current_screen: Screen::Main,
            refresh_failed_tools: BTreeSet::new(),
        }
    }

    /// Total number of rows in the currently-selected tool's right pane.
    pub fn current_row_count(&self) -> usize {
        self.tools
            .get(self.selected_tool)
            .map(|t| t.model_ids.len())
            .unwrap_or(0)
    }

    /// Currently-selected tool view, if any. Returns None only if `tools` is
    /// empty (no plugins registered — pathological case).
    pub fn current_tool(&self) -> Option<&ToolView> {
        self.tools.get(self.selected_tool)
    }
}
