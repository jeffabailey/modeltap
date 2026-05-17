# Story Map: tool-model-info-sqlite-cache

## User: Devon Park (multi-tool local-AI power user, macOS/Linux, parent feature persona)

## Goal: Open modeltap, see a useful inventory **instantly** from cached state, drill into one tool or one model to confirm metadata before acting, and always know how fresh the displayed data is.

## Brownfield Context

This feature is a **cross-cutting extension** to the parent `modeltap-tui` feature (currently in DELIVER wave with 21 stories US-01..US-20 + US-05b live, plus US-05c folder-group-bulk-delete in DELIVER). It introduces:

- **A new user-visible capability** — per-tool and per-model detail screens (J1, J2).
- **An architectural refactor** — SQLite-backed persistence of tool and model metadata (J3, J5, J6, J7).
- **A constraint reversal** — supersedes ADR-003 (stateless rediscovery, no persistent index) with a cache-paint + filesystem-authoritative model.

The full multi-activity story-map exercise from the parent is not re-done; this map shows the new stories within the parent's existing backbone.

## Backbone (within the parent journey)

The parent journey backbone is: `Launch → Discover → Browse → Inspect → Act → Verify`. This feature adds tasks at **Launch** (cached paint), **Browse** (provenance + refresh), and **Inspect** (new tool/model detail screens), plus invariants at **Act** (pre-mutate revalidation).

| Launch | Discover | Browse | Inspect | Act | Verify |
|---|---|---|---|---|---|
| **(NEW)** Warm-start paint from SQLite cache | **(NEW)** Background reconcile after warm paint | **(NEW)** Provenance line "as of <timestamp>" in summary bar | **(NEW)** Tool detail screen (Enter on left pane) | **(NEW invariant)** Pre-mutate re-stat against cache | (unchanged from parent) |
| **(NEW)** Schema migration on launch | **(NEW)** Per-tool TTL eligibility | **(NEW)** `[r]` refresh tool, `[Shift+R]` refresh all | **(EXTENDS US-13)** Model detail screen with tool-native metadata | (existing parent flows: unify, zap, delete-one, folder-delete) | |
| **(NEW)** Cache corruption recovery → cold-start fallback | | **(NEW)** Silent ack indicator on cross-launch inventory diff | **(EXTENDS US-13)** Re-introspect shortcut on detail screens | | |
| **(NEW)** `--no-cache` flag for ADR-003 baseline path | | | | | |

## Story map (backbone + ribs)

| **Launch** | **Discover** | **Browse** | **Inspect** | **Act** | **Verify** |
|---|---|---|---|---|---|
| **US-23** Cache schema + SQLite-backed persistence (incl. corruption recovery, WAL concurrency, `--no-cache` opt-out) **[WS-CACHE]** | **US-25** Warm-start paint from cache (≤100 ms first paint when cache valid) **[WS-CACHE]** | **US-24** Manual refresh keybindings (`[r]`, `[Shift+R]`) + provenance line | **US-21** Tool detail screen (Enter on left-pane row) **[WS-INFO]** | **US-26** Background reconcile after paint + cache-safety rule (pre-mutate revalidation) **[WS-CACHE]** | (parent US-06, US-11 unchanged) |
| | | | **US-22** Model detail screen with tool-native metadata (Enter on right-pane row; extends US-13) **[WS-INFO]** | | |
| | | | | **US-27** Persisted SHA256 cache across launches (mtime+size+inode_dev invalidation) | |

Legend:
- **[WS-INFO]** = walking-skeleton-for-info-views (the minimum to deliver J1+J2 alone, without the cache).
- **[WS-CACHE]** = walking-skeleton-for-cache (the minimum to deliver J3 with safety guardrails).

---

## Walking Skeleton(s)

**Per orchestrator config:** `walking_skeleton = no` (brownfield; the parent feature already established end-to-end TUI flow).

However, this feature has two internal "skeletons" — one for the user-visible value (inspection) and one for the architectural refactor (cache). The release sequencing in `prioritization.md` deliberately puts the info-views skeleton first so the user gets visible value immediately, while the cache ships in a separate slice with explicit corruption-recovery from day one.

### WS-INFO (US-21 + US-22)

The minimum slice for the **user-visible** half:
- US-21 tool detail screen — Enter on left-pane row shows install path, version, model count, disk usage, last scan, plugin version.
- US-22 model detail screen extension — Enter on right-pane row shows tool-native metadata (Ollama manifest fields, GGUF header, HF config.json excerpts).

These two stories ship **WITHOUT** the cache. They work against the existing stateless-rediscovery model. Metadata is introspected on-demand and cached **in-process** (in-memory) for the session — mirroring the existing SHA256 lazy-compute pattern. This means US-21 + US-22 can ship first, deliver O1 (15.5) and O8 (14.0) immediately, and prove the `inspect_tool()` / `inspect_model()` Tool trait additions are sound before the cache layer is built on top.

### WS-CACHE (US-23 + US-25 + US-26)

The minimum slice for the **architectural** half:
- US-23 — SQLite schema, migration framework, corruption-recovery, WAL concurrency, `--no-cache` opt-out. Empty tables on first launch; everything works exactly as ADR-003 because nothing has been written yet.
- US-25 — Warm-start paint reads cache on launch; cold start when no/stale cache.
- US-26 — Background reconcile updates the cache after paint; pre-mutate revalidation against filesystem before any destructive action.

These three stories ship as a unit because they're not independently demonstrable: shipping US-23 without US-25 means the cache is written but never read (no user value); shipping US-25 without US-26 means warm-start works but acts on potentially stale data (regresses K5).

### US-24 (manual refresh) and US-27 (SHA256 persistence)

- **US-24** can ship anywhere after US-26 — it's a small UX add (two hotkeys + provenance line). Folded into the cache release for coherence.
- **US-27** is a Release 2 candidate. The background hash pool (ADR-013) already amortises the hashing cost; persisting hashes is an incremental win on O6 (11.5). Defer until cache infrastructure has proven itself in the wild.

---

## Releases

### Release 1: Inspection (J1 + J2 delivered without architectural risk)

**Stories:** US-21, US-22

**Target outcome:** Devon opens any model or tool, presses Enter, sees the full picture (paths, metadata, dedup key, version, last scan, search paths). Closes the gap between "I have a name and a size" and "I have every fact I need to act."

**KPIs targeted:**
- O1 (15.5): minimise time to confirm a model's quantisation/format/dedup identity.
- O8 (14.0): minimise time to discover tool-specific metadata without leaving the TUI.

**Rationale for shipping first:** The user explicitly led the intake brief with "Add an ability to get information about each tool and each model." The cache is the *enabler* mentioned second. Shipping inspection first delivers user-visible value immediately, validates the `inspect_tool()` / `inspect_model()` Tool trait additions across all four plugins, and gives the cache a real workload to persist when it lands in Release 2.

**Estimated effort:** ~3-4 days (1 day per detail screen layout × 2 + per-plugin `inspect_*()` impls × 4 plugins ≈ 1 day each split across the stories = ~6 plugin-impl days but most are trivial JSON parsing). Largest unknown: HF plugin's metadata variety (config.json schema varies across model_types).

**Dependencies on parent (all PASSED DoR, in DELIVER or shipped):**
- US-03 (two-pane layout) — for cursor targeting
- US-13 (existing model detail screen — extended)
- US-08 (bottom bar) — gains new shortcuts
- US-18 (plugin trait) — gains `inspect_tool` and `inspect_model` methods (Q-INFO-1)

### Release 2: SQLite-backed cache (J3 + J6 + J7 delivered with explicit safety guardrails)

**Stories:** US-23, US-24, US-25, US-26

**Target outcome:** Devon's second-and-subsequent launches paint instantly from cache, provenance is always visible, manual refresh is one keystroke, and cache corruption never blocks the launch.

**KPIs targeted:**
- O2 (13.5): minimise warm-start launch time.
- O5 (12.5): minimise time to reflect out-of-band changes.
- O9 (mandatory guardrail): minimise likelihood of cache corruption causing data loss.
- O10 (mandatory guardrail): minimise likelihood of concurrent-process corruption.
- O3 (11.0, guardrail): ensure pre-mutate revalidation doesn't regress safety.

**Rationale for shipping second:** This is the architectural refactor. Splitting cache infrastructure (US-23) from cache reads (US-25) and reconcile (US-26) within this release is intentional — US-23 must ship corruption-recovery and WAL concurrency from day one, before any cache reads exist that could be poisoned by bad data. US-24 (manual refresh) is folded in because it's a small UX surface that completes the provenance contract.

**Estimated effort:** ~5-7 days (1d schema+migrator+recovery, 1d cache reads, 2d background reconcile + per-tool TTL + drift indicator, 1d revalidation rule + integration tests, 1d manual refresh + provenance line, 1d buffer for ADR + dogfooding).

**Dependencies on Release 1:** None hard — US-23/24/25/26 could ship without US-21/22. Soft dependency: Release 1's `inspect_tool()` / `inspect_model()` impls feed the cache writes for `cache.tools.last_scan_at`, `cache.models.metadata_kv`, etc. If Release 2 ships first, those fields are populated by the existing `discover()` call only.

### Release 3 (deferred): SHA256 persistence (J5 incremental win)

**Stories:** US-27

**Target outcome:** Re-launch of modeltap does not re-hash files that haven't changed since the last session.

**KPI targeted:** O6 (11.5) — appropriately served today via ADR-013 background hash pool; this is incremental.

**Rationale for deferring:** O6 is borderline underserved (11.5); J5 has strong anxiety (mtime-preserving file replacement) requiring careful design (cache validity check must include `inode_dev`, pre-unify must re-stat). After Release 2 ships and is dogfooded, US-27 lands as a config-flag-opt-in feature (`cache.persist_sha256 = true`) for early adopters, then enabled by default in a later release.

**Estimated effort:** ~2-3 days (schema addition + invalidation rule + `modeltap cache verify` dev command).

---

## Scope Assessment (Elephant Carpaccio Gate per Phase 2.7)

| Signal | Threshold | Actual | Status |
|---|---|---|---|
| Story count | ≤10 | 7 (US-21..US-27) | PASS |
| Bounded contexts touched | ≤3 | 3 (modeltap-tui, modeltap-app, new modeltap-store) + 4 plugins | **BORDERLINE** — see below |
| Integration points | ≤5 | 4 (Tool trait extension, cache reads on launch, reconcile orchestrator, write-after-action) | PASS |
| Estimated total effort | ≤2 weeks (10d) | ~10-14 days across 7 stories in 3 releases | PASS |
| Independent user outcomes | OK to split | 2 outcomes (inspection, instant-launch) split into 2 releases | PASS |

**Borderline call (bounded contexts):** A new `modeltap-store` crate is introduced — that's one new context. Modifications to `modeltap-app` (orchestration), `modeltap-tui` (rendering), and 4 plugins (each gains `inspect_*` impls) all stay within their existing contexts. Net: 1 new bounded context, modifications across 6 existing ones. This is at the upper end of what a single feature should touch, but:

- The 4 plugin modifications are mechanically similar (each gains 2 trait methods); they're not 4 independent design problems.
- The `modeltap-store` crate's surface is small (open / read / write / migrate / recover — ~6 public functions).
- Splitting the feature further (separate features for inspection vs. cache vs. SHA256 persistence) was considered and rejected: the intake brief frames them as one coupled change, the user-visible payoff of inspection alone (Release 1) and instant-launch alone (Release 2) is real but they share infrastructure (the Tool trait extension `inspect_tool` / `inspect_model` benefits both).

**Verdict: PASS** — feature is right-sized within the 3-release framing. Each release independently shippable; Release 1 delivers user value without Release 2; Release 2 doesn't depend on Release 1 hard.

## Scope Assessment: PASS — 7 stories, 1 new bounded context + 6 modifications, estimated 10-14 days across 3 releases

---

## Why "two coupled changes" is one feature, not two

The intake brief asks this explicitly. Decision and reasoning:

**One feature.** Reasons:

1. **The user requested them together** ("Add an ability to get information... Let's also refactor this code so it stores..."). Splitting them now would require re-asking the user to confirm both halves still want to ship as one DESIGN/DEVOPS/DISTILL/DELIVER cycle.
2. **They share infrastructure.** Both halves want `Tool::inspect_tool()` and `Tool::inspect_model()`. Splitting them means designing those trait methods twice (or in a separate cross-feature ADR).
3. **The cache is the *enabler* for instant inspection.** Without the cache, US-22's detail screen pays the introspection cost on every open. With the cache, the second-open is sub-100 ms. Shipping inspection without the cache delivers the value but leaves the latency on the table.
4. **The constraint reversal of ADR-003 is the single architectural decision** at stake. Forking the feature would force two separate ADRs (one for inspect_*, one for SQLite cache) — over-fragmenting decisions that are coupled.

**Counter-argument considered:** Inspection could ship as its own feature (`tool-model-info`) and the SQLite cache as its own (`sqlite-cache`). Pro: cleaner bounded-context separation. Con: doubles the wave overhead (two DESIGN handoffs, two DISTILL packages, two DELIVER cycles), and the cache's value is undermined without the inspection workload to populate it.

**Resolution:** keep them as one feature, ship as two releases internally (R1 = inspection without cache, R2 = cache + warm-start + reconcile), with US-27 (SHA256 persistence) deferred as R3.

---

## Dependencies on parent feature(s)

| Parent story | This feature's dependency |
|---|---|
| US-01 (TUI launches) | reused unchanged; cache opens during launch sequence |
| US-02 (Discover Ollama) | reused; `discover()` results now feed `cache.tools` + `cache.models` writes after reconcile |
| US-03 (Two-pane layout) | reused; Enter on left pane now opens tool detail (NEW) |
| US-08 (Bottom bar) | extended with `[r]`, `[Shift+R]`, `[Enter] tool detail` (left pane) |
| US-09 (Compatibility engine) | reused unchanged for indicator computation |
| US-11 (Updated totals after action) | reused; cache write happens post-action |
| US-13 (Model detail screen) | extended with Metadata section + re-introspect shortcut |
| US-17 (Detect running tools) | reused unchanged in pre-mutate revalidation pathway |
| US-18 (Plugin trait) | trait gains `inspect_tool` + `inspect_model` methods (Q-INFO-1) |
| ADR-003 (state model) | **SUPERSEDED** by new ADR that DESIGN must write — see `requirements.md` "Constraint reversal" section |
| ADR-006 (TUI architecture) | unchanged; new Msg variants for cache events, new Cmd variants for cache operations |
| ADR-013 (background SHA256 hash pool) | extended in US-27 (Release 3) to persist hash results |
| folder-group-bulk-delete (US-05c) | **Sequencing decision required** — see `prioritization.md` |

No new dependencies on stories outside the parent feature.
