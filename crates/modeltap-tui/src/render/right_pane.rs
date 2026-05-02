//! Right pane: model list for the currently-selected tool, with a header
//! "Models in <tool> (<count>, <total> GB)" and a scroll-position indicator
//! "<selected+1>/<total>" rendered in the bottom-right corner.

use std::collections::BTreeSet;
use std::path::PathBuf;

use modeltap_core::domain::{classify_by_presence, other_tools_by_presence, ToolPresence};
use modeltap_core::logic::compatibility::{Inventory, InventoryEntry};
use modeltap_core::logic::dedup::{compute_dedup_glyph, InodeMap, ModelKey};
use modeltap_core::types::{DisplayLabel, Format, ModelStatus};
use modeltap_core::{DiscoveredModel, ToolId};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app_state::{AppState, FocusPane};
use crate::render::colors::no_color_active;
use crate::render::last_action;
use crate::render::row::render_row_basic;

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let Some(tool) = state.current_tool() else {
        let block = Block::default().borders(Borders::ALL).title("Models");
        frame.render_widget(block, area);
        return;
    };

    let header = format!(
        "Models in {} ({}, {})",
        tool.tool.0,
        tool.model_ids.len(),
        format_size(tool.total_bytes()),
    );

    // Build the cross-tool presence-view once per render (US-04.AC-4).
    // Cheap to construct: clones each tool's id-string list. Synthetic slots
    // are skipped — they have no per-tool model_ids; the synthesis aggregates
    // the same real-tool data we already iterate.
    let inventory: Vec<ToolPresence> = state
        .real_tools_iter()
        .map(|t| ToolPresence {
            tool: t.tool,
            model_ids: t.model_ids.clone(),
        })
        .collect();

    // Build a transient `Inventory` for the dedup-glyph classifier (step 01-05).
    // Every entry has `content_hash: None` because the hash pool has not yet
    // been wired (lands in step 01-07). With no hashes, `compute_dedup_glyph`
    // returns `Pending` for every row, which is the correct first-paint state.
    // After 01-07 wires real hashes the same call site will start producing
    // `Hashing`/`Unique`/`DedupAble`/`AlreadyUnified`/`Failed`.
    let dedup_inventory = build_dedup_inventory(state);
    // No inode map and no in-progress/failed hashes at first paint. The
    // hash-pool wiring (01-07) will populate these.
    let dedup_inodes: InodeMap = InodeMap::default();
    let in_progress: BTreeSet<ModelKey> = state
        .hash_state
        .in_progress
        .iter()
        .flat_map(|id| {
            // Map each id-string to all (tool, id) keys that match across the
            // current inventory. The hash-pool design (data-models.md) keys by
            // raw id-string only; the classifier wants (tool, id). When the
            // pool lands in 01-07 this set will be re-keyed as ModelKey
            // upstream and this conversion will go away.
            dedup_inventory
                .entries
                .iter()
                .filter(move |e| &e.model.id_in_tool == id)
                .map(|e| (e.tool, e.model.id_in_tool.clone()))
        })
        .collect();
    let failed_keys: BTreeSet<ModelKey> = state
        .hash_state
        .failed
        .iter()
        .flat_map(|id| {
            dedup_inventory
                .entries
                .iter()
                .filter(move |e| &e.model.id_in_tool == id)
                .map(|e| (e.tool, e.model.id_in_tool.clone()))
        })
        .collect();

    let no_color = no_color_active();

    // Visible window for rows.
    let visible = state.visible_rows.max(1);
    let total_rows = tool.model_ids.len();
    let start = state.scroll_offset.min(total_rows.saturating_sub(1));
    let end = (start + visible).min(total_rows);
    let rows = if total_rows == 0 {
        Vec::new()
    } else {
        (start..end)
            .map(|i| {
                let id = &tool.model_ids[i];
                let size = tool.model_sizes_bytes.get(i).copied().unwrap_or(0);
                let indicator = classify_by_presence(id, &inventory);
                let also_in = other_tools_by_presence(id, tool.tool, &inventory);
                let dedup = dedup_glyph_for_row(
                    tool.tool,
                    id,
                    size,
                    &dedup_inventory,
                    &dedup_inodes,
                    &in_progress,
                    &failed_keys,
                );
                let mut line = render_row_basic(id, size, indicator, &also_in, dedup, no_color);
                if i == state.selected_row {
                    let mut style = Style::default().add_modifier(Modifier::REVERSED);
                    if state.focus == FocusPane::Right {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    // Apply the selection style on top of any per-span style
                    // (e.g., the colored indicator glyph) so the highlight is
                    // visible without losing the indicator's color cue.
                    line = line.patch_style(style);
                }
                ListItem::new(line)
            })
            .collect::<Vec<_>>()
    };

    let title = if matches!(state.focus, FocusPane::Right) {
        format!("{header} [focused]")
    } else {
        header
    };

    let block = Block::default().borders(Borders::ALL).title(title);
    let inner_area = block.inner(area);
    frame.render_widget(block, area);
    let widget = List::new(rows);
    frame.render_widget(widget, inner_area);

    // Scroll-position indicator in the bottom-right corner. Format:
    // "<selected+1>/<total>". Only rendered when there are rows.
    if total_rows > 0 {
        let label = format!("{}/{}", state.selected_row + 1, total_rows);
        let label_w = label.len() as u16;
        if inner_area.width >= label_w && inner_area.height >= 1 {
            let x = inner_area.x + inner_area.width - label_w;
            let y = inner_area.y + inner_area.height - 1;
            let indicator_area = Rect::new(x, y, label_w, 1);
            frame.render_widget(Paragraph::new(label), indicator_area);
        }
    }

    // US-06 post-action banner: structured 2-line header + body, drawn at
    // the TOP of the inner area so it appears above the model list (per
    // the Step-5 mockup in journey-cleanup-and-unify-visual.md). Pure-
    // structured rendering via `render::last_action`; the right pane only
    // owns the layout slice.
    if let Some(action) = &state.last_action {
        if inner_area.height >= 2 {
            let banner_area = Rect::new(inner_area.x, inner_area.y, inner_area.width, 2);
            last_action::render(frame, banner_area, action);
        }
    }
}

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

/// Build a transient `Inventory` from `state.real_tools_iter()` for the
/// dedup-glyph classifier. Every entry has `content_hash: None` because the
/// hash pool does not exist yet (lands in step 01-07). The render path
/// recomputes this each frame; performance is fine because rows are bounded
/// by `state.visible_rows` and tools are typically O(4).
///
/// We synthesize a thin `DiscoveredModel` per (tool, id, size) — only the
/// fields the classifier inspects are populated; the rest get sensible
/// defaults. When step 02-05 plumbs the real `DiscoveredModel` through to the
/// right pane this synthesis can be deleted.
fn build_dedup_inventory(state: &AppState) -> Inventory {
    let mut entries: Vec<InventoryEntry> = Vec::new();
    for view in state.real_tools_iter() {
        for (idx, id) in view.model_ids.iter().enumerate() {
            let size = view.model_sizes_bytes.get(idx).copied().unwrap_or(0);
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
                content_hash: None,
            });
        }
    }
    Inventory { entries }
}

/// Find the `InventoryEntry` for `(tool, id)` in `inventory` and run the
/// dedup-glyph classifier on it. Returns `DedupGlyph::Pending` if the entry
/// is not present (defensive — should not happen because we built the
/// inventory from the same `state.real_tools_iter()`).
fn dedup_glyph_for_row(
    tool: ToolId,
    id: &str,
    size: u64,
    inventory: &Inventory,
    inodes: &InodeMap,
    in_progress: &BTreeSet<ModelKey>,
    failed: &BTreeSet<ModelKey>,
) -> modeltap_core::DedupGlyph {
    if let Some(entry) = inventory
        .entries
        .iter()
        .find(|e| e.tool == tool && e.model.id_in_tool == id)
    {
        compute_dedup_glyph(entry, inventory, inodes, in_progress, failed)
    } else {
        // Fallback: synthesize a one-off entry. Pre-hash this still resolves
        // to Pending. The branch only triggers if the dedup-inventory and the
        // visible-row iteration disagree, which should be impossible.
        let entry = InventoryEntry {
            tool,
            model: DiscoveredModel {
                id_in_tool: id.to_string(),
                on_disk_path: PathBuf::new(),
                size_bytes: size,
                format: Format::Other,
                display_label: DisplayLabel::from(id),
                status: ModelStatus::Healthy,
            },
            content_hash: None,
        };
        compute_dedup_glyph(&entry, inventory, inodes, in_progress, failed)
    }
}
