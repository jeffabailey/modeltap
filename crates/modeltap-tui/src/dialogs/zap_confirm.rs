//! `ZapConfirmState` — the typed-name confirmation dialog for US-05.
//!
//! Pure state machine. The dialog opens with a snapshot of the tool's metrics
//! (model count, total bytes, unique-vs-shared breakdown per the WS-slice
//! classifier in `modeltap_core::logic::dedup`), accumulates typed input as
//! the user spells the tool name, and yields a `ZapDecision` on Enter or Esc.
//!
//! Per US-05 AC-2 the typed match is BYTE-EQUAL and CASE-SENSITIVE: anything
//! other than the exact tool name cancels. Per AC-5, a tool with zero models
//! opens the dialog in "empty" mode — there is no destructive path; only Esc
//! closes it. The render layer reads `is_empty_tool()` to decide whether to
//! show the typed-input prompt or the benign "Nothing to zap." message.
//!
//! ADR cites:
//! - ADR-002 §"Conservative deletion" — `unique_count` / `shared_count` come
//!   from the path-equality classifier; `unique_bytes` is what `delete_all`
//!   will actually free.
//! - ADR-009 §"Tool::delete_all" — confirm path emits a single
//!   `ZapDecision::Confirm`; the orchestrator (in `modeltap-app`) then calls
//!   `Tool::delete_all`, NOT a loop of `delete_one`.

use modeltap_core::ToolId;

/// Decision returned by `decide_on_enter` / `decide_on_esc`. The `update()`
/// function maps `Confirm` to a `UpdateEffect::trigger_zap` and `Cancel` to
/// closing the dialog with no destructive side-effect.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ZapDecision {
    /// Typed input matches tool name exactly → proceed with delete_all.
    Confirm,
    /// Typed input does not match (or Esc pressed) → close dialog, no-op.
    Cancel,
}

/// Pure state for the zap confirmation dialog. All mutation is through
/// `handle_char` / `handle_backspace`; all decisions are pure functions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZapConfirmState {
    pub tool: ToolId,
    pub model_count: usize,
    pub total_bytes: u64,
    pub unique_count: u64,
    pub shared_count: u64,
    pub unique_bytes: u64,
    pub shared_bytes: u64,
    typed_input: String,
}

impl ZapConfirmState {
    /// Construct a dialog snapshot for `tool_id`. Caller computes the
    /// classifier numbers via `modeltap_core::logic::dedup::classify_unique_vs_shared`
    /// (or passes 0/0 when no inventory is available — empty-tool path).
    ///
    /// `total_bytes` is the apparent on-disk total for that tool (the value
    /// the left pane shows). `unique_bytes` is the subset that `delete_all`
    /// will actually free; `shared_bytes` is what stays on disk because
    /// another tool still references those paths.
    pub fn for_tool(
        tool_id: ToolId,
        model_count: usize,
        total_bytes: u64,
        unique_bytes: u64,
        shared_bytes: u64,
    ) -> Self {
        // Conservative defaults for the WS slice when the caller has not
        // computed a per-model classification: every model is unique.
        let unique_count = model_count as u64;
        let shared_count = 0u64;
        Self {
            tool: tool_id,
            model_count,
            total_bytes,
            unique_count,
            shared_count,
            unique_bytes,
            shared_bytes,
            typed_input: String::new(),
        }
    }

    /// Construct a dialog with explicit unique/shared counts (used by the
    /// orchestrator once the classifier has produced a `UniqueVsSharedReport`).
    pub fn with_breakdown(
        tool_id: ToolId,
        model_count: usize,
        total_bytes: u64,
        unique_count: u64,
        shared_count: u64,
        unique_bytes: u64,
        shared_bytes: u64,
    ) -> Self {
        Self {
            tool: tool_id,
            model_count,
            total_bytes,
            unique_count,
            shared_count,
            unique_bytes,
            shared_bytes,
            typed_input: String::new(),
        }
    }

    /// True iff the dialog represents the "no models to delete" benign path
    /// (US-05 AC-5). Render layer shows "Nothing to zap." and accepts only Esc.
    pub fn is_empty_tool(&self) -> bool {
        self.model_count == 0
    }

    /// Read-only access to the accumulated typed input.
    pub fn typed_input(&self) -> &str {
        &self.typed_input
    }

    /// Append one printable character to the typed-input buffer. The render
    /// layer translates the buffer into the visible prompt (`<input>_`).
    pub fn handle_char(&mut self, c: char) {
        self.typed_input.push(c);
    }

    /// Remove the last character of the typed-input buffer. No-op when empty
    /// (cannot panic).
    pub fn handle_backspace(&mut self) {
        self.typed_input.pop();
    }

    /// Decide what Enter does given the current typed input. Per US-05 AC-2,
    /// the comparison is BYTE-EQUAL and CASE-SENSITIVE.
    pub fn decide_on_enter(&self) -> ZapDecision {
        if self.is_empty_tool() {
            return ZapDecision::Cancel;
        }
        if self.typed_input == self.tool.0 {
            ZapDecision::Confirm
        } else {
            ZapDecision::Cancel
        }
    }

    /// Esc always cancels per AC-3, regardless of typed input.
    pub fn decide_on_esc(&self) -> ZapDecision {
        ZapDecision::Cancel
    }
}
