//! Right pane: model list for the currently-selected tool, with a header
//! "Models in <tool> (<count>, <total> GB)" and a scroll-position indicator
//! "<selected+1>/<total>" rendered in the bottom-right corner.

use std::collections::BTreeSet;
use std::path::PathBuf;

use modeltap_core::domain::synthetic_slot::LeftPaneSlot;
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
use crate::render::all_unified;
use crate::render::colors::no_color_active;
use crate::render::last_action;
use crate::render::row::render_row_basic;

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    // Step 04-02 dispatch: when the currently-selected left-pane slot is a
    // synthetic entry (`[All Unified]`), the right pane is no longer a
    // per-tool model list — it's a filtered cross-tool unified-rows view.
    // Route to the dedicated renderer; the per-tool path below is only
    // valid when `selected_tool` indexes a `LeftPaneSlot::Real(_)`.
    if matches!(
        state.left_pane_slots.get(state.selected_tool),
        Some(LeftPaneSlot::Synthetic(_))
    ) {
        all_unified::render(frame, area, state);
        return;
    }

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
    // Each entry's `content_hash` is read back from `state.hash_state.completed_hashes`
    // so post-hash the row glyph reflects the real classification (=, #, -, -!).
    // Pre-hash, `completed_hashes` is empty and every entry has
    // `content_hash: None` so the classifier returns `Pending` (`?`).
    let dedup_inventory = build_dedup_inventory(state);
    // Inode map mirrors `state.hash_state.inodes` — populated as the hash pool
    // resolves device+inode for each blob. Required so that pre-hardlinked
    // blobs route to `AlreadyUnified` (`#`) rather than `DedupAble` (`=`).
    let dedup_inodes: InodeMap = state
        .hash_state
        .inodes
        .iter()
        .map(|((tool, id), devino)| ((*tool, id.clone()), *devino))
        .collect();
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

    // US-06 post-action banner: structured 2-line header + body, drawn at
    // the TOP of the inner area. AC-U6.3 (step 05-03): the model list must
    // remain visible BELOW the banner so the per-row dedup glyph (`=`/`#`)
    // stays observable post-action — earlier the list was rendered across
    // the FULL inner_area and the banner overdrew the first 2 rows, hiding
    // the only model row in single-row tools (the partial-unify acceptance
    // test then could not see the `=` glyph at all). Slice 2 rows for the
    // banner and render the list into the remaining area.
    let banner_height: u16 = if state.last_action.is_some() && inner_area.height >= 2 {
        2
    } else {
        0
    };
    let list_area = if banner_height > 0 {
        Rect::new(
            inner_area.x,
            inner_area.y + banner_height,
            inner_area.width,
            inner_area.height.saturating_sub(banner_height),
        )
    } else {
        inner_area
    };
    let widget = List::new(rows);
    frame.render_widget(widget, list_area);

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

    if let Some(action) = &state.last_action {
        if banner_height >= 2 {
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
/// dedup-glyph classifier. Each entry's `content_hash` is looked up in
/// `state.hash_state.completed_hashes`; pre-hash the lookup misses and the
/// entry stays with `content_hash: None`, so the classifier returns
/// `Pending` (`?`). Post-hash the same call site sees the real hashes and
/// the classifier returns `Unique`/`DedupAble`/`AlreadyUnified`/`Failed`.
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
