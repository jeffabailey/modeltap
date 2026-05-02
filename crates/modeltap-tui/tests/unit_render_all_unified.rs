//! Unit tests for `render::all_unified::view_lines` — the pure-function
//! source of truth for the `[All Unified]` right-pane layout (step 04-02 of
//! cross-tool-model-unify).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: empty rows → header + footer "Unified: 0 models | Total reclaimed
//!         by unification: 0 B" (no body rows)
//!     B2: single unified group → 1 body row + footer aggregates the single
//!         group's saves
//!     B3: two unified groups → 2 body rows + footer aggregates both groups'
//!         saves correctly
//!   budget = 3 × 2 = 6 unit tests max. We use 3.
//!
//! Each test enters through:
//!   - `render::all_unified::view_lines` — pure render fn (driving port).
//!     Mirrors the pattern established by `render::summary_bar::summary_text`
//!     and `render::last_action::view_lines`.
//!
//! AC-U7 derivation: the right-pane filtered view shows one row per cross-
//! tool unified group with `<name>  <size>  <N tools>  saves <X.Y GB>` and
//! a footer summing model count + total reclaimed bytes. The row format
//! and footer format are pinned here so the dedicated us_u7 acceptance tests
//! (which unignore at 04-03) can rely on a stable rendering contract.

use modeltap_core::domain::dedup_summary::UnifiedRow;
use modeltap_core::{DisplayLabel, ToolId};
use modeltap_tui::render::all_unified;

fn unified_row(id: &str, label: &str, size_bytes: u64, tools: &[&'static str]) -> UnifiedRow {
    let tools_sharing: Vec<ToolId> = tools.iter().map(|t| ToolId(t)).collect();
    let saves_bytes = (tools_sharing.len() as u64).saturating_sub(1) * size_bytes;
    UnifiedRow {
        model_id_in_tool: id.to_string(),
        display_label: DisplayLabel::from(label),
        size_bytes,
        tools_sharing,
        saves_bytes,
    }
}

// ---------------------------------------------------------------------------
// B1 — empty rows produce a footer with zero models and zero reclaimed bytes
// ---------------------------------------------------------------------------

#[test]
fn empty_rows_renders_header_and_zero_footer() {
    let lines = all_unified::view_lines(&[]);

    let joined = lines.join("\n");
    assert!(
        joined.contains("[All Unified]"),
        "expected header to mention [All Unified], got:\n{joined}"
    );
    assert!(
        joined.contains("Unified: 0 models"),
        "expected footer to report `Unified: 0 models`, got:\n{joined}"
    );
    assert!(
        joined.contains("Total reclaimed by unification: 0 B"),
        "expected footer to report zero reclaimed, got:\n{joined}"
    );
}

// ---------------------------------------------------------------------------
// B2 — single unified group: one body row, footer shows that one group's saves
// ---------------------------------------------------------------------------

#[test]
fn single_unified_group_renders_one_row_and_aggregated_footer() {
    // Two tools sharing a 1 GB model → saves = 1 GB.
    let row = unified_row("mistral:7b", "mistral:7b", 1_000_000_000, &["ollama", "hf"]);
    let lines = all_unified::view_lines(&[row]);

    let joined = lines.join("\n");

    // Body row: contains the model name, size, tool count, and saves.
    assert!(
        joined.contains("mistral:7b"),
        "expected body row to contain model name, got:\n{joined}"
    );
    assert!(
        joined.contains("1.0 GB"),
        "expected body row to contain formatted size, got:\n{joined}"
    );
    assert!(
        joined.contains("2 tools"),
        "expected body row to report `2 tools`, got:\n{joined}"
    );
    assert!(
        joined.contains("saves 1.0 GB"),
        "expected body row to report `saves 1.0 GB`, got:\n{joined}"
    );

    // Footer: 1 model, 1.0 GB total saves.
    assert!(
        joined.contains("Unified: 1 models"),
        "expected footer count = 1, got:\n{joined}"
    );
    assert!(
        joined.contains("Total reclaimed by unification: 1.0 GB"),
        "expected footer total = 1.0 GB, got:\n{joined}"
    );
}

// ---------------------------------------------------------------------------
// B3 — two unified groups: two body rows, footer aggregates both groups' saves
// ---------------------------------------------------------------------------

#[test]
fn two_unified_groups_renders_two_rows_and_summed_footer() {
    // Group A: 3 tools sharing 2 GB → saves = (3-1)*2 GB = 4 GB.
    let row_a = unified_row(
        "llama3:8b",
        "llama3:8b",
        2_000_000_000,
        &["ollama", "hf", "lm-studio"],
    );
    // Group B: 2 tools sharing 500 MB → saves = (2-1)*500 MB = 500 MB.
    let row_b = unified_row("phi3:mini", "phi3:mini", 500_000_000, &["hf", "lm-studio"]);
    let lines = all_unified::view_lines(&[row_a, row_b]);

    let joined = lines.join("\n");

    // Both body rows present.
    assert!(
        joined.contains("llama3:8b"),
        "expected llama3 row, got:\n{joined}"
    );
    assert!(
        joined.contains("3 tools"),
        "expected llama3 row to report `3 tools`, got:\n{joined}"
    );
    assert!(
        joined.contains("saves 4.0 GB"),
        "expected llama3 saves = 4.0 GB, got:\n{joined}"
    );
    assert!(
        joined.contains("phi3:mini"),
        "expected phi3 row, got:\n{joined}"
    );
    assert!(
        joined.contains("2 tools"),
        "expected phi3 row to report `2 tools`, got:\n{joined}"
    );
    assert!(
        joined.contains("saves 500.0 MB"),
        "expected phi3 saves = 500.0 MB, got:\n{joined}"
    );

    // Footer: 2 unified models total, 4.5 GB combined reclaim.
    assert!(
        joined.contains("Unified: 2 models"),
        "expected footer count = 2, got:\n{joined}"
    );
    assert!(
        joined.contains("Total reclaimed by unification: 4.5 GB"),
        "expected footer total = 4.5 GB (4 GB + 500 MB), got:\n{joined}"
    );
}
