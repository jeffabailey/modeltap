# Story Map: modeltap-tui

## User: Devon Park (local-AI power user, macOS/Linux, terminal-comfortable, runs 2+ inference tools)
## Goal: See, deduplicate, and clean up locally-downloaded AI models across multiple tools without copying bytes.

## Backbone

The user activities, in chronological order across the journey:

| Launch | Discover | Browse | Inspect | Act | Verify |
|---|---|---|---|---|---|
| Run modeltap | Build inventory of installed tools and models | Navigate left/right panes | Look at duplicates and locked models | Unify or Zap | See bytes reclaimed |

## Story Map (backbone + ribs)

| **Launch** | **Discover** | **Browse** | **Inspect** | **Act** | **Verify** |
|---|---|---|---|---|---|
| US-01 TUI opens with stub data and quits cleanly **[WS]** | US-02 Discover Ollama models **[WS]** | US-03 Two-pane layout with tools left, models right **[WS]** | US-04 Show model size and registered tools | US-05 Zap a tool's models with typed confirmation **[WS]** | US-06 Show last action and reclaimed bytes **[WS]** |
| | US-07 Discover llama-cli models | US-08 Bottom bar with shortcuts always visible | US-09 Compatibility indicator (o/*/!) on each row | US-10 Unify a model across tools using hardlinks | US-11 Updated totals after action |
| | US-12 Discover Hugging Face cache models | | US-13 Model detail screen with paths and reclaim estimate | US-14 Dry-run preview before unify | |
| | US-15 Discover LM Studio models | | US-16 Format-locked indicator (red `!`) for one-tool-only models | US-17 Detect running tools and warn before unify/zap | |
| | US-18 Plugin trait — adding a 5th tool requires no core changes **[ARCH]** | | | US-19 Hardlink fallback when cross-filesystem | |
| | US-20 Cross-platform path discovery (macOS + Linux) **[ARCH]** | | | | |

Legend: **[WS]** = Walking Skeleton story. **[ARCH]** = architectural constraint expressed as a story.

---

## Walking Skeleton (Release 0)

The thinnest end-to-end slice — `modeltap` opens, discovers Ollama, shows the two-pane layout, lets the user zap (with confirmation), and shows the result.

**Stories in WS:** US-01, US-02, US-03, US-05, US-06.

This is deliberately thinner than the recommendation in the task brief (which suggested stub data → real Ollama → `u`/`z` later). I am proposing a **slightly different walking skeleton**: the very first slice should already include real Ollama discovery and zap, because:

1. Stub-data-only is below the value bar — it does not validate the riskiest assumption (that we can correctly discover and act on a tool's on-disk layout).
2. Zap on a single tool exercises the destructive action path that all other features depend on (confirmation UX, post-action verification).
3. Unify is genuinely harder (canonical store, hardlinks, dedup key strategy — Q6 is still open) and rightly belongs in Release 1 once the framework is shaped.

**Why these 5 stories form a complete end-to-end slice:**

- US-01: process boots and exits cleanly (Launch activity)
- US-02: real discovery against at least one tool (Discover activity)
- US-03: two-pane layout renders inventory (Browse activity)
- US-05: at least one mutating action with safety guard (Act activity — Inspect activity is acknowledged but trivially satisfied by the basic right-pane render in US-03)
- US-06: post-action feedback (Verify activity)

Inspect activity is satisfied minimally by the right pane (US-03 lists models with size). The richer Inspect features (US-04, US-09, US-13, US-16) come in Release 1.

---

## Release 1: "Make the duplication problem visible"

**Outcome target:** Devon can see how much disk space is wasted on duplicate models without doing any math himself. Drives KPI K2 (% of registered models that are deduplicable).

**Stories:** US-04, US-07, US-09, US-12, US-13, US-15, US-16.

This release adds the other three tools (so the cross-tool view is real, not theoretical), the compatibility indicators (so duplicates are visually distinguishable), and the model detail screen (so the size-of-the-problem is quantified per model).

After Release 1, Devon can answer "how much could I reclaim?" without unifying anything — a valuable diagnostic on its own.

---

## Release 2: "Reclaim disk space safely"

**Outcome target:** Devon can deduplicate models cross-tool with zero data loss and clear feedback. Drives KPI K1 (disk space reclaimed per session).

**Stories:** US-08, US-10, US-11, US-14, US-17, US-19.

Adds unify (US-10) with dry-run (US-14), running-tool detection (US-17), cross-filesystem fallback (US-19), and the always-visible bottom bar (US-08 — could move earlier but is a polish story; deferred so Release 1 ships sooner).

---

## Release 3: "Built to grow"

**Outcome target:** A contributor can add a new tool (e.g., Jan) without modifying core. Drives KPI K4 (number of supported tools).

**Stories:** US-18, US-20.

US-18 (plugin trait) is technically already required by Releases 0–2 — the architectural seam exists from day one. This release is about **publishing and documenting the plugin contract**: trait stability, registration mechanism, capability metadata schema, contributor docs. It's the difference between "the code is structured this way" and "external contributors can actually use it."

US-20 (cross-platform path discovery) similarly applies from day one but is broken out so each plugin's macOS-and-Linux paths can be validated explicitly.

> Note: US-18 and US-20 are tagged **[ARCH]** because they encode hard constraints DESIGN must respect. They produce stories not because they are end-user features but because they have observable, testable acceptance criteria.

---

## Scope Assessment: PASS

- 14 user stories total (US-01 through US-20, with gaps for renumbering during DESIGN)
- 1 bounded context (the modeltap process — plugins are extensions of the same context)
- ~10 days estimated total effort across 4 releases
- Walking skeleton: 5 stories, ~2-3 days
- Each release ships independent value

This is right-sized for a single delivery cycle. No Elephant Carpaccio split needed.

> Story IDs (US-01..US-20) are assigned here for cross-document referencing in Phase 4. Final IDs may be renumbered during requirements crafting; trace links will be preserved.
