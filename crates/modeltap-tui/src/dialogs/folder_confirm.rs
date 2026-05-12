//! `FolderConfirmState` — the folder-delete typed-confirm dialog (US-05c).
//!
//! Pure state machine. The dialog opens with a snapshot of the target HF
//! folder's metrics (`FolderGroup` + classifier counts + reclaim/retain
//! bytes) and accumulates typed input as the user spells the canonical
//! `<author>/<repo>` path. On Enter, the typed buffer is compared
//! BYTE-EQUAL, CASE-SENSITIVE against `folder.path` — the single source of
//! truth (INT-FGD-7). Anything else cancels.
//!
//! Per US-05c.AC-2 / AC-6: the dialog body shows the path, the absolute
//! on-disk path, the unique/shared/sidecar counts, the reclaim bytes, and
//! the retain bytes. Per AC-8: the typed confirmation matches `folder.path`
//! exactly (no synonym, no lowercase fold).
//!
//! Step 01-04 covers the **all-unique happy-path body** only — mixed
//! shared/unique itemization lands at 03-01. The dialog state shape carries
//! the full breakdown so the 03-01 renderer can fill it in without a
//! schema change.
//!
//! ADR cites:
//! - ADR-002 §"Conservative deletion" — `unique_count` / `shared_count`
//!   come from the path-equality classifier; `bytes_to_reclaim` is what
//!   `delete_folder` will actually free.
//! - ADR-009 §"Tool::delete_folder" — confirm path emits a single
//!   `FolderConfirmDecision::Confirm`; the orchestrator (in
//!   `modeltap-app`) then calls `Tool::delete_folder`.

use modeltap_core::types::{FolderGroup, SharedModel};

/// Decision returned by `decide_on_enter` / `decide_on_esc`. The `update()`
/// function maps `Confirm` to a `UpdateEffect::trigger_folder_delete` (lands
/// in 03-01) and `Cancel` to closing the dialog with no destructive side-
/// effect.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FolderConfirmDecision {
    /// Typed input matches `folder.path` exactly → proceed with
    /// `delete_folder`.
    Confirm,
    /// Typed input does not match `folder.path` byte-exactly (including
    /// trailing whitespace, leading slash, wrong case, trailing slash, …).
    /// Closes the dialog with no destructive side-effect. The orchestrator
    /// emits an `action.folder_delete` event with `outcome=cancelled_mismatch`
    /// and `outcomes_count=0` (step 02-01).
    CancelMismatch,
    /// Esc was pressed at any point. Closes the dialog with no destructive
    /// side-effect. The orchestrator emits an `action.folder_delete` event
    /// with `outcome=cancelled_escape` and `outcomes_count=0` (step 02-01).
    CancelEscape,
}

/// Pure state for the folder-confirm dialog. All mutation is through
/// `handle_char` / `handle_backspace`; all decisions are pure functions
/// over the current state.
///
/// Note: `FolderGroup` does not implement `PartialEq` (its `ModelMeta`
/// children carry path/byte data that the core crate intentionally does
/// not give an equality contract). `FolderConfirmState` therefore does
/// not derive `PartialEq` either; tests assert on the public accessors
/// (`typed_input()`, `decide_on_enter()`, …) rather than on whole-state
/// equality.
#[derive(Debug, Clone)]
pub struct FolderConfirmState {
    /// Snapshot of the target folder. `path` is the byte-exact comparator
    /// for typed confirmation (INT-FGD-7).
    pub folder: FolderGroup,
    /// Number of unique-classified files in this folder.
    pub unique_count: usize,
    /// Number of shared-classified files in this folder.
    pub shared_count: usize,
    /// Number of sidecar files in this folder (README, .imatrix, .urls,
    /// HF-internal). Sidecars are always treated as unique.
    pub sidecar_count: usize,
    /// Reclaim promise — bytes that will be freed on successful execution.
    pub bytes_to_reclaim: u64,
    /// Retain promise — bytes whose HF registration is removed but whose
    /// inode is kept alive by another tool's hardlink.
    pub bytes_to_retain: u64,
    /// Optional running-tool warning slot (US-05c.AC-6 / US-17). `None` =
    /// no warning. Step 01-04 never sets a non-`None` value; running-tool
    /// detection lands at 03-04.
    pub running_tool_warning: Option<String>,
    /// Per-shared-file detail for mixed shared/unique itemisation (step
    /// 03-01). Empty in the all-unique case; populated by
    /// `for_folder_with_shared` from the classifier's `Vec<SharedModel>` so
    /// the mixed-mode renderer can name each shared file and list the
    /// other tools whose hardlinks keep its inode alive.
    pub shared_models: Vec<SharedModel>,
    typed_input: String,
    /// Step 06-01 (K-FGD-2 / D3): total keystroke events the dialog has
    /// observed since opening. Incremented by `handle_char`,
    /// `handle_backspace`, and `handle_word_delete`. Does NOT include the
    /// Shift+F that opened the dialog (that event never reached this
    /// state machine) nor the final Enter / Esc (those are decision events
    /// consumed by `decide_on_*`, surfaced by the orchestrator separately).
    /// Read by the orchestrator and copied verbatim into the JSONL
    /// `action.folder_delete.keystroke_count` field.
    keystroke_count: u64,
}

impl FolderConfirmState {
    /// Construct a dialog snapshot for a folder. Caller supplies the per-
    /// file classification (unique/shared/sidecar) and the reclaim/retain
    /// promises; the pure-logic builder in
    /// `modeltap_core::logic::folder_group::build_folder_delete_plan` is
    /// the canonical producer.
    pub fn for_folder(
        folder: FolderGroup,
        unique_count: usize,
        shared_count: usize,
        sidecar_count: usize,
        bytes_to_reclaim: u64,
        bytes_to_retain: u64,
    ) -> Self {
        Self {
            folder,
            unique_count,
            shared_count,
            sidecar_count,
            bytes_to_reclaim,
            bytes_to_retain,
            running_tool_warning: None,
            shared_models: Vec::new(),
            typed_input: String::new(),
            keystroke_count: 0,
        }
    }

    /// Construct a dialog snapshot for a mixed shared/unique folder (step
    /// 03-01). `shared_count` is derived from `shared.len()` — the slice
    /// carries the per-shared-file detail (which other tools hold the
    /// inode) that the mixed-mode renderer surfaces as
    /// "<filename> — also linked in <tool>". The reclaim/retain promises
    /// come from the same `build_folder_delete_plan` producer as the
    /// all-unique path.
    pub fn for_folder_with_shared(
        folder: FolderGroup,
        unique_count: usize,
        sidecar_count: usize,
        bytes_to_reclaim: u64,
        bytes_to_retain: u64,
        shared: Vec<SharedModel>,
    ) -> Self {
        let shared_count = shared.len();
        Self {
            folder,
            unique_count,
            shared_count,
            sidecar_count,
            bytes_to_reclaim,
            bytes_to_retain,
            running_tool_warning: None,
            shared_models: shared,
            typed_input: String::new(),
            keystroke_count: 0,
        }
    }

    /// Total file count = `unique + shared + sidecars`. Matches
    /// `folder.file_count()` by construction.
    pub fn file_count(&self) -> usize {
        self.unique_count + self.shared_count + self.sidecar_count
    }

    /// Read-only access to the accumulated typed input. Used by the render
    /// layer to echo `<input>_` in the dialog body.
    pub fn typed_input(&self) -> &str {
        &self.typed_input
    }

    /// Append one printable character to the typed-input buffer. Increments
    /// the K-FGD-2 keystroke counter (every observed input event counts —
    /// see the rationale on the field's docstring).
    pub fn handle_char(&mut self, c: char) {
        self.typed_input.push(c);
        self.keystroke_count = self.keystroke_count.saturating_add(1);
    }

    /// Remove the last character of the typed-input buffer. No-op on empty.
    /// ALWAYS increments the keystroke counter — even if the buffer was
    /// already empty, the user pressed the key and the dialog observed the
    /// event (D3: "Backspace counts toward total"). The K-FGD-2 bound is a
    /// user-facing keystroke budget, not a typed-buffer-length tally.
    pub fn handle_backspace(&mut self) {
        self.typed_input.pop();
        self.keystroke_count = self.keystroke_count.saturating_add(1);
    }

    /// Step 06-01 (D3): word-delete (Ctrl+W). Removes characters from the
    /// end of the typed buffer back to (and including) the preceding word
    /// boundary, AND counts as exactly ONE keystroke regardless of how
    /// many characters were removed — the user pressed one key.
    pub fn handle_word_delete(&mut self) {
        // Strip trailing whitespace first, then the preceding non-whitespace
        // run. This matches the conventional terminal Ctrl+W behaviour. The
        // typed-confirm comparator is byte-exact, so a deletion that
        // overshoots the boundary is a user-visible correction, not a
        // semantic edit.
        while self.typed_input.ends_with(char::is_whitespace) {
            self.typed_input.pop();
        }
        while let Some(c) = self.typed_input.chars().last() {
            if c.is_whitespace() {
                break;
            }
            self.typed_input.pop();
        }
        self.keystroke_count = self.keystroke_count.saturating_add(1);
    }

    /// Read-only access to the K-FGD-2 keystroke counter. The orchestrator
    /// reads this value at Enter/Esc time and emits it into the JSONL
    /// `action.folder_delete.keystroke_count` field.
    pub fn keystroke_count(&self) -> u64 {
        self.keystroke_count
    }

    /// Decide on Enter. BYTE-EQUAL, CASE-SENSITIVE match against
    /// `folder.path` — INT-FGD-7's single-source-of-truth invariant. Any
    /// byte-difference (trailing slash, leading whitespace, wrong case, …)
    /// returns `CancelMismatch` — the orchestrator distinguishes this from
    /// `CancelEscape` to emit the correct JSONL outcome (step 02-01).
    pub fn decide_on_enter(&self) -> FolderConfirmDecision {
        if self.typed_input == self.folder.path {
            FolderConfirmDecision::Confirm
        } else {
            FolderConfirmDecision::CancelMismatch
        }
    }

    /// Esc always cancels with `CancelEscape` — distinct from a mismatched
    /// Enter so the JSONL outcome reflects the user's intent (step 02-01).
    pub fn decide_on_esc(&self) -> FolderConfirmDecision {
        FolderConfirmDecision::CancelEscape
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_core::types::{DedupKey, DisplayLabel, Format, ModelMeta, ModelStatus};
    use modeltap_core::ToolId;
    use std::path::PathBuf;

    fn fixture_folder() -> FolderGroup {
        let model = ModelMeta {
            tool: ToolId("hf"),
            id_in_tool: "alice/foo/model.gguf".to_string(),
            on_disk_path: PathBuf::from("/cache/alice/foo/model.gguf"),
            size_bytes: 1_000_000_000,
            format: Format::Gguf,
            dedup_key: DedupKey::Tentative(DisplayLabel::from("alice/foo/model.gguf@1000000000")),
            display_label: DisplayLabel::from("alice/foo/model.gguf"),
            status: ModelStatus::Healthy,
        };
        FolderGroup::new(
            "alice/foo".to_string(),
            PathBuf::from("/cache/hub/models--alice--foo"),
            ToolId("hf"),
            vec![model],
            vec![],
        )
        .expect("fixture_folder constructs")
    }

    #[test]
    fn typed_path_exact_match_confirms() {
        let folder = fixture_folder();
        let mut d = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_000_000_000, 0);
        for c in "alice/foo".chars() {
            d.handle_char(c);
        }
        assert_eq!(d.decide_on_enter(), FolderConfirmDecision::Confirm);
    }

    #[test]
    fn typed_path_case_mismatch_cancels_with_mismatch_variant() {
        let folder = fixture_folder();
        let mut d = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_000_000_000, 0);
        for c in "ALICE/FOO".chars() {
            d.handle_char(c);
        }
        // Wrong case — must cancel per INT-FGD-7 byte-equal contract.
        assert_eq!(d.decide_on_enter(), FolderConfirmDecision::CancelMismatch);
    }

    /// Step 02-01: byte-exact comparator rejects a trailing slash. The HF
    /// canonical `<author>/<repo>` form has no trailing slash; typing one
    /// must cancel rather than confirm (D2 decision: no normalization).
    #[test]
    fn typed_path_with_trailing_slash_cancels_with_mismatch_variant() {
        let folder = fixture_folder();
        let mut d = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_000_000_000, 0);
        for c in "alice/foo/".chars() {
            d.handle_char(c);
        }
        assert_eq!(d.decide_on_enter(), FolderConfirmDecision::CancelMismatch);
    }

    /// Step 02-01: Esc returns CancelEscape, distinct from CancelMismatch
    /// returned by `decide_on_enter` on a mismatched typed input. The
    /// orchestrator uses this distinction to emit the correct JSONL outcome
    /// (`cancelled_escape` vs `cancelled_mismatch`).
    #[test]
    fn esc_always_cancels_with_escape_variant() {
        let folder = fixture_folder();
        let d = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_000_000_000, 0);
        assert_eq!(d.decide_on_esc(), FolderConfirmDecision::CancelEscape);
    }

    /// Step 02-01: CancelMismatch and CancelEscape are distinct variants;
    /// the orchestrator's `cancelled_mismatch` vs `cancelled_escape` JSONL
    /// outcomes depend on this distinction. If a future refactor collapses
    /// them back to a single Cancel variant, this assertion fails and forces
    /// re-evaluation of the JSONL contract (kpi-instrumentation.md §
    /// "action.folder_delete").
    #[test]
    fn cancel_mismatch_and_cancel_escape_are_distinguishable() {
        assert_ne!(
            FolderConfirmDecision::CancelMismatch,
            FolderConfirmDecision::CancelEscape,
            "CancelMismatch and CancelEscape must be distinct so the orchestrator can route to the right JSONL outcome",
        );
    }

    #[test]
    fn backspace_removes_last_char_no_panic_on_empty() {
        let folder = fixture_folder();
        let mut d = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_000_000_000, 0);
        d.handle_backspace();
        assert_eq!(d.typed_input(), "");
        d.handle_char('a');
        d.handle_char('b');
        d.handle_backspace();
        assert_eq!(d.typed_input(), "a");
    }

    #[test]
    fn file_count_matches_breakdown_sum() {
        let folder = fixture_folder();
        let d = FolderConfirmState::for_folder(folder, 3, 2, 1, 4_000_000_000, 2_000_000_000);
        assert_eq!(d.file_count(), 6);
    }

    /// Step 06-01 (K-FGD-2 / D3): `keystroke_count` accumulates every input
    /// event the dialog handles — printable chars AND corrections (Backspace,
    /// Ctrl+W). It is the property the M6 acceptance scenario asserts is
    /// `<= 40` and `INDEPENDENT of file_count`. Shift+F is excluded because
    /// it transitions FROM main view TO dialog state (it never reaches the
    /// dialog's input handler).
    #[test]
    fn keystroke_count_accumulates_char_input_backspace_and_word_delete() {
        let folder = fixture_folder();
        let mut d = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_000_000_000, 0);
        // Initial state: dialog just opened, no keystrokes yet.
        assert_eq!(d.keystroke_count(), 0);

        // Type 5 chars — each char counts as 1 keystroke.
        for c in "alice".chars() {
            d.handle_char(c);
        }
        assert_eq!(d.keystroke_count(), 5);

        // Backspace counts toward the total per D3.
        d.handle_backspace();
        assert_eq!(d.keystroke_count(), 6);

        // Ctrl+W (word-delete) counts toward the total per D3. The dialog
        // exposes `handle_word_delete` so the keymap can route Ctrl+W to a
        // single, instrumented mutation rather than emitting N Backspaces.
        d.handle_word_delete();
        assert_eq!(d.keystroke_count(), 7);
    }

    /// Step 06-01: a Backspace on an empty buffer is still a keystroke
    /// (the user pressed the key; the dialog observed it). This is what
    /// makes K-FGD-2's "<= 40" a USER-FACING bound rather than a
    /// production-code state-shape artifact.
    #[test]
    fn keystroke_count_counts_backspace_even_on_empty_buffer() {
        let folder = fixture_folder();
        let mut d = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_000_000_000, 0);
        d.handle_backspace();
        d.handle_backspace();
        d.handle_backspace();
        assert_eq!(
            d.keystroke_count(),
            3,
            "every Backspace counts, even when typed_input is already empty",
        );
        assert_eq!(d.typed_input(), "");
    }

    /// Step 03-01: `for_folder_with_shared` derives `shared_count` from the
    /// `Vec<SharedModel>` slice — the caller passes the shared list directly
    /// (the dialog renders per-shared-file detail) instead of just the count.
    #[test]
    fn for_folder_with_shared_derives_shared_count_from_slice() {
        use modeltap_core::types::SharedModel;
        let folder = fixture_folder();
        let shared_model = ModelMeta {
            tool: ToolId("hf"),
            id_in_tool: "alice/foo/shared.gguf".to_string(),
            on_disk_path: PathBuf::from("/cache/alice/foo/shared.gguf"),
            size_bytes: 500_000_000,
            format: Format::Gguf,
            dedup_key: DedupKey::Tentative(DisplayLabel::from("alice/foo/shared.gguf@500m")),
            display_label: DisplayLabel::from("alice/foo/shared.gguf"),
            status: ModelStatus::Healthy,
        };
        let shared = vec![SharedModel {
            model: shared_model,
            other_tools: vec![ToolId("ollama")],
        }];
        let d = FolderConfirmState::for_folder_with_shared(
            folder,
            1,
            0,
            1_000_000_000,
            500_000_000,
            shared,
        );
        assert_eq!(d.shared_count, 1, "shared_count derived from slice length");
        assert_eq!(d.unique_count, 1);
        assert_eq!(d.bytes_to_retain, 500_000_000);
        assert_eq!(d.shared_models.len(), 1);
        assert_eq!(d.shared_models[0].other_tools, vec![ToolId("ollama")]);
    }
}
