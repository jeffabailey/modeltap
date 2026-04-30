//! `actions::delete_one::run` — orchestrates a confirmed single-model delete
//! action (US-05b, step 03-06; ADR-009).
//!
//! Called by the headless / production event loop when the dialog returns
//! `UpdateEffect::trigger_delete_one = Some(DeleteOneTrigger { .. })`. Per
//! ADR-009, this calls `Tool::delete_one` ONCE — NOT `Tool::delete_all` and
//! NOT a loop of `delete_one`. The plugin's `delete_one` performs its own
//! transactional registration+blob cleanup (snapshot-symlink unlink for HF,
//! manifest+ref-counted-blob for Ollama, simple unlink for llama-cli /
//! lm-studio).
//!
//! Per `kpi-instrumentation.md` §"action.zap_one", on completion we emit
//! exactly one JSONL event with: tool name, bytes_reclaimed (u64), was_shared
//! (bool — distinguishes the low-friction y/n path from the typed-id path),
//! outcome string. NO model names, NO paths, NO usernames — the schema is
//! privacy-preserving by design.

use std::path::PathBuf;

use modeltap_core::{DeleteError, DeleteOutcome, ModelMeta, Tool, ToolId};

use crate::observability::{LaunchLogger, RecordKind};

/// Result of a confirmed single-model delete action. Surfaced to the right
/// pane as the "Last action" footer and used by acceptance tests to assert
/// success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteOneOutcome {
    pub tool: ToolId,
    pub model_id: String,
    pub bytes_reclaimed: u64,
    pub was_shared: bool,
    pub outcome: DeleteOneResult,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DeleteOneResult {
    /// The model's registration was removed AND its on-disk file was
    /// unlinked (or, when shared, the registration was removed and the blob
    /// kept on purpose — still a success).
    Success,
    /// Plugin returned NotFound — the model wasn't there. Surface as
    /// "failed" so the user sees something rather than silently succeeding.
    NotFound,
    /// Plugin returned an error before completing the delete.
    Failed,
}

impl DeleteOneResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeleteOneResult::Success => "success",
            DeleteOneResult::NotFound => "not_found",
            DeleteOneResult::Failed => "failed",
        }
    }
}

/// Run a confirmed single-model delete action. Calls `plugin.delete_one`,
/// classifies the outcome, emits one `action.zap_one` JSONL event, and
/// returns a `DeleteOneOutcome` for the UI footer.
///
/// Arguments:
///   - `plugin`: the plugin instance for `tool_id` (caller resolves).
///   - `tool_id`: the tool whose model is being deleted (echoed to JSONL).
///   - `model_id`: the model's `id_in_tool` (used to build `ModelMeta`).
///   - `on_disk_path`: the model's on-disk path (used by HF / llama-cli /
///     lm-studio's `delete_one` to locate the file).
///   - `size_bytes`: the model's apparent size (used by ModelMeta + as a
///     conservative reclaim estimate when the plugin doesn't return one).
///   - `was_shared`: orchestrator-computed share classification (per
///     ADR-002 conservative-when-uncertain). Recorded in the JSONL event;
///     does NOT affect the destructive path.
///   - `logger`: launch.log JSONL sink.
pub async fn run(
    plugin: &dyn Tool,
    tool_id: ToolId,
    model_id: String,
    on_disk_path: PathBuf,
    size_bytes: u64,
    was_shared: bool,
    logger: &mut LaunchLogger,
) -> DeleteOneOutcome {
    let model = synthesize_model_meta(tool_id, model_id.clone(), on_disk_path, size_bytes);
    let outcome = match plugin.delete_one(&model).await {
        Ok(DeleteOutcome {
            bytes_freed,
            registration_removed,
            ..
        }) => {
            let result = if registration_removed {
                DeleteOneResult::Success
            } else {
                DeleteOneResult::Failed
            };
            DeleteOneOutcome {
                tool: tool_id,
                model_id,
                bytes_reclaimed: bytes_freed,
                was_shared,
                outcome: result,
            }
        }
        Err(DeleteError::NotFound(_)) => DeleteOneOutcome {
            tool: tool_id,
            model_id,
            bytes_reclaimed: 0,
            was_shared,
            outcome: DeleteOneResult::NotFound,
        },
        Err(e) => {
            tracing::warn!(
                target: "modeltap.action.delete_one",
                "delete_one failed for {}: {e}",
                tool_id.0
            );
            DeleteOneOutcome {
                tool: tool_id,
                model_id,
                bytes_reclaimed: 0,
                was_shared,
                outcome: DeleteOneResult::Failed,
            }
        }
    };
    emit(logger, &outcome);
    outcome
}

/// Build a synthetic `ModelMeta` for the plugin's `delete_one()` call. Only
/// `tool`, `id_in_tool`, `on_disk_path`, and `size_bytes` are load-bearing
/// for delete; the rest is filled with conservative defaults via the shared
/// `super::synthetic_model_meta` helper.
fn synthesize_model_meta(
    tool: ToolId,
    id_in_tool: String,
    on_disk_path: PathBuf,
    size_bytes: u64,
) -> ModelMeta {
    super::synthetic_model_meta(tool, id_in_tool, on_disk_path, size_bytes)
}

fn emit(logger: &mut LaunchLogger, outcome: &DeleteOneOutcome) {
    logger.record(RecordKind::ActionZapOne {
        tool: outcome.tool.to_string(),
        bytes_reclaimed: outcome.bytes_reclaimed,
        was_shared: outcome.was_shared,
        outcome: outcome.outcome.as_str(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use modeltap_core::{DiscoverError, DiscoveredModel, Format, LinkError, LinkOutcome};
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Mock plugin that counts `delete_one` vs `delete_all` invocations. Used
    /// to verify ADR-009: the orchestrator MUST invoke `delete_one`, NOT
    /// `delete_all`.
    struct CountingPlugin {
        delete_one_calls: Arc<AtomicUsize>,
        delete_all_calls: Arc<AtomicUsize>,
        bytes_freed: u64,
        registration_removed: bool,
    }

    #[async_trait]
    impl Tool for CountingPlugin {
        fn name(&self) -> ToolId {
            ToolId("ollama")
        }
        fn accepted_formats(&self) -> &'static [Format] {
            &[Format::OllamaBlob]
        }
        async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
            Ok(Vec::new())
        }
        async fn link(
            &self,
            _canonical: &Path,
            _model: &ModelMeta,
        ) -> Result<LinkOutcome, LinkError> {
            Err(LinkError::NotYetImplemented("test".to_string()))
        }
        async fn delete_one(&self, model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
            self.delete_one_calls.fetch_add(1, Ordering::SeqCst);
            Ok(DeleteOutcome {
                tool: self.name(),
                model_id_in_tool: model.id_in_tool.clone(),
                bytes_freed: self.bytes_freed,
                registration_removed: self.registration_removed,
                file_deleted: self.registration_removed,
            })
        }
        async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
            self.delete_all_calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    fn null_logger() -> LaunchLogger {
        LaunchLogger::open(None)
    }

    #[tokio::test]
    async fn run_invokes_delete_one_not_delete_all_per_adr_009() {
        // ADR-009 invariant: actions::delete_one::run MUST call
        // Tool::delete_one, never Tool::delete_all. Verified by counter.
        let one = Arc::new(AtomicUsize::new(0));
        let all = Arc::new(AtomicUsize::new(0));
        let plugin = CountingPlugin {
            delete_one_calls: one.clone(),
            delete_all_calls: all.clone(),
            bytes_freed: 4_400_000_000,
            registration_removed: true,
        };
        let mut logger = null_logger();
        let outcome = run(
            &plugin,
            ToolId("ollama"),
            "llama3:8b".to_string(),
            PathBuf::from("/blobs/sha256-aaaa"),
            4_400_000_000,
            false,
            &mut logger,
        )
        .await;
        assert_eq!(
            one.load(Ordering::SeqCst),
            1,
            "Tool::delete_one MUST be invoked exactly once per ADR-009"
        );
        assert_eq!(
            all.load(Ordering::SeqCst),
            0,
            "Tool::delete_all MUST NOT be invoked from the single-model path (ADR-009)"
        );
        assert_eq!(outcome.outcome, DeleteOneResult::Success);
        assert_eq!(outcome.bytes_reclaimed, 4_400_000_000);
        assert!(!outcome.was_shared);
    }

    #[tokio::test]
    async fn run_propagates_was_shared_into_outcome() {
        let one = Arc::new(AtomicUsize::new(0));
        let all = Arc::new(AtomicUsize::new(0));
        let plugin = CountingPlugin {
            delete_one_calls: one,
            delete_all_calls: all,
            bytes_freed: 100,
            registration_removed: true,
        };
        let mut logger = null_logger();
        let outcome = run(
            &plugin,
            ToolId("ollama"),
            "x:y".to_string(),
            PathBuf::from("/p"),
            100,
            true, // was_shared
            &mut logger,
        )
        .await;
        assert!(
            outcome.was_shared,
            "was_shared must round-trip through run()"
        );
    }

    #[tokio::test]
    async fn run_classifies_not_found_when_plugin_returns_not_found() {
        struct NotFoundPlugin;
        #[async_trait]
        impl Tool for NotFoundPlugin {
            fn name(&self) -> ToolId {
                ToolId("ollama")
            }
            fn accepted_formats(&self) -> &'static [Format] {
                &[Format::OllamaBlob]
            }
            async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
                Ok(Vec::new())
            }
            async fn link(&self, _c: &Path, _m: &ModelMeta) -> Result<LinkOutcome, LinkError> {
                Err(LinkError::NotYetImplemented("x".to_string()))
            }
            async fn delete_one(&self, model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
                Err(DeleteError::NotFound(model.id_in_tool.clone()))
            }
            async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
                Ok(Vec::new())
            }
        }
        let mut logger = null_logger();
        let outcome = run(
            &NotFoundPlugin,
            ToolId("ollama"),
            "missing".to_string(),
            PathBuf::from("/p"),
            42,
            false,
            &mut logger,
        )
        .await;
        assert_eq!(outcome.outcome, DeleteOneResult::NotFound);
        assert_eq!(outcome.bytes_reclaimed, 0);
    }
}
