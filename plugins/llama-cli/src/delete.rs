//! `Tool::delete_one` implementation for the llama-cli plugin (US-05b, step 03-06).
//!
//! llama-cli stores models as loose `.gguf` files. There is no manifest, no
//! ref-counted blob — `delete_one` is a single `fs::remove_file` against the
//! model's `on_disk_path`, with `bytes_freed` taken from the file's size at
//! the moment of unlink (so cross-tool sharing stats stay honest).
//!
//! Idempotency / safety:
//! - If the file is already gone, return `NotFound` so the orchestrator can
//!   surface a coherent "nothing to do" outcome rather than crash.
//! - If the size cannot be stat'd before unlink (race), fall back to 0
//!   bytes_freed and proceed with the unlink. The action's JSONL event
//!   reflects the conservative count.

use std::path::Path;

use modeltap_core::{DeleteError, DeleteOutcome, ToolId};

use crate::TOOL_NAME;

/// Delete one llama-cli model file at `target_path`.
///
/// `model_id_in_tool` is the value stored in the discovered model's
/// `id_in_tool`; surfaced through the returned `DeleteOutcome` for
/// orchestrator-level correlation.
pub fn delete_one_at(
    target_path: &Path,
    model_id_in_tool: &str,
) -> Result<DeleteOutcome, DeleteError> {
    let tool: ToolId = TOOL_NAME;
    let size_bytes = std::fs::metadata(target_path).map(|m| m.len()).ok();
    match std::fs::remove_file(target_path) {
        Ok(()) => Ok(DeleteOutcome {
            tool,
            model_id_in_tool: model_id_in_tool.to_string(),
            bytes_freed: size_bytes.unwrap_or(0),
            registration_removed: true,
            file_deleted: true,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(DeleteError::NotFound(model_id_in_tool.to_string()))
        }
        Err(e) => {
            tracing::warn!(
                target: "modeltap.llama-cli.delete",
                "delete_one: remove {}: {e}",
                target_path.display()
            );
            Err(DeleteError::Io(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn delete_one_unlinks_file_and_reports_size() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("model.gguf");
        fs::write(&path, b"abcdefgh").unwrap();
        let outcome = delete_one_at(&path, "model.gguf").expect("ok");
        assert!(!path.exists(), "file must be unlinked");
        assert!(outcome.registration_removed);
        assert!(outcome.file_deleted);
        assert_eq!(outcome.bytes_freed, 8);
    }

    #[test]
    fn delete_one_returns_not_found_when_path_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nope.gguf");
        let err = delete_one_at(&path, "nope.gguf").expect_err("must err");
        assert!(matches!(err, DeleteError::NotFound(_)));
    }
}
