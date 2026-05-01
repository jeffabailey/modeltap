# Prioritization: cross-tool-model-unify

## Release Priority

| Priority | Release | Target Outcome | KPI Anchor | Rationale |
|---|---|---|---|---|
| 1 | **Walking Skeleton (US-U1..U7)** | Devon can press `u` and see real disk reclaimed; the v1 promise becomes true | KPI-1 (Activation), KPI-2 (Adoption), KPI-3 (Success) | Without this, the feature does not exist. The v1 implementation is broken in user-visible ways and this release fixes it end-to-end. |
| 2 | **Polish (US-U8, U9, U10)** | Audit confidence + better failure visibility | KPI-3 (Success), KPI-4 (Retention proxy) | Empty state, detail screen, and partial-success toast improve trust but the core flow already works. |

---

## Story Priority Detail

| Story | Release | P | Outcome Link | Value | Urgency | Effort | Score | Dependencies |
|---|---|---|---|---|---|---|---|---|
| US-U1 — Background SHA256 hashing with progress | WS | P1 | KPI-1 | 5 | 5 | 4 | 6.25 | None |
| US-U2 — Wire dedup-able bytes to summary bar | WS | P1 | KPI-1 | 5 | 5 | 1 | 25.0 | US-U1 |
| US-U3 — Row glyph reflects dedup state | WS | P1 | KPI-1 | 5 | 5 | 2 | 12.5 | US-U1 |
| US-U4 — `u` opens dialog with mates pre-populated | WS | P1 | KPI-2 | 5 | 5 | 2 | 12.5 | US-U3 |
| US-U5 — Dialog shows reclaim preview and applies plan | WS | P1 | KPI-3 | 5 | 4 | 2 | 10.0 | US-U4 |
| US-U6 — Post-unify row + summary bar update | WS | P1 | KPI-3 | 5 | 4 | 2 | 10.0 | US-U5 |
| US-U7 — `[All Unified]` pseudo-tool slot | WS | P1 | KPI-4 | 4 | 4 | 2 | 8.0 | US-U3 |
| US-U8 — `[All Unified]` empty state | R2 | P2 | KPI-4 | 2 | 2 | 1 | 4.0 | US-U7 |
| US-U9 — Detail screen for unified model | R2 | P2 | KPI-4 | 3 | 2 | 2 | 3.0 | US-U7 |
| US-U10 — Partial-success reporting | R2 | P2 | KPI-3 | 3 | 3 | 2 | 4.5 | US-U5 |

Score = `(Value * Urgency) / Effort`, 1-5 scale per dimension.

---

## Riskiest Assumption (validated first)

**Assumption**: Background SHA256 hashing for ~20 GGUF files (typical install) completes within an acceptable window (< 30 s on warm disk, < 2 min on cold).

**Why this could kill the feature**: If hashing routinely takes 5+ minutes, the dedup-able number is "computing..." for so long that Devon gives up before discovering the feature works. The whole emotional arc collapses.

**Validation in WS**: US-U1 establishes hashing infra; the `Hashing N/M` indicator MUST be visible during this period so even slow hashing is felt as "working" rather than "broken." DESIGN/DEVOPS waves should establish a P95 budget (proposed: < 15 s warm, < 60 s cold for typical install).

---

## Backlog Suggestions (by build order)

| Order | Story | Release | Outcome Link | Notes |
|---|---|---|---|---|
| 1 | US-U1 | WS | KPI-1 | Foundation — unblocks everything |
| 2a | US-U2 | WS | KPI-1 | Trivial wiring once classifier has data |
| 2b | US-U3 | WS | KPI-1 | Parallel with U2; same source |
| 3a | US-U4 | WS | KPI-2 | Wire `u` keypress on main view to existing dialog |
| 3b | US-U7 | WS | KPI-4 | Parallel with U4; left-pane filtering |
| 4 | US-U5 | WS | KPI-3 | Apply path; reuses existing `actions::unify::run()` |
| 5 | US-U6 | WS | KPI-3 | Re-classify after action — completes the loop |
| 6 | US-U10 | R2 | KPI-3 | Partial-success toast polish |
| 7 | US-U8 | R2 | KPI-4 | Empty state |
| 8 | US-U9 | R2 | KPI-4 | Detail screen |

---

## Anti-Patterns Avoided

- **Feature-first slicing**: NOT releasing "all hashing" then "all UI" then "all unify" — instead, walking skeleton touches every activity end-to-end. Each WS story is a thin slice across the full backbone.
- **Effort-based priority**: US-U1 is the highest-effort WS story but also the most enabling — it ships first because outcome impact is high, not last because it's hard.
- **Orphan stories**: every story above traces to KPI-1, 2, 3, or 4.
