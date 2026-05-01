# Story Map: cross-tool-model-unify

## User: Devon (small-team developer with Ollama, LM Studio, HF cache, Atomic Chat)
## Goal: Unify duplicate model files across all tools into one shared inode and SEE the disk reclaimed.

## Backbone (user activities, left to right)

| Discover | Compute Hashes | See Dedup-able | Unify | View Unified |
|---|---|---|---|---|
| Launch modeltap | Hash files in background | Read summary bar | Press `u` on a row | Navigate to `[All Unified]` |
| See rows render fast | See progress in status line | Scan row glyphs | Confirm dialog | Read row data |
| (existing v1) | (NEW infra) | (FIX hardcoded 0) | Apply plan | (NEW pseudo-tool) |
| | | | See reclaim | (Optional) detail |

---

## Story Inventory

| ID | Story | Activity | In Skeleton? | Priority |
|---|---|---|---|---|
| US-U1 | Background SHA256 hashing with progress | Compute Hashes | YES | P1 |
| US-U2 | Wire dedup-able bytes from classifier to summary bar | See Dedup-able | YES | P1 |
| US-U3 | Row glyph reflects dedup state ({?, ~, -, =, #}) | See Dedup-able | YES | P1 |
| US-U4 | `u` from main view opens unify dialog with mates pre-populated | Unify | YES | P1 |
| US-U5 | Unify dialog shows concrete reclaim preview and applies plan | Unify | YES | P1 |
| US-U6 | After unify, row glyph and summary bar update without restart | Unify | YES | P1 |
| US-U7 | `[All Unified]` pseudo-tool slot in left pane | View Unified | YES | P1 |
| US-U8 | `[All Unified]` empty state with onboarding guidance | View Unified | NO | P2 |
| US-U9 | Detail screen for unified model shows shared inode and paths | View Unified | NO | P2 |
| US-U10 | Partial-success reporting (per-target outcome in toast) | Unify | NO | P2 |

---

## Walking Skeleton (P1, MVP-1)

The thinnest end-to-end slice that delivers the core promise: "see real dedup-able bytes -> press `u` -> see real reclaim."

**Stories**: US-U1, US-U2, US-U3, US-U4, US-U5, US-U6, US-U7

**End-to-end flow it enables**:

1. Devon launches modeltap.
2. Background hashing kicks off (US-U1) — status line shows progress.
3. As hashes settle, classifier produces dedup-able bytes (US-U2) and row glyphs (US-U3) — both reading from one source.
4. Devon navigates to a `=` row, presses `u` (US-U4) — dialog opens with mates pre-populated.
5. Devon presses Enter (US-U5) — plan applies, reclaim shown.
6. Row glyph flips to `#`, summary bar updates (US-U6) — without restart.
7. Devon navigates left pane to `[All Unified]` (US-U7) — sees the model in the unified list.

This is the minimum that makes the feature work end-to-end. Without any one of US-U1..U7, the user-visible promise is broken.

**MVP-2 (P2 nice-to-haves)**:
- US-U8 empty state (only matters before any model is unified — not blocking core flow).
- US-U9 detail screen (auditing/proof — confidence boost but the row + summary already prove it worked).
- US-U10 partial-success reporting (the apply still works for happy path; partial-success cases are rare and the existing JSONL log already captures them — this story improves the toast only).

---

## Scope Assessment: PASS

- 10 user stories total, **7 in walking skeleton** (within the right-sized envelope)
- **3 bounded contexts touched**: `modeltap-core` (dedup classifier, hash queue), `modeltap-tui` (rendering, dialog, key handling), `modeltap-app` (composition root for background-hash worker spawn)
- **2 integration points** (per Tool plugin: existing `Tool::link()`, existing per-tool path discovery — already in place)
- **Estimated total effort**: 8-12 days for the 7 P1 stories. P2 stories add 3-5 days. Total ~10-15 days.

This is a tight feature within the elephant-carpaccio envelope. No splitting recommended.

---

## Dependencies (between stories)

```
US-U1 (background hashing)
   |
   +-> US-U2 (summary bar reads classifier)
   |     |
   |     +-> US-U6 (post-unify update)
   +-> US-U3 (row glyphs)
   |     |
   |     +-> US-U6 (post-unify update)
   |     +-> US-U7 ([All Unified] slot — needs # glyph to know what to list)
   |
US-U4 (u from main view) -> US-U5 (apply plan) -> US-U6 (post-unify update)
                                                  -> US-U10 (partial success reporting, P2)

US-U7 (pseudo-tool slot) -> US-U8 (empty state, P2)
                          -> US-U9 (detail screen, P2)
```

US-U1 unblocks everything. Build order: U1 -> {U2, U3 in parallel} -> {U4, U7 in parallel} -> U5 -> U6 -> P2 stories.
