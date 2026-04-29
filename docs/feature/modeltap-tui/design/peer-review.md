# Peer Review — DESIGN Wave — modeltap-tui

**Reviewer:** nw-solution-architect-reviewer (independent, Haiku)
**Date:** 2026-04-28
**Iteration:** 1
**Verdict:** **APPROVED** for handoff to DEVOPS.

---

## Executive summary

The DESIGN wave for `modeltap-tui` is approved. Architecture is faithful to the intake brief, addresses all critical quality attributes, implementable within the walking-skeleton timeline (2-3 days). All 10 review criteria satisfied. Three ADRs require verification spikes in DELIVER (HF and LM Studio linking), but these are properly scoped and do not block DESIGN approval.

---

## 1. Critical issues (block DEVOPS)

**None identified.**

## 2. High issues (should fix before DEVOPS)

**None identified.**

## 3. Medium issues (defer to DELIVER acceptable)

### M-1: HF and LM Studio per-tool linking spikes — well-scoped

**Location:** ADR-004 § "Spike risk: MEDIUM" (HF, LM Studio).

ADR-004 honestly flags HF and LM Studio linking as requiring verification spikes:
- **HF:** replace blob with hardlink, confirm `huggingface-cli` and `transformers` load it. Estimated < 1 day.
- **LM Studio:** verify hardlinked models load, file re-read on next selection. Estimated < 1 day.

Spikes are well-scoped and unambiguous (works / doesn't). DELIVER can run them in build-week 1 of US-10. **No blocker.**

**Recommendation:** acknowledge in DEVOPS handoff that HF and LM Studio spikes must run before US-10 implementation. Document outcomes in plugin code comments post-spike.

### M-2: SHA256 cost on 50 GB files — lazy strategy sufficient for v1

**Location:** ADR-002 § "Cost analysis", architecture-design.md § "OQ-5".

A 50 GB GGUF takes 40–100 s to hash depending on hardware. The lazy-with-progress strategy (hash on detail-screen open) is specified correctly. v1 acceptable; OQ-5 documents the user-complaint contingency (Alternative B: persistent SHA256 cache).

**Recommendation:** document the wait-time expectation in detail-screen help text. Monitor post-launch.

### M-3: Stateless + K3 budget — first paint trivial, full inventory tighter

**Location:** architecture-design.md § 7, ADR-003.

Budget allocation:
- Process start → first paint: 150 ms
- First paint → all plugins done: 800 ms (parallel discovery)
- Indicators computed: 200 ms
- **Total to full inventory: ~1.15 s.**

Design satisfies K3 trivially (skeleton paints in < 150 ms). Full-inventory time is realistic for 4-plugin parallel discovery on modern hardware. Risk: very large libraries (1000+ models, slow HDD) could exceed this — ADR-003 § "Negative" and OQ-5 already document this contingency. **No blocker.**

---

## 4. Spot-check results

### 4.1 Intake-brief fidelity — PASSED

| Q | Intake answer | Design respects? |
|---|---|---|
| Q1 | NO `~/.modeltap/store/`; tool dirs are source of truth | ✅ ADR-003 + arch §3 unambiguous |
| Q5 | detect-and-prompt-then-retry | ✅ `LinkError::InUse{tool}` in ADR-007; US-17 AC matches |
| Q6 | SHA256 primary, HF id+quant display fallback | ✅ ADR-002 explicit |
| Q7 | NO persistent index | ✅ ADR-003 full closure |

### 4.2 Plugin extensibility (US-18) — PASSED

`Tool` trait frozen in `modeltap-core`; new plugins use `inventory::submit!` (ADR-001). Adding a 5th plugin = new crate under `plugins/` + workspace `Cargo.toml`. **Zero changes to `modeltap-core` source.** CI rule R1 (architecture.rs) enforces.

### 4.3 Stateless + K3 latency — FEASIBLE

Skeleton paints before I/O (< 150 ms); discovery is O(N models), not O(N bytes); parallel async via tokio multi-thread; SHA256 lazy on detail-screen open. **No performance cliff.** Risk: LOW.

### 4.4 SHA256 hashing cost — SAFE

Lazy compute on first need. In-process cache `(path, mtime, size) → ContentHash` survives navigation; lost on exit (no persistence). Conservative-deletion rule: dedup-key uncertain → treat as unique (preserves data). Optional `--prefetch-hashes` flag deferred to v1.x. **No accidental full-library hash on startup.**

### 4.5 Cross-fs fallback (ADR-008) — UNAMBIGUOUS AND TESTABLE

Refuse-default + explicit user choice. Same code path for dry-run and real-run. Testable scenarios: all-same-fs, some-cross-fs, all-cross-fs.

### 4.6 Per-tool linking specs (ADR-004) — WELL-SCOPED SPIKES

| Tool | Status | Spike risk |
|---|---|---|
| Ollama | Documented blob format, SHA256 = blob filename | NONE |
| llama-cli | Loose `.gguf`, no manifest | NONE |
| HF | Verify hardlinked blob loads via HF tooling | MEDIUM (< 1 day) |
| LM Studio | Verify hardlinked file load + re-read | MEDIUM (< 1 day) |

ADR-004 § "Spike checklist" is explicit. Spikes block US-10, not DESIGN.

### 4.7 Single-model delete (ADR-009) — CORRECT

`Tool::delete_one(model)` is a first-class trait method, distinct from `delete_all`. UI mapping: detail-screen `[d]` → `delete_one`; left-pane `[z]` → `delete_all`. Confirmation logic: typed for unique, [y/n] for shared. `ZapOnePlan` type exists in data-models.md. **DELIVER cannot ship US-05 without `delete_one`.**

### 4.8 C4 diagrams — ACCURATE

L1 (system context), L2 (containers), L3a (`modeltap-core`), L3b (`modeltap-plugins`) — all present in Mermaid, all match prose in component-boundaries.md. No discrepancies.

### 4.9 Anti-pattern check — NONE DETECTED

| Anti-pattern | Status |
|---|---|
| Microservices | Not present (single-user local CLI) |
| Event bus | Not present (Elm-style message queue is appropriate) |
| DDD aggregates | Not present (algebraic types, simple) |
| Under-justified tech | All 9 ADRs include alternatives with rationale |
| Premature abstractions | `modeltap-cli` seam is left open but not shipped — correct |
| Missing dependency inversion | All deps point inward toward `modeltap-core`; CI enforced |
| Unnecessary state | None — stateless per ADR-003 |
| Vague ADRs | All have context / decision / alternatives ≥ 2 / consequences |

### 4.10 DoD spot-check (4 of 10) — PASSED

- ✅ Requirements traced to components (US-02, US-05, US-18 verified to containers + components)
- ✅ Technology choices in ADRs with alternatives (all 9 ADRs have ≥ 2 alternatives)
- ✅ AC behavioral, not implementation-coupled (US-02, US-05, US-18 sampled — clean)
- ✅ C4 diagrams Mermaid, accurate

Remaining 6 items spot-checked visually: all present.

---

## 5. Plugin extensibility (US-18) verdict

**APPROVED.** US-18 AC ("zero changes to `modeltap-core` source files") is achievable via:
- Trait frozen in `modeltap-core`
- Plugin self-registration via `inventory::submit!` (zero source-line changes outside the new plugin crate)
- Composition-root assembly in `modeltap-app::main`
- CI architecture rule R1 mechanically enforces no plugin deps in core

---

## 6. Stateless + K3 latency assessment

**APPROVED.** First paint < 150 ms (skeleton only). Full inventory ~1.15 s (parallel discovery, manifest reads only — no file content reads on critical path). Realistic for typical hardware (SSD/NVMe, 4 plugins, ≤ 500 models). Contingency for massive libraries documented in ADR-003 § "Negative" and OQ-5.

---

## 7. Per-tool linking spike scope assessment

**APPROVED.** Both spikes (HF, LM Studio) are 2–4 hours each, with explicit checklists in ADR-004. Outcomes are unambiguous (works / doesn't). DELIVER can execute in week 1 of US-10 build week. **Not a DESIGN blocker.**

---

## 8. Single-model delete (ADR-009) sufficiency

**APPROVED.** First-class trait method, plan type, UI path, and confirmation logic all defined. BE-7 in arch §11 flags DISCUSS US-05 expansion / US-05b addition. DELIVER cannot ship US-05 without implementing `delete_one`.

---

## 9. Strengths

1. **Faithful to intake brief.** All Q1–Q7 answers reflected (no central store, stateless, SHA256-first, detect-and-retry concurrency).
2. **Plugin extensibility achieves "zero core changes" mechanically.** `inventory` crate + trait dispatch + composition root + CI rule R1.
3. **K3 latency credible.** Budget allocation realistic; skeleton-first paint and parallel discovery mitigate stateless cost.
4. **Comprehensive ADRs.** All 9 with context, alternatives ≥ 2, consequences. Cost tables, ecosystem comparisons, timeline analysis where relevant.
5. **Testable and unambiguous.** C4 diagrams match prose. AC behavioral. Cross-fs fallback explicit. Single-model delete distinct trait method.
6. **Honest about unknowns.** OQ-1 .. OQ-5 and per-tool spikes documented, not hidden. Back-edits BE-1..BE-12 flagged for DISCUSS. No pretending.

---

## 10. Open items for DELIVER

1. HF + LM Studio linking spikes (< 1 day each, week 1 of US-10 build).
2. DISCUSS back-edits BE-1..BE-12 (non-blocking; should be applied during story refinement so user-stories.md/requirements.md reflect intake answers).
3. Persistent SHA256 cache (optional v1.x optimization per OQ-5).
4. Running-tool detection thresholds — Q5 says "soft warning"; design notes DELIVER may revisit if hard-block is needed.

---

## 11. Risk assessment

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| HF linking doesn't work (hardlinked blob not recognized) | Medium | High | Spike in DELIVER week 1 of US-10; outcome unambiguous |
| LM Studio file-handle caching breaks after rename | Medium | High | Spike + Q5 detect-and-retry covers |
| K3 latency exceeded on massive libraries (1000+ models) | Low | Medium | ADR-003 Alternative B (persistent cache) documented |
| SHA256 hash on 50 GB file annoys users | Low | Low | Lazy + progress bar v1; `--prefetch-hashes` flag for v1.x |

**Overall risk: LOW.** No critical unknowns. Two moderate-risk spikes scoped and scheduled.

---

## 12. Recommendation

**APPROVED for DEVOPS handoff.**

Next steps:
1. DEVOPS — schedule CI infrastructure (macOS + Linux runners), K3 timing instrumentation, opt-in telemetry hooks.
2. DELIVER — apply BE-1..BE-12 (now done by parent agent before DEVOPS dispatch).
3. DELIVER — schedule HF + LM Studio spikes in week 1 of US-10 build.
4. Post-launch — monitor K3 latency, K1 bytes reclaimed, K5 accidental-loss incidents.

---

## Reviewer notes

This file was authored from the reviewer's full inline analysis after her Task session ended without writing the file directly. Content is verbatim where she produced explicit text; reorganized faithfully where she produced bullet findings. Any future re-review should re-run the reviewer agent end-to-end.
