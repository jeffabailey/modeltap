# Data Models — cross-tool-model-unify

Type signatures only. **No implementations.** Software-crafter owns internals.

## New types in `modeltap-core`

### `domain::dedup_glyph`

```rust
/// The dedup-state column glyph (separate from RowIndicator which is the
/// compatibility column). One per row in the right pane.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, serde::Serialize)]
pub enum DedupGlyph {
    /// "?" — no hash yet, no worker assigned.
    Pending,
    /// "~" — a worker is currently hashing this file.
    Hashing,
    /// "-" (without decorator) — unique content; no peer has a matching hash.
    Unique,
    /// "-" with "!" decorator — hashing failed (read error, IO error).
    /// Conservative-when-uncertain (BR-3): treated as Unique for action purposes.
    Failed,
    /// "=" — ≥2 separate inodes share the same SHA256.
    DedupAble,
    /// "#" — ≥2 paths already share one inode.
    AlreadyUnified,
}
```

### `domain::synthetic_slot`

```rust
/// A non-Tool entry that may appear in the left pane. Render-only.
/// Per ADR-014, never round-trips through `Box<dyn Tool>`.
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize)]
pub enum SyntheticSlot {
    AllUnified {
        /// Number of `#`-glyph rows. Sourced from the dedup classifier.
        /// `None` while hashing is in progress (renders as "(?)" in the badge).
        count: Option<u64>,
        /// Sum of `(N-1) * size` over all unified models. `None` while hashing.
        total_saved_bytes: Option<u64>,
    },
}

/// One slot in the left pane. Either a real tool (existing `ToolView`) or a
/// synthetic render-only entry. Both are navigable via j/k.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum LeftPaneSlot {
    Real(ToolView),
    Synthetic(SyntheticSlot),
}
```

### `domain::dedup_summary`

```rust
/// Aggregates carried on AppState for the summary bar and other surfaces.
/// Computed once per AppState transition by `core::logic::dedup::dedup_summary`.
#[derive(Debug, Clone, Eq, PartialEq, Default, serde::Serialize)]
pub struct DedupSummary {
    /// `Some(bytes)` once hashing has produced any classification. `None` while
    /// "computing..." should be displayed.
    pub dedup_able_bytes: Option<u64>,
    /// Same nullability semantics. The "Unified: N models" count.
    pub unified_count: Option<u64>,
    /// For the [All Unified] footer: sum of (N-1)*size over `#` rows.
    pub total_saved_by_unification: Option<u64>,
}
```

### `logic::dedup` — new pure functions

```rust
/// Compute the per-row dedup glyph. Called on every render for every row;
/// must be O(1) amortized (the inventory lookups are by hash key in a HashMap).
pub fn compute_dedup_glyph(
    target: &InventoryEntry,
    inventory: &Inventory,
    in_progress: &BTreeSet<ModelId>,
    failed: &BTreeSet<ModelId>,
) -> DedupGlyph;

/// Collect the rows that should appear when [All Unified] is selected.
/// Each `UnifiedRow` carries enough data to render name + size + tool count + saves.
pub fn collect_unified_rows(
    inventory: &Inventory,
) -> Vec<UnifiedRow>;

/// One row in the [All Unified] right-pane view.
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize)]
pub struct UnifiedRow {
    pub model_id: ModelId,
    pub display_label: DisplayLabel,
    pub size_bytes: u64,
    pub tools_sharing: Vec<ToolId>,
    pub saves_bytes: u64, // (tools_sharing.len() - 1) * size_bytes
}

/// Top-level summary across the whole inventory. Reads from the same data
/// `compute_dedup_glyph` consumes; the two are guaranteed consistent.
pub fn dedup_summary(
    inventory: &Inventory,
    hashing_done: bool,
) -> DedupSummary;
```

## Modifications to `modeltap-tui::AppState`

```rust
pub struct AppState {
    // CHANGED: was `tools: Vec<ToolView>`. Now a heterogeneous list.
    pub left_pane_slots: Vec<LeftPaneSlot>,

    // selected_tool, selected_row, scroll, focus, etc. — UNCHANGED in shape.
    // selected_tool now indexes into left_pane_slots; the existing
    // bound-checking logic in update::select_next_tool/select_prev_tool works
    // unchanged.
    pub selected_tool: usize,
    pub selected_row: usize,
    // ... (existing fields unchanged)

    // NEW: hash pool live state.
    pub hash_state: HashPoolState,

    // NEW: cached classifier output for render. Recomputed on hash msgs and
    // on action completion. Carried explicitly on state to keep render fns pure.
    pub dedup_summary: DedupSummary,

    // NEW (transient): for the "(was X GB)" delta after unify. Cleared after
    // ~5 s by a Msg::SummaryDeltaExpired tick handler.
    pub summary_delta: Option<SummaryDelta>,
}
```

```rust
/// Live state of the background hash pool. All counters are derived from
/// AtomicU64s in the pool but cached on AppState so render is pure.
#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub struct HashPoolState {
    pub total: u64,
    pub completed: u64,
    /// Model ids with a worker currently hashing them. Drives the "~" glyph.
    pub in_progress: BTreeSet<ModelId>,
    /// Model ids whose hash failed. Drives the "-" + "!" decorator.
    pub failed: BTreeSet<ModelId>,
}

impl HashPoolState {
    pub fn is_hashing(&self) -> bool;       // true while completed < total
    pub fn is_complete(&self) -> bool;      // total > 0 && completed == total
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SummaryDelta {
    pub previous_dedup_able_bytes: u64,
    pub expires_at: std::time::Instant,
}
```

## New `Msg` variants in `modeltap-tui::update`

```rust
pub enum Msg {
    // ... existing variants ...

    /// Posted by a hash-pool worker when a single SHA256 completes.
    /// AppState.hash_state.completed is incremented; the row is
    /// re-classified into Unique / DedupAble / AlreadyUnified.
    HashComputed { model_id: ModelId, hash: ContentHash },

    /// Posted by a hash-pool worker when SHA256 fails (read error, file gone).
    HashFailed { model_id: ModelId, reason: HashFailureReason },

    /// Posted at 250 ms cadence by the throttle task. Updates the status
    /// line "Hashing N/M..." without a per-completion redraw.
    HashProgressTick { completed: u64, total: u64 },

    /// Posted by the composition root when actions::unify::run completes.
    /// Drives the row-glyph re-classification and the summary-bar delta.
    /// (The dialog-side Msg::UnifyCompleted may exist; this is its *application*
    /// counterpart that carries the orchestrator's UnifyOutcome.)
    UnifyApplied { outcome: UnifyOutcome },

    /// Posted by a 5-second timer after a successful unify; clears
    /// AppState.summary_delta.
    SummaryDeltaExpired,

    /// Pressed `u` in main view; dispatches based on highlighted row's glyph.
    UnifyHighlighted,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HashFailureReason {
    Io(String),    // formatted error string (paths stripped per privacy rule)
    NotFound,
    Permission,
}
```

## Hash pool internal types (`modeltap-app::hash_pool`)

These are crate-private to `modeltap-app` and never appear in `core` or `tui`.

```rust
/// One job pushed to the worker queue at startup.
pub(crate) struct HashJob {
    pub model_id: ModelId,
    pub path: PathBuf,
    pub mtime: u64,
    pub size: u64,
}

/// Returned by `hash_pool::spawn`. Held by the composition root.
pub(crate) struct HashPoolHandle {
    pub cancel: tokio_util::sync::CancellationToken,
    pub join_set: tokio::task::JoinSet<()>,
}

impl HashPoolHandle {
    /// Signal cancellation and await join with a deadline. Used on quit.
    pub async fn shutdown(self, deadline: std::time::Duration) -> ShutdownOutcome;
}
```

## Type guarantees (architecture-lint additions)

Already-enforced rules in `tests/architecture.rs` that this feature relies on:

- `modeltap-core` does not depend on `tokio` (verified by `cargo metadata` parse).
- `modeltap-tui` does not depend on any plugin crate.
- `plugins/*` crates do not depend on each other.

New rule to add:

- `modeltap-core::domain::dedup_glyph` does not depend on any other domain submodule (purity check via `cargo modules tree -p modeltap-core --no-default-features`). Optional; nice-to-have.

## Software-crafter notes (deliberate non-decisions)

The following are **not** decided here; the crafter chooses during GREEN/REFACTOR:

- Whether `dedup_summary` is recomputed on every Msg or memoized between hash events (perf decision; either works for v1's data sizes).
- Whether `compute_dedup_glyph` builds a per-hash index up front or scans (likely an indexed map will emerge; do it during REFACTOR).
- Whether `Msg::HashProgressTick` is replaced by reading the AtomicU64s directly in the throttle task or via channel (channel is the simpler default).
- Worker count constant location (`modeltap_app::hash_pool::DEFAULT_WORKERS`) and exact env-var name if exposed (`MODELTAP_HASH_WORKERS` is the suggestion in the architecture doc; not load-bearing).
