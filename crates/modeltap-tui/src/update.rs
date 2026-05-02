//! Pure Elm-style `update()` (per ADR-006).
//!
//! `update(state, msg) -> (state, effect)` is a pure function — no I/O, no
//! mutation of inputs, no clocks. The composition root interprets the
//! returned `UpdateEffect` (write JSONL events, exit, dispatch zap-all, etc.).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use modeltap_core::logic::canonical_selector::{select_canonical, CandidatePath};
use modeltap_core::logic::compatibility::{Inventory, InventoryEntry};
use modeltap_core::logic::dedup::{compute_dedup_glyph, dedup_summary, InodeMap, ModelKey};
use modeltap_core::logic::plan::{build_plan, PlanCandidate, UnifyPlan};
use modeltap_core::{
    DedupGlyph, DiscoveredModel, DisplayLabel, Format, ModelStatus, ToolId,
};

use crate::app_state::{AppState, FocusPane, Screen, SummaryDelta};
use crate::dialogs::cross_fs_choice::{CrossFsChoice, CrossFsChoiceDialog};
use crate::dialogs::delete_one_confirm::{DeleteOneConfirmState, DeleteOneDecision};
use crate::dialogs::running_tool_prompt::{PendingGatedAction, RunningToolDialog};
use crate::dialogs::unify_confirm::{UnifyDecision, UnifyDialogState};
use crate::dialogs::zap_confirm::{ZapConfirmState, ZapDecision};
use crate::effects::unify_outcome::UnifyOutcome;
use crate::msg::Msg;

/// Side-effects the composition root must perform after this update. The
/// pure update function only describes effects; it does not execute them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateEffect {
    /// When set, the composition root should emit a `launch.ended` JSONL
    /// event before exiting. True ONLY for `Msg::Quit` (NOT for Ctrl+C —
    /// per the master-acceptance KPI invariant).
    pub emit_launch_ended: bool,

    /// When `Some(tool_id)`, the user has confirmed the zap action for that
    /// tool (typed name matched). The composition root invokes
    /// `actions::zap::run` to call `Tool::delete_all` and emit the
    /// `action.zap_all` JSONL event.
    pub trigger_zap: Option<ToolId>,

    /// When `Some(plan)`, the user has confirmed the unify action. The
    /// composition root invokes `actions::unify::run` to call each plugin's
    /// `Tool::link` per the plan, run the all-or-revert sequence (per
    /// ADR-008), and emit the `action.unify` JSONL event.
    pub trigger_unify: Option<UnifyPlan>,

    /// When `Some(choice)` AND `trigger_unify.is_some()`, the user has chosen
    /// the per-target cross-filesystem fallback semantics (US-19, ADR-008):
    /// `Skip` leaves cross-fs targets untouched; `Copy` duplicates the
    /// canonical's bytes to each cross-fs target. `None` means the orchestrator
    /// uses the default link path (US-10, no cross-fs targets).
    pub cross_fs_choice: Option<CrossFsChoice>,

    /// US-14: when `Some(plan)`, the user pressed `[n]` in the unify confirm
    /// dialog. The composition root invokes `actions::unify::dry_run(plan)`
    /// (which emits the `action.unify_dry_run` JSONL event and returns the
    /// formatted preview lines) and then dispatches
    /// `Msg::UnifyDryRunCompleted(lines)` so the dialog transitions to
    /// `UnifyMode::DryRunPreview { lines }`. The plan value is preserved
    /// unchanged in `state.unify_dialog.plan` per the ADR-006 same-value-type
    /// principle.
    pub trigger_dry_run: Option<UnifyPlan>,

    /// US-05b (step 03-06): when `Some((tool, model_id, was_shared))`, the
    /// user has confirmed the single-model delete (typed-id matched in
    /// Unique mode, or pressed `y` in Shared mode). The composition root
    /// invokes `actions::delete_one::run` to call `Tool::delete_one` (NOT
    /// `delete_all` per ADR-009) and emit the `action.zap_one` JSONL event
    /// with `was_shared` recorded.
    pub trigger_delete_one: Option<DeleteOneTrigger>,

    /// US-17 (step 03-07; intake Q5): when `Some(pending_action)`, the user
    /// pressed `[r]` on the running-tool prompt. The composition root
    /// re-runs `FsProbe::detect_running_tools` against the in-scope paths;
    /// if it now returns empty, the orchestrator re-dispatches the gated
    /// action. If it still returns non-empty, the running-tool dialog is
    /// reopened (still gated). In LsofUnavailable mode, this effect is
    /// emitted with the same payload but the orchestrator BYPASSES the
    /// re-probe (the user proceeded anyway).
    pub trigger_running_tool_retry: Option<RunningToolRetry>,
}

/// Payload for `UpdateEffect::trigger_running_tool_retry`. Carries the
/// gated action so the orchestrator knows what to re-dispatch on a clean
/// re-probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningToolRetry {
    pub action: PendingGatedAction,
    /// True iff the dialog was in LsofUnavailable mode and the user pressed
    /// `[r]` (proceed anyway). The orchestrator bypasses the re-probe.
    pub proceed_anyway: bool,
}

/// Payload for `UpdateEffect::trigger_delete_one`. Carries the data the
/// composition root needs to look up the plugin + ModelMeta for the
/// `Tool::delete_one(model)` call. Per ADR-009 + the dialog's
/// shared-vs-unique split, `was_shared` is preserved through to the JSONL
/// event so observability can distinguish the two destructive paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteOneTrigger {
    pub tool: ToolId,
    pub model_id: String,
    pub was_shared: bool,
    pub size_bytes: u64,
}

/// Pure transition. Takes ownership of `state` and returns the next state.
pub fn update(state: AppState, msg: Msg) -> (AppState, UpdateEffect) {
    match msg {
        Msg::Quit => (
            AppState {
                should_quit: true,
                exit_code: 0,
                ..state
            },
            UpdateEffect {
                emit_launch_ended: true,
                ..UpdateEffect::default()
            },
        ),
        Msg::CtrlC => (
            AppState {
                should_quit: true,
                exit_code: 130,
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::SelectNextTool => (
            advance_tool(clear_last_action(state), 1),
            UpdateEffect::default(),
        ),
        Msg::SelectPrevTool => (
            advance_tool(clear_last_action(state), -1),
            UpdateEffect::default(),
        ),
        Msg::SelectNextRow => (
            advance_row(clear_last_action(state), 1),
            UpdateEffect::default(),
        ),
        Msg::SelectPrevRow => (
            advance_row(clear_last_action(state), -1),
            UpdateEffect::default(),
        ),
        Msg::ToggleFocus => (
            AppState {
                focus: match state.focus {
                    FocusPane::Left => FocusPane::Right,
                    FocusPane::Right => FocusPane::Left,
                },
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::ZapTool => (open_zap_dialog(state), UpdateEffect::default()),
        Msg::DialogTextInput(c) => (mutate_dialogs_text_input(state, c), UpdateEffect::default()),
        Msg::DialogBackspace => (mutate_dialogs_backspace(state), UpdateEffect::default()),
        Msg::DialogConfirm => decide_dialog(state, DialogKey::Enter),
        Msg::DialogCancel => decide_dialog(state, DialogKey::Esc),
        Msg::OpenUnifyDialog(plan) => (
            AppState {
                unify_dialog: Some(UnifyDialogState::from_plan(plan)),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::SetLastAction(action) => (
            AppState {
                last_action: Some(action),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::RefreshTool(view) => (replace_tool_slot(state, view), UpdateEffect::default()),
        // US-11 Result-flavored refresh variants (step 03-04).
        Msg::RefreshSucceeded(view) => {
            let tool_id = view.tool;
            let mut next = replace_tool_slot(state, view);
            // The successful refresh resolves any prior failure marker.
            next.refresh_failed_tools.remove(&tool_id);
            (next, UpdateEffect::default())
        }
        Msg::RefreshFailed(tool_id) => {
            let mut next = state;
            next.refresh_failed_tools.insert(tool_id);
            (next, UpdateEffect::default())
        }
        // RetryRefresh is a state-noop in the pure update; the composition
        // root sees the message and re-spawns the refresh task. The failed
        // marker stays in place until RefreshSucceeded arrives.
        Msg::RetryRefresh(_) => (state, UpdateEffect::default()),
        Msg::OpenDetail(detail) => (
            AppState {
                current_screen: Screen::Detail(detail),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::CloseDetail => (
            AppState {
                current_screen: Screen::Main,
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::ToggleHelp => (toggle_help(state), UpdateEffect::default()),
        // ----- US-19 cross-filesystem fallback dialog (ADR-008, step 03-03) ---
        Msg::OpenCrossFsDialog(plan) => (
            AppState {
                cross_fs_dialog: Some(CrossFsChoiceDialog::from_plan(plan)),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::CrossFsSkip => decide_cross_fs(state, CrossFsKey::Skip),
        Msg::CrossFsCopy => decide_cross_fs(state, CrossFsKey::Copy),
        Msg::CrossFsCancel => decide_cross_fs(state, CrossFsKey::Cancel),
        // Step 01-10: Msg::Unify from the MAIN view dispatches on the
        // highlighted row's DedupGlyph. Detail-screen Unify continues to be
        // lifted by `lift_unify_in_detail` in interactive.rs / headless.rs;
        // this branch leaves the state untouched on Detail so the orchestrator
        // path keeps working unchanged.
        Msg::Unify => (handle_unify_from_main(state), UpdateEffect::default()),
        Msg::DeleteFromOne => (state, UpdateEffect::default()),
        // ----- US-05b single-model delete dialog (step 03-06; ADR-009) ------
        Msg::OpenDeleteOneDialog(dialog) => (
            AppState {
                delete_one_dialog: Some(dialog),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::DeleteOneConfirmShared => decide_delete_one_shared(state, DeleteOneSharedKey::Yes),
        Msg::DeleteOneCancelShared => decide_delete_one_shared(state, DeleteOneSharedKey::No),
        // The composition root surfaces the outcome via Msg::SetLastAction;
        // here we just close the dialog if it is still open.
        Msg::DeleteOneCompleted => (
            AppState {
                delete_one_dialog: None,
                ..state
            },
            UpdateEffect::default(),
        ),
        // ----- US-14 dry-run preview (step 03-05) -----------------------
        Msg::UnifyDryRun => decide_dry_run(state),
        Msg::UnifyDryRunCompleted(lines) => apply_dry_run_lines(state, lines),
        // ----- US-U5 reclaim-preview toggle + cursor (step 03-02) -------
        Msg::ToggleTarget(idx) => (apply_toggle_target(state, idx), UpdateEffect::default()),
        Msg::UnifyDialogSelectNext => (apply_unify_dialog_next(state), UpdateEffect::default()),
        Msg::UnifyDialogSelectPrev => (apply_unify_dialog_prev(state), UpdateEffect::default()),
        // ----- US-17 running-tool detect-and-prompt-then-retry (step 03-07) -
        // Per intake Q5: detect-and-prompt-then-retry. The composition root
        // dispatches OpenRunningToolPrompt(dialog) when, after the user
        // attempts unify or delete_one, FsProbe::detect_running_tools returns
        // either Ok(non_empty) (Detected mode) or Err(LsofUnavailable). The
        // dialog REFUSES the action; NO filesystem mutation may occur while
        // the dialog is open. [r] retries (or proceeds-anyway in
        // LsofUnavailable mode); [Esc] cancels.
        Msg::OpenRunningToolPrompt(dialog) => (
            AppState {
                running_tool_dialog: Some(dialog),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::RunningToolRetry => decide_running_tool(state, RunningToolKey::Retry),
        Msg::RunningToolCancel => decide_running_tool(state, RunningToolKey::Cancel),
        Msg::RunningToolProceedAnyway => decide_running_tool(state, RunningToolKey::ProceedAnyway),
        // ----- Step 01-06: hash pool + unify completion ---------------------
        Msg::HashStarted { tool: _, model_id } => {
            (apply_hash_started(state, model_id), UpdateEffect::default())
        }
        Msg::HashComputed {
            tool,
            model_id,
            hash,
            device,
            inode,
        } => (
            apply_hash_computed(state, tool, model_id, hash, device, inode),
            UpdateEffect::default(),
        ),
        Msg::HashFailed {
            tool: _,
            model_id,
            reason: _,
        } => (apply_hash_failed(state, model_id), UpdateEffect::default()),
        Msg::HashProgressTick => (state, UpdateEffect::default()),
        Msg::UnifyApplied(outcome) => (apply_unify_outcome(state, outcome), UpdateEffect::default()),
        Msg::SummaryDeltaExpired => (
            AppState {
                summary_delta: None,
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::UnifyHighlighted { tool, model_id } => (
            AppState {
                unify_highlight: Some((tool, model_id)),
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::UnifyHighlightExpired => (
            AppState {
                unify_highlight: None,
                ..state
            },
            UpdateEffect::default(),
        ),
        Msg::UnboundKey => (state, UpdateEffect::default()),
    }
}

// ---------------------------------------------------------------------------
// Step 01-06 — pure hash-pool / unify completion handlers.
//
// These helpers mutate `state.hash_state` and recompute
// `state.dedup_summary` via the canonical `logic::dedup::dedup_summary` so
// the per-row glyph (compute_dedup_glyph) and the summary bar agree on the
// classification. The recompute is a full pass per the v1 spec — per-affected
// key incremental reclassification is OUT OF SCOPE for v1 (perf path).
// ---------------------------------------------------------------------------

/// Insert `model_id` into `hash_state.in_progress`. The renderer reads this
/// set to display the `~` glyph for actively-hashing rows.
fn apply_hash_started(mut state: AppState, model_id: String) -> AppState {
    state.hash_state.in_progress.insert(model_id);
    state
}

/// Record a successful hash computation: drop from `in_progress`, advance
/// `completed`, persist `(hash, device, inode)` for later glyph + summary
/// computation, then recompute `state.dedup_summary` via a full pass over the
/// derived inventory. Edge case: when `model_id` was never in `in_progress`
/// (out-of-order Started/Completed delivery), this still records the outcome
/// — the BTreeSet `remove` is a no-op for absent keys.
fn apply_hash_computed(
    mut state: AppState,
    tool: ToolId,
    model_id: String,
    hash: modeltap_core::ContentHash,
    device: u64,
    inode: u64,
) -> AppState {
    state.hash_state.in_progress.remove(&model_id);
    state.hash_state.completed = state.hash_state.completed.saturating_add(1);
    let key = (tool, model_id.clone());
    state.hash_state.completed_hashes.insert(key.clone(), hash);
    state.hash_state.inodes.insert(key, (device, inode));
    state.dedup_summary = recompute_dedup_summary(&state);
    state
}

/// Record a failed hash computation: drop from `in_progress`, add to
/// `failed`, advance `completed`. Per BR-3 (conservative-when-uncertain) the
/// classifier treats failed entries as Unique with the `!` decorator — the
/// recompute therefore omits any contribution from this row. Idempotent
/// w.r.t. `failed` set membership; `completed` is also guarded against
/// double-increment by checking the set BEFORE adding.
fn apply_hash_failed(mut state: AppState, model_id: String) -> AppState {
    state.hash_state.in_progress.remove(&model_id);
    let was_new_failure = state.hash_state.failed.insert(model_id);
    if was_new_failure {
        state.hash_state.completed = state.hash_state.completed.saturating_add(1);
    }
    state.dedup_summary = recompute_dedup_summary(&state);
    state
}

/// Apply a unify outcome: refresh the inode map for every affected pair so
/// they all point at the canonical inode, capture the previous
/// `dedup_able_bytes` for the transient "(was X GB)" footer, then recompute
/// `state.dedup_summary` with the new inode map.
///
/// "Canonical inode" is derived as: the inode currently recorded in
/// `hash_state.inodes` for the FIRST entry of `outcome.affected` that has a
/// recorded inode. This mirrors the unify planner's canonical-selection
/// (ADR-002) — every other affected pair is rewritten to that same `(device,
/// inode)`. Pairs without any recorded inode are left untouched (the unify
/// would not have proceeded for an un-stat'd row).
fn apply_unify_outcome(mut state: AppState, outcome: UnifyOutcome) -> AppState {
    let previous_dedup_able_bytes = state.dedup_summary.dedup_able_bytes.unwrap_or(0);

    let canonical_inode = outcome
        .affected
        .iter()
        .find_map(|key| state.hash_state.inodes.get(key).copied());

    if let Some(canonical) = canonical_inode {
        for key in &outcome.affected {
            state.hash_state.inodes.insert(key.clone(), canonical);
        }
    }

    state.dedup_summary = recompute_dedup_summary(&state);
    state.summary_delta = Some(SummaryDelta {
        previous_dedup_able_bytes,
        expires_at: Instant::now() + Duration::from_secs(5),
    });
    state
}

/// Build a synthetic `Inventory` + `InodeMap` from the current `AppState`.
/// Pure: no I/O. Reused by `recompute_dedup_summary` (full pass over the
/// derived inventory) and `handle_unify_from_main` (computes the per-row
/// `DedupGlyph` and looks up content-hash peers without re-walking discovery).
///
/// `on_disk_path` is left empty because `AppState::ToolView` does not carry
/// per-row paths (the right pane only renders ids + sizes); the dedup classifier
/// and the dedup_summary aggregator do not consult `on_disk_path`. For the
/// unify-plan builder in step 01-10 a synthetic per-row path
/// (`/<tool>/<model_id>`) is used at the call site so `select_canonical` and
/// `build_plan` have a stable, deterministic key without reaching into discovery.
fn state_inventory(state: &AppState) -> (Inventory, InodeMap) {
    let mut entries: Vec<InventoryEntry> = Vec::new();
    for view in state.real_tools_iter() {
        for (idx, id_in_tool) in view.model_ids.iter().enumerate() {
            let size_bytes = view
                .model_sizes_bytes
                .get(idx)
                .copied()
                .unwrap_or(0);
            let key = (view.tool, id_in_tool.clone());
            let content_hash = state.hash_state.completed_hashes.get(&key).copied();
            entries.push(InventoryEntry {
                tool: view.tool,
                model: DiscoveredModel {
                    id_in_tool: id_in_tool.clone(),
                    on_disk_path: PathBuf::new(),
                    size_bytes,
                    format: Format::Other,
                    display_label: DisplayLabel::from(id_in_tool.as_str()),
                    status: ModelStatus::Healthy,
                },
                content_hash,
            });
        }
    }
    let inventory = Inventory { entries };

    let mut inodes: InodeMap = HashMap::new();
    for ((tool, id_in_tool), devino) in &state.hash_state.inodes {
        let key: ModelKey = (*tool, id_in_tool.clone());
        inodes.insert(key, *devino);
    }
    (inventory, inodes)
}

/// Build a synthetic `Inventory` + `InodeMap` from the current `AppState`
/// and call the canonical `logic::dedup::dedup_summary`.
///
/// `hashing_done` is `state.hash_state.is_complete()`. Per the
/// `dedup_summary` contract, while not done the function returns
/// `DedupSummary::default()` (all `None` — "computing...").
fn recompute_dedup_summary(state: &AppState) -> modeltap_core::DedupSummary {
    let (inventory, inodes) = state_inventory(state);
    dedup_summary(&inventory, &inodes, state.hash_state.is_complete())
}

// ---------------------------------------------------------------------------
// Step 01-10 — Msg::Unify glyph-aware dispatch from the main view.
//
// Branches on the highlighted row's `DedupGlyph`:
//
//   = (DedupAble)     → build plan from content_hash peers, open unify dialog
//                       in Confirm mode.
//   # (AlreadyUnified)→ build plan the same way (every link reports
//                       already_linked == true), open unify dialog in
//                       AlreadyUnified informational mode.
//   - (Unique)        → set status_line "This model is unique — nothing to
//                       unify". No dialog.
//   ? (Pending) /
//   ~ (Hashing)       → set status_line "Hash still computing — wait for
//                       completion, then press u again". No dialog.
//   -! (Failed)       → set status_line "Hash failed for this row —
//                       re-launch modeltap to retry". No dialog.
//
// On Detail screen, Msg::Unify is a state-noop here — `lift_unify_in_detail`
// in interactive.rs / headless.rs owns that path and continues to do so.
// ---------------------------------------------------------------------------

/// Per-glyph status_line hints. Defined as constants so the test file and
/// future render code share the exact strings without drift.
const STATUS_HINT_UNIQUE: &str = "This model is unique — nothing to unify";
const STATUS_HINT_HASHING: &str =
    "Hash still computing — wait for completion, then press u again";
const STATUS_HINT_FAILED: &str = "Hash failed for this row — re-launch modeltap to retry";

/// Handle `Msg::Unify` dispatched from `Screen::Main`. Returns the next
/// `AppState`. Detail-screen Unify falls through unchanged so the
/// orchestrator-level `lift_unify_in_detail` keeps working.
fn handle_unify_from_main(state: AppState) -> AppState {
    if !matches!(state.current_screen, Screen::Main) {
        // Detail / Help — leave to the orchestrator (or no-op in Help).
        return state;
    }
    let Some((target_tool, target_id)) = highlighted_row(&state) else {
        // No tool / no row → no actionable target.
        return state;
    };
    let (inventory, inodes) = state_inventory(&state);
    let Some(target_entry) = inventory
        .entries
        .iter()
        .find(|e| e.tool == target_tool && e.model.id_in_tool == target_id)
    else {
        return state;
    };
    let glyph = compute_dedup_glyph(
        target_entry,
        &inventory,
        &inodes,
        &model_keys_in_progress(&state),
        &model_keys_failed(&state),
    );
    match glyph {
        DedupGlyph::DedupAble | DedupGlyph::AlreadyUnified => {
            match build_unify_plan_for_row(&state, target_tool, &target_id, &inventory) {
                Some(plan) => AppState {
                    unify_dialog: Some(
                        crate::dialogs::unify_confirm::UnifyDialogState::from_plan(plan),
                    ),
                    status_line: None,
                    ..state
                },
                // Defensive: classifier said dedup-able but plan came back
                // empty (e.g., canonical missing). Fall back to a hint.
                None => AppState {
                    status_line: Some(STATUS_HINT_UNIQUE.to_string()),
                    ..state
                },
            }
        }
        DedupGlyph::Unique => AppState {
            status_line: Some(STATUS_HINT_UNIQUE.to_string()),
            ..state
        },
        DedupGlyph::Pending | DedupGlyph::Hashing => AppState {
            status_line: Some(STATUS_HINT_HASHING.to_string()),
            ..state
        },
        DedupGlyph::Failed => AppState {
            status_line: Some(STATUS_HINT_FAILED.to_string()),
            ..state
        },
    }
}

/// `(tool, model_id)` of the right-pane highlighted row, when one exists.
fn highlighted_row(state: &AppState) -> Option<(ToolId, String)> {
    let tool_view = state.current_tool()?;
    let id = tool_view.model_ids.get(state.selected_row)?.clone();
    Some((tool_view.tool, id))
}

/// `compute_dedup_glyph` takes a `BTreeSet<ModelKey>` of in-progress keys.
/// `state.hash_state.in_progress` is a `BTreeSet<String>` of model_ids only
/// (no tool prefix), so we expand each id into a `(tool, id)` for every tool
/// that registers it. Conservative: a model_id present in `in_progress` is
/// considered hashing for ALL tools that own it, mirroring the renderer's
/// glyph computation in `render::row_dedup_column`.
fn model_keys_in_progress(state: &AppState) -> std::collections::BTreeSet<ModelKey> {
    let mut out = std::collections::BTreeSet::new();
    for view in state.real_tools_iter() {
        for id in &view.model_ids {
            if state.hash_state.in_progress.contains(id) {
                out.insert((view.tool, id.clone()));
            }
        }
    }
    out
}

/// Same as `model_keys_in_progress` but for the `failed` set.
fn model_keys_failed(state: &AppState) -> std::collections::BTreeSet<ModelKey> {
    let mut out = std::collections::BTreeSet::new();
    for view in state.real_tools_iter() {
        for id in &view.model_ids {
            if state.hash_state.failed.contains(id) {
                out.insert((view.tool, id.clone()));
            }
        }
    }
    out
}

/// Build a `UnifyPlan` for the highlighted row by gathering all rows in the
/// inventory whose `content_hash` matches the target's. Synthesises per-row
/// paths as `/<tool>/<model_id>` because `AppState::ToolView` does not carry
/// per-row paths — the actual on-disk paths are resolved by the orchestrator
/// when the user confirms the dialog (deferred to step 01-12 walking-skeleton
/// activation; today the dialog opens but Enter cannot complete an end-to-end
/// link from this code path). Returns `None` when the target has no
/// content_hash OR no peer with a matching hash.
fn build_unify_plan_for_row(
    state: &AppState,
    target_tool: ToolId,
    target_id: &str,
    inventory: &Inventory,
) -> Option<UnifyPlan> {
    let target_entry = inventory
        .entries
        .iter()
        .find(|e| e.tool == target_tool && e.model.id_in_tool == target_id)?;
    let target_hash = target_entry.content_hash?;

    let mut candidates: Vec<CandidatePath> = Vec::new();
    let mut plan_candidates: Vec<PlanCandidate> = Vec::new();
    for entry in &inventory.entries {
        if entry.content_hash != Some(target_hash) {
            continue;
        }
        let key = (entry.tool, entry.model.id_in_tool.clone());
        let (device, inode) = state
            .hash_state
            .inodes
            .get(&key)
            .copied()
            .unwrap_or((0, 0));
        let synth_path = synthetic_row_path(entry.tool, &entry.model.id_in_tool);
        candidates.push(CandidatePath {
            tool: entry.tool,
            path: synth_path.clone(),
            exists: true,
            size_bytes: entry.model.size_bytes,
            is_ollama_blob: entry.tool == ToolId("ollama"),
        });
        plan_candidates.push(PlanCandidate {
            tool: entry.tool,
            path: synth_path,
            exists: true,
            device,
            inode,
            size_bytes: entry.model.size_bytes,
        });
    }
    let canonical = select_canonical(&candidates)?;
    let canonical_plan = plan_candidates
        .iter()
        .find(|p| p.path == canonical.path)?
        .clone();
    build_plan(&canonical_plan, &plan_candidates)
}

/// Synthetic per-row path used when constructing a `UnifyPlan` from
/// `AppState`. Stable, deterministic, and free of filesystem dependencies —
/// the dialog renders these as the canonical path / target paths until the
/// orchestrator-level walking-skeleton wiring (step 01-12) replaces them with
/// real on-disk paths at confirmation time.
fn synthetic_row_path(tool: ToolId, model_id: &str) -> PathBuf {
    PathBuf::from(format!("/{}/{}", tool.0, model_id))
}

/// Toggle the layered help overlay (US-08). When `current_screen` is anything
/// other than `Help`, wrap it in `Screen::Help { previous: <current> }`. When
/// it is already `Help`, restore the wrapped previous screen. This lets `?`
/// open AND close the overlay symmetrically, and Esc-from-help maps to the
/// same `Msg::ToggleHelp` as a second `?`.
fn toggle_help(state: AppState) -> AppState {
    let next_screen = match state.current_screen {
        Screen::Help { previous } => *previous,
        other => Screen::Help {
            previous: Box::new(other),
        },
    };
    AppState {
        current_screen: next_screen,
        ..state
    }
}

/// Clear `last_action` (US-06: any nav Msg dismisses the post-action banner)
/// AND `status_line` (step 01-10: any nav Msg dismisses the unify-from-main
/// status hint). Both fields share the "user moved on, drop the transient
/// post-action narrative" semantics, so they clear together.
fn clear_last_action(state: AppState) -> AppState {
    AppState {
        last_action: None,
        status_line: None,
        ..state
    }
}

/// Replace the matching `ToolView` slot in `state.left_pane_slots` with the
/// freshly-discovered view. Tools are matched by `ToolId`; only `Real(_)`
/// slots are considered — the synthetic `[All Unified]` slot (when present)
/// is left untouched. If no slot matches (e.g. a future plugin id we don't
/// know yet) the state is returned unchanged.
fn replace_tool_slot(mut state: AppState, view: crate::app_state::ToolView) -> AppState {
    if let Some(slot) = state.real_tools_iter_mut().find(|t| t.tool == view.tool) {
        *slot = view;
    }
    state
}

/// Move the tool selection forward or backward (cyclic). Resets the row
/// selection and right-pane scroll offset because the new tool has its own
/// row list. Updates the LEFT-pane scroll offset so the freshly-selected
/// tool stays inside the rendered window — matters when the registry has
/// more tools than the left pane's visible_rows (small terminal or future
/// plugin growth).
fn advance_tool(state: AppState, delta: i32) -> AppState {
    let n = state.left_pane_slots.len();
    if n == 0 {
        return state;
    }
    let current = state.selected_tool as i32;
    let next = ((current + delta).rem_euclid(n as i32)) as usize;
    let left_scroll_offset =
        compute_scroll_offset(next, state.left_scroll_offset, state.left_visible_rows);
    AppState {
        selected_tool: next,
        selected_row: 0,
        scroll_offset: 0,
        left_scroll_offset,
        ..state
    }
}

/// Move the row selection within the current tool. Clamps at boundaries
/// (no wrap; long lists scroll instead of wrapping). Updates `scroll_offset`
/// so the cursor stays inside the visible window.
fn advance_row(state: AppState, delta: i32) -> AppState {
    let row_count = state.current_row_count();
    if row_count == 0 {
        return state;
    }
    let current = state.selected_row as i32;
    let max_idx = (row_count - 1) as i32;
    let next = (current + delta).clamp(0, max_idx) as usize;
    let scroll_offset = compute_scroll_offset(next, state.scroll_offset, state.visible_rows);
    AppState {
        selected_row: next,
        scroll_offset,
        ..state
    }
}

/// Keep the cursor inside the visible window:
/// - if `selected < scroll_offset`, scroll up so cursor is at the top.
/// - if `selected >= scroll_offset + visible`, scroll down so cursor is at
///   the bottom.
/// - otherwise leave scroll_offset unchanged.
///
/// Used for both the right pane (rows of the selected tool) and the left
/// pane (tool list) — the math is identical because both panes render a
/// vertical window of items. Public so the scroll-invariant unit tests can
/// drive the same pure fn the production update path uses.
pub fn compute_scroll_offset(selected: usize, current_offset: usize, visible: usize) -> usize {
    if visible == 0 {
        return current_offset;
    }
    if selected < current_offset {
        return selected;
    }
    if selected >= current_offset + visible {
        return selected + 1 - visible;
    }
    current_offset
}

/// Open the zap-confirm dialog snapshot for the currently-selected tool. The
/// classifier (unique-vs-shared) is computed conservatively from the local
/// `AppState` view: every model is treated as unique because the WS slice
/// has no cross-tool inventory yet (one-tool-installed scenario), which is
/// safe per ADR-002 §"Conservative deletion" — any uncertainty defaults to
/// "unique" (the more cautious estimate).
fn open_zap_dialog(state: AppState) -> AppState {
    let dialog = match state.current_tool() {
        Some(tool) => {
            let total = tool.total_bytes();
            ZapConfirmState::for_tool(tool.tool, tool.model_ids.len(), total, total, 0)
        }
        // No tool selected (pathological — `tools` is empty). Open a benign
        // empty-mode dialog so the user can dismiss with Esc.
        None => ZapConfirmState::for_tool(ToolId(""), 0, 0, 0, 0),
    };
    AppState {
        zap_dialog: Some(dialog),
        ..state
    }
}

/// Route a printable-character keystroke to whichever typed-input dialog is
/// open. The zap dialog and the delete-one dialog (Unique mode only) both
/// accumulate typed input; in Shared mode the delete-one dialog ignores typed
/// input (Shared mode resolves on `[y]` / `[n]` directly via
/// `Msg::DeleteOneConfirmShared` / `Msg::DeleteOneCancelShared`). When no
/// dialog is open, the message is silently ignored (defense in depth — the
/// keymap routes dialog keys only when a dialog is open, but a stray test
/// `Msg::DialogTextInput` would otherwise produce a confusing panic).
fn mutate_dialogs_text_input(mut state: AppState, c: char) -> AppState {
    if let Some(dialog) = state.zap_dialog.as_mut() {
        dialog.handle_char(c);
    }
    if let Some(dialog) = state.delete_one_dialog.as_mut() {
        if !dialog.is_shared() {
            dialog.handle_char(c);
        }
    }
    state
}

/// Backspace counterpart to `mutate_dialogs_text_input`. Same routing rules.
fn mutate_dialogs_backspace(mut state: AppState) -> AppState {
    if let Some(dialog) = state.zap_dialog.as_mut() {
        dialog.handle_backspace();
    }
    if let Some(dialog) = state.delete_one_dialog.as_mut() {
        if !dialog.is_shared() {
            dialog.handle_backspace();
        }
    }
    state
}

/// Which key triggered the dialog decision. Determines whether `decide_on_enter`
/// or `decide_on_esc` is called.
enum DialogKey {
    Enter,
    Esc,
}

/// Resolve a dialog Confirm/Cancel decision. The unify dialog takes
/// precedence over the zap dialog because it's the most recently opened (a
/// well-formed app never opens both at once, but defense-in-depth picks
/// one). On Confirm with the destructive path, emit the matching trigger
/// so the composition root invokes `link()` (unify) or `delete_all` (zap).
/// In every case, close whichever dialog was open.
fn decide_dialog(state: AppState, key: DialogKey) -> (AppState, UpdateEffect) {
    if state.unify_dialog.is_some() {
        return decide_unify_dialog(state, key);
    }
    if state.zap_dialog.is_some() {
        return decide_zap_dialog(state, key);
    }
    if state.delete_one_dialog.is_some() {
        return decide_delete_one_unique(state, key);
    }
    (state, UpdateEffect::default())
}

/// Resolve the delete-one dialog's Unique typed-id path. Enter triggers the
/// `decide_on_enter` BYTE-EQUAL CASE-SENSITIVE comparison; Esc always
/// cancels. In Shared mode, Enter is a no-op (Pending) per the dialog state
/// machine — the dialog stays open so the user can press `[y]` or `[n]`.
fn decide_delete_one_unique(state: AppState, key: DialogKey) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.delete_one_dialog else {
        return (state, UpdateEffect::default());
    };
    let decision = match key {
        DialogKey::Enter => dialog.decide_on_enter(),
        DialogKey::Esc => dialog.decide_on_esc(),
    };
    match decision {
        DeleteOneDecision::Confirm => {
            let trigger = trigger_from_dialog(dialog);
            let next_state = AppState {
                delete_one_dialog: None,
                ..state
            };
            (
                next_state,
                UpdateEffect {
                    trigger_delete_one: Some(trigger),
                    ..UpdateEffect::default()
                },
            )
        }
        DeleteOneDecision::Cancel => (
            AppState {
                delete_one_dialog: None,
                ..state
            },
            UpdateEffect::default(),
        ),
        // Pending: Enter pressed in Shared mode — dialog stays open. The
        // user must press [y] or [n] explicitly to resolve.
        DeleteOneDecision::Pending => (state, UpdateEffect::default()),
    }
}

/// Build the `DeleteOneTrigger` payload from the dialog's snapshot fields.
fn trigger_from_dialog(dialog: &DeleteOneConfirmState) -> DeleteOneTrigger {
    DeleteOneTrigger {
        tool: dialog.tool,
        model_id: dialog.model_id.clone(),
        was_shared: dialog.is_shared(),
        size_bytes: dialog.size_bytes,
    }
}

/// Which key drove the Shared-mode delete-one decision. Only `[y]` and `[n]`
/// reach this path; Esc flows through `Msg::DialogCancel` -> `decide_dialog`
/// -> `decide_delete_one_unique` (which calls `decide_on_esc`).
enum DeleteOneSharedKey {
    Yes,
    No,
}

/// Resolve the Shared-mode delete-one dialog. `[y]` confirms (low-friction
/// per ADR-002 + ADR-009 — content preserved elsewhere); `[n]` cancels.
fn decide_delete_one_shared(state: AppState, key: DeleteOneSharedKey) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.delete_one_dialog else {
        return (state, UpdateEffect::default());
    };
    let decision = match key {
        DeleteOneSharedKey::Yes => dialog.decide_on_y(),
        DeleteOneSharedKey::No => dialog.decide_on_n(),
    };
    match decision {
        DeleteOneDecision::Confirm => {
            let trigger = trigger_from_dialog(dialog);
            let next_state = AppState {
                delete_one_dialog: None,
                ..state
            };
            (
                next_state,
                UpdateEffect {
                    trigger_delete_one: Some(trigger),
                    ..UpdateEffect::default()
                },
            )
        }
        DeleteOneDecision::Cancel => (
            AppState {
                delete_one_dialog: None,
                ..state
            },
            UpdateEffect::default(),
        ),
        // Pending: [y]/[n] pressed in Unique mode — typed input, not a
        // decision. Caller should not have dispatched Shared-mode Msg in
        // Unique mode, but defense in depth: leave dialog open.
        DeleteOneDecision::Pending => (state, UpdateEffect::default()),
    }
}

fn decide_zap_dialog(state: AppState, key: DialogKey) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.zap_dialog else {
        return (state, UpdateEffect::default());
    };
    let decision = match key {
        DialogKey::Enter => dialog.decide_on_enter(),
        DialogKey::Esc => dialog.decide_on_esc(),
    };
    let trigger_zap = match decision {
        ZapDecision::Confirm if !dialog.is_empty_tool() => Some(dialog.tool),
        _ => None,
    };
    let next_state = AppState {
        zap_dialog: None,
        ..state
    };
    (
        next_state,
        UpdateEffect {
            trigger_zap,
            ..UpdateEffect::default()
        },
    )
}

fn decide_unify_dialog(state: AppState, key: DialogKey) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.unify_dialog else {
        return (state, UpdateEffect::default());
    };
    let decision = match key {
        DialogKey::Enter => dialog.decide_on_enter(),
        DialogKey::Esc => dialog.decide_on_esc(),
    };
    match decision {
        UnifyDecision::Confirm => {
            let plan = dialog.plan.clone();
            let next_state = AppState {
                unify_dialog: None,
                ..state
            };
            (
                next_state,
                UpdateEffect {
                    trigger_unify: Some(plan),
                    ..UpdateEffect::default()
                },
            )
        }
        UnifyDecision::Cancel => {
            let next_state = AppState {
                unify_dialog: None,
                ..state
            };
            (next_state, UpdateEffect::default())
        }
        UnifyDecision::BackToConfirm => {
            // From DryRunPreview, Esc returns the dialog to Confirm mode
            // with the same plan. The dialog stays open.
            let mut next_state = state;
            if let Some(d) = next_state.unify_dialog.as_mut() {
                d.back_to_confirm();
            }
            (next_state, UpdateEffect::default())
        }
        UnifyDecision::DryRun => {
            // Defensive: decide_on_enter / decide_on_esc never return DryRun.
            // A future change might wire dry-run through DialogKey; until
            // then, no-op.
            (state, UpdateEffect::default())
        }
    }
}

/// US-14: handle `Msg::UnifyDryRun` (the `[n]` key while the unify dialog is
/// open). Reads the dialog mode; when Confirm, emits
/// `UpdateEffect::trigger_dry_run = Some(plan)` so the composition root can
/// invoke `actions::unify::dry_run`. The dialog STAYS OPEN — the dry-run
/// effect later dispatches `Msg::UnifyDryRunCompleted(lines)` which
/// transitions the dialog to `UnifyMode::DryRunPreview { lines }`.
fn decide_dry_run(state: AppState) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.unify_dialog else {
        return (state, UpdateEffect::default());
    };
    if !matches!(dialog.decide_on_dry_run_key(), UnifyDecision::DryRun) {
        // [n] outside Confirm mode is a no-op (AlreadyUnified, DryRunPreview).
        return (state, UpdateEffect::default());
    }
    let plan = dialog.plan.clone();
    (
        state,
        UpdateEffect {
            trigger_dry_run: Some(plan),
            ..UpdateEffect::default()
        },
    )
}

/// US-14: handle `Msg::UnifyDryRunCompleted(lines)`. Transition the open
/// unify dialog into `UnifyMode::DryRunPreview { lines }` so the render
/// layer shows the formatted "(dry-run) Would..." lines. If the dialog has
/// closed for any reason between the dry-run dispatch and its completion,
/// the message is silently dropped (defense in depth).
fn apply_dry_run_lines(mut state: AppState, lines: Vec<String>) -> (AppState, UpdateEffect) {
    if let Some(dialog) = state.unify_dialog.as_mut() {
        dialog.enter_dry_run_preview(lines);
    }
    (state, UpdateEffect::default())
}

/// Which key drove the cross-fs dialog decision (US-19).
enum CrossFsKey {
    Skip,
    Copy,
    Cancel,
}

/// Resolve a cross-fs choice (Skip/Copy/Cancel). On Skip or Copy, close the
/// dialog and emit `trigger_unify = Some(plan)` plus `cross_fs_choice =
/// Some(...)` so the orchestrator routes through the cross-fs aware path. On
/// Cancel, close the dialog with no destructive side-effect (refuse default).
fn decide_cross_fs(state: AppState, key: CrossFsKey) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.cross_fs_dialog else {
        return (state, UpdateEffect::default());
    };
    let plan = dialog.plan.clone();
    let (trigger_unify, cross_fs_choice) = match key {
        CrossFsKey::Skip => (Some(plan), Some(CrossFsChoice::Skip)),
        CrossFsKey::Copy => (Some(plan), Some(CrossFsChoice::Copy)),
        CrossFsKey::Cancel => (None, None),
    };
    let next_state = AppState {
        cross_fs_dialog: None,
        ..state
    };
    (
        next_state,
        UpdateEffect {
            trigger_unify,
            cross_fs_choice,
            ..UpdateEffect::default()
        },
    )
}

/// Which key drove the running-tool dialog decision (US-17, step 03-07).
/// `Retry` and `ProceedAnyway` close the dialog and emit
/// `trigger_running_tool_retry` so the orchestrator re-runs the gate (or
/// bypasses it for the lsof-unavailable case). `Cancel` just closes the
/// dialog with no side-effect (refuse default per intake Q5).
enum RunningToolKey {
    Retry,
    ProceedAnyway,
    Cancel,
}

/// Resolve a running-tool dialog decision. The pure update closes the
/// dialog and (for Retry / ProceedAnyway) emits a `RunningToolRetry`
/// effect carrying the pending action so the composition root re-runs the
/// gate (Retry) or bypasses it (ProceedAnyway). On Cancel, no effect is
/// emitted — the gated action is dropped, no filesystem mutation occurs.
fn decide_running_tool(state: AppState, key: RunningToolKey) -> (AppState, UpdateEffect) {
    let Some(dialog) = &state.running_tool_dialog else {
        return (state, UpdateEffect::default());
    };
    let trigger = match key {
        RunningToolKey::Retry => Some(retry_from_dialog(dialog, false)),
        RunningToolKey::ProceedAnyway => Some(retry_from_dialog(dialog, true)),
        RunningToolKey::Cancel => None,
    };
    let next_state = AppState {
        running_tool_dialog: None,
        ..state
    };
    (
        next_state,
        UpdateEffect {
            trigger_running_tool_retry: trigger,
            ..UpdateEffect::default()
        },
    )
}

/// Build the `RunningToolRetry` payload from the dialog's snapshot. The
/// `proceed_anyway` flag tells the orchestrator whether to BYPASS the
/// re-probe (true — user has acknowledged the missing safety check) or
/// re-RUN it (false — user closed the tool and wants to retry the gate).
fn retry_from_dialog(dialog: &RunningToolDialog, proceed_anyway: bool) -> RunningToolRetry {
    RunningToolRetry {
        action: dialog.pending_action.clone(),
        proceed_anyway,
    }
}

/// US-U5: flip the checkbox for target `idx` in the open unify dialog. No-op
/// when no dialog is open or `idx` is out of range (defense in depth).
fn apply_toggle_target(mut state: AppState, idx: usize) -> AppState {
    if let Some(dialog) = state.unify_dialog.as_mut() {
        dialog.toggle_target(idx);
    }
    state
}

/// US-U5: advance the per-target cursor in the open unify dialog. No-op
/// when no dialog is open.
fn apply_unify_dialog_next(mut state: AppState) -> AppState {
    if let Some(dialog) = state.unify_dialog.as_mut() {
        dialog.select_next_target();
    }
    state
}

/// US-U5: regress the per-target cursor in the open unify dialog. No-op
/// when no dialog is open.
fn apply_unify_dialog_prev(mut state: AppState) -> AppState {
    if let Some(dialog) = state.unify_dialog.as_mut() {
        dialog.select_prev_target();
    }
    state
}
