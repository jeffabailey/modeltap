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

use modeltap_core::types::FolderGroup;

/// Decision returned by `decide_on_enter` / `decide_on_esc`. The `update()`
/// function maps `Confirm` to a `UpdateEffect::trigger_folder_delete` (lands
/// in 03-01) and `Cancel` to closing the dialog with no destructive side-
/// effect.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FolderConfirmDecision {
    /// Typed input matches `folder.path` exactly → proceed with
    /// `delete_folder`.
    Confirm,
    /// Typed input does not match (or Esc pressed) → close dialog, no-op.
    Cancel,
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
    typed_input: String,
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
            typed_input: String::new(),
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

    /// Append one printable character to the typed-input buffer.
    pub fn handle_char(&mut self, c: char) {
        self.typed_input.push(c);
    }

    /// Remove the last character of the typed-input buffer. No-op on empty.
    pub fn handle_backspace(&mut self) {
        self.typed_input.pop();
    }

    /// Decide on Enter. BYTE-EQUAL, CASE-SENSITIVE match against
    /// `folder.path` — INT-FGD-7's single-source-of-truth invariant.
    pub fn decide_on_enter(&self) -> FolderConfirmDecision {
        if self.typed_input == self.folder.path {
            FolderConfirmDecision::Confirm
        } else {
            FolderConfirmDecision::Cancel
        }
    }

    /// Esc always cancels.
    pub fn decide_on_esc(&self) -> FolderConfirmDecision {
        FolderConfirmDecision::Cancel
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
    fn typed_path_case_mismatch_cancels() {
        let folder = fixture_folder();
        let mut d = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_000_000_000, 0);
        for c in "ALICE/FOO".chars() {
            d.handle_char(c);
        }
        // Wrong case — must cancel per INT-FGD-7 byte-equal contract.
        assert_eq!(d.decide_on_enter(), FolderConfirmDecision::Cancel);
    }

    #[test]
    fn esc_always_cancels() {
        let folder = fixture_folder();
        let d = FolderConfirmState::for_folder(folder, 1, 0, 0, 1_000_000_000, 0);
        assert_eq!(d.decide_on_esc(), FolderConfirmDecision::Cancel);
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
}
