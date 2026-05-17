# JTBD Opportunity Scoring — tool-model-info-sqlite-cache

Applies Ulwick's opportunity algorithm to the seven jobs from `jtbd-job-stories.md`. Outcomes are derived per job (walking the 8-step job map where applicable) and rated for Importance and Satisfaction.

**Scoring method:** `Score = Importance + max(0, Importance - Satisfaction)` (0-20 scale).

**Data source:** Team estimate (single solo developer Jeff Bailey playing Devon persona). Per the small-team adaptation in `nw-jtbd-opportunity-scoring`, scores are **relative rankings** rather than absolute. Will be refined after first-week-post-release self-reported telemetry from the developer's own usage.

**Confidence:** MEDIUM — single-rater team estimate; rationale captured per outcome so scores can be challenged.

## Outcome statements

Outcomes derived from the 8-step job map applied to "manage and trust my local-AI model inventory across multiple tools." Each outcome is solution-free and measurable.

| # | Outcome Statement | Job link | Importance % | Satisfaction (today, with parent feature shipped) % | Score | Priority |
|---|---|---|---|---|---|---|
| O1 | Minimize the time to confirm a model's quantisation, format, and dedup identity before acting on it | J1 | 90 | 25 | 15.5 | Extremely Underserved |
| O2 | Minimize the time from `modeltap` launch to a usable inventory view across launches 2..N (warm start) | J3 | 85 | 35 | 13.5 | Underserved |
| O3 | Minimize the likelihood of acting on stale or wrong model metadata during destructive operations | J3, J5 | 95 | 80 | 11.0 | Appropriately Served (guardrail — already strong via stateless rediscovery) |
| O4 | Minimize the time to diagnose why a specific tool shows "(error)" or surprising model counts | J2 | 70 | 30 | 12.0 | Underserved |
| O5 | Minimize the time to reflect an out-of-band model change (e.g., `ollama pull` in another terminal) in modeltap | J4 | 65 | 20 | 12.5 | Underserved |
| O6 | Minimize the time to re-hash unchanged files across launches | J5 | 60 | 15 | 11.5 | Appropriately Served (just below underserved) |
| O7 | Minimize the likelihood of "modeltap won't start" caused by its own persistence layer | J6 | 95 | 100 | 9.5 | Overserved (today — but creating the cache creates the gap; this is the guardrail that prevents J6 from becoming a top-3 opportunity post-cache) |
| O8 | Minimize the time to discover tool-specific metadata (Ollama manifest fields, GGUF header, HF config.json) without leaving the TUI | J1, J2 | 80 | 20 | 14.0 | Underserved |
| O9 | Minimize the likelihood of cache corruption causing data loss or unrecoverable launch failure | J6 | 90 | (n/a — feature doesn't exist yet) | n/a | (cannot score until cache exists; called out as a mandatory guardrail for the feature) |
| O10 | Minimize the likelihood of two concurrent modeltap processes corrupting shared state | J7 | 50 | (n/a) | n/a | Same — table-stakes for the feature; not an opportunity-scoring target |

## Scoring interpretation

| Score Range | Bucket | Outcomes in this bucket |
|---|---|---|
| 15-20 | Extremely Underserved | O1 |
| 12-15 | Underserved | O2, O4, O5, O8 |
| 10-12 | Appropriately Served | O3, O6 |
| < 10 | Overserved | O7 (today) |
| n/a | Mandatory guardrails | O9, O10 (cannot score until cache exists) |

## Top 3 opportunities (driving story priority)

### #1 — O1: Confirm a model's quantisation, format, and dedup identity (Score 15.5)

**Job:** J1 (Verify a model)

**Why it's the top opportunity:** Devon's mental model of any model is "name + size + indicator," which is insufficient for high-stakes actions (unify, delete). Today he context-switches to `gguf-dump`/`huggingface-cli` to close the gap — high friction, low satisfaction. The intake brief leads with "Add an ability to get information about each tool and each model" — confirming the user's own ranking matches.

**Translates to stories:** US-22 (model detail view) is the primary carrier. US-24 (manual refresh) supports it (Devon must trust the metadata he sees).

### #2 — O8: Discover tool-specific metadata without leaving the TUI (Score 14.0)

**Job:** J1 + J2

**Why it's #2:** Closely related to O1; differs in being about *discoverability* of metadata that already exists per tool but is currently invisible. Ollama exposes a manifest JSON, GGUF files have a structured header, HF caches have `manifest.json` and `config.json`. Surfacing these is mechanical work with high user payoff.

**Translates to stories:** US-22 (model detail), US-21 (tool detail). Both stories surface tool-native introspection.

### #3 — O2: Warm-start launch latency (Score 13.5)

**Job:** J3 (Trust first-paint)

**Why it's #3:** The user-visible payoff is sharpest here — sub-100ms warm-start is "instant" in human perception, vs. the current ~150 ms skeleton + ~1.15 s full inventory. This is the architectural-refactor justification: persistence exists to make warm-start instant. But the score is moderated by the high anxiety force (J3 in `jtbd-four-forces.md`) — getting this wrong creates K5 (safety) regressions.

**Translates to stories:** US-23 (cache schema and persistence), US-25 (warm-start cache reads), US-26 (background reconcile).

## Underserved (12-15) drivers

### O4 — Diagnose tool health (Score 12.0)

**Job:** J2

Lower importance than O1/O8 because "(error)" doesn't happen often, but when it does the satisfaction is very low (Devon goes to a separate file). One story (US-21 tool detail) covers this.

### O5 — Reflect out-of-band changes (Score 12.5)

**Job:** J4

Lower importance because Devon's primary workflow is read-then-act-in-modeltap (not download-then-check), but the satisfaction gap is large (quit+relaunch is the current refresh). Cheap to deliver via US-24.

## Appropriately served (do not over-invest)

### O3 — Don't act on stale data (Score 11.0)

The current stateless rediscovery model already provides high satisfaction (filesystem is always re-checked). Introducing the cache MUST not regress this — design rule: cache is paint-only, filesystem is authoritative on mutate. US-25 and US-26 must include the re-validate-before-mutate guardrail.

### O6 — Avoid re-hashing unchanged files (Score 11.5)

Background hash pool (ADR-013) already amortises this cost. The persistent SHA256 cache (US-27) is an incremental improvement, not the primary win. Defer to Release 2 if needed.

## Overserved / Guardrails

### O7 — modeltap won't start (Score 9.5 today; could spike post-cache)

ADR-003 explicitly avoided this whole class of failure by not having persistence. Introducing persistence re-opens it. The mitigation — auto-recovery on corruption (US-23 includes corruption-recovery AC) — IS the design that keeps O7 in the "overserved" bucket post-cache. Without that recovery, O7 would score 15+ on its first failure.

### O9 + O10 — Mandatory guardrails

Not opportunity-scoreable because the feature creates the gap; they must be solved as part of the feature itself, not deferred.

## Mapping outcomes to user stories

| Outcome | Score | Story |
|---|---|---|
| O1 | 15.5 | US-22 Model detail view (primary), US-24 Manual refresh (supports trust) |
| O8 | 14.0 | US-22 Model detail view, US-21 Tool detail view |
| O2 | 13.5 | US-23 Cache schema, US-25 Warm-start cache reads |
| O5 | 12.5 | US-24 Manual refresh keybinding |
| O4 | 12.0 | US-21 Tool detail view |
| O3 | 11.0 | Cross-cutting AC in US-25/US-26 (revalidate-before-mutate guardrail) |
| O6 | 11.5 | US-27 SHA256 persistence (Release 2 candidate) |
| O7 | 9.5 | US-23 AC includes corruption-recovery (keeps O7 overserved post-cache) |
| O9 | n/a (mandatory) | US-23 corruption-recovery AC |
| O10 | n/a (mandatory) | US-23 concurrency AC (SQLite WAL + busy timeout) |

## Top-of-backlog ordering (input to `prioritization.md`)

1. **US-22 + US-21** (info-views) — Highest scoring opportunities (O1, O8); user-visible motivator the user himself ranked first in the intake brief; usable on day one with stateless cold reads (no cache dependency).
2. **US-23** (cache schema + corruption recovery + concurrency) — Architectural prerequisite for US-25/US-26; ships the SQLite layer with empty tables; corruption recovery and WAL mode mandatory from day one.
3. **US-24** (manual refresh) — Cheap multiplier on J3/J4; needed to support the "as of <timestamp>" provenance contract.
4. **US-25 + US-26** (warm-start reads + background reconcile) — Delivers O2 (the architectural refactor's user-facing payoff). Cannot ship before US-23.
5. **US-27** (SHA256 persistence) — Incremental win on O6; defer to Release 2.

This ordering is the basis for `prioritization.md`.

## Data quality notes

- **Source:** Single-rater team estimate (solo developer = product owner = persona).
- **Sample size:** N=1.
- **Confidence:** MEDIUM — rationale documented per outcome; revisit after first-week-post-release self-reported telemetry.
- **Caveat:** Importance scores are biased toward "what I notice when I dogfood." Outcomes that other users care about (e.g., team-wide model audits, GPU memory mapping) are not represented.
