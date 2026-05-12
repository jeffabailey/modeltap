//! Keymap — single source of truth for keypress → `Msg` translation AND for
//! the bottom-bar shortcut labels (per ADR-006 §"Keymap").
//!
//! `SHORTCUT_TABLE` is consumed by:
//! - `keymap::dispatch(KeyEvent) -> Msg` — the event handler.
//! - `render::bottom_bar::render_bottom_bar` — the dynamic shortcut bar.
//! - `screens::help_overlay::render_help_lines` — the layered help overlay.
//!
//! Keeping one `&'static [Shortcut]` array means the displayed key cannot
//! drift from the dispatched key. The unit tests
//! `shortcut_table_drives_both_render_label_and_dispatch_msg` (US-03) and
//! `int_6_invariant_every_visible_bar_key_dispatches_to_non_noop` (US-08)
//! enforce this in opposite directions.
//!
//! ## Schema (US-08)
//!
//! Each entry carries:
//! - `key`: the `KeyEvent` `dispatch()` matches against.
//! - `label`: the static text the bottom bar / help overlay displays.
//! - `msg`: the `Msg` to produce on a match.
//! - `sections`: which bottom-bar contexts this shortcut belongs to (Main,
//!   Detail, Help, Dialog). The render fn filters SHORTCUT_TABLE by the
//!   currently-active section.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use modeltap_core::ToolId;

use crate::app_state::FocusPane;
use crate::msg::Msg;

/// Which bottom-bar context a shortcut belongs to. The dynamic bar render
/// fn filters SHORTCUT_TABLE by the currently-active section so the bar
/// text is derived purely from the table.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BarSection {
    /// Default two-pane discovery view (Screen::Main, no dialog).
    Main,
    /// Per-model detail screen (Screen::Detail).
    Detail,
    /// Help overlay (Screen::Help).
    Help,
    /// Modal dialog active (e.g., zap confirm).
    Dialog,
}

/// One shortcut entry. The `label` is what the bottom bar displays
/// (e.g. "[q] quit"); the `key` is what `dispatch()` matches against.
/// `sections` controls which bar context this shortcut appears in.
#[derive(Debug, Clone)]
pub struct Shortcut {
    pub key: KeyEvent,
    pub label: &'static str,
    pub msg: Msg,
    /// Bar contexts this shortcut belongs to. A shortcut with multiple
    /// sections (e.g., `[?] help` is global) shows up in each.
    pub sections: &'static [BarSection],
}

/// The single source of truth for shortcuts shown in the bottom bar AND
/// dispatched by `keymap::dispatch`. Per US-08 AC-5: "Shortcuts shown in
/// the bar match the actual key handler dispatch table (single source of
/// truth)."
pub const SHORTCUT_TABLE: &[Shortcut] = &[
    // ----- Main view ------------------------------------------------------
    Shortcut {
        key: KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        label: "[<-/->] tools",
        msg: Msg::SelectPrevTool,
        sections: &[BarSection::Main],
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        label: "[->] next tool",
        msg: Msg::SelectNextTool,
        // Right Arrow is implicit in the "[<-/->] tools" main label so we
        // do not show it separately in the bar — it is dispatched only.
        sections: &[],
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        label: "[up/down] models",
        msg: Msg::SelectPrevRow,
        sections: &[BarSection::Main],
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        label: "[down] next row",
        msg: Msg::SelectNextRow,
        // Down is implicit in "[up/down] models" — dispatch-only.
        sections: &[],
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        label: "[tab] focus",
        msg: Msg::ToggleFocus,
        sections: &[],
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        label: "[u] unify",
        msg: Msg::Unify,
        sections: &[BarSection::Main, BarSection::Detail],
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
        label: "[z] zap tool",
        msg: Msg::ZapTool,
        sections: &[BarSection::Main],
    },
    // US-11.AC-2: [r] retry — visible only when state.refresh_failed_tools is
    // non-empty (the bottom bar's `is_available` predicate dims the entry
    // otherwise; the bar render filter further omits it on Main with no
    // failure). The keymap dispatches a sentinel ToolId(""); the composition
    // root resolves the actual failed tool from state when re-spawning.
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        label: "[r] retry",
        msg: Msg::RetryRefresh(ToolId("")),
        sections: &[BarSection::Main, BarSection::Detail],
    },
    // ----- Detail view ----------------------------------------------------
    Shortcut {
        key: KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        label: "[Esc] back",
        msg: Msg::CloseDetail,
        sections: &[BarSection::Detail],
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        label: "[d] delete-from-one",
        msg: Msg::DeleteFromOne,
        // Visible in BOTH Main and Detail. Main fires the dialog against the
        // currently-highlighted right-pane row; Detail fires against the
        // detail screen's first registration (or the env-overridden tool in
        // headless tests). The orchestrator builds the DeleteOneConfirmState
        // appropriately for each screen.
        sections: &[BarSection::Main, BarSection::Detail],
    },
    // US-05c.AC-4 / AC-19: Shift+F — folder-group bulk delete. The keymap
    // emits the bare `RequestFolderDelete` request; the composition root
    // (step 01-05) resolves the cursor's folder from AppState. INT-FGD-7's
    // single-source-of-truth invariant: NO hardcoded `<author>/<repo>`
    // literal lives in this file — see `tests/lint.rs` for the architecture
    // lint that enforces it.
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT),
        label: "[F] folder-delete",
        msg: Msg::RequestFolderDelete,
        sections: &[BarSection::Main],
    },
    // ----- Global ---------------------------------------------------------
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        label: "[?] help",
        msg: Msg::ToggleHelp,
        sections: &[BarSection::Main, BarSection::Detail, BarSection::Help],
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        label: "[q] quit",
        msg: Msg::Quit,
        sections: &[BarSection::Main],
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        label: "[^C] interrupt",
        msg: Msg::CtrlC,
        sections: &[],
    },
];

/// Translate a `KeyEvent` into the corresponding `Msg`. Unmapped keys
/// produce `Msg::UnboundKey` so the update loop receives a single,
/// well-typed event for every keystroke (silently-ignored unbound keys
/// are still a state transition: the brief-highlight effect lands here
/// in subsequent steps).
///
/// Compatibility shim: delegates to `dispatch_focus_aware` with
/// `FocusPane::Right` so callers that do not (yet) thread focus state
/// continue to receive the legacy single-pane semantics — Up/Down navigate
/// model rows. The composition root (`modeltap-app::interactive` and
/// `modeltap-app::headless`) calls `dispatch_focus_aware` directly so the
/// left pane can navigate tools when it has focus.
pub fn dispatch(key: KeyEvent) -> Msg {
    dispatch_focus_aware(key, FocusPane::Right)
}

/// Focus-aware variant of `dispatch`. When the left pane has focus, Up/Down
/// navigate TOOLS (`SelectPrevTool` / `SelectNextTool`); when the right pane
/// has focus, Up/Down navigate ROWS (`SelectPrevRow` / `SelectNextRow`).
/// Left/Right and Tab are focus-independent.
pub fn dispatch_focus_aware(key: KeyEvent, focus: FocusPane) -> Msg {
    // Focus-aware Up/Down: when the left pane has focus, Up/Down move the
    // tool selection so a single mental model ("arrow keys move the cursor in
    // the focused pane") works for both panes. Right-pane focus retains the
    // legacy row-navigation semantics.
    if let FocusPane::Left = focus {
        match (key.code, key.modifiers) {
            (KeyCode::Up, KeyModifiers::NONE) => return Msg::SelectPrevTool,
            (KeyCode::Down, KeyModifiers::NONE) => return Msg::SelectNextTool,
            _ => {}
        }
    }
    for entry in SHORTCUT_TABLE {
        if key_event_matches(&entry.key, &key) {
            return entry.msg.clone();
        }
    }
    Msg::UnboundKey
}

/// Truthful bottom-bar / help-overlay label for the Up/Down arrow row given
/// the current pane focus. The bottom-bar render fn (and the help overlay)
/// substitute this for the static `[up/down] models` label whenever the
/// shortcut entry's key code is Up/Down — so the bar tells the user what
/// the keys WILL do in the current focus state.
///
/// Returning `&'static str` keeps the render layer allocation-free.
pub fn up_down_bar_label(focus: FocusPane) -> &'static str {
    match focus {
        FocusPane::Left => "[up/down] tools",
        FocusPane::Right => "[up/down] models",
    }
}

/// Translate a `KeyEvent` into a dialog `Msg` while a typed-input dialog is
/// open. Routes the keys US-05 cares about (printable chars → DialogTextInput,
/// Backspace → DialogBackspace, Enter → DialogConfirm, Esc → DialogCancel).
/// Anything else falls through to `dispatch()` so global shortcuts (Ctrl+C)
/// still work.
///
/// US-U5: when `unify_dialog_open` is true, [space] dispatches
/// `Msg::ToggleTarget(0)` (the call site lifts it to the cursor position via
/// `lift_toggle_in_unify_dialog`-equivalent in the production loop) and the
/// arrow keys move the per-target cursor.
pub fn dispatch_in_dialog(key: KeyEvent, unify_dialog_open: bool) -> Msg {
    // Global override: Ctrl+C must always interrupt, even with a dialog open.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Msg::CtrlC;
    }
    if unify_dialog_open {
        match key.code {
            KeyCode::Char(' ') => return Msg::ToggleTarget(0),
            KeyCode::Up => return Msg::UnifyDialogSelectPrev,
            KeyCode::Down => return Msg::UnifyDialogSelectNext,
            _ => {}
        }
    }
    match key.code {
        KeyCode::Esc => Msg::DialogCancel,
        KeyCode::Enter => Msg::DialogConfirm,
        KeyCode::Backspace => Msg::DialogBackspace,
        KeyCode::Char(c) => Msg::DialogTextInput(c),
        _ => Msg::UnboundKey,
    }
}

/// Compare two `KeyEvent`s by code + modifiers (ignoring kind/state, which
/// are crossterm-version-dependent and not part of the contract).
fn key_event_matches(a: &KeyEvent, b: &KeyEvent) -> bool {
    a.code == b.code && a.modifiers == b.modifiers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::FocusPane;

    // -----------------------------------------------------------------------
    // RED_ACCEPTANCE — focus-aware Up/Down dispatch (AC #1-4).
    //
    // When FocusPane::Left is active, Up/Down navigate TOOLS (the left pane).
    // When FocusPane::Right is active, Up/Down navigate ROWS (regression of
    // the prior single-pane behavior).
    // -----------------------------------------------------------------------
    #[test]
    fn up_down_dispatch_is_focus_aware() {
        let cases: &[(FocusPane, KeyCode, Msg)] = &[
            // AC #1, #2 — Left pane focused: Up/Down navigate tools.
            (FocusPane::Left, KeyCode::Up, Msg::SelectPrevTool),
            (FocusPane::Left, KeyCode::Down, Msg::SelectNextTool),
            // AC #3, #4 — Right pane focused (regression): Up/Down navigate rows.
            (FocusPane::Right, KeyCode::Up, Msg::SelectPrevRow),
            (FocusPane::Right, KeyCode::Down, Msg::SelectNextRow),
        ];
        for (focus, code, expected) in cases {
            let key = KeyEvent::new(*code, KeyModifiers::NONE);
            let got = dispatch_focus_aware(key, *focus);
            assert_eq!(
                got, *expected,
                "dispatch_focus_aware({:?}, {:?}) → {:?}, expected {:?}",
                code, focus, got, expected
            );
        }
    }

    // -----------------------------------------------------------------------
    // RED_UNIT — AC #5: Left/Right arrows are focus-INDEPENDENT.
    // -----------------------------------------------------------------------
    #[test]
    fn left_right_arrows_dispatch_to_tool_navigation_regardless_of_focus() {
        for focus in [FocusPane::Left, FocusPane::Right] {
            let left = KeyEvent::new(KeyCode::Left, KeyModifiers::NONE);
            let right = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
            assert_eq!(
                dispatch_focus_aware(left, focus),
                Msg::SelectPrevTool,
                "Left arrow must always dispatch SelectPrevTool (focus={:?})",
                focus
            );
            assert_eq!(
                dispatch_focus_aware(right, focus),
                Msg::SelectNextTool,
                "Right arrow must always dispatch SelectNextTool (focus={:?})",
                focus
            );
        }
    }

    // -----------------------------------------------------------------------
    // RED_UNIT — AC #6: Tab is focus-INDEPENDENT (no regression).
    // -----------------------------------------------------------------------
    #[test]
    fn tab_dispatches_toggle_focus_regardless_of_focus() {
        for focus in [FocusPane::Left, FocusPane::Right] {
            let key = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
            assert_eq!(
                dispatch_focus_aware(key, focus),
                Msg::ToggleFocus,
                "Tab must always dispatch ToggleFocus (focus={:?})",
                focus
            );
        }
    }

    // -----------------------------------------------------------------------
    // RED_UNIT — AC #7: SHORTCUT_TABLE Up row's bar label reflects focus.
    //
    // The bottom bar's Up/Down entry must read "[up/down] tools" when Left
    // pane has focus and "[up/down] models" when Right pane has focus, so the
    // help bar tells the truth in BOTH focus states. Lookup is done by the
    // `up_down_bar_label(focus)` accessor (single source of truth lifted from
    // SHORTCUT_TABLE).
    // -----------------------------------------------------------------------
    #[test]
    fn up_down_bar_label_is_focus_aware() {
        assert_eq!(
            up_down_bar_label(FocusPane::Left),
            "[up/down] tools",
            "When Left pane has focus, Up/Down navigate tools — bar must say so"
        );
        assert_eq!(
            up_down_bar_label(FocusPane::Right),
            "[up/down] models",
            "When Right pane has focus, Up/Down navigate model rows — bar must say so"
        );
    }

    // -----------------------------------------------------------------------
    // RED_UNIT — AC #10: dispatch_in_dialog is unchanged.
    //
    // Arrow keys inside the unify dialog continue to drive
    // UnifyDialogSelectPrev/Next per US-U5. Focus-aware dispatch only applies
    // to the top-level (non-dialog) keymap.
    // -----------------------------------------------------------------------
    #[test]
    fn dispatch_in_dialog_unify_arrows_unchanged_by_focus_refactor() {
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(
            dispatch_in_dialog(up, /* unify_dialog_open */ true),
            Msg::UnifyDialogSelectPrev,
            "US-U5: arrow Up inside unify dialog must dispatch UnifyDialogSelectPrev"
        );
        assert_eq!(
            dispatch_in_dialog(down, /* unify_dialog_open */ true),
            Msg::UnifyDialogSelectNext,
            "US-U5: arrow Down inside unify dialog must dispatch UnifyDialogSelectNext"
        );
    }

    // -----------------------------------------------------------------------
    // SHORTCUT_TABLE invariant extended to BOTH focus states (AC #8).
    //
    // Existing invariant: every SHORTCUT_TABLE entry's `key` dispatches to its
    // declared `msg`. With focus-aware dispatch, Up/Down rows are special-
    // cased — but their declared `msg` still holds for at least one focus
    // (the focus the row was authored for). We preserve the original
    // single-focus invariant here for the Right-pane case (which is the
    // legacy default — "Up = SelectPrevRow") so the existing user-facing
    // contract is not weakened.
    // -----------------------------------------------------------------------
    #[test]
    fn shortcut_table_dispatches_consistently_under_focus_right() {
        for entry in SHORTCUT_TABLE {
            let mapped = dispatch_focus_aware(entry.key, FocusPane::Right);
            // Up/Down rows in SHORTCUT_TABLE were authored for the right
            // pane (label "[up/down] models" + msg SelectPrevRow/SelectNextRow).
            // Under FocusPane::Right the table-declared msg must hold.
            if matches!(entry.key.code, KeyCode::Up | KeyCode::Down) {
                assert_eq!(
                    mapped, entry.msg,
                    "SHORTCUT_TABLE Up/Down entry under FocusPane::Right \
                     must produce its declared msg, got {:?} expected {:?}",
                    mapped, entry.msg
                );
            } else {
                // All other entries must produce their declared msg in EITHER
                // focus (focus only changes Up/Down semantics).
                assert_eq!(
                    mapped, entry.msg,
                    "SHORTCUT_TABLE entry {:?} dispatch mismatch under \
                     FocusPane::Right",
                    entry.key
                );
                let mapped_left = dispatch_focus_aware(entry.key, FocusPane::Left);
                assert_eq!(
                    mapped_left, entry.msg,
                    "SHORTCUT_TABLE entry {:?} dispatch mismatch under \
                     FocusPane::Left",
                    entry.key
                );
            }
        }
    }
}
