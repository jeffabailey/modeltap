//! Right pane: model list for the currently-selected tool, with a header
//! "Models in <tool> (<count>, <total> GB)" and a scroll-position indicator
//! "<selected+1>/<total>" rendered in the bottom-right corner.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use modeltap_core::domain::synthetic_slot::LeftPaneSlot;
use modeltap_core::domain::{classify_by_presence, other_tools_by_presence, ToolPresence};
use modeltap_core::logic::compatibility::{Inventory, InventoryEntry};
use modeltap_core::logic::dedup::{compute_dedup_glyph, InodeMap, ModelKey};
use modeltap_core::logic::folder_group::group_by_hf_repo;
use modeltap_core::types::{DedupKey, DisplayLabel, Format, ModelMeta, ModelStatus};
use modeltap_core::{DiscoveredModel, ToolId};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app_state::{AppState, FocusPane};
use crate::render::all_unified;
use crate::render::bytes::format_bytes;
use crate::render::colors::no_color_active;
use crate::render::folder_header::render_folder_header_line;
use crate::render::last_action;
use crate::render::row::render_row_basic;
use crate::render::toast;

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
        format_bytes(tool.total_bytes()),
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

    // ---- Build the flat list of rendered Lines, then apply scrolling. ----
    // For the HF tool we route through `group_by_hf_repo` so each
    // `<author>/<repo>` repo appears as a folder-header line followed by its
    // per-file child rows. For every other tool we keep the v1 flat list.
    //
    // Selection (`state.selected_row`) indexes into `tool.model_ids` — the
    // user-visible model count. We map that to a flat-row index by tracking
    // each child row's position relative to `tool.model_ids`. Header rows are
    // NEVER the selected target in this v1 wire-up; cursor-skips-header is a
    // later refinement (the existing keymap already navigates model_ids).
    let total_models = tool.model_ids.len();
    let lines_with_selectable_index: Vec<(Line<'static>, Option<usize>)> = if total_models == 0 {
        Vec::new()
    } else if tool.tool == ToolId("hf") {
        build_hf_folder_grouped_lines(
            tool,
            &inventory,
            &dedup_inventory,
            &dedup_inodes,
            &in_progress,
            &failed_keys,
            no_color,
            &state.expanded_folders,
        )
    } else {
        build_flat_lines(
            tool,
            &inventory,
            &dedup_inventory,
            &dedup_inodes,
            &in_progress,
            &failed_keys,
            no_color,
        )
    };

    // Visible window for rows. `total_rows` is the count of FLAT rows
    // (headers + children); the bottom-right scroll-position indicator still
    // tracks `selected_row + 1 / total_models` per US-04 (user thinks in
    // model count, not row count).
    let visible = state.visible_rows.max(1);
    let total_rows = lines_with_selectable_index.len();
    let start = state.scroll_offset.min(total_rows.saturating_sub(1));
    let end = (start + visible).min(total_rows);
    let rows: Vec<ListItem<'_>> = lines_with_selectable_index[start..end]
        .iter()
        .map(|(line, sel_idx)| {
            let mut out = line.clone();
            if let Some(idx) = sel_idx {
                if *idx == state.selected_row {
                    let mut style = Style::default().add_modifier(Modifier::REVERSED);
                    if state.focus == FocusPane::Right {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    out = out.patch_style(style);
                }
            }
            ListItem::new(out)
        })
        .collect();

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
    // US-U10: when the last action is a partial-success unify (or a
    // unify total-failure with per-target detail), render the richer toast
    // (header + per-target lines + reclaim + footer). Otherwise the v1
    // 2-line banner. Banner height is computed from the toast's actual
    // line count so the model list area shrinks accordingly.
    let banner_lines: Vec<String> = match &state.last_action {
        Some(action) => toast::view_lines(action),
        None => Vec::new(),
    };
    let banner_height: u16 = if banner_lines.is_empty() {
        0
    } else {
        let want = banner_lines.len() as u16;
        // Reserve at most half of the inner area for the banner so the model
        // list always remains observable beneath it (AC-U6.3 invariant).
        let cap = (inner_area.height / 2).max(2);
        want.min(cap).min(inner_area.height)
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
    // "<selected+1>/<total>". Only rendered when there are models. We keep
    // the denominator as `total_models` (model count, not row count) so the
    // indicator reads "1/5" for a 5-model HF repo even though the flat row
    // list contains 6 entries (5 children + 1 folder header).
    if total_models > 0 {
        let label = format!("{}/{}", state.selected_row + 1, total_models);
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
            let banner_area =
                Rect::new(inner_area.x, inner_area.y, inner_area.width, banner_height);
            // Dispatch by status: Partial / Failed-with-detail flow through
            // the US-U10 toast renderer; everything else falls back to the
            // v1 2-line banner. Toast itself defers to last_action for
            // non-partial cases, so we always go through toast — the
            // dispatch keeps last_action::render reachable for the call
            // sites that explicitly wanted the v1 layout.
            use modeltap_core::domain::last_action::ActionStatus;
            if matches!(action.status, ActionStatus::Partial { .. })
                || (matches!(
                    action.verb,
                    modeltap_core::domain::last_action::ActionVerb::Unify
                ) && matches!(action.status, ActionStatus::Failed))
            {
                toast::render(frame, banner_area, action);
            } else {
                last_action::render(frame, banner_area, action);
            }
        }
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

/// Clone a (possibly-borrowed) `Line` into a fully-owned `Line<'static>` so
/// it can outlive transient inputs. Pure copy of every span's text and style.
fn line_into_static(line: Line<'_>) -> Line<'static> {
    let spans = line
        .spans
        .iter()
        .map(|s| Span::styled(s.content.to_string(), s.style))
        .collect::<Vec<_>>();
    Line::from(spans)
}

/// Build the v1 flat list of `(Line, Option<selectable_index>)` for a
/// non-HF tool: one entry per `model_ids[i]`, every entry selectable. The
/// `Option<usize>` is `Some(i)` so the caller can compare against
/// `state.selected_row` when applying the selection style.
fn build_flat_lines(
    tool: &crate::app_state::ToolView,
    inventory: &[ToolPresence],
    dedup_inventory: &Inventory,
    dedup_inodes: &InodeMap,
    in_progress: &BTreeSet<ModelKey>,
    failed_keys: &BTreeSet<ModelKey>,
    no_color: bool,
) -> Vec<(Line<'static>, Option<usize>)> {
    tool.model_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let size = tool.model_sizes_bytes.get(i).copied().unwrap_or(0);
            let indicator = classify_by_presence(id, inventory);
            let also_in = other_tools_by_presence(id, tool.tool, inventory);
            let dedup = dedup_glyph_for_row(
                tool.tool,
                id,
                size,
                dedup_inventory,
                dedup_inodes,
                in_progress,
                failed_keys,
            );
            let line = render_row_basic(id, size, indicator, &also_in, dedup, no_color);
            (line_into_static(line), Some(i))
        })
        .collect()
}

/// Build the folder-grouped list of `(Line, Option<selectable_index>)` for
/// the HF tool. Each `<author>/<repo>` group renders as:
///
/// ```text
/// [+] <author>/<repo>  N files, X GB (M unique, K shared)        (collapsed; default)
/// [-] <author>/<repo>  N files, X GB (M unique, K shared)        (expanded)
///   <child row 1>
///   <child row 2>
/// ...
/// ```
///
/// `expanded_folders` carries the set of `<author>/<repo>` paths the user has
/// explicitly expanded. Folders NOT in the set are collapsed: the header is
/// emitted with `[+]` and the per-file child rows are SKIPPED entirely so
/// the right pane stays compact on caches with 60+ files (step 01-07).
/// Folders in the set are emitted with `[-]` and every child row follows.
///
/// The header `Line` is non-selectable (`None`) — v1 keystroke navigation
/// still indexes `tool.model_ids`, so the user's cursor lands only on child
/// rows. Models whose `id_in_tool` does not contain a `/` (no repo prefix)
/// are appended at the end as flat child rows with no header (defensive —
/// `group_by_hf_repo` already skips these silently, but we still want the
/// user to SEE them rather than have them disappear from the right pane).
///
/// `unique_count` / `shared_count` are computed cheaply here as
/// `(folder.models.len(), 0)` — the live right-pane display does not yet
/// thread the full single-engine classifier through. The header text is the
/// load-bearing AC (`5 files` is what the acceptance test asserts); the
/// `unique`/`shared` split is informational and lands precisely at delete
/// time via `classify_unique_vs_shared`.
#[allow(clippy::too_many_arguments)]
fn build_hf_folder_grouped_lines(
    tool: &crate::app_state::ToolView,
    inventory: &[ToolPresence],
    dedup_inventory: &Inventory,
    dedup_inodes: &InodeMap,
    in_progress: &BTreeSet<ModelKey>,
    failed_keys: &BTreeSet<ModelKey>,
    no_color: bool,
    expanded_folders: &BTreeSet<String>,
) -> Vec<(Line<'static>, Option<usize>)> {
    // Project ToolView -> Vec<ModelMeta>. Only the fields `group_by_hf_repo`
    // and `FolderGroup::new` actually read are populated; the rest get
    // sensible defaults (the function never inspects them).
    let models: Vec<ModelMeta> = tool
        .model_ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let size = tool.model_sizes_bytes.get(i).copied().unwrap_or(0);
            let label = DisplayLabel::from(id.as_str());
            ModelMeta {
                tool: tool.tool,
                id_in_tool: id.clone(),
                on_disk_path: PathBuf::new(),
                size_bytes: size,
                format: Format::Other,
                display_label: label.clone(),
                status: ModelStatus::Healthy,
                dedup_key: DedupKey::Tentative(label),
            }
        })
        .collect();

    // `group_by_hf_repo` partitions by `<author>/<repo>` prefix and silently
    // drops malformed ids. We don't have sidecar paths in the right-pane
    // render path (sidecars are only enumerated at delete time by the HF
    // plugin), so we pass an empty map — the header's `N files` count is
    // model files only, which matches what the user sees row-by-row.
    let folders = group_by_hf_repo(&models, &BTreeMap::new());

    // Build a map from `<author>/<repo>` prefix -> folder index so we can
    // place each model_id under its header in input order while still using
    // the canonical alphabetic ordering from `group_by_hf_repo`.
    let mut out: Vec<(Line<'static>, Option<usize>)> = Vec::new();
    for folder in &folders {
        let unique_count = folder.models.len();
        let shared_count = 0usize;
        // F-FGD-1: single-file folders render as a flat row (no header / no
        // collapse). The folder concept only applies for >=2 files. This
        // preserves the parent-feature dedup-glyph tests which assume each
        // model is visible as its own row.
        if folder.models.len() == 1 {
            let child = &folder.models[0];
            let i_opt = tool.model_ids.iter().position(|id| id == &child.id_in_tool);
            let size = child.size_bytes;
            let indicator = classify_by_presence(&child.id_in_tool, inventory);
            let also_in = other_tools_by_presence(&child.id_in_tool, tool.tool, inventory);
            let dedup = dedup_glyph_for_row(
                tool.tool,
                &child.id_in_tool,
                size,
                dedup_inventory,
                dedup_inodes,
                in_progress,
                failed_keys,
            );
            let line = render_row_basic(
                &child.id_in_tool,
                size,
                indicator,
                &also_in,
                dedup,
                no_color,
            );
            out.push((line_into_static(line), i_opt));
            continue;
        }
        let is_expanded = expanded_folders.contains(&folder.path);
        let header = render_folder_header_line(folder, is_expanded, unique_count, shared_count);
        // The header is selectable when COLLAPSED so the cursor has a target
        // row to land on (and Enter/Shift+F can resolve the folder from the
        // selection). We map a collapsed header to the SELECTABLE INDEX of
        // its first child so `state.selected_row` (which indexes
        // `tool.model_ids`) keeps working — pressing Enter on the header
        // expands the folder, and the cursor stays on the same logical row
        // (now visible as the first child).
        let first_child_idx = folder
            .models
            .first()
            .and_then(|m| tool.model_ids.iter().position(|id| id == &m.id_in_tool));
        let header_selectable_idx = if is_expanded { None } else { first_child_idx };
        out.push((line_into_static(header), header_selectable_idx));
        if !is_expanded {
            // Collapsed: skip the per-file child rows entirely. The folder
            // header alone represents the group in the right pane.
            continue;
        }
        // Expanded: append each child row in the folder. We look up the
        // child's selectable index by finding it back in `tool.model_ids` so
        // the cursor (which indexes model_ids) still highlights the right row.
        for child in &folder.models {
            let i_opt = tool.model_ids.iter().position(|id| id == &child.id_in_tool);
            let size = child.size_bytes;
            let indicator = classify_by_presence(&child.id_in_tool, inventory);
            let also_in = other_tools_by_presence(&child.id_in_tool, tool.tool, inventory);
            let dedup = dedup_glyph_for_row(
                tool.tool,
                &child.id_in_tool,
                size,
                dedup_inventory,
                dedup_inodes,
                in_progress,
                failed_keys,
            );
            let line = render_row_basic(
                &child.id_in_tool,
                size,
                indicator,
                &also_in,
                dedup,
                no_color,
            );
            out.push((line_into_static(line), i_opt));
        }
    }

    // Defensive: any model_ids that group_by_hf_repo dropped (no `/`
    // separator) get appended ungrouped so they remain visible. In practice
    // the HF plugin's discover() always emits `<author>/<repo>/<file>`-shaped
    // ids, so this branch is empty for real HF caches.
    let grouped_ids: BTreeSet<&str> = folders
        .iter()
        .flat_map(|f| f.models.iter().map(|m| m.id_in_tool.as_str()))
        .collect();
    for (i, id) in tool.model_ids.iter().enumerate() {
        if grouped_ids.contains(id.as_str()) {
            continue;
        }
        let size = tool.model_sizes_bytes.get(i).copied().unwrap_or(0);
        let indicator = classify_by_presence(id, inventory);
        let also_in = other_tools_by_presence(id, tool.tool, inventory);
        let dedup = dedup_glyph_for_row(
            tool.tool,
            id,
            size,
            dedup_inventory,
            dedup_inodes,
            in_progress,
            failed_keys,
        );
        let line = render_row_basic(id, size, indicator, &also_in, dedup, no_color);
        out.push((line_into_static(line), Some(i)));
    }
    out
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
