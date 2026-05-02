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
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app_state::AppState;
use crate::render::bytes::format_bytes;

/// US-U8 (step 06-01): empty-state guidance lines for the `[All Unified]`
/// right pane. Returned when `collect_unified_rows` is empty so the caller
/// (`render`) can paint actionable copy instead of a blank body.
///
/// Branches:
///   - `hashing_complete == true` → onboarding text inviting the user to
///     find a "=" row and press [u]. AC-U8.1 + AC-U8.3.
///   - `hashing_complete == false` → "Hashing in progress" message. AC-U8.2
///     (honest UI: don't tell the user "no models" before we know).
///
/// Pure function — no `state`, no I/O. The boolean parameter is the only
/// disambiguator so the caller decides which branch to render based on
/// `state.hash_state.is_complete()`.
pub fn empty_state_lines(hashing_complete: bool) -> Vec<String> {
    if hashing_complete {
        vec![String::from(
            "No models are unified yet. Navigate to a tool, find a row \
                 marked \"=\", and press [u] to unify it.",
        )]
    } else {
        vec![String::from(
            "Hashing in progress. Unified models will appear here as soon \
             as hashing completes.",
        )]
    }
}

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
        format_bytes(total_saved),
    ));
    lines
}

/// Render the `[All Unified]` view into `area`. Pulls the inventory from
/// `state` (mirroring `right_pane::build_dedup_inventory`), invokes the
/// pure `collect_unified_rows`, then paints either the row-list view
/// (`view_lines`) or — when there are zero unified groups — the US-U8
/// empty-state guidance (`empty_state_lines`). Header + footer remain
/// painted so the user always sees `Models in [All Unified] (0)` and
/// `Unified: 0 models | Total reclaimed by unification: 0 B`; the body
/// section is what changes shape between the two states.
///
/// Called by `right_pane::render` when the selected slot is synthetic.
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let rows = collect_rows_from_state(state);

    let title = "[All Unified]";
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if rows.is_empty() {
        // US-U8 (step 06-01): empty-state body. Header + footer match the
        // row-list shape (both come from `view_lines(&[])` → `[header, "",
        // footer]`); the body section is replaced with onboarding OR
        // hashing-in-progress guidance, wrapped to the inner width so the
        // full sentence (incl. the actionable `[u]` suffix) is visible.
        render_empty_state(frame, inner, state);
        return;
    }

    let lines = view_lines(&rows);
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

/// Paint the empty-state shape: single-row header, multi-row wrapping
/// guidance body, single-row blank, single-row footer. The guidance lines
/// are rendered with `Wrap { trim: false }` so the full message — including
/// the actionable `press [u]` hint at the tail — is visible on right-pane
/// widths around 80 cols (default test fixture has a ~82-col right pane).
fn render_empty_state(frame: &mut Frame<'_>, inner: Rect, state: &AppState) {
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    // Header + footer both come from `view_lines(&[])` so the empty branch
    // shape stays in lock-step with the row-list footer/header format.
    let frame_lines = view_lines(&[]);
    let header_line = frame_lines.first().cloned().unwrap_or_default();
    let footer_line = frame_lines.last().cloned().unwrap_or_default();

    // Row 0: header.
    let header_area = Rect::new(inner.x, inner.y, inner.width, 1);
    frame.render_widget(Paragraph::new(header_line), header_area);

    // Rows 1..(inner.height - 2): guidance body (wrapped). Reserve the
    // last two rows for blank + footer when we have the vertical room.
    let footer_visible = inner.height >= 3;
    let body_y = inner.y.saturating_add(1);
    let body_height: u16 = if footer_visible {
        // Reserve 1 row for blank separator + 1 for footer = 2 rows.
        inner.height.saturating_sub(3)
    } else {
        inner.height.saturating_sub(1)
    };
    if body_height > 0 {
        let guidance_lines = empty_state_lines(state.hash_state.is_complete());
        let guidance_text = guidance_lines.join("\n");
        let body_area = Rect::new(inner.x, body_y, inner.width, body_height);
        frame.render_widget(
            Paragraph::new(guidance_text).wrap(Wrap { trim: false }),
            body_area,
        );
    }

    if footer_visible {
        // Row (inner.height - 1): footer (the `view_lines(&[])` separator
        // line at index 1 stays implicit — the blank-row gap between
        // body and footer is enforced by reserving an empty row.
        let footer_area = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
        frame.render_widget(Paragraph::new(footer_line), footer_area);
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
/// use the canonical `render::bytes::format_bytes` helper.
fn format_row(row: &UnifiedRow) -> String {
    let name = if row.display_label.0.is_empty() {
        row.model_id_in_tool.as_str()
    } else {
        row.display_label.0.as_str()
    };
    format!(
        "{}  {}  {} tools  saves {}",
        name,
        format_bytes(row.size_bytes),
        row.tools_sharing.len(),
        format_bytes(row.saves_bytes),
    )
}
