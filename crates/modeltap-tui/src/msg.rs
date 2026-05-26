//! `Msg` — the Elm-style message type for the TUI (per ADR-006).
//!
//! Every keystroke (and every async-arriving event in later steps) becomes
//! one variant of this enum. The pure `update::update()` function consumes
//! a `Msg` and returns the next `AppState` plus a description of any side
//! effects (`UpdateEffect`).

use modeltap_core::domain::inspect::ToolDetail;
use modeltap_core::domain::last_action::LastAction;
use modeltap_core::logic::plan::UnifyPlan;
use modeltap_core::{ContentHash, ToolId};

use crate::app_state::ToolView;
use crate::dialogs::delete_one_confirm::DeleteOneConfirmState;
use crate::dialogs::running_tool_prompt::RunningToolDialog;
use crate::effects::unify_outcome::UnifyOutcome;
use crate::screens::detail::{DetailScreenState, MetadataSection};

/// Scope for the user-initiated refresh hotkeys (US-24, step 05-03).
///
/// Carried inside `Msg::RequestRefresh(_)` so the keymap stays a pure
/// `KeyEvent -> Msg` translation and the composition root resolves the
/// scope into the orchestrator's own `ReconcileScope` at dispatch time.
///
/// This mirror type exists because `modeltap-tui` MUST NOT import
/// `modeltap-app::orchestration::*` — architecture layer R7: the TUI is
/// a pure view-model crate, the composition root in `modeltap-app` is the
/// only place orchestrators and the TUI come together. The mapping
/// `RefreshScope -> orchestration::reconcile::ReconcileScope` lives in the
/// composition root (`modeltap-app::interactive` / `headless`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshScope {
    /// Refresh every registered plugin (dispatched on `[Shift+R]`).
    All,
    /// Refresh exactly one plugin (dispatched on `[r]` with the
    /// currently-selected left-pane tool).
    Tool(ToolId),
}

/// Reason a hash-pool worker reported a failure for a given (tool, model_id).
/// Carried inside `Msg::HashFailed` so the renderer / observability layer can
/// distinguish read errors from cancellation. Per BR-3 the classifier treats
/// every failure mode identically (Unique sentinel + `!` decorator) — this
/// taxonomy is for diagnostics only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HashFailureReason {
    /// Read error / permission / EIO. The string is the underlying message
    /// captured at the worker boundary (no chained source — pool workers
    /// flatten errors before crossing the channel).
    Io(String),
    /// Pool was shut down before the hash completed (tear-down on quit /
    /// rediscover). Not a true I/O failure; surfaced separately so the
    /// observability layer does not log it as an error.
    Cancelled,
    /// Fallback bucket for failure modes that do not fit the above (panics
    /// caught at the worker boundary, future taxonomy entries).
    Other(String),
}

/// All the messages that can drive `update()`. Step 01-03 covers keyboard
/// navigation; later steps add discovery-progress, action-completion, and
/// tick variants.
///
/// Step 02-01 dropped the `Eq` derive: `ToolDetailReady(Box<ToolDetail>)`
/// carries a `ToolDetail` from `modeltap-core` whose `Option<SystemTime>`
/// fields (`last_scan_at`, `last_error_at`, `introspected_at`) legitimately
/// do not implement `Eq` (wall-clock time is not reflexively equal across
/// system clock changes — a well-established stdlib choice). `PartialEq` is
/// retained for `assert_eq!` and all in-tree equality checks; nothing in the
/// TUI ever required the stronger `Eq` bound.
#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    /// User pressed `q`. Clean shutdown, exit 0.
    Quit,
    /// User pressed Ctrl+C. Shutdown with POSIX SIGINT exit code (130).
    CtrlC,
    /// Right Arrow — advance to the next tool slot (cycles).
    SelectNextTool,
    /// Left Arrow — regress to the previous tool slot (cycles).
    SelectPrevTool,
    /// Down Arrow — advance to the next row in the current tool.
    SelectNextRow,
    /// Up Arrow — regress to the previous row in the current tool.
    SelectPrevRow,
    /// Tab — toggle focus between left and right panes.
    ToggleFocus,

    // -----------------------------------------------------------------------
    // US-05 zap-all dialog. The dialog state machine lives in
    // `dialogs::zap_confirm`; these variants are how the keymap drives it
    // through `update()`.
    // -----------------------------------------------------------------------
    /// User pressed `z`. Open the zap-confirmation dialog for the
    /// currently-selected tool. No payload — `update()` reads the selection
    /// from `AppState`.
    ZapTool,
    /// One printable character pressed while a typed-input dialog is open.
    /// Appended to the dialog's input buffer.
    DialogTextInput(char),
    /// Backspace pressed while a typed-input dialog is open. Removes the
    /// last character from the dialog's input buffer (no-op when empty).
    DialogBackspace,
    /// Enter pressed while a dialog is open. The dialog's `decide_on_enter`
    /// determines confirm-vs-cancel.
    DialogConfirm,
    /// Esc pressed while a dialog is open. Always cancels per US-05 AC-3.
    DialogCancel,

    // -----------------------------------------------------------------------
    // US-06 post-action banner (in-memory only, per intake Q7).
    // -----------------------------------------------------------------------
    /// Composition root dispatches this after an action effect completes
    /// (zap in WS scope; unify/delete in 03-02/03-06). `update()` writes the
    /// payload into `AppState.last_action`.
    SetLastAction(LastAction),
    /// Composition root dispatches this after re-running discovery for a
    /// single tool (per US-06.AC-4 / US-11.AC-1: incremental refresh stays
    /// under 500 ms by NOT re-running every plugin's discover()). `update()`
    /// replaces the matching `ToolView` slot in `AppState.tools`.
    RefreshTool(ToolView),

    // -----------------------------------------------------------------------
    // US-11 incremental refresh — Result-flavored variants (step 03-04).
    //
    // The composition root spawns `refresh_tool_incremental` after every
    // mutating action (zap/unify/delete-from-one) and dispatches one of these
    // three Msgs based on the outcome:
    //   - Ok(view)  -> RefreshSucceeded(view): replace slot AND clear the
    //                  tool from `state.refresh_failed_tools`.
    //   - Err(_)    -> RefreshFailed(tool):    leave slot unchanged AND add
    //                  the tool to `state.refresh_failed_tools` so the
    //                  summary bar shows "(refresh failed)" + [r] retry.
    //   - User [r]  -> RetryRefresh(tool):     state-noop in the pure
    //                  update; the composition root re-spawns the refresh.
    // -----------------------------------------------------------------------
    /// Composition root dispatches this when `refresh_tool_incremental`
    /// returns `Ok(view)`. `update()` replaces the matching slot AND clears
    /// the tool from `state.refresh_failed_tools` (the prior failure has
    /// resolved).
    RefreshSucceeded(ToolView),
    /// Composition root dispatches this when `refresh_tool_incremental`
    /// returns `Err(_)`. `update()` adds the tool to
    /// `state.refresh_failed_tools` and leaves `state.tools` unchanged so
    /// the user keeps seeing the prior totals (degraded indicator path).
    RefreshFailed(ToolId),
    /// User pressed `[r]` to retry refresh for a tool currently in
    /// `state.refresh_failed_tools`. State-noop in the pure update; the
    /// composition root sees this Msg and re-spawns the refresh task. The
    /// keymap dispatches `RetryRefresh(ToolId(""))` (sentinel) and the
    /// composition root resolves the actual failed tool from state.
    RetryRefresh(ToolId),

    // -----------------------------------------------------------------------
    // Step 05-03 — US-24 manual refresh hotkeys.
    //
    // `[r]` dispatches `RequestRefresh(RefreshScope::Tool(<selected>))` —
    // the composition root resolves the selected tool from state at peek
    // time (mirrors RetryRefresh's sentinel pattern) and dispatches the
    // step 05-01 `orchestration::reconcile::run` orchestrator with the
    // mapped `ReconcileScope::Tool(_)`. `[Shift+R]` dispatches
    // `RequestRefresh(RefreshScope::All)` for the parallel-all variant.
    //
    // Pure update inserts the affected tool ids into
    // `state.reconciling` so the summary-bar provenance line gains the
    // `", refreshing <tool>..."` / `", reconciling..."` suffix per
    // AC-24-2. The set is cleared per-tool on `Msg::ReconcileCompleted`
    // / `Msg::ReconcileFailed` from step 05-01.
    //
    // Both hotkeys are silent no-ops while any dialog is open per AC-24-5.
    // The keymap-level enforcement is documented in keymap.rs: the
    // SHORTCUT_TABLE entries declare `BarSection::Main` so the dispatcher
    // never sees them while `dispatch_in_dialog` is in effect.
    /// User pressed `[r]` (Tool scope) or `[Shift+R]` (All scope). The
    /// pure update marks the affected tool ids as reconciling so the
    /// summary-bar provenance line surfaces the in-flight suffix; the
    /// composition root sees this Msg and dispatches the
    /// `orchestration::reconcile::run` orchestrator with the mapped
    /// scope.
    RequestRefresh(RefreshScope),

    // -----------------------------------------------------------------------
    // US-13 per-model detail screen.
    // -----------------------------------------------------------------------
    /// User pressed Enter on a model row in the main view. The composition
    /// root has assembled the `DetailScreenState` (with cross-tool registrations
    /// and an optional already-cached SHA-256). `update()` writes
    /// `current_screen = Screen::Detail(...)`.
    OpenDetail(DetailScreenState),
    /// User pressed Esc while on the detail screen. `update()` writes
    /// `current_screen = Screen::Main` and preserves the prior selection.
    CloseDetail,

    // -----------------------------------------------------------------------
    // US-22 per-model detail Metadata section (step 03-01).
    //
    // Mirrors the US-21 ToolDetailReady pattern. After `Msg::OpenDetail`
    // transitions into `Screen::Detail(state)`, the composition root dispatches
    // the async `open_model_detail` orchestration; on completion it dispatches
    // `Msg::ModelDetailReady(metadata)` so the pure update can attach the
    // metadata to the active detail screen's state.
    //
    // `[r]` while on the detail screen dispatches `Msg::ReintrospectModel`;
    // the composition root re-runs the orchestrator in `RunMode::ForceReintrospect`
    // so the next ModelDetailReady carries refreshed metadata (AC-22-2, AC-22-8).
    // -----------------------------------------------------------------------
    /// Composition root dispatches this when `open_model_detail` returns. The
    /// payload is the fully-resolved Metadata section (kv pairs OR sentinel
    /// status); `update()` attaches it to the active `Screen::Detail(state)`'s
    /// `state.metadata`. Stale dispatches (the user already Esc'd back to
    /// Main) are silently dropped.
    ModelDetailReady(Box<MetadataSection>),
    /// User pressed `[r]` while on the detail screen. Pure-update is a
    /// state-noop; the composition root re-runs `open_model_detail` in
    /// `RunMode::ForceReintrospect` and dispatches `Msg::ModelDetailReady`
    /// when it completes (AC-22-2, AC-22-8).
    ReintrospectModel,

    // -----------------------------------------------------------------------
    // US-21 per-tool detail screen (step 02-01).
    //
    // The composition root (modeltap-app::orchestration::open_tool_detail)
    // composes the cached `CachedTool` with `Tool::inspect_tool()` into a
    // single `ToolDetail`. The keymap dispatches `Msg::OpenToolDetail` when
    // Enter is pressed in the left pane; the orchestrator runs asynchronously
    // and dispatches `Msg::ToolDetailReady` once the merged detail is in
    // hand. Esc dispatches `Msg::CloseToolDetail` (preserving the left-pane
    // cursor).
    // -----------------------------------------------------------------------
    /// User pressed Enter on a left-pane row. `update()` transitions into
    /// `Screen::ToolDetail { tool_id, detail: None, left_pane_cursor }`
    /// (loading state); the composition root sees this Msg and dispatches the
    /// async `open_tool_detail` orchestration which will subsequently
    /// dispatch `Msg::ToolDetailReady`. The payload carries the selected
    /// `tool_id` so the orchestrator does not need to inspect AppState; the
    /// composition root resolves the cursor from `state.selected_tool` and
    /// passes it through `Msg::OpenToolDetail(tool_id)`.
    OpenToolDetail(ToolId),
    /// Composition root dispatches this when `open_tool_detail` returns the
    /// merged `ToolDetail`. `update()` writes `detail = Some(...)` on the
    /// existing `Screen::ToolDetail` variant — preserving `left_pane_cursor`
    /// so the Esc handler keeps working.
    ToolDetailReady(Box<ToolDetail>),
    /// User pressed Esc while on the tool detail screen. `update()` writes
    /// `current_screen = Screen::Main` and restores `selected_tool` from the
    /// saved `left_pane_cursor` (AC-21-7).
    CloseToolDetail,

    // -----------------------------------------------------------------------
    // [i] info — production hotkey opening the appropriate detail screen
    // (tool vs model) based on which pane has focus.
    //
    // Bug context: US-21 / US-22 detail screens shipped with their renderers
    // and `Msg::OpenToolDetail` / `Msg::OpenDetail` Msgs, but no production
    // keybinding ever constructed either Msg outside the
    // `MODELTAP_HEADLESS_*` env-var seams. Users had no path to either
    // screen. The fix introduces a payload-free `Msg::OpenInfo` that the
    // composition root translates into the focus-appropriate Msg at
    // peek-then-dispatch time — same pattern as `RequestRefresh(RefreshScope)`
    // (step 05-03), where the keymap stays a pure `KeyEvent -> Msg`
    // translation and the composition root resolves the scope from AppState.
    //
    // The keymap (`crates/modeltap-tui/src/keymap.rs`) dispatches `[i]` to
    // this variant; `crates/modeltap-app/src/interactive.rs::lift_open_info_in_main`
    // rewrites it into `Msg::OpenToolDetail(tool_id)` (left-pane focus) or
    // `Msg::OpenDetail(detail)` (right-pane focus, with the
    // `DetailScreenState` synthesised from real `AppState` — not env-vars).
    // -----------------------------------------------------------------------
    /// User pressed `[i]` on the main screen. Payload-free request to open
    /// the focus-appropriate detail screen — the composition root translates
    /// this into `Msg::OpenToolDetail` or `Msg::OpenDetail` before the pure
    /// `update` runs. If the lift cannot resolve a target (synthetic
    /// `[All Unified]` slot, empty right pane, dialog open) the Msg passes
    /// through unchanged and `update()` treats it as a no-op.
    OpenInfo,

    // -----------------------------------------------------------------------
    // US-08 help overlay + cross-step shortcut placeholders.
    // -----------------------------------------------------------------------
    /// User pressed `?`. Layer the help overlay on top of the current screen
    /// (via `Screen::Help { previous }`); pressing `?` again or Esc restores
    /// the previous screen with selection state intact.
    ToggleHelp,
    /// User pressed `u` (unify). Bound here so the bottom-bar INT-6 invariant
    /// holds (every visible shortcut maps to a non-noop Msg). On the main
    /// screen this is a no-op (unify needs the per-model context the detail
    /// screen provides) — see `Msg::OpenUnifyDialog` for the productive path.
    Unify,
    /// Composition root dispatches this when the user presses `u` on a
    /// multi-tool model in the detail screen. Carries the `UnifyPlan` the
    /// orchestrator built from the cross-tool registrations. `update()`
    /// writes `unify_dialog = Some(UnifyDialogState::from_plan(plan))`.
    OpenUnifyDialog(UnifyPlan),

    // -----------------------------------------------------------------------
    // US-19 cross-filesystem fallback dialog (ADR-008, step 03-03).
    //
    // When the unify planner detects 1+ cross-fs target, the orchestrator
    // dispatches `OpenCrossFsDialog` instead of `OpenUnifyDialog`. The dialog
    // accepts [s] / [c] / [x] keys (mapped to the three Msgs below) and
    // Enter/Esc default to refuse (Cancel) per ADR-008's no-silent-copy rule.
    // -----------------------------------------------------------------------
    /// Composition root dispatches this when `u` is pressed and the plan has
    /// 1+ cross-fs target. Carries the `UnifyPlan` so the orchestrator has
    /// the per-target same-fs flags + canonical path on confirmation.
    OpenCrossFsDialog(UnifyPlan),
    /// User pressed `s` while the cross-fs dialog is open — proceed with
    /// skip semantics (cross-fs targets untouched; same-fs targets linked).
    CrossFsSkip,
    /// User pressed `c` while the cross-fs dialog is open — proceed with
    /// copy semantics (cross-fs targets duplicated; same-fs targets linked).
    CrossFsCopy,
    /// User pressed `x` (or Enter / Esc per ADR-008 refuse default) while
    /// the cross-fs dialog is open — abort the unify entirely.
    CrossFsCancel,

    /// User pressed `d` (delete-from-one). Bound here so the bottom-bar INT-6
    /// invariant holds. On the detail screen, the headless harness /
    /// production loop intercepts this and dispatches `OpenDeleteOneDialog`
    /// with the orchestrator-built dialog state (shared-vs-unique
    /// classification per ADR-002).
    DeleteFromOne,

    // -----------------------------------------------------------------------
    // US-05b single-model delete dialog (step 03-06; ADR-009).
    //
    // The dialog state machine lives in `dialogs::delete_one_confirm`. On
    // the detail screen, pressing `[d]` dispatches `Msg::DeleteFromOne`
    // (kept for INT-6 invariant) which the orchestrator lifts into
    // `Msg::OpenDeleteOneDialog(state)` with `was_shared` already
    // classified. Confirmation flows through the same Dialog* messages as
    // the zap dialog: Shared mode interprets [y]/[n] via decide_on_y /
    // decide_on_n, Unique mode interprets typed input + Enter via
    // decide_on_enter.
    // -----------------------------------------------------------------------
    /// Composition root dispatches this when `[d]` is pressed on the detail
    /// screen and the orchestrator has built the dialog (shared-vs-unique
    /// classification, model id, size). `update()` writes
    /// `delete_one_dialog = Some(state)`.
    OpenDeleteOneDialog(DeleteOneConfirmState),
    /// User pressed `[y]` while the delete-one dialog is open in Shared
    /// mode — proceed with delete_one (low-friction path; content preserved
    /// elsewhere).
    DeleteOneConfirmShared,
    /// User pressed `[n]` while the delete-one dialog is open in Shared
    /// mode — close dialog with no destructive side-effect.
    DeleteOneCancelShared,
    /// Composition root dispatches this when `actions::delete_one::run`
    /// returns. Carries the outcome so the right pane / detail screen can
    /// surface it as a `LastAction` banner (banner construction lives in
    /// the composition root for symmetry with zap/unify).
    DeleteOneCompleted,

    // -----------------------------------------------------------------------
    // US-14 dry-run preview before unify (step 03-05).
    //
    // The unify dialog grows a tri-state machine: Confirm -> DryRunPreview ->
    // Confirm (back via Esc) OR Confirm -> DryRunPreview -> real run (via
    // Enter, which dispatches DialogConfirm again with the same plan).
    // -----------------------------------------------------------------------
    /// User pressed `[n]` while the unify confirm dialog is open. `update()`
    /// emits `UpdateEffect::trigger_dry_run = Some(plan)`; the composition
    /// root invokes `actions::unify::dry_run` to walk the plan descriptively
    /// (no fs mutation), emit the `action.unify_dry_run` JSONL event, and
    /// dispatch `Msg::UnifyDryRunCompleted(lines)` once the lines are ready.
    UnifyDryRun,
    /// US-U5: user pressed `[space]` while the unify confirm dialog is open.
    /// `update()` flips `unify_dialog.target_active[idx]` so the live total
    /// reclaim recomputes. The dispatcher (headless harness / interactive
    /// keymap) resolves `idx` from the dialog's cursor (`selected_target_idx`).
    ToggleTarget(usize),
    /// US-U5: user pressed `[down]` while the unify confirm dialog is open.
    /// Advances the per-target cursor (`selected_target_idx`) so the next
    /// [space] toggles the row below.
    UnifyDialogSelectNext,
    /// US-U5: user pressed `[up]` while the unify confirm dialog is open.
    UnifyDialogSelectPrev,
    /// Composition root dispatches this after `actions::unify::dry_run`
    /// returns. Carries the formatted "(dry-run) Would..." lines; `update()`
    /// transitions the dialog to `UnifyMode::DryRunPreview { lines }`.
    UnifyDryRunCompleted(Vec<String>),

    // -----------------------------------------------------------------------
    // US-17 running-tool detect-and-prompt-then-retry (intake Q5; step 03-07).
    //
    // The composition root opens this dialog when, after the user attempts
    // a unify or delete_one action, `FsProbe::detect_running_tools` returns
    // either `Ok(non_empty)` (Detected mode) or `Err(LsofUnavailable)`
    // (LsofUnavailable mode). The dialog REFUSES the action and prompts
    // close-and-retry — NOT a soft-warning. NO filesystem mutation may
    // occur while the dialog is open.
    // -----------------------------------------------------------------------
    /// Composition root dispatches this after `detect_running_tools` returns
    /// `Ok(non_empty)` OR `Err(LsofUnavailable)` and the user was attempting
    /// a unify or delete_one. `update()` writes
    /// `running_tool_dialog = Some(state)` and ensures no other dialog slot
    /// is also occupied (mutual exclusion).
    OpenRunningToolPrompt(RunningToolDialog),
    /// User pressed `[r]` while the running-tool prompt is open. In Detected
    /// mode, the composition root re-runs `detect_running_tools`; if it now
    /// returns empty, the original gated action proceeds. In LsofUnavailable
    /// mode, this Msg is dispatched as `Msg::RunningToolProceedAnyway` by
    /// the keymap (the dialog's `decide_on_retry` returns `ProceedAnyway`).
    RunningToolRetry,
    /// User pressed `[Esc]` while the running-tool prompt is open. Always
    /// cancels — closes the dialog with no destructive side-effect.
    RunningToolCancel,
    /// User pressed `[r]` while the running-tool prompt is open in
    /// LsofUnavailable mode. The orchestrator bypasses the gate and runs
    /// the original action despite the missing safety check (the user has
    /// acknowledged it).
    RunningToolProceedAnyway,

    // -----------------------------------------------------------------------
    // Step 01-06 — hash pool + unify completion (cross-tool-model-unify).
    //
    // The composition root spawns the background SHA256 hash pool (lands in
    // 01-07) which dispatches these variants asynchronously. The pure update
    // handlers mutate `state.hash_state` and recompute `state.dedup_summary`
    // so the per-row dedup glyph and summary bar reflect the new
    // classification. NFR: the recompute is a full pass; per-affected-key
    // optimization is OUT OF SCOPE for v1.
    // -----------------------------------------------------------------------
    /// Composition root dispatches this when the hash pool assigns a worker
    /// to a model. `update()` inserts `model_id` into
    /// `state.hash_state.in_progress` so the renderer shows the `~` glyph.
    HashStarted { tool: ToolId, model_id: String },
    /// Composition root dispatches this when a worker successfully computes
    /// a SHA256 hash. `update()` removes the model from `in_progress`,
    /// increments `completed`, stores the hash + (device, inode) in the
    /// hash-pool's caches, and recomputes `state.dedup_summary` (full pass).
    HashComputed {
        tool: ToolId,
        model_id: String,
        hash: ContentHash,
        device: u64,
        inode: u64,
    },
    /// Composition root dispatches this when a worker fails to compute a
    /// SHA256 hash (read error, cancellation, etc.). Per BR-3 the
    /// classifier treats failed entries as Unique with the `!` decorator
    /// — carried by `state.hash_state.failed`. `update()` increments
    /// `completed` (the worker IS done — just unsuccessfully) and
    /// recomputes `state.dedup_summary`.
    HashFailed {
        tool: ToolId,
        model_id: String,
        reason: HashFailureReason,
    },
    /// 250ms throttled tick from the hash pool — only used to trigger a
    /// re-render so updated `(completed/total)` counters are visible. Pure
    /// state-noop; the renderer reads `state.hash_state` on its own.
    HashProgressTick,
    /// Composition root dispatches this when `actions::unify::run` returns.
    /// `update()` refreshes the inode map for the affected `(tool, model_id)`
    /// pairs so all of them now point at the canonical's inode, recomputes
    /// `state.dedup_summary`, and sets `state.summary_delta = Some(...)` for
    /// the transient "(was X GB)" right-pane footer.
    UnifyApplied(UnifyOutcome),
    /// Composition root dispatches this when the 5-second `summary_delta`
    /// timer fires (lands in 01-08). `update()` clears `state.summary_delta`
    /// to `None` so the "(was X GB)" annotation disappears.
    SummaryDeltaExpired,
    /// Composition root dispatches this immediately after a successful unify
    /// to give the just-unified row a brief visual highlight (~1 s). The
    /// renderer reads `state.unify_highlight` and applies a reverse-video
    /// style. Cleared by `Msg::UnifyHighlightExpired`.
    UnifyHighlighted { tool: ToolId, model_id: String },
    /// Composition root dispatches this when the ~1s highlight timer fires.
    /// `update()` clears `state.unify_highlight` to `None`.
    UnifyHighlightExpired,

    // -----------------------------------------------------------------------
    // US-05c folder-group bulk delete (step 01-04).
    //
    // The keymap dispatches `Msg::RequestFolderDelete` on Shift+F (per AC-4 /
    // AC-19). Payload is intentionally empty — the composition root resolves
    // the cursor's currently-targeted folder from `state` at handle time, the
    // same pattern `RetryRefresh(ToolId(""))` uses. This keeps SHORTCUT_TABLE
    // a `const` array (no heap-allocated payload). Per the running-tool
    // contract (US-05c.AC-5), a Shift+F on a non-folder row or against a
    // non-HF active tool is a no-op at the composition-root resolver, not at
    // dispatch — keeping the bottom-bar single-source-of-truth invariant
    // (INT-FGD-7) clean.
    // -----------------------------------------------------------------------
    /// User pressed `Shift+F`. Open the folder-delete dialog for the currently-
    /// targeted folder. The composition root reads the FolderGroup from the
    /// cursor (right-pane row context) at step 01-05 wiring time.
    RequestFolderDelete,

    // -----------------------------------------------------------------------
    // Step 01-07 — folder collapse/expand (US-05c UX).
    //
    // Default state is COLLAPSED for every folder. Pressing Enter while a
    // folder header is the cursor target toggles its presence in
    // `AppState.expanded_folders`. The `update` handler resolves the cursor's
    // current folder from `(active tool, selected_row)` so the keymap does
    // not need to know the right-pane layout — same pattern as
    // `RequestFolderDelete`.
    // -----------------------------------------------------------------------
    /// User pressed `Enter` on the main screen. Toggle the cursor-targeted
    /// folder's expansion. `update` resolves the folder from the highlighted
    /// row's `<author>/<repo>` prefix; non-HF tools and rows without a `/`
    /// prefix are no-ops at the resolver level.
    ToggleFolderExpansion,

    // -----------------------------------------------------------------------
    // Step 04-01 — cache recovery banner (US-23, AC-23-7 / AC-23-11).
    //
    // The composition root populates `AppState.recovery_reason` when
    // `Cache::open` returns `OpenedAfterRecovery`. The banner paints at row
    // 0 of the main view; `[Esc]` dismisses it by setting the field back to
    // `None` via this Msg.
    // -----------------------------------------------------------------------
    /// User pressed `[Esc]` while the recovery banner is visible (no other
    /// dialog open). `update()` clears `state.recovery_reason` so the banner
    /// stops rendering on the next frame. Per AC-23-11 the inventory below
    /// the banner is unaffected.
    DismissRecoveryBanner,

    // -----------------------------------------------------------------------
    // Step 05-01 — background reconcile orchestrator (US-24 / US-26).
    //
    // The composition root dispatches `orchestration::reconcile::run` after
    // warm-paint (automatic All-scope reconcile) and — in 05-03 — on [r] /
    // [Shift+R] manual-refresh hotkeys. The orchestrator emits one of the
    // following two Msgs per tool: `ReconcileCompleted { tool, has_diff }`
    // (cache written successfully, optionally surfacing the silent-ack
    // indicator) or `ReconcileFailed { tool }` (per-tool write transaction
    // rolled back; cache stays at last-known-good per AC-26-3, a
    // `reconcile_failed` line is appended to diagnostics.log by the
    // orchestrator).
    //
    // The silent-ack indicator surfaces a 3-second blue `*` next to the
    // affected tool row when the diff is non-empty (AC-26-4); state is
    // carried in `AppState.silent_ack_until` as
    // `BTreeMap<ToolId, Instant>`. `Msg::DismissSilentAck { tool }` is
    // dispatched by the tick timer (lands fully in 05-03) — pure update
    // removes the entry.
    // -----------------------------------------------------------------------
    /// Composition root dispatches this when the per-tool reconcile write
    /// succeeded. `has_diff = true` means the orchestrator's
    /// `compute_inventory_diff` returned a non-empty drift — `update()`
    /// inserts the tool into `state.silent_ack_until` with the 3-second
    /// expiry instant (AC-26-4). `has_diff = false` is a state-noop (no
    /// indicator).
    ReconcileCompleted { tool: ToolId, has_diff: bool },
    /// Composition root dispatches this when the per-tool reconcile write
    /// transaction failed (CacheError or plugin discover panic). Pure
    /// `update()` is a state-noop — the cache stays at last-known-good
    /// (AC-26-3) and the diagnostics.log line is appended by the
    /// orchestrator before this Msg is sent. The variant exists so a
    /// future per-tool error indicator can plug in without a Msg-shape
    /// change.
    ReconcileFailed { tool: ToolId },
    /// Composition root dispatches this when the 3-second silent-ack timer
    /// expires for a specific tool. `update()` removes that tool from
    /// `state.silent_ack_until` so the blue `*` indicator disappears on the
    /// next frame. Per-tool granularity matches AC-26-4: simultaneous
    /// reconciles surface independent indicators with independent expiries.
    DismissSilentAck { tool: ToolId },

    /// Any unrecognized key. No-op per US-03 AC-6 (silently ignored).
    UnboundKey,
}
