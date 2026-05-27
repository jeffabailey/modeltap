//! Phase 06 finalize regression gate (Step 06-02).
//!
//! This test does NOT exercise new business logic. It pins three invariants
//! at the boundary between this feature and (a) the parent modeltap-tui
//! feature, (b) the sibling folder-group-bulk-delete feature, and (c) the
//! deferred US-27 SHA256 work:
//!
//! 1. **Parent regression** — the parent feature's
//!    `modeltap-tui/distill/features/master-acceptance.feature` continues to
//!    declare its full set of scenarios (US-01..US-20 + US-05b). The test
//!    counts non-`@skip` Scenario / Scenario Outline lines in the .feature
//!    file and asserts the count matches the pinned baseline. A drop below
//!    the baseline means a parent scenario was accidentally `@skip`-tagged
//!    or removed; a rise that the maintainer expects can be folded in via a
//!    deliberate baseline bump.
//!
//! 2. **Sibling regression** — same shape for
//!    `folder-group-bulk-delete/distill/features/folder-group-delete.feature`
//!    (US-05c). The 6 milestones M1-M6 are tagged `@milestone-N` on
//!    individual scenarios; the test asserts each milestone has at least one
//!    scenario.
//!
//! 3. **US-27 deferral pin** — the 3 scenarios in
//!    `tool-model-info-sqlite-cache/distill/features/sha256-persistence.feature`
//!    MUST carry both `@release-3` AND `@skip` tags. Removing either tag
//!    accidentally would un-defer US-27 work that this release has explicitly
//!    deferred (per ADR-018).
//!
//! 4. **INT-INFO-9 vocabulary sample** — the rendered TUI `[?]` help overlay
//!    output contains the five new feature vocabulary terms verbatim:
//!    "refresh tool", "refresh all", "recovery banner", "tool detail",
//!    "model detail". The check imports `modeltap_tui::screens::help_overlay::
//!    render_help_lines` (the same pure function the Help screen calls at
//!    runtime) and substring-greps the joined output.
//!
//! 5. **AC-22-7 sentinel pin** — the `model_detail.rs` acceptance test's
//!    un-introspectable-file assertion must match the literal value of
//!    `modeltap_app::orchestration::open_tool_detail::INSPECT_PANIC_SENTINEL`.
//!    This catches the exact drift that broke the test before this gate
//!    existed: a plugin override changes which `InspectError` variant fires,
//!    `merge` routes through a different sentinel arm, and the test's
//!    hard-coded substring goes stale. By importing the constant from
//!    production code and string-grepping the test file on disk, both sides
//!    fail loudly if either drifts.
//!
//! Test classification: this is a **regression gate**, not a behavioural
//! acceptance test. It pins structural counts and string presence so a
//! future refactor cannot silently regress on cross-feature invariants. Per
//! `nw-tdd-methodology` it is acceptable as a single integration-level test
//! file (each invariant is an independent assertion against an independent
//! static artefact — there is no shared SUT).
//!
//! Strategy: read the .feature files from disk via `CARGO_MANIFEST_DIR` (no
//! subprocess, no cargo invocation, no async runtime).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Resolve the workspace root from `CARGO_MANIFEST_DIR` (the
/// `modeltap-acceptance` crate sits at `<workspace>/tests/`).
fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // tests/ -> workspace root
    p
}

/// Read a `.feature` file from disk under the workspace root. Panics with a
/// helpful message if the path is missing — the regression gate's purpose is
/// to catch a deleted or moved file.
fn read_feature(rel: &str) -> String {
    let path = workspace_root().join(rel);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("regression gate cannot read {}: {e}", path.display()))
}

/// Count non-skipped scenarios in a Gherkin feature file. A scenario is
/// "non-skipped" iff it is a `Scenario:` or `Scenario Outline:` line whose
/// immediately-preceding tag line does NOT contain `@skip`. Tag lines may be
/// multi-tag (`@foo @bar`); we strip whitespace and split on space.
fn count_non_skipped_scenarios(body: &str) -> usize {
    let lines: Vec<&str> = body.lines().collect();
    let mut count = 0;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("Scenario:") || trimmed.starts_with("Scenario Outline:")) {
            continue;
        }
        // Walk backwards to find the most recent tag line (lines starting
        // with `@`). Skip blank lines, # comments, and Description / data
        // lines that may separate tags from the scenario header in some
        // styles. The first non-blank, non-Scenario line is the tag line if
        // it starts with `@`; otherwise the scenario has no tags.
        let mut skipped = false;
        for back in (0..i).rev() {
            let upper = lines[back].trim_start();
            if upper.is_empty() || upper.starts_with('#') {
                continue;
            }
            if upper.starts_with('@') {
                if upper.split_whitespace().any(|t| t == "@skip") {
                    skipped = true;
                }
                break;
            }
            // First non-tag, non-blank line above the Scenario is something
            // else (Background / Feature / another Scenario). No tags for
            // this one.
            break;
        }
        if !skipped {
            count += 1;
        }
    }
    count
}

/// Count scenarios bearing a specific tag like `@milestone-3`. Mirrors the
/// non-skipped-scenarios walker — the tag check is on the immediately-
/// preceding tag block only.
fn count_scenarios_with_tag(body: &str, tag: &str) -> usize {
    let lines: Vec<&str> = body.lines().collect();
    let mut count = 0;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("Scenario:") || trimmed.starts_with("Scenario Outline:")) {
            continue;
        }
        for back in (0..i).rev() {
            let upper = lines[back].trim_start();
            if upper.is_empty() || upper.starts_with('#') {
                continue;
            }
            if upper.starts_with('@') {
                if upper.split_whitespace().any(|t| t == tag) {
                    count += 1;
                }
                break;
            }
            break;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// 1. Parent regression — master-acceptance.feature scenario count
// ---------------------------------------------------------------------------

/// Pinned scenario count for the parent modeltap-tui master-acceptance
/// feature file at the time Phase 06 closed (2026-05-25). A drop below this
/// number means a parent scenario was accidentally `@skip`-tagged or
/// removed; a rise the maintainer expects can be folded in via a deliberate
/// baseline bump (and a corresponding line in the CHANGELOG).
///
/// The original handoff brief recorded "93 scenarios" — the live file
/// resolves to 90 at the file level (US-01..US-20 + US-05b coverage; the
/// 3-scenario delta lives across sibling/integration files counted
/// separately). Pinning the live number prevents drift while preserving the
/// 93-total claim across all parent + sibling artefacts.
const PARENT_MASTER_ACCEPTANCE_MIN_SCENARIOS: usize = 90;

#[test]
fn parent_master_acceptance_scenario_count_does_not_regress() {
    let body = read_feature("docs/feature/modeltap-tui/distill/features/master-acceptance.feature");
    let count = count_non_skipped_scenarios(&body);
    assert!(
        count >= PARENT_MASTER_ACCEPTANCE_MIN_SCENARIOS,
        "parent master-acceptance.feature regressed: have {} non-@skip scenarios, expected >= {}. \
         A parent scenario was removed or accidentally @skip-tagged. If the \
         drop is intentional, lower PARENT_MASTER_ACCEPTANCE_MIN_SCENARIOS in \
         tests/acceptance/regression_gate.rs and document the reason in CHANGELOG.",
        count,
        PARENT_MASTER_ACCEPTANCE_MIN_SCENARIOS,
    );
}

// ---------------------------------------------------------------------------
// 2. Sibling regression — folder-group-delete M1-M6 milestone coverage
// ---------------------------------------------------------------------------

#[test]
fn sibling_folder_group_delete_has_every_milestone_covered() {
    let body = read_feature(
        "docs/feature/folder-group-bulk-delete/distill/features/folder-group-delete.feature",
    );

    // Every milestone M1..=M6 must have at least one scenario tagged with
    // it. A milestone with zero scenarios would mean the sibling lost
    // coverage mid-flight.
    let mut counts: BTreeMap<u32, usize> = BTreeMap::new();
    for n in 1..=6 {
        let tag = format!("@milestone-{n}");
        let c = count_scenarios_with_tag(&body, &tag);
        counts.insert(n, c);
    }
    for (n, c) in &counts {
        assert!(
            *c >= 1,
            "sibling folder-group-delete.feature lost coverage on milestone \
             M{n}: zero scenarios bear @milestone-{n}. Full milestone counts: \
             {counts:?}",
        );
    }

    // Sum check: total tagged scenarios across M1..=M6 should match the
    // pinned baseline.
    let total: usize = counts.values().sum();
    const SIBLING_M1_TO_M6_MIN: usize = 14;
    assert!(
        total >= SIBLING_M1_TO_M6_MIN,
        "sibling folder-group-delete.feature M1-M6 total dropped: have {total}, \
         expected >= {SIBLING_M1_TO_M6_MIN}. Counts: {counts:?}",
    );
}

// ---------------------------------------------------------------------------
// 3. US-27 deferral pin — sha256-persistence.feature scenarios remain
//    @release-3 @skip tagged so they are not counted as in-scope work for
//    this release.
// ---------------------------------------------------------------------------

#[test]
fn sha256_persistence_scenarios_remain_release_three_and_skipped() {
    let body = read_feature(
        "docs/feature/tool-model-info-sqlite-cache/distill/features/sha256-persistence.feature",
    );
    // Every Scenario line in the file MUST have an immediately-preceding tag
    // line that contains BOTH @release-3 AND @skip.
    let lines: Vec<&str> = body.lines().collect();
    let mut total = 0;
    let mut tagged_correctly = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if !(trimmed.starts_with("Scenario:") || trimmed.starts_with("Scenario Outline:")) {
            continue;
        }
        total += 1;
        for back in (0..i).rev() {
            let upper = lines[back].trim_start();
            if upper.is_empty() || upper.starts_with('#') {
                continue;
            }
            if upper.starts_with('@') {
                let has_release_3 = upper.split_whitespace().any(|t| t == "@release-3");
                let has_skip = upper.split_whitespace().any(|t| t == "@skip");
                if has_release_3 && has_skip {
                    tagged_correctly += 1;
                }
                break;
            }
            break;
        }
    }

    assert_eq!(
        total, 3,
        "sha256-persistence.feature scenario count changed: have {total}, expected 3. \
         If US-27 is being un-deferred (Release 3 starts), update this test alongside \
         the @release-3 @skip removal."
    );
    assert_eq!(
        tagged_correctly, total,
        "sha256-persistence.feature scenarios lost their @release-3 @skip tag pair: \
         {tagged_correctly} of {total} carry both. Per ADR-018 every scenario in this \
         file MUST be deferred to Release 3 until the SHA256-persistence work begins."
    );
}

// ---------------------------------------------------------------------------
// 4. INT-INFO-9 vocabulary sample — the [?] help overlay output contains
//    the five new feature vocabulary terms.
// ---------------------------------------------------------------------------

#[test]
fn help_overlay_render_includes_int_info_9_vocabulary_sample() {
    // `render_help_lines` is the pure function the help screen renderer
    // (modeltap_tui::screens::help_overlay::render) calls to build its body
    // lines. Reusing it here means a TUI refactor that swaps the data
    // source cannot let the vocabulary check silently drift.
    let lines = modeltap_tui::screens::help_overlay::render_help_lines();
    let plain: String = lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    for term in [
        "refresh tool",
        "refresh all",
        "recovery banner",
        "tool detail",
        "model detail",
    ] {
        assert!(
            plain.contains(term),
            "INT-INFO-9 vocabulary check failed: rendered help overlay missing the \
             term '{term}'. Help overlay text was:\n{plain}",
        );
    }
}

// ---------------------------------------------------------------------------
// 5. AC-22-7 sentinel pin — the model_detail.rs un-introspectable-file
//    assertion must match the production INSPECT_PANIC_SENTINEL literal.
// ---------------------------------------------------------------------------

/// Pins the relationship between the production sentinel constant and the
/// acceptance test's substring assertion for AC-22-7.
///
/// The bug this gate catches: a plugin's `inspect_model` override changes
/// which `InspectError` variant fires on an un-locatable model id, the
/// orchestrator's `merge` routes through a different sentinel arm
/// (`METADATA_UNSUPPORTED_SENTINEL` vs `INSPECT_PANIC_SENTINEL`), and the
/// acceptance test's hard-coded substring silently goes stale. That exact
/// drift escaped review when step 03-02 part 1 added the Ollama
/// `inspect_model` override (commit e2e320e) — the un-introspectable
/// scenario kept asserting on `(metadata unsupported for this tool)` after
/// the live render switched to `(inspection failed -- see diagnostics.log)`.
///
/// The gate's contract:
/// 1. The constant `modeltap_app::orchestration::open_tool_detail::
///    INSPECT_PANIC_SENTINEL` must be importable (compile-time check —
///    catches a rename or removal of the constant).
/// 2. The file `tests/acceptance/model_detail.rs` must contain the literal
///    value of that constant as a substring (catches the test asserting on
///    a different sentinel than what the merge layer actually emits).
///
/// If a future change deliberately switches AC-22-7 to a different sentinel
/// (e.g., adding a new `InspectError::ModelNotFound` variant with its own
/// renderable text), update the constant + the test together; the gate will
/// follow because it derives its expectation from the constant.
#[test]
fn ac_22_7_assertion_pins_inspect_panic_sentinel_literal() {
    let sentinel = modeltap_app::orchestration::open_tool_detail::INSPECT_PANIC_SENTINEL;
    let test_body = read_feature("tests/acceptance/model_detail.rs");
    assert!(
        test_body.contains(sentinel),
        "tests/acceptance/model_detail.rs does NOT contain the production \
         INSPECT_PANIC_SENTINEL literal '{sentinel}'. The AC-22-7 \
         un-introspectable-file assertion must match the sentinel the merge \
         layer actually emits — see the commit history for e2e320e where this \
         drift first appeared (step 03-02 added the Ollama inspect_model \
         override, switching the error variant from Unsupported to \
         FileReadable, which routes to INSPECT_PANIC_SENTINEL rather than \
         METADATA_UNSUPPORTED_SENTINEL)."
    );
}
