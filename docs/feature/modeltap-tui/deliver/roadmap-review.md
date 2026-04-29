# Roadmap Review — modeltap-tui DELIVER wave

**Reviewer:** nw-software-crafter-reviewer (independent)
**Date:** 2026-04-29
**Verdict:** **APPROVED-WITH-FIXES** (M1 applied by orchestrator before dispatch of 01-01)

---

## Critical (block execution)

**None.** No validation errors, hard-constraint breaches, or blocking defects.

## High (must fix before 01-01)

**None.** All mandatory controls present.

## Medium (DELIVER can address during step execution)

### M-1 — Q1 logging-vs-state distinction in 01-01 criteria — **APPLIED**

The original criterion read: *"If ~/.modeltap/ log directory is unwritable, modeltap renders the TUI and warns to stderr but does not crash"* — implied that `~/.modeltap/` is required state. Per intake Q7 + ADR-003, modeltap is stateless: logs are operational, not state. Wording amended to make this explicit:

> "If ~/.modeltap/ logging directory is unwritable, modeltap renders the TUI and warns to stderr but does not crash. Logs (launch.log, diagnostics.log) are operational only — their absence does not affect discovery, mutations, or correctness."

### M-2 — Spike outputs in dod — **RETRACTED**

Initial concern was that spike outputs (`LINKING.md`, `PATHS.md`, `OLLAMA_BLOB_VERIFICATION.md`) were missing from `dod`. On re-check they ARE present. No issue.

### M-3 — US-05b sequencing clarification — **INFO**

The `Tool` trait freezes in 01-02 with both `delete_one` and `delete_all`. Plugins implement both from 01-02 onward, but the UI exposes `delete_one` only at 03-06 (US-05b). This is correct; optional description note in 03-06 to call out "wiring an already-implemented trait method, not new infrastructure."

---

## Spot-check details

### Story coverage
**PASS.** All 21 stories (US-01..US-20 + US-05b) map 1:1 to steps. No duplicates, no gaps.

### AC fidelity (6 sampled)
| Story | Step | AC count | Match |
|---|---|---|---|
| US-01 | 01-01 | 5 | ✅ EXACT |
| US-05 | 01-04 | 5 | ✅ EXACT |
| US-09 | 02-05 | 4 | ✅ EXACT |
| US-10 | 03-02 | 5 | ✅ EXACT |
| US-14 | 03-05 | 3 | ✅ EXACT |
| US-18 | 04-01 | 5 | ✅ EXACT |

### Dependency graph
**PASS.** Acyclic; no later-phase step is referenced by an earlier-phase step. Notable cross-phase deps validated:
- 02-05 (compatibility engine) depends on all 4 plugin discovery steps (01-02, 02-02, 02-03, 02-04) ✅
- 03-02 (unify) depends on 02-05, 02-06, 03-01 ✅ (engine + detail + bottom-bar polish all needed)
- 04-01 (plugin trait cert) depends on 02-04, 03-07 (all plugins done, all phases done) ✅

### Hard constraints (Q1 / Q7 / trait / telemetry)
| Constraint | Status |
|---|---|
| Q1 — no `~/.modeltap/store/` | ✅ Not present anywhere in roadmap |
| Q7 — no persistent index file | ✅ No `index.json` / `index.toml` references; only operational logs |
| Trait surface frozen at 01-02 | ✅ ADR-001 / ADR-009 honored; later steps implement, not redefine |
| C5 — no telemetry upload in v1 | ✅ JSONL events emitted to local file only; ADR-012 layer-3 upload not in scope |

### Walking-skeleton soundness
**PASS.** Phase 01 (28h, 3.5 days) is slightly over the 2-3 day target, justified by ratatui + plugin trait + async tokio scaffolding required up-front. Nothing critical missing; unify correctly deferred to 03-02.

### Time estimates
**PASS.** 114h total / 14.2 dev-days / fits a 2-week sprint. Largest single step is 03-02 unify @ 14h (4 distinct per-tool linking implementations + canonical-selection + rollback) — realistic.

### Spike folding (DESIGN M-1)
**PASS.** All three DESIGN spikes correctly folded:
- OQ-1 HF linking → spike inside 02-03, output `plugins/hf/LINKING.md`, consumed by 03-02
- OQ-2 LM Studio paths → spike inside 02-04, output `plugins/lm-studio/PATHS.md`, consumed by 03-02
- OQ-3 Ollama blob hash → spike inside 03-02 PREPARE, output `plugins/ollama/OLLAMA_BLOB_VERIFICATION.md`

All three appear in both `implementation_scope` and `dod`.

### CI / DEVOPS integration
**PASS** with one info note. JSONL launch / action events emit per `kpi-instrumentation.md`. Architecture rule R1 certified in 04-01. CI workflow set up in 01-01. **Info:** finalize gate ("CI green on macOS+Linux for 7 days; all AC met") is a Phase 7 concern handled by the platform-architect's finalize task — not a roadmap defect.

### Anti-patterns
**1 noted, declined:** 03-02 (unify, 14h) bundles 4 per-tool link() implementations. Splitting into 03-02a (Ollama + llama-cli, simple) and 03-02b (HF + LM Studio, complex) would isolate per-tool failure modes. Architect kept as one step; reviewer accepted on the grounds that fine-grained commits within the step provide equivalent rollback granularity. Not a blocker.

---

## Recommendation

**APPROVED for execution.** M1 wording fix applied by orchestrator before dispatching 01-01. M3 is informational only; M2 retracted.

Proceed to Phase 2 (per-step TDD execution) starting with 01-01.

---

## Reviewer notes

This file was authored from the reviewer agent's full inline analysis after her Task session ended without writing the file directly. Content is verbatim where she produced explicit text; reorganized faithfully where she produced bullet findings.
