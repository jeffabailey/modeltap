//! Right-pane filtered view for the `[All Unified]` synthetic slot
//! (step 04-02 of cross-tool-model-unify).
//!
//! When the user selects the `[All Unified]` row in the left pane, the right
//! pane dispatches here instead of the per-tool model list. We render:
//!
//! - Header: `Models in [All Unified] (N)` (N = unified-row count)
//! - Body:   one row per `UnifiedRow` returned by `collect_unified_rows`,
//!   formatted as `<name>  <size>  <N tools>  saves <X.Y GB>`.
//! - Footer: `Unified: N models | Total reclaimed by unification: X.Y GB`.
//!
//! The view function is split into a pure `view_lines(rows)` (testable
//! without ratatui) and a `render(frame, area, state)` widget wrapper that
//! the right-pane dispatch calls. This mirrors the
//! `render::summary_bar::summary_text` / `render::last_action::view_lines`
//! pattern already used elsewhere in the crate.
//!
//! AC-CONS-2 invariant (left-pane badge count == footer count == row count)
//! is satisfied by computing both the badge and the footer from the same
//! `collect_unified_rows` source. The runtime check lives in the us_u7
//! acceptance suite (unignores at 04-03); this module pins the row + footer
//! formats so that suite has a stable contract to assert against.

use std::path::PathBuf;

use modeltap_core::domain::dedup_summary::UnifiedRow;
use modeltap_core::logic::compatibility::{Inventory, InventoryEntry};
use modeltap_core::logic::dedup::{collect_unified_rows, InodeMap};
use modeltap_core::types::{DisplayLabel, Format, ModelStatus};
use modeltap_core::DiscoveredModel;
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app_state::AppState;

/// Format the `[All Unified]` view into a flat list of lines: header, one
/// per body row, separator (blank), footer. Pure function — no ratatui, no
/// I/O. Direct unit-test target so the row + footer formats can be pinned
/// without spinning up `TestBackend`.
///
/// Row format: `<name>  <size>  <N tools>  saves <X.Y GB>`.
/// Footer format: `Unified: N models | Total reclaimed by unification: X.Y GB`.
pub fn view_lines(rows: &[UnifiedRow]) -> Vec<String> {
    let count = rows.len();
    let total_saved: u64 = rows.iter().map(|r| r.saves_bytes).sum();

    let mut lines: Vec<String> = Vec::with_capacity(rows.len() + 3);
    lines.push(format!("Models in [All Unified] ({})", count));
    for row in rows {
        lines.push(format_row(row));
    }
    lines.push(String::new());
    lines.push(format!(
        "Unified: {} models | Total reclaimed by unification: {}",
        count,
        format_size(total_saved),
    ));
    lines
}

/// Render the `[All Unified]` view into `area`. Pulls the inventory from
/// `state` (mirroring `right_pane::build_dedup_inventory`), invokes the
/// pure `collect_unified_rows`, then paints `view_lines` into a bordered
/// Paragraph. Called by `right_pane::render` when the selected slot is
/// synthetic.
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let rows = collect_rows_from_state(state);
    let lines = view_lines(&rows);

    let title = "[All Unified]";
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Render line-by-line so we can take advantage of ratatui's built-in
    // wrapping defaults (no wrap — truncate at width). One Paragraph per
    // row keeps each line addressable for the us_u7 acceptance suite's
    // frame-text scan.
    for (i, line) in lines.iter().enumerate() {
        if (i as u16) >= inner.height {
            break;
        }
        let row_area = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        frame.render_widget(Paragraph::new(line.clone()), row_area);
    }
}

/// Build the unified-row vec by mirroring the same Inventory/InodeMap shape
/// the right-pane dedup-glyph classifier uses (`right_pane::build_dedup_inventory`).
/// Pure assembly — no I/O. Public so the left-pane synthetic-slot badge
/// renderer can derive its `(N)` count from the same source as the right-
/// pane footer (AC-CONS-2 single source of truth).
pub fn collect_rows_from_state(state: &AppState) -> Vec<UnifiedRow> {
    let inventory = build_inventory(state);
    let inodes: InodeMap = state
        .hash_state
        .inodes
        .iter()
        .map(|((tool, id), devino)| ((*tool, id.clone()), *devino))
        .collect();
    collect_unified_rows(&inventory, &inodes)
}

/// Construct a thin `Inventory` from `state.real_tools_iter()`. Same
/// pattern as `right_pane::build_dedup_inventory` — only the fields that
/// the dedup logic inspects are populated; the rest get sensible defaults.
fn build_inventory(state: &AppState) -> Inventory {
    let mut entries: Vec<InventoryEntry> = Vec::new();
    for view in state.real_tools_iter() {
        for (idx, id) in view.model_ids.iter().enumerate() {
            let size = view.model_sizes_bytes.get(idx).copied().unwrap_or(0);
            let key = (view.tool, id.clone());
            let content_hash = state.hash_state.completed_hashes.get(&key).copied();
            entries.push(InventoryEntry {
                tool: view.tool,
                model: DiscoveredModel {
                    id_in_tool: id.clone(),
                    on_disk_path: PathBuf::new(),
                    size_bytes: size,
                    format: Format::Other,
                    display_label: DisplayLabel::from(id.as_str()),
                    status: ModelStatus::Healthy,
                },
                content_hash,
            });
        }
    }
    Inventory { entries }
}

/// Format one body row: `<name>  <size>  <N tools>  saves <X.Y GB>`.
/// The display name is taken from `display_label` (falls back to
/// `model_id_in_tool` only when the label is empty); the size and saves
/// use the same `format_size` helper as the rest of the render layer.
fn format_row(row: &UnifiedRow) -> String {
    let name = if row.display_label.0.is_empty() {
        row.model_id_in_tool.as_str()
    } else {
        row.display_label.0.as_str()
    };
    format!(
        "{}  {}  {} tools  saves {}",
        name,
        format_size(row.size_bytes),
        row.tools_sharing.len(),
        format_size(row.saves_bytes),
    )
}

/// Display-formatter for byte counts — identical contract to
/// `right_pane::format_size` and `summary_bar::format_bytes`. Inlined to
/// avoid a cross-module dep.
fn format_size(bytes: u64) -> String {
    const GB: u64 = 1_000_000_000;
    const MB: u64 = 1_000_000;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{} B", bytes)
    }
}
