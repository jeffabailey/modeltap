//! Keymap — single source of truth for keypress → `Msg` translation AND for
//! the bottom-bar shortcut labels (per ADR-006 §"Keymap").
//!
//! `SHORTCUT_TABLE` is consumed by:
//! - `keymap::dispatch(KeyEvent) -> Msg` — the event handler.
//! - `render::bottom_bar::render` (in subsequent steps) — the shortcut display.
//!
//! Keeping one `&'static [Shortcut]` array means the displayed key cannot
//! drift from the dispatched key. The unit test
//! `shortcut_table_drives_both_render_label_and_dispatch_msg` enforces this.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::msg::Msg;

/// One shortcut entry. The `label` is what the bottom bar displays
/// (e.g. "[q] quit"); the `key` is what `dispatch()` matches against.
#[derive(Debug, Clone)]
pub struct Shortcut {
    pub key: KeyEvent,
    pub label: &'static str,
    pub msg: Msg,
}

/// The single source of truth for navigation + quit shortcuts. Per US-08
/// AC-5: "Shortcuts shown in the bar match the actual key handler dispatch
/// table (single source of truth)." Step 01-03 covers the navigation +
/// quit subset; later steps extend (z, u, ?, d, Esc, etc.).
pub const SHORTCUT_TABLE: &[Shortcut] = &[
    Shortcut {
        key: KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        label: "[<-] prev tool",
        msg: Msg::SelectPrevTool,
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        label: "[->] next tool",
        msg: Msg::SelectNextTool,
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        label: "[up] prev row",
        msg: Msg::SelectPrevRow,
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        label: "[down] next row",
        msg: Msg::SelectNextRow,
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        label: "[tab] focus",
        msg: Msg::ToggleFocus,
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        label: "[q] quit",
        msg: Msg::Quit,
    },
    Shortcut {
        key: KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        label: "[^C] interrupt",
        msg: Msg::CtrlC,
    },
];

/// Translate a `KeyEvent` into the corresponding `Msg`. Unmapped keys
/// produce `Msg::UnboundKey` so the update loop receives a single,
/// well-typed event for every keystroke (silently-ignored unbound keys
/// are still a state transition: the brief-highlight effect lands here
/// in subsequent steps).
pub fn dispatch(key: KeyEvent) -> Msg {
    for entry in SHORTCUT_TABLE {
        if key_event_matches(&entry.key, &key) {
            return entry.msg.clone();
        }
    }
    Msg::UnboundKey
}

/// Compare two `KeyEvent`s by code + modifiers (ignoring kind/state, which
/// are crossterm-version-dependent and not part of the contract).
fn key_event_matches(a: &KeyEvent, b: &KeyEvent) -> bool {
    a.code == b.code && a.modifiers == b.modifiers
}
