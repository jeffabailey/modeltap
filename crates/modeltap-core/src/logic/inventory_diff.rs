//! Pure inventory-diff computation (Phase 05 step 05-01 / US-24 / US-26).
//!
//! `compute_inventory_diff(cached, fresh) -> Vec<InventoryDiff>` projects a
//! cached vs fresh per-tool model list into a structured diff: added,
//! removed, and modified (sha256 OR size changed). The orchestrator at
//! `modeltap-app::orchestration::reconcile` consumes the diff to drive the
//! silent-ack indicator (AC-26-4) and to decide whether the per-tool write
//! transaction actually changes anything.
//!
//! Pure function — no I/O, no clocks. Tested as its own driving port: the
//! function signature IS the public interface (per `nw-tdd-methodology`
//! port-to-port convention for pure domain functions).

use std::collections::BTreeMap;

use crate::types::ToolId;

/// One model identifier as the diff sees it. Mirrors the `model_id` column on
/// `cache_models` and the `id_in_tool` field on `DiscoveredModel`.
pub type ModelId = String;

/// What changed for a given model — sha256 OR size, individually or together.
/// The orchestrator does not branch on which dimension drifted (silent-ack
/// fires on any non-empty diff); the fine-grained field is kept so future
/// observability events can attribute drift causes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelDrift {
    pub sha256_changed: bool,
    pub size_changed: bool,
}

impl ModelDrift {
    /// True iff at least one dimension drifted. A drift entry must be
    /// non-empty by construction; this guards against a future bug that adds
    /// a `Default::default()` entry to `modified_models` with no flags set.
    pub fn is_meaningful(&self) -> bool {
        self.sha256_changed || self.size_changed
    }
}

/// Cached-vs-fresh projection of one model's drift-relevant fields. The
/// orchestrator builds these from `CachedModel` rows (cached side) and from
/// `DiscoveredModel` returned by `Tool::discover()` (fresh side). Kept as a
/// pure data type so the diff function itself stays free of store/types
/// dependencies (modeltap-core does not depend on modeltap-store).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSignature {
    pub model_id: ModelId,
    pub size_bytes: u64,
    /// `None` when sha256 has not been computed yet for this row (the
    /// background hash pool fills these in lazily per ADR-002). A side that
    /// never had a hash compared against a side that has one is treated as
    /// "no sha256 drift" — only two non-None values can disagree. This avoids
    /// spurious silent-ack signals whenever the hash pool catches up.
    pub sha256: Option<String>,
}

/// Per-tool diff summary. `added_models` / `removed_models` carry plain
/// model_id strings; `modified_models` carries the model_id plus the drift
/// flags so observability can attribute the cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryDiff {
    pub tool_id: ToolId,
    pub added_models: Vec<ModelId>,
    pub removed_models: Vec<ModelId>,
    pub modified_models: Vec<(ModelId, ModelDrift)>,
}

impl InventoryDiff {
    /// True iff every drift category is empty — used by the orchestrator to
    /// short-circuit the silent-ack indicator (a no-op reconcile does not
    /// surface a `*` marker per AC-26-4).
    pub fn is_empty(&self) -> bool {
        self.added_models.is_empty()
            && self.removed_models.is_empty()
            && self.modified_models.is_empty()
    }
}

/// Pure diff over one tool's (cached, fresh) signature lists. Order of the
/// output vectors is deterministic (sorted by `model_id`) so the JSONL
/// observability events the orchestrator emits are stable across runs and
/// test snapshots compare byte-for-byte.
///
/// Both input lists are treated as sets keyed by `model_id`. Duplicate ids
/// inside one side are collapsed to the LAST occurrence (the BTreeMap insert
/// semantics) — production callers never produce duplicates because both
/// `cache_models` (PRIMARY KEY (model_id, tool_id)) and live `discover()`
/// results are uniquely keyed.
pub fn compute_inventory_diff(
    tool_id: ToolId,
    cached: &[ModelSignature],
    fresh: &[ModelSignature],
) -> InventoryDiff {
    let cached_by_id: BTreeMap<&ModelId, &ModelSignature> =
        cached.iter().map(|m| (&m.model_id, m)).collect();
    let fresh_by_id: BTreeMap<&ModelId, &ModelSignature> =
        fresh.iter().map(|m| (&m.model_id, m)).collect();

    let mut added: Vec<ModelId> = Vec::new();
    let mut removed: Vec<ModelId> = Vec::new();
    let mut modified: Vec<(ModelId, ModelDrift)> = Vec::new();

    for (model_id, fresh_sig) in &fresh_by_id {
        match cached_by_id.get(model_id) {
            None => added.push((*model_id).clone()),
            Some(cached_sig) => {
                let drift = drift_between(cached_sig, fresh_sig);
                if drift.is_meaningful() {
                    modified.push(((*model_id).clone(), drift));
                }
            }
        }
    }

    for model_id in cached_by_id.keys() {
        if !fresh_by_id.contains_key(model_id) {
            removed.push((*model_id).clone());
        }
    }

    // BTreeMap iteration already yields sorted order for added/modified; the
    // removed list is built in cached-key order which is also sorted. Belt-
    // and-braces sort anyway so a future refactor that swaps the map type
    // cannot silently break the ordering contract.
    added.sort();
    removed.sort();
    modified.sort_by(|a, b| a.0.cmp(&b.0));

    InventoryDiff {
        tool_id,
        added_models: added,
        removed_models: removed,
        modified_models: modified,
    }
}

/// Field-by-field drift detector. `sha256_changed` requires BOTH sides to
/// carry a hash (an `Option::None` on either side is "unknown", not
/// "different") — the lazy hash pool would otherwise produce a steady stream
/// of false-positive drift events as it catches up.
fn drift_between(cached: &ModelSignature, fresh: &ModelSignature) -> ModelDrift {
    let size_changed = cached.size_bytes != fresh.size_bytes;
    let sha256_changed = match (&cached.sha256, &fresh.sha256) {
        (Some(a), Some(b)) => a != b,
        _ => false,
    };
    ModelDrift {
        sha256_changed,
        size_changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(id: &str, size: u64, sha: Option<&str>) -> ModelSignature {
        ModelSignature {
            model_id: id.to_string(),
            size_bytes: size,
            sha256: sha.map(|s| s.to_string()),
        }
    }

    #[test]
    fn empty_inputs_produce_empty_diff() {
        let diff = compute_inventory_diff(ToolId("tool-a"), &[], &[]);
        assert!(diff.is_empty());
    }

    #[test]
    fn fresh_model_absent_from_cache_classifies_as_added() {
        let cached = vec![];
        let fresh = vec![sig("m1", 100, None)];
        let diff = compute_inventory_diff(ToolId("tool-a"), &cached, &fresh);
        assert_eq!(diff.added_models, vec!["m1".to_string()]);
        assert!(diff.removed_models.is_empty());
        assert!(diff.modified_models.is_empty());
    }

    #[test]
    fn cached_model_absent_from_fresh_classifies_as_removed() {
        let cached = vec![sig("m1", 100, None)];
        let fresh = vec![];
        let diff = compute_inventory_diff(ToolId("tool-a"), &cached, &fresh);
        assert_eq!(diff.removed_models, vec!["m1".to_string()]);
        assert!(diff.added_models.is_empty());
    }

    #[test]
    fn size_change_classifies_as_modified() {
        let cached = vec![sig("m1", 100, None)];
        let fresh = vec![sig("m1", 200, None)];
        let diff = compute_inventory_diff(ToolId("tool-a"), &cached, &fresh);
        assert_eq!(diff.modified_models.len(), 1);
        assert_eq!(diff.modified_models[0].0, "m1");
        assert!(diff.modified_models[0].1.size_changed);
        assert!(!diff.modified_models[0].1.sha256_changed);
    }

    #[test]
    fn sha256_change_classifies_as_modified_only_when_both_sides_present() {
        let cached = vec![sig("m1", 100, Some("aaaa"))];
        let fresh = vec![sig("m1", 100, Some("bbbb"))];
        let diff = compute_inventory_diff(ToolId("tool-a"), &cached, &fresh);
        assert_eq!(diff.modified_models.len(), 1);
        assert!(diff.modified_models[0].1.sha256_changed);
        assert!(!diff.modified_models[0].1.size_changed);
    }

    #[test]
    fn missing_sha256_on_one_side_is_unknown_not_drift() {
        // Hash pool has not yet hashed this model — the cache row has None on
        // sha256 while the fresh discover() also returns None. No drift.
        let cached = vec![sig("m1", 100, None)];
        let fresh = vec![sig("m1", 100, Some("aaaa"))];
        let diff = compute_inventory_diff(ToolId("tool-a"), &cached, &fresh);
        assert!(diff.is_empty(), "asymmetric None must not produce drift");
    }

    #[test]
    fn output_vectors_are_sorted_by_model_id() {
        let cached = vec![sig("z1", 100, None)];
        let fresh = vec![
            sig("b1", 100, None),
            sig("a1", 100, None),
            sig("c1", 100, None),
        ];
        let diff = compute_inventory_diff(ToolId("tool-a"), &cached, &fresh);
        assert_eq!(
            diff.added_models,
            vec!["a1".to_string(), "b1".to_string(), "c1".to_string()]
        );
        assert_eq!(diff.removed_models, vec!["z1".to_string()]);
    }
}
