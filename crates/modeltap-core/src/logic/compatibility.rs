//! Format-aware compatibility-indicator engine (US-09).
//!
//! Pure function `compute_indicator(target, inventory, plugin_capabilities)`
//! returns one of `{Compatible, Shared, FormatLocked, Unknown}` (rendered as
//! `{o, *, !, ?}` in the right pane).
//!
//! ## Decision rules (in evaluation order)
//!
//! 1. **Format Other** OR **target plugin's `accepted_formats()` is empty** →
//!    `Unknown`. The first guard says we cannot parse the on-disk file so
//!    compatibility is undecidable. The second is defensive (per US-16 AC-3):
//!    a plugin that declared ZERO accepted formats has no contract we can
//!    reason about — we render its models as Unknown rather than silently
//!    classifying them as FormatLocked or Compatible.
//!
//! 2. **Shared (`*`)** — there exists at least one OTHER `InventoryEntry` whose
//!    dedup-key matches the target's. Per ADR-002, the dedup-key is SHA256
//!    when computed (`Some(ContentHash)`), with a HF id+quant fallback to be
//!    layered on later. **ADR-002 conservative-when-uncertain rule** is cited
//!    in `is_dedup_key_match` below: when the SHA256 is `None` for either
//!    side, the engine MUST NOT classify as Shared.
//!
//! 3. **Compatible (`o`)** — single-tool registration AND at least one other
//!    plugin's `accepted_formats()` contains the target's format.
//!
//! 4. **FormatLocked (`!`)** — single-tool registration AND no other plugin
//!    accepts the format.
//!
//! ## Purity contract
//!
//! No I/O, no global state, no `&mut self`. Inputs in → indicator out. The
//! engine is recomputed on every render so any inventory change (zap, unify,
//! refresh) automatically reflects in the indicator without explicit
//! invalidation.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::domain::RowIndicator;
use crate::types::{ContentHash, DiscoveredModel, Format, ToolId};

/// Plugin name → list of formats that plugin's `accepted_formats()` declares.
///
/// `BTreeMap` over `HashMap` so iteration order is deterministic — a property
/// the engine itself does not require but tests and JSONL diagnostic events
/// do (per ADR-007 stable-output discipline).
pub type PluginCapabilityMap = BTreeMap<ToolId, Vec<Format>>;

/// One discovered model paired with the tool that owns it. The cross-plugin
/// inventory is a `Vec<InventoryEntry>`. `content_hash` is `Some(_)` once
/// SHA256 has been computed (lazy, per ADR-002) and `None` until then.
#[derive(Debug, Clone, Serialize)]
pub struct InventoryEntry {
    pub tool: ToolId,
    pub model: DiscoveredModel,
    /// SHA256 of the file content, when computed. Per ADR-002 hashing is lazy;
    /// most entries will have `None` on first paint.
    pub content_hash: Option<ContentHash>,
}

/// Cross-plugin inventory aggregated by the orchestrator. The engine takes
/// this by reference and never mutates it.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Inventory {
    pub entries: Vec<InventoryEntry>,
}

/// Compute the row indicator for `target` given the full cross-plugin
/// `inventory` and each plugin's declared `plugin_capabilities`. Pure
/// function — no I/O, no state, no panics.
///
/// Evaluation order matches the rules above; see module-level docstring for
/// the full decision tree.
pub fn compute_indicator(
    target: &InventoryEntry,
    inventory: &Inventory,
    plugin_capabilities: &PluginCapabilityMap,
) -> RowIndicator {
    // Rule 1a: undecidable format.
    if matches!(target.model.format, Format::Other) {
        return RowIndicator::Unknown;
    }
    // Rule 1b: defensive — empty `accepted_formats()` for the target's plugin
    // means the plugin's contract is undefined w.r.t. format compatibility.
    // Render as Unknown rather than guessing FormatLocked. (US-16 AC-3)
    if plugin_capabilities
        .get(&target.tool)
        .is_some_and(|fmts| fmts.is_empty())
    {
        return RowIndicator::Unknown;
    }

    // Rule 2: Shared via dedup-key match against any OTHER inventory entry.
    if has_dedup_match_in_other_tool(target, inventory) {
        return RowIndicator::Shared;
    }

    // Rule 3: another plugin (other than `target.tool`) accepts the format.
    if any_other_plugin_accepts(target.tool, target.model.format, plugin_capabilities) {
        return RowIndicator::Compatible;
    }

    // Rule 4: format-locked into the current tool.
    RowIndicator::FormatLocked
}

/// True if the inventory contains some entry — in a tool DIFFERENT from
/// `target.tool` — whose dedup-key matches the target's. Per ADR-002 the
/// dedup-key match is **conservative-when-uncertain**: if either side's
/// SHA256 is `None`, the engine returns `false` (NOT shared). This preserves
/// the safety invariant that we never silently overstate compatibility.
fn has_dedup_match_in_other_tool(target: &InventoryEntry, inventory: &Inventory) -> bool {
    inventory
        .entries
        .iter()
        .filter(|e| e.tool != target.tool)
        .any(|peer| is_dedup_key_match(target, peer))
}

/// Pure dedup-key match between two inventory entries. Per ADR-002
/// (conservative-when-uncertain rule): if either side's content hash is
/// `None`, we are NOT confident the entries are byte-identical — return
/// `false`. SHA256-equality is the only positive match we accept here.
///
/// HF id+quant fallback is intentionally not yet implemented; the
/// conservative path is the safe default until US-12 lands the parser. The
/// rule says "preserve data when uncertain": missing SHA256 + no parsed
/// id+quant = treat as not-shared. The behavior is monotonic — adding the
/// fallback later only flips additional pairs from `false` to `true`, never
/// the reverse.
pub fn is_dedup_key_match(a: &InventoryEntry, b: &InventoryEntry) -> bool {
    match (a.content_hash, b.content_hash) {
        (Some(ha), Some(hb)) => ha == hb,
        // ADR-002 conservative-when-uncertain rule: missing SHA256 means we
        // are not sure these two files are byte-identical. Returning `false`
        // ensures the indicator never overstates sharing.
        _ => false,
    }
}

/// True if some plugin OTHER than `target_tool` declares `format` in its
/// `accepted_formats()`. Plugins with empty `accepted_formats()` cannot
/// accept anything (vacuously) so they don't contribute to compatibility.
fn any_other_plugin_accepts(
    target_tool: ToolId,
    format: Format,
    plugin_capabilities: &PluginCapabilityMap,
) -> bool {
    plugin_capabilities
        .iter()
        .filter(|(tool, _)| **tool != target_tool)
        .any(|(_, fmts)| fmts.contains(&format))
}
