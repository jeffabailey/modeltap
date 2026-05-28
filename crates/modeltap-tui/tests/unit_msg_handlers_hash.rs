//! Unit tests for the hash-pool / unify-completion `Msg` variants added in
//! step 01-06 (cross-tool-model-unify DELIVER wave).
//!
//! Per `quality-framework` test-budget calculation:
//!   distinct behaviors:
//!     B1: `Msg::HashStarted { tool, model_id }`
//!         inserts `(tool, model_id)` into `state.hash_state.in_progress`.
//!     B2: `Msg::HashComputed { tool, model_id, hash, device, inode }`
//!         removes the key from `in_progress`, increments `completed`,
//!         records the hash + (device, inode), and recomputes
//!         `state.dedup_summary`.
//!     B3: `Msg::HashFailed { tool, model_id, reason }`
//!         removes the key from `in_progress`, adds it to `failed`,
//!         increments `completed`, and recomputes `state.dedup_summary`
//!         (BR-3: failed entries treated as Unique — no contribution).
//!     B4: `Msg::HashProgressTick` is a pure state-noop (250ms timer
//!         from the hash pool; used only as a re-render trigger).
//!     B5: `Msg::UnifyApplied(outcome)` refreshes the inode map for each
//!         affected (tool, model_id), recomputes `state.dedup_summary`,
//!         and sets `state.summary_delta = Some(SummaryDelta {
//!         previous_dedup_able_bytes, expires_at: now + 5s })`.
//!     B6: `Msg::SummaryDeltaExpired` clears `state.summary_delta` to `None`.
//!     B7: `Msg::UnifyHighlighted { tool, model_id }` sets
//!         `state.unify_highlight = Some((tool, model_id))`; the paired
//!         `Msg::UnifyHighlightExpired` clears it back to `None`.
//!   budget = 7 × 2 = 14 unit tests max. We use ~10 (7 happy paths + 3 edge
//!   cases per task spec).
//!
//! All tests enter through the `update::update(state, msg)` driving port and
//! assert observable state transitions. No internal classes are instantiated
//! directly except the test data builders (helpers).

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use modeltap_core::logic::compatibility::{Inventory, InventoryEntry};
use modeltap_core::logic::dedup::{dedup_summary, InodeMap, ModelKey};
use modeltap_core::{
    ContentHash, DiscoveredModel, DisplayLabel, Format, ModelStatus, ToolId, ToolStatus,
};
use modeltap_tui::app_state::{AppState, HashPoolState, SummaryDelta, ToolView};
use modeltap_tui::effects::unify_outcome::UnifyOutcome;
use modeltap_tui::msg::{HashFailureReason, Msg};
use modeltap_tui::update::update;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `ToolView` with `n` models named `<tool>:m{i}`.
fn tool_view(name: &'static str, status: ToolStatus, sizes: &[u64]) -> ToolView {
    ToolView {
        tool: ToolId(name),
        status,
        model_ids: (0..sizes.len()).map(|i| format!("{name}:m{i}")).collect(),
        model_sizes_bytes: sizes.to_vec(),
    }
}

/// A two-tool state with a single duplicated model:
///   ollama -> "ollama:m0" (1 KB)
///   hf     -> "hf:m0"     (1 KB)
fn state_two_tools_one_dup() -> AppState {
    AppState::new_with_default_selection(vec![
        tool_view("hf", ToolStatus::Ok, &[1024]),
        tool_view("ollama", ToolStatus::Ok, &[1024]),
    ])
}

fn h(byte: u8) -> ContentHash {
    ContentHash([byte; 32])
}

fn key(tool: &'static str, id: &str) -> ModelKey {
    (ToolId(tool), id.to_string())
}

/// Build an Inventory mirroring `state_two_tools_one_dup`'s rows. Used by
/// tests that pre-stage a `dedup_summary` baseline so the post-Msg
/// recompute can be compared.
fn inventory_two_tools_one_dup(hash_for: Option<(ContentHash, ContentHash)>) -> Inventory {
    let (oh, hh) = match hash_for {
        Some((a, b)) => (Some(a), Some(b)),
        None => (None, None),
    };
    Inventory {
        entries: vec![
            InventoryEntry {
                tool: ToolId("ollama"),
                model: DiscoveredModel {
                    id_in_tool: "ollama:m0".to_string(),
                    on_disk_path: PathBuf::from("/o/m0"),
                    size_bytes: 1024,
                    format: Format::Gguf,
                    display_label: DisplayLabel::from("ollama:m0"),
                    status: ModelStatus::Healthy,
                },
                content_hash: oh,
            },
            InventoryEntry {
                tool: ToolId("hf"),
                model: DiscoveredModel {
                    id_in_tool: "hf:m0".to_string(),
                    on_disk_path: PathBuf::from("/h/m0"),
                    size_bytes: 1024,
                    format: Format::Gguf,
                    display_label: DisplayLabel::from("hf:m0"),
                    status: ModelStatus::Healthy,
                },
                content_hash: hh,
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// B1 — Msg::HashStarted inserts into in_progress.
// ---------------------------------------------------------------------------

#[test]
fn hash_started_inserts_into_in_progress() {
    let mut state = state_two_tools_one_dup();
    state.hash_state.total = 2;
    let pre_completed = state.hash_state.completed;

    let (next, _eff) = update(
        state,
        Msg::HashStarted {
            tool: ToolId("ollama"),
            model_id: "ollama:m0".to_string(),
        },
    );

    assert!(
        next.hash_state.in_progress.contains("ollama:m0"),
        "in_progress must contain newly-started model id, got: {:?}",
        next.hash_state.in_progress
    );
    assert_eq!(
        next.hash_state.completed, pre_completed,
        "HashStarted must NOT increment completed"
    );
}

// ---------------------------------------------------------------------------
// B2 — Msg::HashComputed: removes from in_progress, increments completed,
// stores hash + inode, recomputes dedup_summary. The two-tool/one-dup setup
// produces dedup_able_bytes = 1024 once both hashes match AND inodes differ.
// ---------------------------------------------------------------------------

#[test]
fn hash_computed_records_hash_and_inode_and_recomputes_summary() {
    let mut state = state_two_tools_one_dup();
    state.hash_state.total = 2;
    state.hash_state.completed = 1; // hf already done in this scenario
    state
        .hash_state
        .completed_hashes
        .insert((ToolId("hf"), "hf:m0".to_string()), h(7));
    state
        .hash_state
        .inodes
        .insert((ToolId("hf"), "hf:m0".to_string()), (1, 200));
    state.hash_state.in_progress.insert("ollama:m0".to_string());

    let (next, _eff) = update(
        state,
        Msg::HashComputed {
            tool: ToolId("ollama"),
            model_id: "ollama:m0".to_string(),
            hash: h(7),
            device: 1,
            inode: 100,
            was_computed: true,
        },
    );

    // Removed from in_progress.
    assert!(
        !next.hash_state.in_progress.contains("ollama:m0"),
        "in_progress must drop completed model, got: {:?}",
        next.hash_state.in_progress
    );
    // Completed counter advanced.
    assert_eq!(next.hash_state.completed, 2);
    // Hash + inode persisted.
    let stored_hash = next
        .hash_state
        .completed_hashes
        .get(&(ToolId("ollama"), "ollama:m0".to_string()))
        .copied();
    assert_eq!(stored_hash, Some(h(7)));
    let stored_inode = next
        .hash_state
        .inodes
        .get(&(ToolId("ollama"), "ollama:m0".to_string()))
        .copied();
    assert_eq!(stored_inode, Some((1, 100)));

    // dedup_summary recomputed: with both hashes matching AND distinct
    // inodes, the 1024-byte payload is dedup-able.
    let inv = inventory_two_tools_one_dup(Some((h(7), h(7))));
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("ollama", "ollama:m0"), (1, 100));
    inodes.insert(key("hf", "hf:m0"), (1, 200));
    let expected = dedup_summary(&inv, &inodes, /* hashing_done */ true);
    assert_eq!(
        next.dedup_summary, expected,
        "dedup_summary must reflect dedup-able state after final hash arrives"
    );
    assert_eq!(next.dedup_summary.dedup_able_bytes, Some(1024));
}

// ---------------------------------------------------------------------------
// B3 — Msg::HashFailed: removes from in_progress, adds to failed, increments
// completed, recomputes dedup_summary (failed entries do not contribute).
// ---------------------------------------------------------------------------

#[test]
fn hash_failed_records_failure_and_recomputes_summary() {
    let mut state = state_two_tools_one_dup();
    state.hash_state.total = 2;
    state.hash_state.completed = 1;
    state
        .hash_state
        .completed_hashes
        .insert((ToolId("hf"), "hf:m0".to_string()), h(7));
    state
        .hash_state
        .inodes
        .insert((ToolId("hf"), "hf:m0".to_string()), (1, 200));
    state.hash_state.in_progress.insert("ollama:m0".to_string());

    let (next, _eff) = update(
        state,
        Msg::HashFailed {
            tool: ToolId("ollama"),
            model_id: "ollama:m0".to_string(),
            reason: HashFailureReason::Io("EIO".to_string()),
        },
    );

    assert!(
        !next.hash_state.in_progress.contains("ollama:m0"),
        "in_progress must drop failed model"
    );
    assert!(
        next.hash_state.failed.contains("ollama:m0"),
        "failed must contain the failing model id"
    );
    assert_eq!(next.hash_state.completed, 2);

    // BR-3: a failed entry contributes nothing → dedup_summary stays at the
    // hf-only baseline (no peer with matching hash → 0 dedup-able bytes).
    let inv = inventory_two_tools_one_dup(Some((h(7), h(7))));
    let mut inodes: InodeMap = HashMap::new();
    inodes.insert(key("hf", "hf:m0"), (1, 200));
    // ollama deliberately omitted — its hash is not in completed_hashes.
    let baseline_with_only_hf_hashed = {
        // Build inventory where ONLY hf has a content_hash; ollama is None.
        let inv_partial = Inventory {
            entries: vec![
                InventoryEntry {
                    tool: ToolId("ollama"),
                    model: DiscoveredModel {
                        id_in_tool: "ollama:m0".to_string(),
                        on_disk_path: PathBuf::from("/o/m0"),
                        size_bytes: 1024,
                        format: Format::Gguf,
                        display_label: DisplayLabel::from("ollama:m0"),
                        status: ModelStatus::Healthy,
                    },
                    content_hash: None, // hash failed
                },
                inv.entries[1].clone(),
            ],
        };
        dedup_summary(&inv_partial, &inodes, true)
    };
    assert_eq!(next.dedup_summary, baseline_with_only_hf_hashed);
    assert_eq!(
        next.dedup_summary.dedup_able_bytes,
        Some(0),
        "BR-3: failed-hash entry does not count toward dedup-able bytes"
    );
}

// ---------------------------------------------------------------------------
// B4 — Msg::HashProgressTick is a pure state-noop.
// ---------------------------------------------------------------------------

#[test]
fn hash_progress_tick_is_state_noop() {
    let mut state = state_two_tools_one_dup();
    state.hash_state.total = 5;
    state.hash_state.completed = 2;
    state.hash_state.in_progress.insert("ollama:m0".to_string());
    let pre = state.clone();

    let (next, eff) = update(state, Msg::HashProgressTick);

    assert_eq!(next, pre, "HashProgressTick must NOT mutate state");
    assert_eq!(
        eff,
        modeltap_tui::update::UpdateEffect::default(),
        "HashProgressTick must not request side effects"
    );
}

// ---------------------------------------------------------------------------
// B5 — Msg::UnifyApplied refreshes inodes for affected pairs, recomputes
// dedup_summary, and sets summary_delta with the previous dedup-able bytes.
// ---------------------------------------------------------------------------

#[test]
fn unify_applied_records_summary_delta_and_recomputes() {
    // Pre-state: hashing complete on both tools; both hashes match but
    // inodes differ → dedup_able_bytes = Some(1024).
    let mut state = state_two_tools_one_dup();
    state.hash_state.total = 2;
    state.hash_state.completed = 2;
    state
        .hash_state
        .completed_hashes
        .insert((ToolId("hf"), "hf:m0".to_string()), h(7));
    state
        .hash_state
        .completed_hashes
        .insert((ToolId("ollama"), "ollama:m0".to_string()), h(7));
    state
        .hash_state
        .inodes
        .insert((ToolId("hf"), "hf:m0".to_string()), (1, 200));
    state
        .hash_state
        .inodes
        .insert((ToolId("ollama"), "ollama:m0".to_string()), (1, 100));
    // Pre-compute the baseline dedup_summary.
    let inv = inventory_two_tools_one_dup(Some((h(7), h(7))));
    let mut pre_inodes: InodeMap = HashMap::new();
    pre_inodes.insert(key("ollama", "ollama:m0"), (1, 100));
    pre_inodes.insert(key("hf", "hf:m0"), (1, 200));
    state.dedup_summary = dedup_summary(&inv, &pre_inodes, true);
    let prev_dedup_able = state.dedup_summary.dedup_able_bytes.unwrap_or(0);
    assert_eq!(prev_dedup_able, 1024, "baseline must be dedup-able");

    let outcome = UnifyOutcome {
        affected: vec![
            (ToolId("ollama"), "ollama:m0".to_string()),
            (ToolId("hf"), "hf:m0".to_string()),
        ],
        // Both tools' targets now share the canonical inode. Composition root
        // re-stat'd them after the link succeeded; here we hand the canonical
        // inode in via the outcome's affected list (ollama's pre-link inode).
        bytes_reclaimed: 1024,
    };
    let before = Instant::now();
    let (next, _eff) = update(state, Msg::UnifyApplied(outcome));
    let after = Instant::now();

    // summary_delta must be set with the prior dedup-able total.
    let delta = next
        .summary_delta
        .as_ref()
        .expect("UnifyApplied must populate summary_delta");
    assert_eq!(delta.previous_dedup_able_bytes, prev_dedup_able);
    // expires_at is roughly now + 5s. Be lenient: just ensure it's in the future.
    assert!(
        delta.expires_at >= before,
        "expires_at must be >= now at dispatch time"
    );
    assert!(
        delta.expires_at <= after + std::time::Duration::from_secs(6),
        "expires_at must be roughly 5s from now (≤ 6s leeway)"
    );

    // dedup_summary must have been recomputed (no panic / changed shape).
    // The exact value depends on how the handler interprets `affected` — at
    // minimum the call must NOT leave the field stale-and-untouched (a
    // regression we want to catch). Assert by recomputing what the handler
    // SHOULD compute given the now-shared-inode semantics: every affected
    // pair shares the canonical (1, 100) inode.
    let mut post_inodes: InodeMap = HashMap::new();
    post_inodes.insert(key("ollama", "ollama:m0"), (1, 100));
    post_inodes.insert(key("hf", "hf:m0"), (1, 100));
    let expected_post = dedup_summary(&inv, &post_inodes, true);
    assert_eq!(
        next.dedup_summary, expected_post,
        "UnifyApplied must recompute dedup_summary using post-link inode map"
    );
    assert_eq!(next.dedup_summary.unified_count, Some(1));
    assert_eq!(next.dedup_summary.dedup_able_bytes, Some(0));
}

// ---------------------------------------------------------------------------
// B6 — Msg::SummaryDeltaExpired clears summary_delta to None.
// ---------------------------------------------------------------------------

#[test]
fn summary_delta_expired_clears_field() {
    let mut state = state_two_tools_one_dup();
    state.summary_delta = Some(SummaryDelta {
        previous_dedup_able_bytes: 1024,
        expires_at: Instant::now() + std::time::Duration::from_secs(5),
    });

    let (next, _eff) = update(state, Msg::SummaryDeltaExpired);

    assert_eq!(
        next.summary_delta, None,
        "SummaryDeltaExpired must clear summary_delta to None"
    );
}

// ---------------------------------------------------------------------------
// B7 — Msg::UnifyHighlighted sets unify_highlight; UnifyHighlightExpired
// clears it.
// ---------------------------------------------------------------------------

#[test]
fn unify_highlighted_sets_then_expired_clears_unify_highlight() {
    let state = state_two_tools_one_dup();
    let (mid, _eff) = update(
        state,
        Msg::UnifyHighlighted {
            tool: ToolId("ollama"),
            model_id: "ollama:m0".to_string(),
        },
    );
    assert_eq!(
        mid.unify_highlight,
        Some((ToolId("ollama"), "ollama:m0".to_string())),
        "UnifyHighlighted must set unify_highlight"
    );

    let (cleared, _eff2) = update(mid, Msg::UnifyHighlightExpired);
    assert_eq!(
        cleared.unify_highlight, None,
        "UnifyHighlightExpired must clear unify_highlight"
    );
}

// ---------------------------------------------------------------------------
// Edge case 1 — HashComputed when target is NOT in in_progress is still safe
// (defensive; pool may emit the start/complete pair out of order under load).
// ---------------------------------------------------------------------------

#[test]
fn hash_computed_when_not_in_progress_still_records_outcome() {
    let mut state = state_two_tools_one_dup();
    state.hash_state.total = 1;
    state.hash_state.completed = 0;
    // Note: in_progress is empty — no HashStarted preceded this Computed.

    let (next, _eff) = update(
        state,
        Msg::HashComputed {
            tool: ToolId("ollama"),
            model_id: "ollama:m0".to_string(),
            hash: h(9),
            device: 1,
            inode: 100,
            was_computed: true,
        },
    );

    // Counter still advances; hash + inode still recorded.
    assert_eq!(next.hash_state.completed, 1);
    assert_eq!(
        next.hash_state
            .completed_hashes
            .get(&(ToolId("ollama"), "ollama:m0".to_string()))
            .copied(),
        Some(h(9))
    );
    assert_eq!(
        next.hash_state
            .inodes
            .get(&(ToolId("ollama"), "ollama:m0".to_string()))
            .copied(),
        Some((1, 100))
    );
    // in_progress remains empty (no panic on missing key).
    assert!(next.hash_state.in_progress.is_empty());
}

// ---------------------------------------------------------------------------
// Edge case 2 — HashFailed is idempotent when state.failed already contains
// the key (re-delivered failure must not double-count completed).
// ---------------------------------------------------------------------------

#[test]
fn hash_failed_is_idempotent_when_already_failed() {
    let mut state = state_two_tools_one_dup();
    state.hash_state.total = 2;
    state.hash_state.completed = 2;
    state.hash_state.failed.insert("ollama:m0".to_string());
    let pre_failed: BTreeSet<String> = state.hash_state.failed.clone();
    let pre_completed = state.hash_state.completed;

    let (next, _eff) = update(
        state,
        Msg::HashFailed {
            tool: ToolId("ollama"),
            model_id: "ollama:m0".to_string(),
            reason: HashFailureReason::Cancelled,
        },
    );

    // Set membership is unchanged (BTreeSet semantics already give this).
    assert_eq!(next.hash_state.failed, pre_failed);
    // CRITICAL: completed must not double-increment on re-delivered failures.
    assert_eq!(
        next.hash_state.completed, pre_completed,
        "HashFailed must be idempotent w.r.t. completed counter"
    );
}

// ---------------------------------------------------------------------------
// Edge case 3 — SummaryDeltaExpired is a no-op when summary_delta is already
// None (defensive against a stale 5s timer firing after a fresh unify
// reset/clear).
// ---------------------------------------------------------------------------

#[test]
fn summary_delta_expired_when_already_none_is_noop() {
    let state = state_two_tools_one_dup();
    assert_eq!(state.summary_delta, None, "precondition: starts None");
    let pre = state.clone();

    let (next, eff) = update(state, Msg::SummaryDeltaExpired);

    assert_eq!(
        next, pre,
        "no state change when summary_delta was already None"
    );
    assert_eq!(eff, modeltap_tui::update::UpdateEffect::default());
}

// ---------------------------------------------------------------------------
// Edge case 4 — HashPoolState shape: the new completed_hashes / inodes fields
// must default to empty so existing AppState constructors still work.
// ---------------------------------------------------------------------------

#[test]
fn hash_pool_state_default_includes_new_fields_empty() {
    let s = HashPoolState::default();
    assert!(s.completed_hashes.is_empty());
    assert!(s.inodes.is_empty());
}
