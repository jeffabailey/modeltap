# Peer Review — DISCUSS wave — modeltap-tui

**Reviewer:** nw-product-owner-reviewer (independent, Haiku)
**Date:** 2026-04-28
**Verdict:** **APPROVED**

---

## Summary

DISCUSS artifacts for `modeltap-tui` are coherent, complete, well-scoped, and faithful to the intake brief. No critical or high issues. One medium issue (US-14 has 2 UAT scenarios vs the recommended 3-7) is acceptable in context. DESIGN may begin immediately on Release 0 (US-01..US-06).

---

## Critical issues (must fix before DESIGN)

**NONE.**

---

## High issues (should fix)

**NONE.**

---

## Medium issues (consider)

### M-1: US-14 UAT scenario count (2 vs recommended 3-7)

**Location:** `user-stories.md`, US-14, "UAT Scenarios" section.

**Evidence:** US-14 has 2 UAT scenarios: "Dry-run shows plan" and "Dry-run reveals cross-filesystem issue." Per DoR item 4, stories should have 3-7 UAT scenarios. Per `dor-checklist.md` footnote: "US-14 has 2 UAT scenarios but one is a clearly distinguished happy path with internal variants. Acceptable: <3 scenarios is allowed when the story is genuinely small (<1 day) and the alternative paths are exhaustive."

**Severity:** Medium.

**Recommendation:** ACCEPT as-is. US-14 is genuinely tiny (~0.5d). The 2 scenarios (dry-run succeeds cleanly vs dry-run reveals an issue) are exhaustive. Adding a third scenario ("dry-run after dry-run produces same plan") would be redundant. Not a blocker.

---

## Spot-check results

- **Stories sampled:** US-01 (WS foundation), US-02 (WS discovery), US-05 (WS destructive action), US-10 (Release 2 high-value, complex), US-14 (flagged by Luna), US-18 (architectural, high-stakes).
- **DoR items most stressed:** 1 (problem clarity), 3 (real data), 4 (UAT count/quality), 6 (sizing), 8 (dependencies).
- **Drift from intake-brief:** **NONE.** Requirements faithfully preserve the user's brief on red-icon UX, hotkeys `u`/`z`, plugin extensibility, cross-platform macOS+Linux, and the MLX/Windows deferrals.
- **LeanUX antipatterns found:** **0.**
  - **Implement-X:** none. All stories framed from user pain (Devon's disk pressure, Riley's contribution friction).
  - **Generic data:** none. Real model names (`mistral:7b-instruct-q4_K_M`), real paths (`~/.ollama/models/blobs/`), real PIDs (4421), real sizes (47.3 GB).
  - **Technical AC:** acceptable scope. US-18 mentions trait method names — appropriate for an architectural story. All other AC are user-observable.
  - **Oversized stories:** none. All ≤3 days, all ≤7 scenarios.
  - **Missing examples:** none. All 20 stories have ≥3 domain examples.
  - **Vague AC:** none. All AC checkboxes are observable.

### DoR detail (sampled stories)

| Story | DoR result | Notes |
|---|---|---|
| US-01 | All 8 items PASS | WS foundation, well-scoped |
| US-02 | All 8 items PASS | Real Ollama discovery, 47.3 GB example, 5 UAT scenarios |
| US-05 | All 8 items PASS | Typed-name confirmation, partial-state recovery in tech notes |
| US-10 | All 8 items PASS | Unify story, dependencies on Q2/Q6 explicitly tracked |
| US-14 | 7/8 PASS, 1 caveat | UAT count = 2 (acceptable per context, see M-1) |
| US-18 | All 8 items PASS | Plugin trait, AC includes "5th plugin = zero core changes" |

---

## Walking skeleton assessment

**Thinner or thicker than intake suggested?** Slightly **thicker**. Intake suggested "stub data first, then real Ollama." Luna proposed "real Ollama from day one + zap as the first mutating action."

**Rationale sound?** **YES.** Luna's argument in `story-map.md`:

1. Stub-data-only sits below the value bar — does not validate the riskiest assumption (correctly enumerating a real on-disk layout).
2. Including zap forces the destructive-action confirmation UX into the most-scrutinized slice, where it gets written right the first time.
3. Unify is genuinely harder (canonical store, hardlinks, dedup-key Q6 still open) and rightly belongs in Release 1.

**Riskiest-first?** **YES.** The five WS stories (US-01, US-02, US-03, US-05, US-06) form a complete loop: process → discovery → browse → destroy safely → feedback. The skeleton validates "can we discover one tool and act destructively safely?" before layering on cross-tool and compatibility logic.

**Coverage of backbone?** All 6 backbone activities (Launch, Discover, Browse, Inspect minimally, Act, Verify) are covered.

---

## Architectural constraint check (plugin extensibility)

**Is C1 a hard requirement with story+AC, or dissolved into vague prose?**

**Hard requirement.** Expressed as **US-18: "Plugin trait — adding a 5th tool requires no core changes."**

Evidence:

- AC-1: "Tool trait defined with `name`, `discover`, `list_models`, `link`, `delete`, `accepted_formats`."
- AC-3: "Adding a 5th plugin requires zero changes to `modeltap-core` source files."
- AC-4: "Plugin panics are caught — one bad plugin does not crash the TUI."
- AC-5: "Trait is documented in `CONTRIBUTING.md` with a worked example."

Domain examples include Riley adding Jan as concrete proof. UAT includes "Plugin trait is stable across minor versions."

**Will DESIGN be able to close this?** **YES.** The constraint is clear: trait shape, registration mechanism, stability across minor versions. DESIGN chooses static vs dynamic dispatch; the "core untouched" boundary is the constraint, not the mechanism.

---

## Q2 / Q6 / Q7 framing quality

### Q2 — Per-tool linking strategy (Ollama blob layout, llama-cli loose file, HF symlink farm, LM Studio config)

- **Location:** `requirements.md`, Open Questions table, Q2.
- **Framing:** "DEFERRED — DESIGN must close. Each plugin's `link()` is part of the Tool trait. DESIGN must produce a per-plugin linking spec. May need a light spike per tool."
- **Blocks:** US-10, US-19.
- **Assessment:** **CRISP.** DESIGN has a clear deliverable: a linking-spec document per tool before coding US-10. Four tools listed; the problem is well-scoped.

### Q6 — Dedup key strategy (content hash vs HF id+quant vs hybrid)

- **Location:** `shared-artifacts-registry.md` and `requirements.md`, Q6.
- **Framing:** "DEFERRED — DESIGN must close. Candidates: (a) sha256 of file content, (b) HF repo+quant identifier, (c) hybrid. Each has tradeoffs."
- **Blocks:** US-09, US-10, US-13.
- **Criticality:** **CRITICAL** for unify safety. Wrong choice corrupts dedup detection silently.
- **Assessment:** **CRISP.** Options listed with tradeoffs. DESIGN-level decision; impacts US-09 (indicator), US-10 (unify hardlinking), US-13 (detail screen "3 copies" matching).

### Q7 — State persistence (persistent index vs stateless rediscovery)

- **Location:** `requirements.md`, Q7.
- **Framing:** "PARTIAL: store directory yes, registry file deferred. The canonical store at `~/.modeltap/store/` IS persistent state. Whether modeltap also maintains an explicit JSON/SQLite index is DEFERRED to DESIGN."
- **Blocks:** US-02, US-07, US-12, US-15 (affects performance / K3).
- **Assessment:** **CRISP.** Tradeoff well-framed: startup latency (K3) vs state complexity. v1 can plausibly choose stateless rediscovery; option is open.

---

## Other quality dimensions

### Confirmation-bias detection

- **Technology bias:** none. Rust + Ratatui are intake-supplied implementation choices, not requirements artifacts.
- **Happy-path bias:** none. Each story has ≥1 error scenario (permission errors, corrupt files, cross-filesystem, running tools, incomplete linker, wrong confirmation input).
- **Availability bias:** none. Design justified by Devon/Riley personas and UMR reference.

### Completeness validation

- **Stakeholder coverage:** Devon (primary, power user), Riley (secondary, contributor). Operations not applicable (local CLI).
- **NFR coverage:** Performance, Safety, Cross-platform, Privacy, Accessibility, Reliability — all addressed in `requirements.md` with quantified targets.

### Clarity and measurability

- All performance requirements quantified ("< 1 second," "≤ 500 ms," "≥ 4.5:1 contrast").
- All AC are observable checkboxes ("Total disk usage equals sum of unique blob sizes," "Pressing `z` opens confirmation dialog," "Hardlinks have same inode as canonical").

### Outcome KPIs

| KPI | Measurable? | Baseline | Target |
|---|---|---|---|
| K1 disk reclaimed (GB / session) | yes | 0 (greenfield) | set |
| K2 dedupable % | yes | deferred post-release | TBD |
| K3 first-paint latency (s) | yes | 0 | < 1 s, regression alert > 2 s |
| K4 community plugins (count) | yes | 0 | set |
| K5 accidental-loss issues / 90 days | yes | 0 | 0 (any = manual review) |

K2 baseline-deferred is acceptable for first release.

---

## Recommendation

**APPROVED for DESIGN handoff.** No blockers.

DESIGN can begin immediately on Release 0 (US-01..US-06) with full confidence. The three deferred questions (Q2 per-tool linking specs, Q6 dedup-key strategy, Q7 state persistence) are framed crisply enough that the solution architect can close each one as part of architecture work — Q6 is critical-path for US-09/US-10 and should be resolved early in DESIGN.

---

## Reviewer notes

- This file was authored from the reviewer's full inline analysis after the reviewer agent's session ended before writing the file directly. Content is verbatim where the reviewer produced explicit text and faithfully reorganized where the reviewer produced bullet findings. Any future re-review should re-run the reviewer agent end-to-end.
