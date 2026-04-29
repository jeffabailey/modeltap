//! Pure-domain unique-vs-shared classifier for the zap-all dialog.
//!
//! Per ADR-002 conservative-deletion rule: when the dedup-key is uncertain,
//! treat the model as unique (preserves data). For the WS slice we have no
//! SHA256 yet (lazy hashing arrives in 01-05), so the classifier uses the
//! `on_disk_path` as the only authoritative same-content signal:
//!
//!   - Two `ToolModel` entries with the SAME `on_disk_path` from DIFFERENT
//!     tools are SHARED (deleting from one tool does NOT free those bytes
//!     because another tool still references the same file).
//!   - Everything else is UNIQUE to the queried tool.
//!
//! ADR-002 conservative-deletion rule citation: this conservative posture is
//! the safety guarantee — if we are unsure two files are duplicates, we keep
//! both. The classifier never flags as "shared" anything whose dedup-key is
//! uncertain; the only "shared" signal it accepts is byte-identical paths.
//!
//! When SHA256 hashing lands (01-05+), the classifier will additionally treat
//! same-content-different-paths as shared, but that change is purely additive
//! — the safety property holds.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::types::{DisplayLabel, Format, ModelStatus, ToolId};

/// Per-tool projection of one discovered model — the cross-plugin view the
/// classifier consumes. Identical fields to `ModelMeta` minus the dedup_key
/// (the classifier IS the dedup-key authority for the WS slice).
#[derive(Debug, Clone, Serialize)]
pub struct ToolModel {
    pub tool: ToolId,
    pub id_in_tool: String,
    pub on_disk_path: PathBuf,
    pub size_bytes: u64,
    pub format: Format,
    pub display_label: DisplayLabel,
    pub status: ModelStatus,
}

/// Output of `classify_unique_vs_shared`: counts and byte totals for the
/// queried tool. `unique_count + shared_count` equals the number of models
/// that tool registers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueVsSharedReport {
    pub unique_count: u64,
    pub shared_count: u64,
    pub unique_bytes: u64,
    pub shared_bytes: u64,
}

/// Classify each model registered with `tool_id` as either unique to that
/// tool (deleting it actually frees its bytes) or shared with another tool
/// (deleting it only removes the registration; bytes remain referenced
/// elsewhere on disk).
///
/// Per ADR-002 conservative-deletion rule, this WS-slice implementation
/// treats two entries as SHARED only when their `on_disk_path` is byte-equal
/// AND they belong to different tools. All other cases (different paths,
/// uncomputed hashes) are treated as UNIQUE — preserving data is the safer
/// default.
pub fn classify_unique_vs_shared(
    inventory: &[ToolModel],
    tool_id: &ToolId,
) -> UniqueVsSharedReport {
    // Index every on_disk_path → set of tools that reference it.
    let mut path_tools: HashMap<&PathBuf, Vec<ToolId>> = HashMap::new();
    for m in inventory {
        path_tools.entry(&m.on_disk_path).or_default().push(m.tool);
    }

    let mut report = UniqueVsSharedReport {
        unique_count: 0,
        shared_count: 0,
        unique_bytes: 0,
        shared_bytes: 0,
    };

    for m in inventory {
        if m.tool != *tool_id {
            continue;
        }
        // Shared iff some OTHER tool also references this exact path.
        let referenced_by_others = path_tools
            .get(&m.on_disk_path)
            .map(|tools| tools.iter().any(|t| t != tool_id))
            .unwrap_or(false);
        if referenced_by_others {
            report.shared_count = report.shared_count.saturating_add(1);
            report.shared_bytes = report.shared_bytes.saturating_add(m.size_bytes);
        } else {
            report.unique_count = report.unique_count.saturating_add(1);
            report.unique_bytes = report.unique_bytes.saturating_add(m.size_bytes);
        }
    }

    report
}
