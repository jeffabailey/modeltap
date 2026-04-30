//! `DeleteOneConfirmState` — the single-model delete confirmation dialog
//! (US-05b, step 03-06; per ADR-009).
//!
//! Pure state machine. The dialog opens with a snapshot of the targeted
//! model's metadata (model id, tool, size, was_shared classification) and
//! splits its UX on shared-vs-unique:
//!
//! - **Shared** (`was_shared = true`): single-key `[y/n]` confirmation. The
//!   model's content is preserved on disk under another tool's tree, so the
//!   blast radius is limited to one registration row + (for some tools) a
//!   single-file unlink. Low-friction confirmation matches the small-blast-
//!   radius.
//! - **Unique** (`was_shared = false`): typed-id confirmation matching US-05's
//!   safety bar. The model's content vanishes entirely from this machine; we
//!   require the user to spell the model id exactly (BYTE-EQUAL, CASE-
//!   SENSITIVE) before destructive work proceeds.
//!
//! Per ADR-002 §"Conservative deletion": the orchestrator must classify
//! shared-vs-unique conservatively. When dedup-key uncertainty exists
//! (Tentative key only, hashes not yet computed), classify as **unique** —
//! preserves data, requires the stronger typed confirmation.
//!
//! `Esc` cancels at any point. The dialog never opens with a destructive
//! path on a model with no resolvable target (defense in depth — the
//! orchestrator also gates this at the call site).
//!
//! ADR cites:
//! - ADR-009 — separate `Tool::delete_one` method (NOT a special case of
//!   `delete_all`).
//! - ADR-002 — conservative-when-uncertain classification.

use modeltap_core::ToolId;

/// Decision returned by `decide_on_enter` / `decide_on_esc` / `decide_on_y` /
/// `decide_on_n`. The `update()` function maps `Confirm` to a
/// `UpdateEffect::trigger_delete_one` and `Cancel` to closing the dialog.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DeleteOneDecision {
    /// User confirmed (typed id matches, OR pressed `y` in shared mode).
    Confirm,
    /// User cancelled (Esc, OR wrong typed id, OR pressed `n` in shared mode).
    Cancel,
    /// No-op (e.g., wrong key in shared mode that's neither y nor n) — keep
    /// dialog open. Not currently produced (any non-y/n key in shared mode
    /// is a no-op handled by `update()` directly), but reserved for
    /// future-state extension.
    Pending,
}

/// Which confirmation mode the dialog opened in. Driven by the `was_shared`
/// classification computed by the orchestrator.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DeleteOneMode {
    /// `was_shared = true`. Single-key `[y/n]` confirmation.
    Shared,
    /// `was_shared = false`. Typed-id confirmation (BYTE-EQUAL, CASE-SENSITIVE).
    Unique,
}

/// Pure state for the single-model delete confirmation dialog. All mutation
/// is through `handle_char` / `handle_backspace`; all decisions are pure
/// functions over the current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteOneConfirmState {
    pub tool: ToolId,
    pub model_id: String,
    pub size_bytes: u64,
    pub mode: DeleteOneMode,
    typed_input: String,
}

impl DeleteOneConfirmState {
    /// Construct a dialog snapshot for a single model. `was_shared` is the
    /// orchestrator's classification (per ADR-002 conservative rule).
    pub fn for_model(
        tool: ToolId,
        model_id: impl Into<String>,
        size_bytes: u64,
        was_shared: bool,
    ) -> Self {
        let mode = if was_shared {
            DeleteOneMode::Shared
        } else {
            DeleteOneMode::Unique
        };
        Self {
            tool,
            model_id: model_id.into(),
            size_bytes,
            mode,
            typed_input: String::new(),
        }
    }

    /// True iff the dialog opened in Shared mode (single-key [y/n]).
    pub fn is_shared(&self) -> bool {
        matches!(self.mode, DeleteOneMode::Shared)
    }

    /// Read-only access to the accumulated typed input (Unique mode only).
    pub fn typed_input(&self) -> &str {
        &self.typed_input
    }

    /// Append one printable character to the typed-input buffer. Only
    /// meaningful in Unique mode; the render layer translates the buffer
    /// into the visible prompt (`<input>_`).
    pub fn handle_char(&mut self, c: char) {
        self.typed_input.push(c);
    }

    /// Remove the last character of the typed-input buffer. No-op when empty.
    pub fn handle_backspace(&mut self) {
        self.typed_input.pop();
    }

    /// Decide what Enter does. In Unique mode, BYTE-EQUAL CASE-SENSITIVE
    /// match against `model_id`. In Shared mode, Enter is a no-op (Pending)
    /// because the user is expected to press y or n explicitly.
    pub fn decide_on_enter(&self) -> DeleteOneDecision {
        match self.mode {
            DeleteOneMode::Unique => {
                if self.typed_input == self.model_id {
                    DeleteOneDecision::Confirm
                } else {
                    DeleteOneDecision::Cancel
                }
            }
            DeleteOneMode::Shared => DeleteOneDecision::Pending,
        }
    }

    /// Esc always cancels regardless of mode.
    pub fn decide_on_esc(&self) -> DeleteOneDecision {
        DeleteOneDecision::Cancel
    }

    /// Decide on `y` keypress (Shared mode → Confirm; Unique mode → no-op
    /// because `y` is a typed-input character there).
    pub fn decide_on_y(&self) -> DeleteOneDecision {
        match self.mode {
            DeleteOneMode::Shared => DeleteOneDecision::Confirm,
            DeleteOneMode::Unique => DeleteOneDecision::Pending,
        }
    }

    /// Decide on `n` keypress (Shared mode → Cancel; Unique mode → no-op).
    pub fn decide_on_n(&self) -> DeleteOneDecision {
        match self.mode {
            DeleteOneMode::Shared => DeleteOneDecision::Cancel,
            DeleteOneMode::Unique => DeleteOneDecision::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_dialog_uses_yn_mode() {
        let d =
            DeleteOneConfirmState::for_model(ToolId("ollama"), "llama3:8b", 4_400_000_000, true);
        assert_eq!(d.mode, DeleteOneMode::Shared);
        assert!(d.is_shared());
    }

    #[test]
    fn unique_dialog_uses_typed_mode() {
        let d =
            DeleteOneConfirmState::for_model(ToolId("ollama"), "llama3:8b", 4_400_000_000, false);
        assert_eq!(d.mode, DeleteOneMode::Unique);
        assert!(!d.is_shared());
    }

    #[test]
    fn shared_y_confirms_n_cancels() {
        let d =
            DeleteOneConfirmState::for_model(ToolId("ollama"), "llama3:8b", 4_400_000_000, true);
        assert_eq!(d.decide_on_y(), DeleteOneDecision::Confirm);
        assert_eq!(d.decide_on_n(), DeleteOneDecision::Cancel);
    }

    #[test]
    fn unique_typed_id_match_confirms() {
        let mut d =
            DeleteOneConfirmState::for_model(ToolId("ollama"), "llama3:8b", 4_400_000_000, false);
        for c in "llama3:8b".chars() {
            d.handle_char(c);
        }
        assert_eq!(d.decide_on_enter(), DeleteOneDecision::Confirm);
    }

    #[test]
    fn unique_typed_id_mismatch_cancels() {
        let mut d =
            DeleteOneConfirmState::for_model(ToolId("ollama"), "llama3:8b", 4_400_000_000, false);
        for c in "LLAMA3:8B".chars() {
            d.handle_char(c);
        }
        // Wrong case — must cancel, BYTE-EQUAL CASE-SENSITIVE per US-05.
        assert_eq!(d.decide_on_enter(), DeleteOneDecision::Cancel);
    }

    #[test]
    fn esc_always_cancels() {
        let shared = DeleteOneConfirmState::for_model(ToolId("ollama"), "x", 0, true);
        let unique = DeleteOneConfirmState::for_model(ToolId("ollama"), "x", 0, false);
        assert_eq!(shared.decide_on_esc(), DeleteOneDecision::Cancel);
        assert_eq!(unique.decide_on_esc(), DeleteOneDecision::Cancel);
    }

    #[test]
    fn shared_enter_is_pending_no_op() {
        let d = DeleteOneConfirmState::for_model(ToolId("ollama"), "x", 0, true);
        // In shared mode, Enter is a no-op: user must press y or n.
        assert_eq!(d.decide_on_enter(), DeleteOneDecision::Pending);
    }

    #[test]
    fn unique_y_and_n_are_typed_input_not_decisions() {
        // In unique mode `y` and `n` are buffered as typed input; the
        // dialog state machine returns Pending so update.rs treats them
        // as DialogTextInput. The render layer + decide_on_enter handle
        // the comparison.
        let d = DeleteOneConfirmState::for_model(ToolId("ollama"), "llama3:8b", 0, false);
        assert_eq!(d.decide_on_y(), DeleteOneDecision::Pending);
        assert_eq!(d.decide_on_n(), DeleteOneDecision::Pending);
    }

    #[test]
    fn backspace_removes_last_char_no_panic_when_empty() {
        let mut d = DeleteOneConfirmState::for_model(ToolId("ollama"), "x", 0, false);
        d.handle_backspace();
        assert_eq!(d.typed_input(), "");
        d.handle_char('a');
        d.handle_char('b');
        d.handle_backspace();
        assert_eq!(d.typed_input(), "a");
    }
}
