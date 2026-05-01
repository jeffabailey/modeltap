# ADR-014: Synthetic Left-Pane Slots Are Render-Only

## Status

Accepted (2026-04-30). Clarifies the boundary set by ADR-001 ("the `Tool` trait is the only extension contract") for the cross-tool-model-unify feature's `[All Unified]` slot.

## Context

The DISCUSS wave's `journey-unified-view-visual.md` chose a left-pane slot labeled `[All Unified]` as the surface where Devon audits cumulative unification. The slot:

- Sits in the same left-pane navigation as the four real tools (j/k navigates onto and off it).
- Has a count badge and, when selected, populates the right pane with `#`-glyph models plus a footer.
- Has no `discover()`, no `link()`, no `delete_one()` — it is purely a viewing surface.

ADR-001 froze the `Tool` trait at six methods. The first reading of "add a slot to the left pane" is to implement `Tool` for an `AllUnifiedFakeTool`. The question is whether that interpretation is correct.

The risk of the wrong interpretation: every `Tool` impl flows into `Vec<Box<dyn Tool>>` consumed by `actions::unify::run`, `actions::delete_one::run`, `actions::zap::run`. Any of those orchestrators could in principle iterate the registry and call `link()` or `delete_*()` on the synthetic slot. Defensive code ("if name == 'All Unified' skip") would have to be added to every action — a textbook example of the kind of contamination ADR-001 explicitly avoids.

## Decision

**Synthetic left-pane slots are a TUI render concept, encoded in `modeltap-core::domain::synthetic_slot::SyntheticSlot` and `LeftPaneSlot`. They never round-trip through `Box<dyn Tool>` and never appear in the action-side plugin registry. The TUI's `AppState.left_pane_slots: Vec<LeftPaneSlot>` is heterogeneous: `LeftPaneSlot::Real(ToolView)` for actual plugins and `LeftPaneSlot::Synthetic(SyntheticSlot)` for the `[All Unified]` view. Selection navigation (`update::select_next_tool`) operates on `left_pane_slots.len()` unchanged. The render layer dispatches on the variant: real slots render the existing per-tool right pane; synthetic slots delegate to a per-variant renderer (e.g., `render::all_unified`). Action keypresses (`u`, `d`, `z`) on a synthetic slot's right-pane row dispatch to the same handlers as on a real-tool row, because the rows themselves are still `(model_id, tool_id)` pairs sourced from the inventory; the synthetic slot is only a filter.**

## Alternatives considered

### A — `impl Tool for AllUnifiedFakeTool`

A no-op implementation that returns empty results from every method.

- **Pros:** uniform iteration; one type for the registry.
- **Cons:**
  - Violates ADR-001's intent: the `Tool` trait now has a member that is not a tool. New plugin authors reading `CONTRIBUTING.md` would have to be told "do not look at AllUnifiedFakeTool as an example."
  - Forces `actions::unify::run`, `actions::delete_one::run`, `actions::zap::run` to defensively skip the fake (or live with the fake's no-op responses). The skip logic spreads across the codebase.
  - The fake's `name()` value bleeds into JSONL events emitted by actions even when invocation is suppressed.
- **Rejected.**

### B — Hotkey-triggered separate screen

Bind a keystroke (e.g., `U`) to a dedicated "All Unified" screen that is not in the left pane.

- **Pros:** zero changes to `AppState.tools` shape.
- **Cons:** new mental location; new keybinding to teach; loses the muscle-memory benefit of left-pane navigation. UX rejected in DISCUSS.
- **Rejected.**

### C — Right-pane filter toggle

A modal toggle ("show only `#`-rows") that the user enables/disables.

- **Pros:** zero new structural concept.
- **Cons:** modal state the user can forget is on; right-pane meaning becomes ambiguous depending on toggle state; counts in summary bar disagree with the right pane's row count when the toggle is off.
- **Rejected.**

### D (CHOSEN) — Heterogeneous left-pane slot enum

The decision above.

- **Pros:**
  - Preserves ADR-001 strictly: the `Tool` trait surface is unchanged; the action registry is unchanged.
  - Selection navigation requires no change beyond an index-into-a-different-vec; existing bound-checking logic reused verbatim.
  - The synthetic slot's render is isolated to `render::all_unified` and one match arm in the left-pane renderer.
  - Future synthetic slots (e.g., a hypothetical `[All Format-Locked]` audit view) extend the enum without trait pressure.
- **Cons:**
  - `AppState.tools: Vec<ToolView>` is renamed to `AppState.left_pane_slots: Vec<LeftPaneSlot>`. Mechanical refactor across the TUI render and update layers.
  - Any test that assumed `tools[0].tool == ToolId("ollama")` needs to address by name or by `LeftPaneSlot::Real` filtering.

## Consequences

### Positive

- ADR-001's contract is intact: `Tool` remains the sole extension point for actual tool plugins.
- Action orchestrators do not need to know synthetic slots exist.
- Future synthetic slots can be added without trait-design pressure.
- The synthetic slot's badge count and right-pane row data flow from the existing dedup classifier in `core::logic::dedup`, satisfying NFR-5 (single source of truth).

### Negative

- One-time mechanical refactor of `AppState.tools` → `AppState.left_pane_slots`. Bounded by the number of test fixtures that hardcode tool indices.
- Render-layer match arm has to handle each `SyntheticSlot` variant. For v1 there is exactly one variant; growth is linear.

### Neutral

- The synthetic slot's right-pane rows ARE still real models owned by real tools — pressing `u` or `d` on a row dispatches to the normal action path for that model's owning tool. Only the slot itself is synthetic.

## Enforcement

- Compile-time: the action registry's type is `Vec<Box<dyn Tool>>`; `SyntheticSlot` is not a `Tool`, so a misuse is a type error, not a runtime check.
- Architecture-lint test (existing `tests/architecture.rs`): no rule change required. The new types live in `modeltap-core::domain::synthetic_slot`, which is consumed by `modeltap-tui` only.
- DISTILL acceptance scenario (cross-artifact consistency AC-CONS-2): badge count, summary count, and right-pane row count all equal — exercised in the master suite.

## Future hooks

If the count of synthetic slots grows beyond ~3, consider extracting a `trait LeftPaneSlotRender` to keep `render::left_pane` polymorphic. v1 does not need this; the explicit match is the simplest tool that fits the seam.
