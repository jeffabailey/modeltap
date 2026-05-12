//! `Tool::delete_one` implementation for the LM Studio plugin (US-05b, step 03-06).
//!
//! LM Studio stores models as path-style `<root>/<org>/<repo>/<file>.gguf`
//! files. There is no manifest, no ref-counted blob — `delete_one` is a
//! single `fs::remove_file` against the model's `on_disk_path` with
//! `bytes_freed` taken from a stat at the moment of unlink.
//!
//! Idempotency / safety:
//! - If the file is already gone, return `NotFound` so the orchestrator can
//!   surface a coherent "nothing to do" outcome rather than crash.
//! - If the size cannot be stat'd before unlink (race), fall back to 0
//!   bytes_freed and proceed.

use std::path::Path;

use modeltap_core::{DeleteError, DeleteOutcome, ToolId};

use crate::TOOL_NAME;

/// Delete one LM Studio model file at `target_path`. Mirror of llama-cli's
/// `delete_one_at` — kept inline rather than shared so each plugin owns its
/// own deletion semantics.
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
            failure_reason: None,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(DeleteError::NotFound(model_id_in_tool.to_string()))
        }
        Err(e) => {
            tracing::warn!(
                target: "modeltap.lm-studio.delete",
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
        let path = temp.path().join("repo/model.gguf");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"0123456789").unwrap();
        let outcome = delete_one_at(&path, "repo/model.gguf").expect("ok");
        assert!(!path.exists());
        assert!(outcome.file_deleted);
        assert_eq!(outcome.bytes_freed, 10);
    }

    #[test]
    fn delete_one_returns_not_found_when_missing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nope.gguf");
        let err = delete_one_at(&path, "nope").expect_err("must err");
        assert!(matches!(err, DeleteError::NotFound(_)));
    }
}
