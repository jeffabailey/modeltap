# Definition of Ready (DoR) Validation — tool-model-info-sqlite-cache

Hard gate: every story must pass all 9 DoR items before DESIGN handoff.

## Per-Story DoR Status

| Story | 1. Problem clear | 2. Persona specific | 3. ≥3 examples | 4. UAT 3-7 | 5. AC from UAT | 6. Right-sized | 7. Tech notes | 8. Deps tracked | 9. KPI defined | Status |
|---|---|---|---|---|---|---|---|---|---|---|
| US-21 | PASS | PASS | PASS (3) | PASS (5) | PASS (9) | PASS (~2d) | PASS | PASS (3 parent + Q-INFO-1 DESIGN-open) | PASS (K-INFO-5, O4, O8) | **PASSED** |
| US-22 | PASS | PASS | PASS (4) | PASS (6) | PASS (10) | PASS (~2d) | PASS | PASS (US-13 parent + Q-INFO-1 DESIGN-open) | PASS (O1, O8, K-INFO-6) | **PASSED** |
| US-23 | PASS | PASS | PASS (7) | PASS (7) | PASS (12) | PASS (~2-3d) | PASS | PASS (new modeltap-store crate + ADR-pending) | PASS (K-INFO-4, K-INFO-7, O9, O10) | **PASSED** |
| US-24 | PASS | PASS | PASS (4) | PASS (4) | PASS (9) | PASS (~1d) | PASS | PASS (US-23, US-26, parent US-08) | PASS (O5, K-INFO-2) | **PASSED** |
| US-25 | PASS | PASS | PASS (3) | PASS (3) | PASS (7) | PASS (~1d) | PASS | PASS (US-23, US-26) | PASS (K-INFO-1, K-INFO-3, O2) | **PASSED** |
| US-26 | PASS | PASS | PASS (6) | PASS (6) | PASS (9) | PASS (~2-3d) | PASS | PASS (US-23, US-25, parent ADR-006, parent US-05/05b/05c/10) | PASS (K-INFO-3, O3 guardrail, K5 extends) | **PASSED** |
| US-27 | PASS | PASS | PASS (3) | PASS (3) | PASS (8) | PASS (~2d) | PASS | PASS (US-23, US-26, parent ADR-013) | PASS (O6, K-INFO-8) | **PASSED** (Release 3 deferred) |

## Aggregate

- **Total stories:** 7
- **Stories PASSED:** 7
- **Stories BLOCKED:** 0
- **Overall DoR status:** **PASSED**

## Item-by-Item Evidence (representative coverage; not exhaustive per story)

### 1. Problem statement clear and in domain language

Every story's Problem section uses domain language: "multi-tool local-AI power user", "Ollama manifest fields", "GGUF header KVs", "HF config.json excerpts", "SQLite cache", "warm-start paint", "pre-mutate revalidation", "cache corruption", "schema migration". No engineering jargon detached from user concerns ("microservice", "REST endpoint"). The persona (Devon Park, parent feature persona) and the artifacts (cache.sqlite, tool detail screen, model detail screen) are concrete and traceable.

Example (US-22): "Devon Park is about to run `mistral:7b-instruct-q4_K_M` via Ollama for a real task and wants to confirm: is this the same Mistral he downloaded last week, or did `ollama pull` upgrade it to a newer revision?"

### 2. User/persona identified with specific characteristics

All 7 stories use Devon Park, the parent feature persona, with additional context per story:

- US-21: hits the tool detail screen when a tool's row looks suspicious or when triaging bug reports.
- US-22: hits the model detail screen before running a model for real work, before unify/delete, or when comparing two similar models.
- US-23: hits the cache layer on every launch; needs concurrent processes to work; needs corruption recovery.
- US-24: hits manual refresh during any download / install workflow with modeltap open in parallel.
- US-25: hits warm-start on every launch after the first.
- US-26: hits background reconcile on every launch and pre-mutate revalidation on every destructive action.
- US-27: power-user case — large library, fresh launches.

Maintainer (Jeff Bailey, solo dev) is also called out as a stakeholder for US-23 (schema migration). No "user" or "developer" generic terms.

### 3. At least 3 domain examples with real data

Every story has 3+ numbered domain examples using real model names, real paths, real sizes, real timestamps, real PIDs:

- **US-21:** 3 examples (Ollama 0.6.4, 13 models, 66.5 GB; llama-cli undetectable version; Ollama `permission denied` with errno 13).
- **US-22:** 4 examples (Mistral-7B-Instruct-q4_K_M GGUF v3 with full KV listing; re-introspect after `ollama pull`; corrupt GGUF graceful degrade; HF `meta-llama/Llama-3-8B` safetensors with config.json).
- **US-23:** 7 examples (fresh install; v1→v3 migration; SQLITE_CORRUPT recovery; concurrent reads via WAL; concurrent write serialisation via busy_timeout=5000; --no-cache bypass; downgrade detection).
- **US-24:** 4 examples (post-`ollama pull` refresh; Shift+R all tools; dialog-open no-op; provenance line during reconcile).
- **US-25:** 3 examples (warm-start instant paint; cold start fresh install; mixed warm/cold per-tool when stale).
- **US-26:** 6 examples (silent reconcile; inventory diff with blue `*` ack; failed reconcile preserves last-known-good; pre-unify match; pre-unify drift re-introspect; pre-mutate file-gone abort).
- **US-27:** 3 examples (hash persists when file unchanged; hash invalidates on mtime change; `modeltap cache verify` drift detection).

All examples use real data: `bartowski/Llama-3.2-1B-Instruct-GGUF`, `mistral:7b-instruct-q4_K_M`, `meta-llama/Llama-3-8B`, `~/.ollama/models/blobs/sha256-8f3e9c102a4b...c102`, PID 4421, errno 13, schema versions 1/3/5, `dirs::data_dir().join("modeltap/cache.sqlite")`. No `user123`, `test_model`, or `foo@bar.com`.

### 4. UAT scenarios in Given/When/Then (3-7 scenarios)

Each story has 3-7 UAT scenarios (all within the right-sized band):

- US-21: 5 scenarios | US-22: 6 scenarios | US-23: 7 scenarios | US-24: 4 scenarios | US-25: 3 scenarios | US-26: 6 scenarios | US-27: 3 scenarios.

All scenarios are Given/When/Then. The full `journey-info-and-cache.feature` file contains 30+ scenarios — these per-story UAT subsets are the canonical-acceptance subset; the others are integration / edge-case coverage that DISTILL will own.

### 5. Acceptance criteria derived from UAT

Each story has explicit ACs (AC-21-1..AC-27-8) plus cross-feature integration ACs (INT-INFO-1..INT-INFO-9). Total: **73 ACs** across the 7 stories. Trace table in `acceptance-criteria.md`. Every AC cites the UAT scenario(s) it derives from OR a `(implementation-checkpoint)` / `(NFR — performance)` / `(cross-trace AC-XX-Y)` source.

### 6. Story right-sized (1-3 days, 3-7 scenarios)

All 7 stories are within the 1-3 day / 3-7 scenario band. Largest is US-23 (cache schema + recovery + concurrency + `--no-cache`) at ~2-3 days, 7 scenarios — still within the band, and explicitly composed as the load-bearing infrastructure story that the other cache stories depend on. Per `story-map.md` Scope Assessment:

- 7 stories total (≤10 threshold)
- 1 new bounded context (`modeltap-store`) + 6 existing context modifications (borderline; justified in story-map.md)
- 4 integration points (Tool trait extension, cache reads on launch, reconcile orchestrator, write-after-action)
- ~10-14 days estimated total effort
- 2 user outcomes (inspection, instant-launch) split into 2 releases

Scope Assessment verdict: **PASS** — feature is right-sized within 3-release framing.

### 7. Technical notes identify constraints

Each story's "Technical Notes" section identifies:

- Required Tool trait extensions (Q-INFO-1)
- Cross-platform path resolution (`dirs` crate, C-INFO-5)
- Schema versioning strategy (Q-INFO-3, recommended rusqlite_migration)
- Per-plugin metadata sources (Q-INFO-7, best-effort per-plugin)
- The cache safety rule (C-INFO-1 — cache paint-only, filesystem authoritative on mutate)
- Plugin panic isolation invariant (extends parent US-18)
- ADR-013 background hash pool reuse for US-27
- `rusqlite_migration` recommendation for migration tooling
- `cache.write_queue` channel for cache-write discipline

Plus 8 open questions for DESIGN (Q-INFO-1..Q-INFO-8) explicitly tracked and allowed to stay open through DISCUSS handoff — they belong to DESIGN's bounded context.

### 8. Dependencies resolved or tracked

Each story explicitly lists dependencies:

- **US-21:** parent US-03 (existing), US-08 (existing), US-18 (Tool trait — extension Q-INFO-1 DESIGN-open).
- **US-22:** parent US-13 (existing, extended), US-18 (same extension), US-21 (shares trait scaffolding).
- **US-23:** new `modeltap-store` crate; DESIGN ADR superseding ADR-003 must be written; DESIGN ADR for schema migration strategy.
- **US-24:** US-23 (cache exists), US-26 (reconcile orchestrator), parent US-08.
- **US-25:** US-23, US-26.
- **US-26:** US-23, US-25, parent ADR-006, parent US-05/05b/05c/10.
- **US-27:** US-23, US-26, parent ADR-013, new CLI subcommand surface.

All cross-story dependencies are tracked. All parent dependencies are PASSED-DoR + in-DELIVER or shipped. The folder-group-bulk-delete sequencing question is explicitly resolved in `prioritization.md` (Option C — queue this feature's DESIGN behind folder-group DELIVER).

### 9. Outcome KPIs defined with measurable targets

8 KPIs defined in `outcome-kpis.md` covering all 7 stories:

- K-INFO-1 (warm-start ≤ 100 ms p90) — US-25
- K-INFO-2 (refresh ≤ 1 s) — US-24
- K-INFO-3 (cache hit ratio ≥ 80%) — US-25, US-26
- K-INFO-4 (corruption recovery 100%) — US-23 (guardrail)
- K-INFO-5 (in-TUI error resolution ≥ 80%) — US-21
- K-INFO-6 (decisive action from detail screen ≥ 90%) — US-22
- K-INFO-7 (cache layer overhead ≤ 50 ms) — US-23 (guardrail)
- K-INFO-8 (unify dialog hash readiness ≥ 70%, Release 3) — US-27

All KPIs have measurement methods, baselines (where applicable), and traceability to JTBD opportunity scores. K3 (parent) is explicitly redefined into K3a (warm) + K3b (cold) with both passing requirements.

## Anti-Pattern Scan

| Anti-Pattern | Detected? | Notes |
|---|---|---|
| Implement-X | NO | Every story framed from Devon's pain (context-switch to `gguf-dump`, quit-and-relaunch ceremony, "(error)" investigation, cumulative launch cost). US-23 includes the maintainer pain (schema migration safety) but the user-visible outcome (clean recovery) is what's tested. |
| Generic Data | NO | All examples use real model names, real paths, real sizes, real timestamps, real PIDs. No `user123`. |
| Technical AC | NO | ACs are observable user outcomes. The implementation-checkpoint ACs (e.g., AC-23-1 "Cache file location resolves via `dirs::data_dir()`") are observable via filesystem inspection (where does the file end up?), not implementation directives. AC-26-8 ("Integration test asserts every mutation site goes through revalidator") is the explicit safety-rule enforcement AC; observable via test pass/fail. |
| Oversized Story | NO | Largest is US-23 at ~2-3 days, 7 scenarios — within the band. US-26 at ~2-3 days, 6 scenarios — within the band. All others 1-2 days. |
| No Examples | NO | 3-7 examples per story with real data. |
| Tests After Code | N/A | DELIVER wave concern; UAT scenarios defined here, tests will be RED-first per parent feature's discipline. |
| Solution-prescriptive AC | NO | ACs describe observable outcomes (e.g., "Cache layer overhead ≤ 50 ms additional startup") not implementation choices (e.g., does NOT say "Use connection pool"). Choices like "rusqlite_migration" appear in Technical Notes / `prioritization.md` as RECOMMENDATIONS for DESIGN, not as ACs. |

## Recovery / Remediation

No DoR failures requiring remediation. All 9 items PASS across all 7 stories. Anti-pattern scan clean.

## DoR Status: **PASSED (9/9 across all 7 stories)**

Ready for peer review and DESIGN handoff. Open questions Q-INFO-1..Q-INFO-8 are explicitly tracked as DESIGN-must-close items and do NOT block this DoR. Sequencing recommendation (queue behind folder-group-bulk-delete DELIVER) is in `prioritization.md`; user-confirmable at DESIGN handoff.
