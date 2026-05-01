# Component Boundaries — cross-tool-model-unify

Brownfield delta on the v1 boundaries documented in `docs/feature/modeltap-tui/design/component-boundaries.md`. Reuse over reimplementation. Every new module is justified with "no existing alternative."

## Crate-level dependency rules (UNCHANGED from v1)

```
modeltap-core ── (depends on nothing concrete)
plugins/* ── modeltap-core only
modeltap-tui ── modeltap-core, ratatui, crossterm
modeltap-app ── modeltap-core, modeltap-tui, every plugin, tokio
```

Architecture-lint test in `tests/architecture.rs` already enforces this; **no changes needed.**

## New / modified modules

### `modeltap-core` (pure logic, no I/O)

| Module | Status | Responsibility |
|---|---|---|
| `logic::dedup` | **EXTENDED** | Add `compute_dedup_glyph(...)`, `collect_unified_rows(...)`, `dedup_summary(...)` — three new pure functions. Existing `classify_unique_vs_shared` stays. |
| `domain::dedup_glyph` | **NEW** | Defines `enum DedupGlyph { Pending, Hashing, Unique, DedupAble, AlreadyUnified, Failed }`. New module because the existing `domain::indicator::RowIndicator` is the *compatibility* glyph (`o/*/!/?`), a separate concept. Two enums avoid a single overloaded one. |
| `domain::synthetic_slot` | **NEW** | Defines `enum SyntheticSlot { AllUnified { count: u64, total_saved_bytes: u64 } }` and `enum LeftPaneSlot { Real(ToolView), Synthetic(SyntheticSlot) }`. Lives in core because it's pure data shared between TUI render and update logic. |

### `modeltap-tui` (render + update, no I/O)

| Module | Status | Responsibility |
|---|---|---|
| `app_state::AppState` | **MODIFIED** | Replace `tools: Vec<ToolView>` with `left_pane_slots: Vec<LeftPaneSlot>`. Add `hash_state: HashPoolState`. Add `dedup_summary: DedupSummary`. See `data-models.md`. |
| `update` | **EXTENDED** | Add `Msg::HashComputed`, `Msg::HashProgressTick`, `Msg::HashFailed`, `Msg::SelectAllUnifiedSlot` (the last is just a bound-checked tool-index move; no new dispatch). Pure handlers. |
| `render::summary_bar` | **MODIFIED** | Remove the hardcoded `"Dedup-able: 0 B"` literal at line 36. Read `state.dedup_summary.dedup_able_bytes` and render `computing...` while `state.hash_state.is_hashing()`. Add `(was X GB)` transient delta from `state.last_action`. |
| `render::left_pane` | **MODIFIED** | Branch on `LeftPaneSlot::Real` vs `Synthetic` to render the badge. |
| `render::row` | **MODIFIED** | Add a fixed dedup-glyph column (1 char + 1 space) between the existing compatibility glyph column and the size column. Reads from a per-row `DedupGlyph` carried on the rendered row data. |
| `render::all_unified` | **NEW** | Right-pane renderer used when `SyntheticSlot::AllUnified` is selected. Renders the unified-row list + footer. Empty state when count is 0 or hashing is in progress (US-U8). |
| `keymap::dispatch` | **MODIFIED** | `u` from main view is currently a no-op; route it to a new `Msg::UnifyHighlighted` that the update handler dispatches based on the highlighted row's glyph (open dialog / show hint). |

### `modeltap-app` (composition root, owns I/O)

| Module | Status | Responsibility |
|---|---|---|
| `hash_pool` | **NEW** | The background hashing subsystem. Contains: `spawn(plugins, cache, msg_tx, cancel_token) -> HashPoolHandle`, `worker(rx, cache, msg_tx)`, `progress_throttle(counters, msg_tx, cancel_token)`. Sole new module in `modeltap-app`. Justification: tokio-bound; cannot live in core. |
| `actions::unify::run` | **NO CHANGE** | Already produces the JSONL events used by post-unify re-classification. |
| `actions::reclassify` | **NEW** (small) | Pure function `reclassify_after_unify(state, unify_outcome) -> AppState'` that recomputes affected rows' `DedupGlyph`s and the `dedup_summary` aggregates. Lives in `modeltap-app` because it consumes the orchestrator's `UnifyOutcome`; the actual recomputation calls `core::logic::dedup` pure functions. |
| `headless` / `interactive` | **MODIFIED** | After first paint, call `hash_pool::spawn(...)`. On `Msg::Quit`, signal cancellation token and await join with 200 ms timeout. Drain `Msg::Hash*` from the existing event channel (no new channel). |
| `Sha256Cache` | **NO CHANGE** | Already exposes `get_or_compute`. Pool workers call this directly. |

### `plugins/*` (UNCHANGED)

All four plugin crates and the `Tool` trait are untouched.

## Module-level dependency invariants (NEW)

To be added to the existing `tests/architecture.rs`:

1. `hash_pool` may import from `modeltap-core::ports::Hasher` and `modeltap_core::Inventory`, but NOT from `modeltap_tui` (it sends Msgs through a generic `mpsc::UnboundedSender<Msg>` whose type is re-exported by `modeltap-tui`; the import is allowed).
2. `domain::dedup_glyph` is a `serde`-derive-only module — must not depend on any I/O crate.
3. `render::all_unified` may not import any `modeltap-app` symbol.

## What gets reused (and explicitly NOT rewritten)

| Existing component | Reuse mode |
|---|---|
| `Sha256Cache` | Pool workers call `cache.get_or_compute(...)`. |
| `Sha2Hasher` | Pool workers instantiate it directly (no DI needed; the cache port is the seam). |
| `core::logic::canonical_selector::select_canonical` | Called by the unify-dialog open path to pre-populate canonical. |
| `core::logic::plan::build_plan` | Called by the unify-dialog open path to build the plan from the highlighted row's mates. |
| `actions::unify::run` | Called by Enter in the dialog (already the case in v1; the path from main view to dialog is the only new wiring). |
| `actions::unify::dry_run` | Available; not required for this feature but no removal. |
| Existing acceptance harness in `headless.rs` | Drains `Msg::HashComputed` exactly like any other Msg; no harness extension needed beyond a script-token grammar update for a "wait-for-hash-complete" sentinel (a one-line addition to the tokenizer). |

## Sequencing implication for DELIVER

Outside-In TDD natural order:

1. New core types (`DedupGlyph`, `SyntheticSlot`, `LeftPaneSlot`, `DedupSummary`) — pure data, trivial tests.
2. New core pure fns (`compute_dedup_glyph`, `collect_unified_rows`, `dedup_summary`) — unit tests in `modeltap-core`.
3. `AppState` shape change + `update` handlers — unit tests in `modeltap-tui` against pure update fn.
4. Render-layer changes — snapshot tests via `TestBackend`.
5. `hash_pool` — integration tests with the real `Sha2Hasher` against tempdir fixtures.
6. End-to-end acceptance through `headless.rs` driving the whole stack.

This is the existing v1 sequence pattern; no methodology change.
