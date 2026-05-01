# Test Scenarios: cross-tool-model-unify

Index mapping each user story (US-UN) to its acceptance criteria, the
corresponding Gherkin scenarios in
`distill/features/master-acceptance.feature`, and the Rust integration test
files under `crates/modeltap-app/tests/acceptance/`.

All scenarios are tagged with `@us-uN` for traceability. The single
`@walking-skeleton` scenario crosses six stories by design.

---

## Walking skeleton

| Story span | Gherkin scenario | Rust file |
|---|---|---|
| US-U1..U6 | `Devon reclaims disk by unifying a duplicated model from the main view` (`@walking-skeleton`) | `acceptance/us_u1_walking_skeleton.rs::devon_reclaims_disk_by_unifying_a_duplicated_model_from_the_main_view` |

Fixture helpers (NOT scenarios; non-ignored tests in the same file):
- `walking_skeleton_fixture_produces_two_distinct_inodes_with_identical_bytes` — proves the fixture-builder is healthy.
- `walking_skeleton_smoke_post_paint_bottom_bar_renders` — tripwire for the bottom-bar text.

---

## US-U1 — Background SHA256 hashing with progress

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U1.1 | First paint completes before any hashing begins | (covered in WS scenario; explicit one is `@skip`) |
| AC-U1.2 | Hashing-progress count advances as hashes complete | `@skip` (no Rust scaffold yet — DELIVER will add) |
| AC-U1.3 | UI key handlers stay responsive while hashing runs | `@skip` (NFR-3 latency probe) |
| AC-U1.4 | Hashing completes within NFR-2 budget | `@skip @property` |
| AC-U1.5 | Quitting during hashing exits within shutdown budget | `@skip` |

Rust file: covered by the walking-skeleton scaffold for the WS slice. Per-AC
focused tests are deferred — the crafter will add them during DELIVER as
each AC is independently exercised.

---

## US-U2 — Wire dedup-able bytes from classifier to summary bar

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U2.1, AC-U2.3 | Summary bar shows "computing..." while hashing pending | `us_u2_dedup_able_bytes_wired::summary_bar_shows_computing_while_hashing_pending` |
| AC-U2.1 | Summary bar does NOT show hardcoded "0 B" | `us_u2_dedup_able_bytes_wired::summary_bar_does_not_show_hardcoded_dedup_able_zero_during_hashing` |
| AC-U2.2, AC-U2.4 | Summary bar reads from same source as row glyphs | `us_u2_dedup_able_bytes_wired::summary_bar_value_equals_sum_of_dedup_able_row_sizes` |
| AC-U2.5 | Summary bar honestly shows 0 when no duplicates | `us_u2_dedup_able_bytes_wired::summary_bar_shows_honest_zero_when_no_duplicates_after_hashing` |

---

## US-U3 — Row glyph reflects dedup state

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U3.1, AC-U3.2 (=) | Dedup-able model shows "=" glyph after hashing | `us_u3_row_dedup_glyphs::dedup_able_model_shows_equals_glyph_after_hashing` |
| AC-U3.2 (#) | Already-hardlinked shows "#", not "=" | `us_u3_row_dedup_glyphs::already_unified_model_shows_hash_glyph_not_equals` |
| AC-U3.2 (?) | Pre-hash row shows "?" glyph | `us_u3_row_dedup_glyphs::pre_hash_row_shows_pending_glyph` |
| AC-U3.2 (-) | Unique model shows "-" glyph | `us_u3_row_dedup_glyphs::unique_model_shows_dash_glyph_after_hashing` |
| AC-U3.5 | Hash failure shows "-" + "!" decorator | `us_u3_row_dedup_glyphs::hash_failure_row_shows_dash_with_bang_decorator` |
| AC-U3.4 | Glyph updates reactively (~) | (covered by Gherkin only — no dedicated Rust test; implicit in U3 family) |
| AC-U3.6 | Help screen documents legend | (deferred — help-screen scenario not yet scaffolded in Rust) |

---

## US-U4 — `u` from main view opens unify dialog with mates pre-populated

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U4.1, AC-U4.2 | u on "=" row opens dialog with mates | `us_u4_unify_from_main_view::pressing_u_on_dedup_able_row_opens_dialog_with_mates_prepopulated` |
| AC-U4.3 | u on "#" row opens informational dialog | `us_u4_unify_from_main_view::pressing_u_on_already_unified_row_opens_informational_dialog` |
| AC-U4.4 | u on "-" row shows status hint, no dialog | `us_u4_unify_from_main_view::pressing_u_on_unique_row_shows_status_hint_no_dialog` |
| AC-U4.5 | u on "?" row shows still-computing hint | `us_u4_unify_from_main_view::pressing_u_on_pending_hash_row_shows_still_computing_hint` |
| AC-U4.6 | u from Detail still works (no v1 regression) | `us_u4_unify_from_main_view::pressing_u_on_detail_screen_still_opens_dialog` |

---

## US-U5 — Unify dialog shows reclaim preview and applies plan

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U5.1 | Dialog body shows canonical, targets, savings, total | `us_u5_unify_dialog_preview_and_apply::dialog_body_shows_canonical_targets_savings_and_total_reclaim` |
| AC-U5.2, AC-U5.3 | Toggling a target updates total live | `us_u5_unify_dialog_preview_and_apply::toggling_a_target_with_space_updates_the_total_reclaim` |
| AC-U5.4 | Enter applies the plan | `us_u5_unify_dialog_preview_and_apply::pressing_enter_applies_the_plan_and_creates_hardlink` |
| AC-U5.5 | Esc cancels with no fs effect | `us_u5_unify_dialog_preview_and_apply::pressing_esc_cancels_dialog_with_no_filesystem_change` |
| AC-U5.6 | Cross-fs dialog still fires (ADR-008) | (deferred — covered by v1 us_19_cross_fs_fallback) |
| AC-U5.7 | Lsof gate still fires | (deferred — covered by v1 us_17_running_tool_detect) |

---

## US-U6 — Post-unify row glyph and summary bar update without restart

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U6.1, AC-U6.2, AC-U6.4, AC-U6.6, AC-U6.7 | Successful full unify flips glyph + summary | `us_u6_post_unify_no_restart::successful_unify_flips_glyph_and_updates_summary_bar_without_restart` |
| AC-U6.5 | Transient (was X GB) delta then collapses | `us_u6_post_unify_no_restart::summary_bar_shows_transient_delta_then_collapses` |
| AC-U6.3, AC-U6.6 | Partial unify leaves glyph "=" | `us_u6_post_unify_no_restart::partial_unify_leaves_glyph_as_equals_and_unified_count_unchanged` |

---

## US-U7 — `[All Unified]` pseudo-tool slot in left pane

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U7.1 | Slot appears in left pane below real tools | `us_u7_all_unified_pseudo_slot::all_unified_slot_appears_in_left_pane_below_real_tools` |
| AC-U7.2, AC-U7.6 | Badge count agrees with summary bar | `us_u7_all_unified_pseudo_slot::all_unified_badge_matches_summary_bar_unified_count` |
| AC-U7.3 | Selecting filters right pane to "#" rows | `us_u7_all_unified_pseudo_slot::selecting_all_unified_slot_filters_right_pane_to_hash_rows` |
| AC-U7.4, AC-U7.5 | Row format + footer aggregates | `us_u7_all_unified_pseudo_slot::all_unified_view_row_format_and_footer_aggregates_totals` |

---

## US-U8 — `[All Unified]` empty state with onboarding (P2 polish)

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U8.1, AC-U8.3 | Empty-state guidance when zero unified | `@skip` (no Rust scaffold yet — P2 release) |
| AC-U8.2 | Hashing-in-progress empty state distinct | `@skip` |

Per Luna's prioritization, US-U8/U9/U10 are R2 (polish). DELIVER will add
Rust scaffolds at the start of the R2 step.

---

## US-U9 — Detail screen for unified model shows shared inode (P2)

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U9.1 | "#" detail shows shared inode + paths | `@skip` |
| AC-U9.2 | "=" detail groups paths by inode | `@skip` |
| AC-U9.4 | Missing-inode handled gracefully | `@skip` |

---

## US-U10 — Partial-success reporting (P2)

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-U10.1, AC-U10.2, AC-U10.5 | Toast lists per-target outcomes | `@skip` |
| AC-U10.3, AC-U10.4 | "r" retries failed targets only | `@skip` |
| (total failure) | Toast shows zero reclaim, glyph stays "=" | `@skip` |

---

## Cross-artifact consistency

| AC | Gherkin scenario | Rust test |
|---|---|---|
| AC-CONS-1 | bar.dedup_able == sum(row.size where glyph='=') | (asserted inside US-U2 + WS scenarios) |
| AC-CONS-2 | unified_count parity across surfaces | (asserted inside US-U7 scenarios) |
| AC-CONS-3 | Pane-switch invariance for same model glyph | `@skip @property` (no Rust scaffold yet) |
| AC-CONS-4 | bar.dedup_able_delta == toast.reclaimed_bytes | (asserted inside WS + US-U6 scenarios) |
| AC-CONS-5 | Hashing progress monotonic | `@skip @property` |

---

## Coverage summary

- **All 10 stories US-U1..U10 have at least one Gherkin scenario.**
- **All 7 P1 stories US-U1..U7 have at least one Rust integration test scaffold.** US-U1 is covered by the walking-skeleton file (which crosses U1..U6); the per-AC focused U1 scenarios are Gherkin-only `@skip`s.
- **P2 stories (US-U8, U9, U10) are Gherkin-only `@skip`s.** Rust scaffolds will be written when DELIVER begins the R2 release.
- **Total Gherkin scenarios: 43.** **Walking-skeleton scenarios: 1.** **Skipped scenarios: 42.**
- **Total ignored Rust RED tests: 26.** **Non-ignored helper tests in WS file: 2** (fixture-builder sanity).
