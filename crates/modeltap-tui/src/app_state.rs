//! `AppState` — the Elm-style view-model for the TUI (per ADR-006).
//!
//! Pure data. No I/O. Constructed once at startup from the discovered
//! `Inventory`; updated by `update::update()`. The view function in
//! `render::*` reads `&AppState` and writes ratatui widgets.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use modeltap_core::domain::last_action::LastAction;
use modeltap_core::domain::synthetic_slot::LeftPaneSlot;
use modeltap_core::{ContentHash, DedupSummary, ToolId, ToolStatus};

use crate::dialogs::cross_fs_choice::CrossFsChoiceDialog;
pub use crate::dialogs::cross_fs_choice::{CrossFsChoice, CrossFsDecision, CrossFsMode};
use crate::dialogs::delete_one_confirm::DeleteOneConfirmState;
use crate::dialogs::running_tool_prompt::RunningToolDialog;
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

/// Live state of the background hash pool. All counters are derived from the
/// pool's `AtomicU64`s but cached on `AppState` so render fns stay pure.
///
/// Per `data-models.md` §HashPoolState — populated by hash-pool worker Msgs in
/// later steps (01-04 onwards). For step 01-03 the field exists with a
/// `Default` value so the render path can read it without panicking.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct HashPoolState {
    /// Total jobs queued at startup.
    pub total: u64,
    /// Jobs whose hash has been computed (success or failure).
    pub completed: u64,
    /// Model ids with a worker currently hashing them. Drives the `~` glyph.
    pub in_progress: BTreeSet<String>,
    /// Model ids whose hash failed. Drives the `-` + `!` decorator.
    pub failed: BTreeSet<String>,
    /// Per-row SHA256 cache populated by `Msg::HashComputed`. Read by the
    /// render-data assembly layer when building the `Inventory` argument to
    /// `compute_dedup_glyph` and `dedup_summary` so a row's classification
    /// reflects the just-computed hash without a re-discover. `BTreeMap` for
    /// deterministic iteration order.
    pub completed_hashes: BTreeMap<(ToolId, String), ContentHash>,
    /// Per-row `(device, inode)` cache populated by `Msg::HashComputed`.
    /// Same role as `completed_hashes` but for inode equality — feeds the
    /// `InodeMap` argument that distinguishes `AlreadyUnified` (`#`) from
    /// `DedupAble` (`=`).
    pub inodes: BTreeMap<(ToolId, String), (u64, u64)>,
}

impl HashPoolState {
    /// True while jobs remain. Used by render code for the "Hashing N/M..."
    /// summary indicator.
    pub fn is_hashing(&self) -> bool {
        self.completed < self.total
    }

    /// True iff hashing has started AND every job has finished.
    pub fn is_complete(&self) -> bool {
        self.total > 0 && self.completed == self.total
    }
}

/// Transient "(was X GB)" delta after a successful unify (US-10/US-11).
/// Cleared by a 5-second timer via `Msg::SummaryDeltaExpired` in later steps.
///
/// `Instant` is intentionally a transient runtime field — the field exists on
/// `AppState` for shape compatibility with the future expiry tick handler.
/// Step 01-03 never sets a non-`None` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryDelta {
    pub previous_dedup_able_bytes: u64,
    pub expires_at: Instant,
}

/// The pure view-model. Cloned per Elm `update` call. Per ADR-006, a few KB
/// of allocation per keystroke is negligible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    /// Left-pane slots in render order. Per ADR-014 the left pane is a
    /// heterogeneous list: `LeftPaneSlot::Real(ToolView)` for registered
    /// tools, `LeftPaneSlot::Synthetic(_)` for render-only entries such as
    /// the future `[All Unified]` slot.
    ///
    /// Real tools are sorted alphabetically by `tool.0` at construction time
    /// so navigation order is deterministic across plugin-registry orderings.
    /// The synthetic slot (when populated by step 04-02) is APPENDED LAST
    /// after all real tools. For step 01-03 only `Real(_)` entries appear.
    ///
    /// Selection navigation operates on `len()` unchanged — `selected_tool`
    /// indexes into this vec; both Real and Synthetic count as slot positions.
    pub left_pane_slots: Vec<LeftPaneSlot<ToolView>>,

    /// Index into `left_pane_slots` of the currently-selected slot. Always a
    /// valid index — invariant enforced by all constructors and updates.
    pub selected_tool: usize,

    /// Index into the current tool's `model_ids` of the highlighted row.
    /// Reset to 0 when the selected slot changes.
    pub selected_row: usize,

    /// First visible row in the right pane. Advances when `selected_row`
    /// would otherwise scroll off the bottom of the visible window.
    pub scroll_offset: usize,

    /// Number of right-pane rows visible at once. Set by the production
    /// interactive event loop on every tick from the actual terminal layout
    /// (see `modeltap_tui::right_pane_body_rows`); defaults to 28 (the
    /// @us-03 scenario value used by headless tests where there is no real
    /// terminal). On terminals shorter than the default, the live sync
    /// shrinks this value so `compute_scroll_offset` keeps the highlighted
    /// row inside the rendered window.
    pub visible_rows: usize,

    /// First visible row in the LEFT pane. Symmetric to `scroll_offset` but
    /// over `left_pane_slots`. Advances when `selected_tool` would otherwise
    /// scroll off the visible window.
    pub left_scroll_offset: usize,

    /// Number of left-pane rows visible at once. Set by the production
    /// interactive event loop on every tick from
    /// `modeltap_tui::left_pane_body_rows`. Defaults to 28 to match the
    /// right pane's headless-test default; the live sync overwrites it on
    /// real terminals.
    pub left_visible_rows: usize,

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

    /// `Some(...)` when the US-05b single-model delete dialog is open
    /// (ADR-009; step 03-06). Opened by `Msg::OpenDeleteOneDialog(state)`
    /// (orchestrator constructs it after `[d]` is pressed on the detail
    /// screen). Closed by `Msg::DeleteOneConfirmShared` /
    /// `Msg::DeleteOneCancelShared` / `Msg::DialogConfirm` (Unique typed-id
    /// path) / `Msg::DialogCancel`. Mutually exclusive with the other
    /// dialog slots.
    pub delete_one_dialog: Option<DeleteOneConfirmState>,

    /// `Some(...)` when the US-17 running-tool prompt is open (intake Q5).
    /// Opened by the composition root after `FsProbe::detect_running_tools`
    /// returns either `Ok(non_empty)` (Detected mode) or `Err(LsofUnavailable)`
    /// (LsofUnavailable mode). The prompt REFUSES the gated unify/delete-one
    /// action and offers `[r] retry / [Esc] cancel`. NO filesystem mutation
    /// may occur while this dialog is open. Closed by
    /// `Msg::RunningToolRetry` / `Msg::RunningToolCancel` /
    /// `Msg::RunningToolProceedAnyway` (LsofUnavailable mode only).
    pub running_tool_dialog: Option<RunningToolDialog>,

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

    /// Live state of the background hash pool. Driven by `Msg::HashComputed`
    /// / `Msg::HashFailed` / `Msg::HashProgressTick` in later steps; for step
    /// 01-03 always at its `Default` value.
    pub hash_state: HashPoolState,

    /// Cached classifier output for render. Recomputed by
    /// `logic::dedup::dedup_summary` on hash msgs and on action completion;
    /// carried explicitly so render fns stay pure. Step 01-03 leaves this at
    /// `Default` (all `None`) — populated in step 01-04+.
    pub dedup_summary: DedupSummary,

    /// Transient "(was X GB)" delta after unify (US-10). `Some(...)` for
    /// ~5 s after a successful unify; cleared by `Msg::SummaryDeltaExpired`.
    /// Step 01-03 never sets a non-`None` value.
    pub summary_delta: Option<SummaryDelta>,

    /// Transient `(tool, model_id)` highlight applied by the renderer for
    /// ~1 s after `Msg::UnifyHighlighted`. Cleared by
    /// `Msg::UnifyHighlightExpired` (the composition root dispatches this
    /// when the 1 s timer fires; lands in 01-08). Step 01-06 introduces the
    /// field; render integration lands in a later step.
    pub unify_highlight: Option<(ToolId, String)>,

    /// Single-line hint surfaced just below the summary bar. Set by
    /// `Msg::Unify` from the main view when the highlighted row's
    /// `DedupGlyph` is one of `{Unique, Pending, Hashing, Failed}` —
    /// the unify dialog cannot be opened in those states (no peers / hash
    /// not ready / hash failed), so a brief textual hint informs the user
    /// what to do next. Cleared on any nav Msg
    /// (`SelectNext/PrevTool`, `SelectNext/PrevRow`) so it disappears
    /// the moment the user moves away from the row that produced it.
    /// Step 01-10 introduces the field and the state-machine plumbing;
    /// visual surfacing in the right pane follows in a later step.
    pub status_line: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            left_pane_slots: Vec::new(),
            selected_tool: 0,
            selected_row: 0,
            scroll_offset: 0,
            visible_rows: 28,
            left_scroll_offset: 0,
            left_visible_rows: 28,
            focus: FocusPane::Left,
            should_quit: false,
            exit_code: 0,
            zap_dialog: None,
            unify_dialog: None,
            cross_fs_dialog: None,
            delete_one_dialog: None,
            running_tool_dialog: None,
            last_action: None,
            current_screen: Screen::Main,
            refresh_failed_tools: BTreeSet::new(),
            hash_state: HashPoolState::default(),
            dedup_summary: DedupSummary::default(),
            summary_delta: None,
            unify_highlight: None,
            status_line: None,
        }
    }
}

impl AppState {
    /// Construct an `AppState` from the discovered tool views. Sorts the
    /// tools alphabetically by `ToolId`, wraps each in `LeftPaneSlot::Real(_)`,
    /// and lands the default selection on the alphabetically-first INSTALLED
    /// tool. If no tool is installed, the selection falls back to index 0 so
    /// the right pane has something to render.
    ///
    /// Step 01-03 does NOT yet append the `[All Unified]` synthetic slot —
    /// that wiring lands in step 04-02. The constructor signature stays
    /// `Vec<ToolView>` for back-compat with v1 acceptance tests; internally
    /// each view is wrapped in `LeftPaneSlot::Real(_)`.
    pub fn new_with_default_selection(mut tools: Vec<ToolView>) -> Self {
        tools.sort_by(|a, b| a.tool.0.cmp(b.tool.0));
        let selected_tool = tools.iter().position(ToolView::is_installed).unwrap_or(0);
        let left_pane_slots = tools.into_iter().map(LeftPaneSlot::Real).collect();
        Self {
            left_pane_slots,
            selected_tool,
            selected_row: 0,
            scroll_offset: 0,
            visible_rows: 28,
            left_scroll_offset: 0,
            left_visible_rows: 28,
            focus: FocusPane::Left,
            should_quit: false,
            exit_code: 0,
            zap_dialog: None,
            unify_dialog: None,
            cross_fs_dialog: None,
            delete_one_dialog: None,
            running_tool_dialog: None,
            last_action: None,
            current_screen: Screen::Main,
            refresh_failed_tools: BTreeSet::new(),
            hash_state: HashPoolState::default(),
            dedup_summary: DedupSummary::default(),
            summary_delta: None,
            unify_highlight: None,
            status_line: None,
        }
    }

    /// Iterator over `&ToolView` for every `LeftPaneSlot::Real(_)` slot in
    /// render order. The synthetic slot (when present) is silently skipped —
    /// callers that need to operate on real tools only (summary aggregation,
    /// content-hash dedup) use this iterator. Mirror of the pre-refactor
    /// `state.tools.iter()` call site, with the `LeftPaneSlot::Real` arm
    /// lifted out so callers do not pattern-match.
    pub fn real_tools_iter(&self) -> impl Iterator<Item = &ToolView> + '_ {
        self.left_pane_slots.iter().filter_map(|slot| match slot {
            LeftPaneSlot::Real(view) => Some(view),
            LeftPaneSlot::Synthetic(_) => None,
        })
    }

    /// Mutable iterator counterpart to `real_tools_iter`. Used by
    /// `replace_tool_slot` in `update.rs` to mutate a single matching slot.
    pub fn real_tools_iter_mut(&mut self) -> impl Iterator<Item = &mut ToolView> + '_ {
        self.left_pane_slots
            .iter_mut()
            .filter_map(|slot| match slot {
                LeftPaneSlot::Real(view) => Some(view),
                LeftPaneSlot::Synthetic(_) => None,
            })
    }

    /// Borrow the `ToolView` at slot index `idx`, returning `None` for
    /// out-of-bounds OR when the slot at that index is synthetic (the caller
    /// must handle the synthetic arm explicitly — it has no `ToolView`).
    pub fn real_tool_at(&self, idx: usize) -> Option<&ToolView> {
        match self.left_pane_slots.get(idx) {
            Some(LeftPaneSlot::Real(view)) => Some(view),
            _ => None,
        }
    }

    /// Total number of rows in the currently-selected slot's right pane.
    /// Returns 0 when the selected slot is synthetic (the synthetic slot has
    /// no per-tool rows; step 04-02 will reroute the right pane to its own
    /// row source).
    pub fn current_row_count(&self) -> usize {
        self.real_tool_at(self.selected_tool)
            .map(|t| t.model_ids.len())
            .unwrap_or(0)
    }

    /// Currently-selected tool view, if any. Returns None when
    /// `left_pane_slots` is empty OR when the selected slot is synthetic.
    pub fn current_tool(&self) -> Option<&ToolView> {
        self.real_tool_at(self.selected_tool)
    }
}
