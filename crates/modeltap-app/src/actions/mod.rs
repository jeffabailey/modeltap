//! Action orchestrators — bridge from a `UpdateEffect::trigger_*` flag to a
//! plugin call + JSONL emission. Pure orchestration; the destructive work
//! lives behind `Tool::link` / `Tool::delete_one` / `Tool::delete_all`.
//!
//! Each action module owns its own JSONL-event payload mapping (e.g.,
//! `actions::zap` builds the `action.zap_all` envelope) and returns a
//! structured `*Outcome` the composition root surfaces in the UI.
//!
//! Per the kpi-instrumentation §"Privacy" rule: NO model names, NO paths,
//! NO usernames in any action JSONL event. The orchestrator is responsible
//! for redaction at this seam.

pub mod delete_one;
pub mod reclassify;
pub mod unify;
pub mod zap;

use std::path::PathBuf;

use modeltap_core::{DedupKey, DisplayLabel, Format, ModelMeta, ModelStatus, ToolId};

/// Build a synthetic `ModelMeta` for plugin calls (`link()`, `delete_one()`)
/// at the orchestrator boundary.
///
/// Only `tool`, `id_in_tool`, `on_disk_path`, and `size_bytes` are
/// load-bearing for those plugin paths; the other fields are filled with
/// conservative defaults (`Format::Other`, `ModelStatus::Healthy`,
/// `DedupKey::Tentative(label)`) because the orchestrator does not always
/// have them at the dialog seam.
///
/// Shared between `unify::synthesize_model_meta` and
/// `delete_one::synthesize_model_meta` — both consumed the same defaults block.
pub(super) fn synthetic_model_meta(
    tool: ToolId,
    id_in_tool: String,
    on_disk_path: PathBuf,
    size_bytes: u64,
) -> ModelMeta {
    let label = DisplayLabel::from(id_in_tool.clone());
    ModelMeta {
        tool,
        id_in_tool,
        on_disk_path,
        size_bytes,
        format: Format::Other,
        display_label: label.clone(),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(label),
    }
}
