# Walking Skeleton: cross-tool-model-unify

## What it proves end-to-end

The walking-skeleton scenario is **"Devon reclaims disk by unifying a
duplicated model from the main view"**. It is the smallest end-to-end slice
that proves the v1 promise becomes true: a stakeholder can watch Devon
launch modeltap with two tools holding byte-identical copies of one model
on separate inodes, see the summary bar honestly say "Dedup-able:
computing..." (instead of the v1 hardcoded "0 B" lie), see hashing finish
and the bar update with the duplicated bytes, see the row glyph become
"=", press [u] **from the main view** (not Detail) to open the unify
dialog with the second tool's copy pre-populated as a target, press Enter
to apply the plan, and watch the two copies collapse onto a single inode
while the summary bar's "Dedup-able" decreases by exactly the model size
and "Unified" increments by 1 — all without restarting the program.

This single scenario crosses six P1 stories by design (US-U1 background
hashing, US-U2 dedup-able wiring, US-U3 row glyph, US-U4 u-from-main-view,
US-U5 dialog applies plan, US-U6 post-unify update). That spread is
intentional: walking skeletons are vertical slices that prove the seam at
every layer simultaneously, not horizontal coverage of any one layer.
Per Luna's prioritization, the walking skeleton release is US-U1..U7;
the WS scenario itself is the smallest demonstrable subset of that release.

## Demo-ability

A non-technical stakeholder can confirm "yes, that is what Devon needs":

1. Watch Devon launch the tool. (1 second, K3 budget.)
2. See the summary bar say "computing..." for ~10 seconds.
3. See it become "Dedup-able: 4 KB" (or whatever the fixture's model size).
4. See the duplicated row light up with a "=" glyph.
5. Watch Devon press one key, then Enter on a confirmation dialog.
6. See "Dedup-able: 0 B" and "Unified: 1 model" appear, in the same session.

No Rust types in the demo. No reading code. The stakeholder confirms by
watching disk and the summary bar update.

## What it does NOT prove

- **NFR-2 60 s p95 budget**: the WS uses a 4 KB fixture; full hashing-budget
  validation is a separate `@skip` scenario per US-U1 AC-U1.4.
- **NFR-3 100 ms key-handler latency during hashing**: separate `@skip`
  scenario per US-U1 AC-U1.3.
- **Cross-fs fallback (ADR-008)**: covered by v1's existing
  `us_19_cross_fs_fallback` integration test.
- **Lsof / running-tool detection**: covered by v1's existing
  `us_17_running_tool_detect` integration test.
- **Partial-success path**: separate US-U6 scenario.
- **`[All Unified]` slot**: separate US-U7 scenarios; not on the WS path.

## Walking-skeleton checklist

The DELIVER wave ships the walking skeleton when this list is green:

- [ ] Production code: `modeltap-app::hash_pool` module exists and is
      spawned from the composition root after first paint (per ADR-013).
- [ ] Production code: `crates/modeltap-tui/src/render/summary_bar.rs:36`
      no longer contains the hardcoded `"Dedup-able: 0 B"` literal; the
      bar reads from `core::logic::dedup` aggregates carried on
      `AppState.dedup_summary` (per US-U2 AC-U2.1).
- [ ] Production code: `core::logic::dedup::compute_dedup_glyph(...)` is
      a pure function returning `DedupGlyph::DedupAble` for cross-tool
      duplicates on separate inodes and `AlreadyUnified` for shared inodes
      (per data-models.md).
- [ ] Production code: `render::row` paints the dedup-glyph column reading
      from a per-row `DedupGlyph` (per component-boundaries.md).
- [ ] Production code: `keymap::dispatch` routes `u` from main view to
      `Msg::UnifyHighlighted`, which the update handler dispatches based on
      the highlighted row's glyph (per US-U4 AC-U4.1, AC-U4.2).
- [ ] Production code: `actions::reclassify::reclassify_after_unify`
      consumes the `UnifyOutcome` event and recomputes affected rows'
      glyphs and the summary bar within 200 ms (per US-U6 AC-U6.1).
- [ ] Test green: `crates/modeltap-app/tests/acceptance/us_u1_walking_skeleton.rs::devon_reclaims_disk_by_unifying_a_duplicated_model_from_the_main_view`
      passes after `#[ignore]` is removed.
- [ ] Test green: the two non-ignored fixture-helper tests in
      `us_u1_walking_skeleton.rs` continue to pass.
- [ ] No regression: every v1 acceptance test (`us_01..us_20`, `us_05b`,
      `us_atomic_chat_discovery`) continues to pass.
- [ ] No new env-var seam: production must not introduce any env-var
      seam not already in the existing list (see CLAUDE.md `Constraints
      DESIGN Has Already Closed`). The harness `script` may need a new
      sentinel token (e.g., `<hash-complete>`) — this is a one-line
      tokenizer addition, NOT a new env var.
- [ ] Demo: the WS scenario is recorded as an asciicast and embedded in
      the release notes for v1.x of cross-tool-model-unify.

## Why this scenario, not another

Alternatives that were considered and rejected for WS:

- **"Devon launches and sees the bar say computing..."**: only proves
  US-U2 partially. Doesn't exercise the action path. Not demonstrable as
  a feature.
- **"Devon presses u on Detail and the dialog applies"**: this is what v1
  already does; nothing about the bug-fix is exercised.
- **"Devon sees the [All Unified] slot"**: US-U7 only. Doesn't exercise
  the dedup-able number, the glyph, the dialog, or the reclaim arithmetic.
- **"Devon presses u and a hardlink is created"**: too thin — doesn't
  exercise the summary bar or the no-restart requirement, both of which
  are explicit AC.

The chosen scenario is the unique one that exercises every layer and is
demonstrable to a non-technical stakeholder in under a minute.
