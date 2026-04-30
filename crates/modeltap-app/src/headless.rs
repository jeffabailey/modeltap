//! Headless TUI mode (per `docs/feature/modeltap-tui/distill/acceptance-test-plan.md` §4).
//!
//! When `MODELTAP_HEADLESS=1` (or `--headless`), the binary runs the same
//! `update()` and `view()` functions as production, but renders against
//! ratatui's `TestBackend` and consumes scripted input from
//! `MODELTAP_HEADLESS_INPUT` instead of the real terminal.
//!
//! This contract gives the acceptance suite a deterministic, scriptable, fast
//! test harness while preserving production code paths.

use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use modeltap_core::domain::last_action::LastAction;
use modeltap_core::logic::canonical_selector::{select_canonical, CandidatePath};
use modeltap_core::logic::plan::{build_plan, PlanCandidate, UnifyPlan};
use modeltap_core::{Tool, ToolId};
use modeltap_tui::app_state::Screen;
use modeltap_tui::screens::detail::DetailScreenState;
use modeltap_tui::{update, view, AppState, Msg, UpdateEffect};
use ratatui::backend::TestBackend;
use ratatui::Terminal;

use crate::actions::unify::{self, UnifyOutcome, UnifyResult};
use crate::actions::zap::{self, ZapOutcome, ZapResult};
use crate::observability::{LaunchLogger, RecordKind};
use crate::refresh;

/// Configuration parsed from CLI args + env at startup.
pub struct HeadlessConfig {
    pub cols: u16,
    pub rows: u16,
    /// Scripted input. Empty string means "no input — paint once and quit when
    /// `--quit-after-paint` is set".
    pub input: String,
    /// When true, render one frame and exit cleanly. Used by `launch.timing`
    /// and the K3 benchmark.
    pub quit_after_paint: bool,
}

/// Run the headless event loop. Returns the process exit code.
pub fn run(
    config: HeadlessConfig,
    initial_state: AppState,
    mut logger: LaunchLogger,
    plugins: Vec<Box<dyn Tool>>,
) -> i32 {
    let mut terminal = match Terminal::new(TestBackend::new(config.cols, config.rows)) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("modeltap: failed to construct TestBackend: {e}");
            return 1;
        }
    };

    let mut state = initial_state;

    // Initial paint — required by US-01 AC-1 (cold start to first paint).
    if let Err(e) = terminal.draw(|f| view(&state, f)) {
        eprintln!("modeltap: initial paint failed: {e}");
        return 1;
    }

    // Tokens parsed up-front (script tokens are independent of state); we
    // resolve each token to the right Msg per-iteration based on whether a
    // dialog is open at that moment.
    let tokens = tokenize_script(&config.input);
    let token_count = tokens.len();

    // Lazy-construct a tokio runtime ONLY if a zap actually fires (the @us-01
    // K3 path must not pay the runtime cost).
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("modeltap: failed to construct tokio runtime: {e}");
            return 1;
        }
    };

    for token in tokens {
        let dialog_open = state.zap_dialog.is_some() || state.unify_dialog.is_some();
        let cross_fs_open = state.cross_fs_dialog.is_some();
        let raw_msg = token_to_msg(&token, dialog_open, cross_fs_open);
        // Intercept Msg::Unify on the detail screen so we can build the
        // UnifyPlan from the registrations + plugins (the plan needs `stat`
        // results, which the pure update() can't compute). Outside the
        // detail screen, `Msg::Unify` stays a no-op per the keymap docs.
        let msg = lift_unify_in_detail(&state, raw_msg);
        // Intercept Enter on the main screen so we can open the detail
        // screen with synthesized registrations from the AppState.
        let msg = lift_enter_in_main(&state, msg);
        let (next, effect) = update(state, msg);
        state = next;
        if let Err(e) = terminal.draw(|f| view(&state, f)) {
            eprintln!("modeltap: redraw failed: {e}");
            return 1;
        }
        apply_effect(&effect, &mut logger, &plugins, &rt, &mut state);
        if state.should_quit {
            break;
        }
    }

    if !state.should_quit && !config.quit_after_paint {
        eprintln!("modeltap: headless mode invoked without input and without --quit-after-paint");
        return 1;
    }

    // Final repaint so footer messages set by zap are visible in the captured
    // frame.
    if let Err(e) = terminal.draw(|f| view(&state, f)) {
        eprintln!("modeltap: final paint failed: {e}");
        return 1;
    }

    print_frame(&terminal);

    let summary = serde_json::json!({
        "schema": "modeltap.session_summary.v1",
        "frames_captured": 1 + token_count,
        "exit_reason": exit_reason(&state),
        "exit_code": state.exit_code,
        "log_path": logger.path().map(|p| p.display().to_string()),
    });
    println!("{}", summary);

    state.exit_code
}

fn exit_reason(state: &AppState) -> &'static str {
    match state.exit_code {
        0 => "user_quit",
        130 => "ctrl_c",
        _ => "other",
    }
}

fn print_frame(terminal: &Terminal<TestBackend>) {
    let backend = terminal.backend();
    let buffer = backend.buffer();
    for y in 0..buffer.area.height {
        let mut line = String::with_capacity(buffer.area.width as usize);
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        let _ = std::io::stdout().write_all(line.trim_end().as_bytes());
        let _ = std::io::stdout().write_all(b"\n");
    }
}

fn apply_effect(
    effect: &UpdateEffect,
    logger: &mut LaunchLogger,
    plugins: &[Box<dyn Tool>],
    rt: &tokio::runtime::Runtime,
    state: &mut AppState,
) {
    if effect.emit_launch_ended {
        logger.record(RecordKind::LaunchEnded);
    }
    if let Some(tool_id) = effect.trigger_zap {
        if let Some(plugin) = find_plugin(plugins, tool_id) {
            let outcome: ZapOutcome = rt.block_on(zap::run(plugin, logger));
            // Build the structured LastAction and dispatch it as a Msg so
            // the Elm-style update is the only place that mutates AppState
            // (per ADR-006).
            let action = build_last_action(&outcome);
            let (next, _) = update(std::mem::take(state), Msg::SetLastAction(action));
            *state = next;

            // Per US-06.AC-4 / US-11.AC-1: re-run discover() ONLY for the
            // affected tool to keep the summary refresh under 500 ms (the
            // alternative — re-running every plugin's discover() — scales
            // O(N plugins) and would break the budget once HF/llama-cli
            // populate). Failures here are logged but non-fatal: the
            // existing slot stays in place so the UI doesn't go blank.
            match rt.block_on(refresh::refresh_tool(plugin)) {
                Ok(view) => {
                    let (next, _) = update(std::mem::take(state), Msg::RefreshTool(view));
                    *state = next;
                }
                Err(e) => {
                    tracing::warn!(
                        target: "modeltap.refresh",
                        "refresh_tool failed for {}: {e}",
                        tool_id.0
                    );
                }
            }
        } else {
            // Pathological — UI selected a tool that's not in the plugin set.
            tracing::warn!(target: "modeltap.action.zap", "no plugin for {}", tool_id.0);
        }
    }
    if let Some(plan) = effect.trigger_unify.clone() {
        // Synthesize the on-screen target name from the model id in the
        // detail screen state (if any) — fall back to the canonical's
        // basename. Used for the LastAction banner only.
        let target_name = match &state.current_screen {
            Screen::Detail(d) => d.model.id.clone(),
            _ => plan
                .canonical
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("model")
                .to_string(),
        };
        let outcome: UnifyOutcome = rt.block_on(unify::run(
            plan.clone(),
            plugins,
            logger,
            effect.cross_fs_choice,
        ));
        let last_action = build_unify_last_action(&outcome, target_name);
        let (next, _) = update(std::mem::take(state), Msg::SetLastAction(last_action));
        *state = next;
    }
}

/// On the detail screen, lift `Msg::Unify` (the keymap-bound no-op variant)
/// into a `Msg::OpenUnifyDialog(plan)` after building the plan from the
/// detail screen's registrations + a `stat` of each path. On any other
/// screen — or when the registrations don't form a valid multi-path unify —
/// the original message passes through unchanged.
///
/// US-19 (step 03-03): when the constructed plan has 1+ cross-filesystem
/// target, lift to `Msg::OpenCrossFsDialog(plan)` instead so the user gets
/// the per-target [s] skip / [c] copy / [x] cancel choice per ADR-008's
/// refuse-default policy.
fn lift_unify_in_detail(state: &AppState, msg: Msg) -> Msg {
    if !matches!(msg, Msg::Unify) {
        return msg;
    }
    let Screen::Detail(detail) = &state.current_screen else {
        return msg;
    };
    let Some(plan) = build_plan_from_detail(detail) else {
        // Can't build a plan (e.g., single-tool model, missing files) —
        // leave Msg::Unify as a no-op. The dialog opens only when there's
        // something to do.
        return msg;
    };
    // US-19 — any active cross-fs target routes to the choice dialog.
    let has_cross_fs = plan
        .links
        .iter()
        .any(|l| !l.already_linked && l.cross_filesystem);
    if has_cross_fs {
        Msg::OpenCrossFsDialog(plan)
    } else {
        Msg::OpenUnifyDialog(plan)
    }
}

/// Build a `UnifyPlan` by stat-ing each registration's path. Used by the
/// headless harness when intercepting `Msg::Unify` on the detail screen.
/// Returns None when no candidate has both a non-empty stat result AND the
/// resulting candidates can produce a non-degenerate plan (every
/// registration must point at an existing file). The headless harness
/// treats None as "leave Msg::Unify as a no-op".
fn build_plan_from_detail(detail: &DetailScreenState) -> Option<UnifyPlan> {
    if detail.registrations.is_empty() {
        return None;
    }
    // US-19 fake-fs-probe injection (test seam only). The env var is a
    // colon-separated list of canonicalized path prefixes; any registration
    // whose canonicalized path starts with one of these prefixes has its
    // `device` overridden to a synthetic non-canonical value so the
    // `cross_filesystem` flag fires for that target. Production paths never
    // match (the env var is unset), so this is zero-impact in real runs.
    let fake_cross_fs: Vec<PathBuf> = std::env::var("MODELTAP_FAKE_CROSS_FS_PATHS")
        .ok()
        .map(|s| s.split(':').map(PathBuf::from).collect())
        .unwrap_or_default();
    let mut candidates: Vec<CandidatePath> = Vec::new();
    let mut plan_candidates: Vec<PlanCandidate> = Vec::new();
    for reg in &detail.registrations {
        // Resolve symlinks to the underlying blob path. Real-production HF
        // discovery already does this (per plugins/hf/src/discover.rs); the
        // headless detail-regs JSON seam may pass a snapshot symlink, so we
        // canonicalize here to mirror production semantics. `canonicalize`
        // returns Err for non-existent paths, in which case we fall back to
        // the original path so the build_plan defensive branches still fire.
        let resolved_path = std::fs::canonicalize(&reg.path).unwrap_or_else(|_| reg.path.clone());
        let (exists, mut device, inode, size_bytes) = match std::fs::metadata(&resolved_path) {
            Ok(m) => (true, m.dev(), m.ino(), m.len()),
            Err(_) => (false, 0, 0, 0),
        };
        // Apply the fake-fs-probe override. We use a sentinel device id
        // (`u64::MAX`) so it cannot collide with any real `dev_t`; per-path
        // mismatch is enough for `build_plan` to flag `cross_filesystem`.
        if path_matches_fake_cross_fs(&resolved_path, &fake_cross_fs) {
            device = u64::MAX;
        }
        candidates.push(CandidatePath {
            tool: reg.tool,
            path: resolved_path.clone(),
            exists,
            size_bytes,
            // The detail-screen-driven path doesn't know which is an Ollama
            // blob; rely on the lexicographic tiebreak in select_canonical.
            // (Production wires this via the plugin's path-classifier.)
            is_ollama_blob: reg.tool == ToolId("ollama"),
        });
        plan_candidates.push(PlanCandidate {
            tool: reg.tool,
            path: resolved_path,
            exists,
            device,
            inode,
            size_bytes,
        });
    }
    let canonical = select_canonical(&candidates)?;
    let canonical_plan = plan_candidates
        .iter()
        .find(|p| p.path == canonical.path)?
        .clone();
    build_plan(&canonical_plan, &plan_candidates)
}

/// On the main screen, lift Enter (which `update` would otherwise treat as
/// `Msg::DialogConfirm` no-op) into `Msg::OpenDetail(...)` so the headless
/// harness can navigate from the row list into the detail screen. The
/// detail screen's registrations are synthesized from the headless test
/// fixture environment via `MODELTAP_HEADLESS_DETAIL_REGS` — a JSON array
/// of `{tool, path}` entries. Falls through unchanged when the env-var is
/// not set OR when we're not on the main screen.
fn lift_enter_in_main(state: &AppState, msg: Msg) -> Msg {
    if !matches!(msg, Msg::DialogConfirm) {
        return msg;
    }
    if !matches!(state.current_screen, Screen::Main) {
        return msg;
    }
    if state.zap_dialog.is_some() || state.unify_dialog.is_some() {
        return msg; // dialog confirm — let update handle it
    }
    let Some(detail) = synthesize_detail_from_env(state) else {
        return msg;
    };
    Msg::OpenDetail(detail)
}

/// Build a `DetailScreenState` from the `MODELTAP_HEADLESS_DETAIL_REGS`
/// env-var JSON payload. The env-var is the headless-test-only seam that
/// lets a test inject the cross-tool registrations a real production
/// orchestrator would compute from the Inventory's dedup-key index. Format:
///
/// ```json
/// {
///   "id": "mistralai/Mistral-7B-v0.3",
///   "regs": [
///     {"tool": "ollama",    "path": "/abs/path/a"},
///     {"tool": "hf",        "path": "/abs/path/b"},
///     {"tool": "llama-cli", "path": "/abs/path/c"}
///   ]
/// }
/// ```
fn synthesize_detail_from_env(_state: &AppState) -> Option<DetailScreenState> {
    use modeltap_core::logic::unification_status::{DetailModelView, DetailRegistration};
    use modeltap_core::{ContentHash, DisplayLabel, Format, ModelStatus};

    let raw = std::env::var("MODELTAP_HEADLESS_DETAIL_REGS").ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let id = value.get("id")?.as_str()?.to_string();
    let regs_json = value.get("regs")?.as_array()?;
    let mut registrations: Vec<DetailRegistration> = Vec::new();
    for r in regs_json {
        let tool_str = r.get("tool")?.as_str()?;
        let path_str = r.get("path")?.as_str()?;
        let tool = match tool_str {
            "ollama" => ToolId("ollama"),
            "hf" => ToolId("hf"),
            "llama-cli" => ToolId("llama-cli"),
            "lm-studio" => ToolId("lm-studio"),
            _ => continue,
        };
        let path = PathBuf::from(path_str);
        let inode = std::fs::metadata(&path).ok().map(|m| m.ino());
        registrations.push(DetailRegistration { tool, path, inode });
    }
    if registrations.is_empty() {
        return None;
    }
    // Compute canonical_size from the largest existing reg.
    let canonical_size_bytes = registrations
        .iter()
        .filter_map(|r| std::fs::metadata(&r.path).ok().map(|m| m.len()))
        .max()
        .unwrap_or(0);
    let model_view = DetailModelView {
        id: id.clone(),
        format: Format::Other,
        format_quant: None,
        canonical_size_bytes,
        display_label: DisplayLabel::from(id),
        status: ModelStatus::Healthy,
    };
    // Provide a fake content hash so the detail screen renders fully (the
    // hash isn't asserted in US-10 scenarios; the Hasher port wiring is a
    // separate seam).
    let hash = ContentHash([0xAA; 32]);
    Some(DetailScreenState::new(
        model_view,
        registrations,
        Some(hash),
    ))
}

/// Map a `UnifyOutcome` to a structured `LastAction` for the right-pane
/// banner. `target_name` is the model identifier (display id) — NOT a tool
/// id — because unify acts on a single model across multiple tools.
fn build_unify_last_action(outcome: &UnifyOutcome, target_name: String) -> LastAction {
    let hardlink_count = outcome.tools_unified.len();
    match outcome.outcome {
        UnifyResult::Success => {
            LastAction::for_unify_success(target_name, outcome.bytes_reclaimed, hardlink_count)
        }
        UnifyResult::AlreadyUnified => {
            LastAction::for_unify_already_unified(target_name, hardlink_count)
        }
        UnifyResult::Partial => {
            // 03-03 will plumb per-target failure detail; for now we
            // construct a partial banner with the failures the orchestrator
            // collected.
            use modeltap_core::domain::last_action::TargetError;
            let failures: Vec<TargetError> = outcome
                .failures
                .iter()
                .map(|f| TargetError {
                    path: f.target.display().to_string(),
                    reason: f.reason.clone(),
                })
                .collect();
            let successes = hardlink_count as u64;
            LastAction::for_unify_partial(target_name, outcome.bytes_reclaimed, successes, failures)
        }
        UnifyResult::Failed => LastAction::for_unify_failed(target_name),
    }
}

/// Map a `ZapOutcome` to a structured `LastAction` for the right-pane banner.
/// Bytes-retained is 0 in the WS slice — cross-tool sharing classifier lands
/// in 03-01.
fn build_last_action(outcome: &ZapOutcome) -> LastAction {
    match outcome.outcome {
        ZapResult::Success => LastAction::for_zap_success(outcome.tool, outcome.bytes_reclaimed, 0),
        ZapResult::Partial => {
            // WS slice never produces Partial from a real plugin; render
            // it as Failed so the user is not misled into thinking a partial
            // success is reflected. Once 03-03 lands, switch to the proper
            // Partial constructor with target-error detail.
            LastAction::for_zap_failed(outcome.tool)
        }
        ZapResult::Empty | ZapResult::Failed => LastAction::for_zap_failed(outcome.tool),
    }
}

fn find_plugin(plugins: &[Box<dyn Tool>], tool_id: ToolId) -> Option<&dyn Tool> {
    plugins
        .iter()
        .find(|p| p.name().0 == tool_id.0)
        .map(|b| b.as_ref())
}

/// One scripted token. Parsed once up-front; resolved to a `Msg` per
/// iteration based on whether a dialog is open at that moment.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ScriptToken {
    Char(char),
    Tag(String),
    CtrlC,
}

fn tokenize_script(raw: &str) -> Vec<ScriptToken> {
    let mut out = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '^' => match chars.next() {
                Some('C') => out.push(ScriptToken::CtrlC),
                Some(other) => out.push(ScriptToken::Char(other)),
                None => {}
            },
            '<' => {
                let mut tag = String::new();
                let mut closed = false;
                for tc in chars.by_ref() {
                    if tc == '>' {
                        closed = true;
                        break;
                    }
                    tag.push(tc);
                }
                if !closed {
                    out.push(ScriptToken::Char('<'));
                    continue;
                }
                out.push(ScriptToken::Tag(tag));
            }
            _ if c.is_whitespace() => {}
            _ => out.push(ScriptToken::Char(c)),
        }
    }
    out
}

/// True iff `path` (or any of its ancestors) appears in `fake_cross_fs`. The
/// list of fake-cross-fs paths is treated as a prefix list — registering a
/// directory marks every file beneath it as cross-fs from the canonical's
/// perspective. Used by the US-19 acceptance harness; never set in production.
fn path_matches_fake_cross_fs(path: &std::path::Path, fake_cross_fs: &[PathBuf]) -> bool {
    if fake_cross_fs.is_empty() {
        return false;
    }
    fake_cross_fs
        .iter()
        .any(|p| path.starts_with(p) || p == path)
}

/// Resolve a `ScriptToken` to an `Msg`, accounting for whether a typed-input
/// dialog is currently open. Mirrors `keymap::dispatch_in_dialog` in spirit
/// (printable chars go to the dialog buffer; only Esc/Enter/Backspace are
/// dialog control). Outside a dialog, the script-token-to-Msg mapping
/// matches the @us-03 acceptance contract.
///
/// US-19 cross-fs dialog: when `cross_fs_open` is true, `s` / `c` / `x`
/// (and Esc / Enter, per the refuse-default policy) are interpreted as
/// `Msg::CrossFsSkip` / `Msg::CrossFsCopy` / `Msg::CrossFsCancel`.
fn token_to_msg(token: &ScriptToken, dialog_open: bool, cross_fs_open: bool) -> Msg {
    if cross_fs_open {
        return match token {
            ScriptToken::CtrlC => Msg::CtrlC,
            ScriptToken::Tag(t) => match t.as_str() {
                // Esc and Enter both default to refuse per ADR-008 OQ-4.
                "esc" | "enter" => Msg::CrossFsCancel,
                _ => Msg::UnboundKey,
            },
            ScriptToken::Char(c) => match c {
                's' => Msg::CrossFsSkip,
                'c' => Msg::CrossFsCopy,
                'x' => Msg::CrossFsCancel,
                _ => Msg::UnboundKey,
            },
        };
    }
    if dialog_open {
        return match token {
            ScriptToken::CtrlC => Msg::CtrlC,
            ScriptToken::Tag(t) => match t.as_str() {
                "esc" => Msg::DialogCancel,
                "enter" => Msg::DialogConfirm,
                "backspace" => Msg::DialogBackspace,
                _ => Msg::UnboundKey,
            },
            ScriptToken::Char(c) => Msg::DialogTextInput(*c),
        };
    }
    match token {
        ScriptToken::CtrlC => Msg::CtrlC,
        ScriptToken::Tag(t) => match t.as_str() {
            "right" => Msg::SelectNextTool,
            "left" => Msg::SelectPrevTool,
            "down" => Msg::SelectNextRow,
            "up" => Msg::SelectPrevRow,
            "tab" => Msg::ToggleFocus,
            "esc" => Msg::DialogCancel,
            "enter" => Msg::DialogConfirm,
            "backspace" => Msg::DialogBackspace,
            _ => Msg::UnboundKey,
        },
        ScriptToken::Char(c) => match c {
            'q' => Msg::Quit,
            'z' => Msg::ZapTool,
            'u' => Msg::Unify,
            'd' => Msg::DeleteFromOne,
            '?' => Msg::ToggleHelp,
            _ => Msg::UnboundKey,
        },
    }
}
