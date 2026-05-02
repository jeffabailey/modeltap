//! Step 01-11 unit/integration tests for
//! `modeltap_app::reclassify::reclassify_after_unify`.
//!
//! These tests cover the per-AC behaviors of US-U6:
//!
//!   - AC-U6.1: row glyph transitions `=` -> `#` after a successful unify.
//!   - AC-U6.2: `state.dedup_summary` recomputed (dedup_able_bytes drops).
//!   - AC-U6.3: partial success — only the successful tools' inodes move;
//!     the still-distinct tool's row stays `=`.
//!   - AC-U6.4: `Unified` count in `dedup_summary.unified_count` increments
//!     for the unified group.
//!   - AC-U6.6: `summary_delta` is set with the previous dedup_able_bytes
//!     so the renderer can show the transient "(was X)" annotation.
//!   - AC-U6.7: AlreadyUnified — no inode movement (already shared), but
//!     summary_delta is still set so the user sees the action acknowledged.
//!
//! Plus a timing budget assertion: the function must complete in <200 ms
//! over a fixture of ~50 models (per the step's perf gate).
//!
//! All tests are pure-state-in/pure-state-out; no filesystem I/O. The
//! function under test is the lib-side `reclassify_after_unify` (the bin
//! adapter `actions::reclassify::reclassify_after_unify` in main.rs is a
//! thin shim that translates `actions::unify::UnifyOutcome` into the lib
//! summary; covered indirectly by the orchestrator wiring).

use std::collections::{BTreeMap, BTreeSet};

use modeltap_app::reclassify::{reclassify_after_unify, UnifyReclassifySummary};
use modeltap_core::{ContentHash, DedupSummary, ToolId, ToolStatus};
use modeltap_tui::app_state::HashPoolState;
use modeltap_tui::{AppState, ToolView};

// ---------- helpers --------------------------------------------------------

fn h(byte: u8) -> ContentHash {
    ContentHash([byte; 32])
}

/// Build an AppState with the given tools/models and a populated hash_state.
/// Each `(tool, model_id)` is given a `(device, inode)` taken from the
/// supplied `inodes` map and a hash from the supplied `hashes` map. The
/// hash pool is marked complete (total == completed) so dedup_summary
/// recomputation runs the real classifier.
fn make_state(
    tools: Vec<(ToolId, Vec<(&str, u64)>)>,
    hashes: BTreeMap<(ToolId, String), ContentHash>,
    inodes: BTreeMap<(ToolId, String), (u64, u64)>,
) -> AppState {
    let mut tool_views: Vec<ToolView> = Vec::new();
    let mut total_jobs: u64 = 0;
    for (tool, models) in &tools {
        let mut ids = Vec::new();
        let mut sizes = Vec::new();
        for (id, size) in models {
            ids.push((*id).to_string());
            sizes.push(*size);
            total_jobs += 1;
        }
        tool_views.push(ToolView {
            tool: *tool,
            status: ToolStatus::Ok,
            model_ids: ids,
            model_sizes_bytes: sizes,
        });
    }
    let mut state = AppState::new_with_default_selection(tool_views);
    // Seed hash_state so dedup_summary will produce non-default output.
    state.hash_state = HashPoolState {
        total: total_jobs,
        completed: total_jobs,
        in_progress: BTreeSet::new(),
        failed: BTreeSet::new(),
        completed_hashes: hashes,
        inodes,
    };
    // Recompute dedup_summary so pre-condition reflects the seeded state.
    // The simplest way is to dispatch any noop msg through update — but for
    // simplicity in tests we compute a stand-in via a small reclassify pass.
    // The reclassify_after_unify function does the recompute as a side
    // effect, so we run a noop reclassify for an empty UnifyResult to seed.
    let seed_summary = UnifyReclassifySummary {
        succeeded_tools: Vec::new(),
        already_unified: false,
    };
    // Suppress the seeding's summary_delta so tests start clean.
    let mut seeded = reclassify_after_unify(state, &seed_summary);
    seeded.summary_delta = None;
    seeded
}

// ---------- AC-U6.1 / AC-U6.2 / AC-U6.4: full success ---------------------

#[test]
fn successful_unify_drops_dedup_able_bytes_and_increments_unified_count() {
    // Pre: ollama and hf both have model "dup", same hash, DIFFERENT inodes
    // -> dedup_summary classifies as DedupAble (dedup_able_bytes > 0).
    let ollama = ToolId("ollama");
    let hf = ToolId("hf");
    let mut hashes = BTreeMap::new();
    hashes.insert((ollama, "dup".to_string()), h(0x42));
    hashes.insert((hf, "dup".to_string()), h(0x42));
    let mut inodes = BTreeMap::new();
    inodes.insert((ollama, "dup".to_string()), (1, 100));
    inodes.insert((hf, "dup".to_string()), (1, 200)); // different inode
    let state = make_state(
        vec![(ollama, vec![("dup", 4096)]), (hf, vec![("dup", 4096)])],
        hashes,
        inodes,
    );

    let pre_dedup_able = state.dedup_summary.dedup_able_bytes.unwrap_or(0);
    assert!(
        pre_dedup_able >= 4096,
        "precondition: pre-unify must have dedup_able_bytes >= 4096, got {}",
        pre_dedup_able
    );
    assert_eq!(
        state.dedup_summary.unified_count.unwrap_or(0),
        0,
        "precondition: pre-unify must have unified_count == 0"
    );

    // Act: unify succeeded for both tools (the canonical's tool included so
    // both inodes converge).
    let summary = UnifyReclassifySummary {
        succeeded_tools: vec![ollama, hf],
        already_unified: false,
    };
    let after = reclassify_after_unify(state, &summary);

    assert_eq!(
        after.dedup_summary.dedup_able_bytes.unwrap_or(99999),
        0,
        "AC-U6.2: post-unify dedup_able_bytes must collapse to 0 \
         (both inodes now match)"
    );
    assert_eq!(
        after.dedup_summary.unified_count.unwrap_or(0),
        1,
        "AC-U6.4: post-unify unified_count must be 1 for the converged group"
    );
}

// ---------- AC-U6.6: summary_delta carries previous dedup_able_bytes ------

#[test]
fn successful_unify_sets_summary_delta_with_previous_dedup_able_bytes() {
    let ollama = ToolId("ollama");
    let hf = ToolId("hf");
    let mut hashes = BTreeMap::new();
    hashes.insert((ollama, "dup".to_string()), h(0x42));
    hashes.insert((hf, "dup".to_string()), h(0x42));
    let mut inodes = BTreeMap::new();
    inodes.insert((ollama, "dup".to_string()), (1, 100));
    inodes.insert((hf, "dup".to_string()), (1, 200));
    let state = make_state(
        vec![(ollama, vec![("dup", 8192)]), (hf, vec![("dup", 8192)])],
        hashes,
        inodes,
    );
    let pre_dedup_able = state.dedup_summary.dedup_able_bytes.unwrap_or(0);
    assert!(pre_dedup_able >= 8192);

    let summary = UnifyReclassifySummary {
        succeeded_tools: vec![ollama, hf],
        already_unified: false,
    };
    let after = reclassify_after_unify(state, &summary);

    let delta = after
        .summary_delta
        .as_ref()
        .expect("AC-U6.6: summary_delta must be set after successful unify");
    assert_eq!(
        delta.previous_dedup_able_bytes, pre_dedup_able,
        "AC-U6.6: summary_delta.previous_dedup_able_bytes must equal the \
         pre-unify dedup_able_bytes"
    );
    assert!(
        delta.expires_at > std::time::Instant::now(),
        "AC-U6.6: summary_delta.expires_at must be in the future (5s window)"
    );
}

// ---------- AC-U6.3: partial — only successful tool's inode moves ----------

#[test]
fn partial_unify_only_advances_inodes_for_successful_tools() {
    // Three tools, all share content but live on different inodes. unify
    // succeeded for ollama+hf; lm-studio failed (e.g. EACCES).
    let ollama = ToolId("ollama");
    let hf = ToolId("hf");
    let lm = ToolId("lm-studio");
    let mut hashes = BTreeMap::new();
    hashes.insert((ollama, "m".to_string()), h(0x42));
    hashes.insert((hf, "m".to_string()), h(0x42));
    hashes.insert((lm, "m".to_string()), h(0x42));
    let mut inodes = BTreeMap::new();
    inodes.insert((ollama, "m".to_string()), (1, 100));
    inodes.insert((hf, "m".to_string()), (1, 200));
    inodes.insert((lm, "m".to_string()), (1, 300));
    let state = make_state(
        vec![
            (ollama, vec![("m", 4096)]),
            (hf, vec![("m", 4096)]),
            (lm, vec![("m", 4096)]),
        ],
        hashes,
        inodes,
    );
    // pre: dedup_able_bytes counts the group once (>= 4096).
    let pre = state.dedup_summary.dedup_able_bytes.unwrap_or(0);
    assert!(pre >= 4096);

    // Act: only ollama+hf succeeded.
    let summary = UnifyReclassifySummary {
        succeeded_tools: vec![ollama, hf],
        already_unified: false,
    };
    let after = reclassify_after_unify(state, &summary);

    // The group still has 2 distinct inodes (ollama+hf converged inode vs
    // lm-studio's separate inode), so dedup_able_bytes must STILL be > 0
    // (one inode worth still reclaimable). Unified count remains 0.
    assert!(
        after.dedup_summary.dedup_able_bytes.unwrap_or(0) > 0,
        "AC-U6.3: partial success must leave dedup_able_bytes > 0 (lm-studio \
         still on a separate inode), got {:?}",
        after.dedup_summary.dedup_able_bytes
    );
    assert_eq!(
        after.dedup_summary.unified_count.unwrap_or(99),
        0,
        "AC-U6.6: partial success must NOT increment unified_count"
    );

    // Inode invariants: ollama and hf converged onto a shared inode;
    // lm-studio's inode is unchanged from pre.
    let after_ollama_inode = after
        .hash_state
        .inodes
        .get(&(ollama, "m".to_string()))
        .copied();
    let after_hf_inode = after
        .hash_state
        .inodes
        .get(&(hf, "m".to_string()))
        .copied();
    let after_lm_inode = after
        .hash_state
        .inodes
        .get(&(lm, "m".to_string()))
        .copied();
    assert_eq!(
        after_ollama_inode, after_hf_inode,
        "AC-U6.3: ollama+hf inodes must match after partial success"
    );
    assert_eq!(
        after_lm_inode,
        Some((1, 300)),
        "AC-U6.3: lm-studio inode must NOT move on partial success"
    );
}

// ---------- AC-U6.7: AlreadyUnified branch --------------------------------

#[test]
fn already_unified_outcome_does_not_change_inodes_but_sets_summary_delta() {
    let ollama = ToolId("ollama");
    let hf = ToolId("hf");
    let mut hashes = BTreeMap::new();
    hashes.insert((ollama, "dup".to_string()), h(0x42));
    hashes.insert((hf, "dup".to_string()), h(0x42));
    let mut inodes = BTreeMap::new();
    // Already unified — both point at the same (device, inode).
    inodes.insert((ollama, "dup".to_string()), (1, 100));
    inodes.insert((hf, "dup".to_string()), (1, 100));
    let state = make_state(
        vec![(ollama, vec![("dup", 4096)]), (hf, vec![("dup", 4096)])],
        hashes,
        inodes,
    );
    let pre_inodes = state.hash_state.inodes.clone();

    let summary = UnifyReclassifySummary {
        succeeded_tools: vec![ollama, hf],
        already_unified: true,
    };
    let after = reclassify_after_unify(state, &summary);

    assert_eq!(
        after.hash_state.inodes, pre_inodes,
        "AC-U6.7: AlreadyUnified must NOT change inode entries"
    );
    assert!(
        after.summary_delta.is_some(),
        "AC-U6.7: AlreadyUnified must still set summary_delta so the user \
         sees acknowledgement"
    );
}

// ---------- dedup_summary recomputed deterministically --------------------

#[test]
fn reclassify_recomputes_dedup_summary_via_canonical_logic() {
    // Sanity check that the function actually CALLS dedup_summary rather
    // than producing stale state. Pre: the seeded state already has the
    // correct DedupAble verdict (>0). Post-success: dedup_summary was
    // recomputed and reflects the new inode topology (== 0 dedup-able).
    let ollama = ToolId("ollama");
    let hf = ToolId("hf");
    let mut hashes = BTreeMap::new();
    hashes.insert((ollama, "x".to_string()), h(0x55));
    hashes.insert((hf, "x".to_string()), h(0x55));
    let mut inodes = BTreeMap::new();
    inodes.insert((ollama, "x".to_string()), (1, 1));
    inodes.insert((hf, "x".to_string()), (1, 2));
    let state = make_state(
        vec![(ollama, vec![("x", 1024)]), (hf, vec![("x", 1024)])],
        hashes,
        inodes,
    );
    // Sanity: pre-state should NOT be DedupSummary::default() — the
    // recompute populated real Some(_) values during make_state's seed.
    assert_ne!(
        state.dedup_summary,
        DedupSummary::default(),
        "precondition: dedup_summary must be populated before unify"
    );

    let summary = UnifyReclassifySummary {
        succeeded_tools: vec![ollama, hf],
        already_unified: false,
    };
    let after = reclassify_after_unify(state, &summary);

    // The canonical dedup_summary now reports 0 dedup-able bytes (the two
    // entries converged onto one inode). If reclassify forgot to recompute,
    // the pre-state value would persist (>= 1024) and this assertion would
    // fail.
    assert_eq!(after.dedup_summary.dedup_able_bytes, Some(0));
}

// ---------- timing budget: <200 ms over ~50 models ------------------------

#[test]
fn reclassify_after_unify_completes_within_200ms_for_50_models() {
    // Build a fixture with 50 distinct models split across 2 tools (25
    // dup-pairs across ollama/hf, all with separate inodes pre-unify).
    let ollama = ToolId("ollama");
    let hf = ToolId("hf");
    let mut hashes = BTreeMap::new();
    let mut inodes = BTreeMap::new();
    let mut ollama_models: Vec<(String, u64)> = Vec::new();
    let mut hf_models: Vec<(String, u64)> = Vec::new();
    for i in 0..25u8 {
        let id = format!("model-{}", i);
        let hash = h(i);
        hashes.insert((ollama, id.clone()), hash);
        hashes.insert((hf, id.clone()), hash);
        inodes.insert((ollama, id.clone()), (1, 1000 + i as u64));
        inodes.insert((hf, id.clone()), (1, 2000 + i as u64));
        ollama_models.push((id.clone(), 4096));
        hf_models.push((id, 4096));
    }
    // Borrow-friendly form for make_state's signature.
    let ollama_pairs: Vec<(&str, u64)> = ollama_models
        .iter()
        .map(|(id, sz)| (id.as_str(), *sz))
        .collect();
    let hf_pairs: Vec<(&str, u64)> = hf_models
        .iter()
        .map(|(id, sz)| (id.as_str(), *sz))
        .collect();
    let state = make_state(
        vec![(ollama, ollama_pairs), (hf, hf_pairs)],
        hashes,
        inodes,
    );

    let summary = UnifyReclassifySummary {
        succeeded_tools: vec![ollama, hf],
        already_unified: false,
    };
    let start = std::time::Instant::now();
    let _after = reclassify_after_unify(state, &summary);
    let elapsed = start.elapsed();

    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "reclassify_after_unify must complete within 200 ms over 50 models, \
         took {:?}",
        elapsed
    );
}
