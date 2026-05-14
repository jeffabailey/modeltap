# Definition of Ready (DoR) Validation — folder-group-bulk-delete

Hard gate: every story must pass all 9 DoR items before DESIGN handoff.

## Per-Story DoR Status

| Story | 1. Problem clear | 2. Persona specific | 3. ≥3 examples | 4. UAT 3-7 | 5. AC from UAT | 6. Right-sized | 7. Tech notes | 8. Deps tracked | 9. KPI defined | Status |
|---|---|---|---|---|---|---|---|---|---|---|
| US-05c | PASS | PASS | PASS (3) | PASS (7) | PASS (19) | PASS (~2-3d) | PASS | PASS (10 parent + 3 DESIGN-open) | PASS (K-FGD-1, K-FGD-2, K-FGD-3) | PASSED |

## Aggregate

- **Total stories:** 1
- **Stories PASSED:** 1
- **Stories BLOCKED:** 0
- **Overall DoR status:** PASSED

## Item-by-Item Evidence

### 1. Problem statement clear and in domain language

US-05c's Problem section uses domain language throughout: "audits multiple quant variants", "Hugging Face repo", "sidecar files", "whole-tool zap (US-05) is too coarse". No engineering jargon ("microservice", "API"). The persona (Devon Park) and the artifact (HF repo folder) are concrete.

### 2. User/persona identified with specific characteristics

Devon Park is the same persona used in 18 of the 20 parent stories. Characteristics specific to this story added: "uses HF cache as his primary download path for GGUF quants", "typically downloads 5-10 new HF repos per month, discards 1-2 of them after auditioning". Not "user" or "developer".

### 3. At least 3 domain examples with real data

Story has 3 numbered domain examples:

- **Example 1 (happy path):** `bartowski/Llama-3.2-1B-Instruct-GGUF/` with 20 .gguf files (657 MB ... 2.5 GB), 3 named sidecars, 1 file hardlinked into Ollama. Real reclaim numbers: 14.0 GB / 0.7 GB retained.
- **Example 2 (edge):** wrong-typed path. Real malformed input: `Llama-3.2-1B-Instruct-GGUF` (forgot author prefix).
- **Example 3 (error):** Ollama running with PID 4421 holding 2 of 21 files open. Real partial-failure numbers: 19/21 succeeded, 1.6 GB remain on disk.

All examples use real model/repo names (bartowski, Llama-3.2-1B-Instruct-GGUF, Q4_K_M, Q4_0), real paths (`~/.cache/huggingface/hub/models--bartowski--Llama-3.2-1B-Instruct-GGUF/`), real sizes, real PIDs. No `user123` or `test-repo`.

### 4. UAT scenarios in Given/When/Then (3-7 scenarios)

US-05c has 7 UAT scenarios (right at the upper bound of the right-sized range):

1. Hugging Face right pane groups files under repo folder headers
2. Pressing Shift+F on a folder header opens the typed-confirmation dialog
3. Correct typed-confirmation executes the folder-delete
4. Wrong typed path cancels the folder-delete
5. Partial failure when Ollama holds files open
6. Shift+F is a no-op when the active tool is not Hugging Face
7. Shared file's other-tool hardlink survives folder-delete

The full `journey-folder-group-delete.feature` has 17 scenarios — these 7 are the canonical-UAT subset for the story; the others are integration / edge-case coverage that DISTILL will pick up. All Given/When/Then.

### 5. Acceptance criteria derived from UAT

19 ACs (US-05c.AC-1 .. US-05c.AC-19), each tracing back to a UAT scenario or a requirements/registry checkpoint. Plus 8 cross-feature integration ACs (INT-FGD-1 .. INT-FGD-8) covering parent-feature invariants extended by this story. Trace table in `acceptance-criteria.md`.

### 6. Story right-sized (1-3 days, 3-7 scenarios)

- Effort estimate: **2-3 days** (folder grouping logic + dialog + unlink loop + sidecar enumeration). Validated against parent feature's similar US-05 (~2d) and US-05b (~1.5d).
- UAT scenarios: **7** (top of the 3-7 range; acceptable)
- Single user outcome: folder-delete operation
- Demonstrable in single session: launch -> select HF -> expand a repo -> press F -> type path -> see reclaim message. ~30 seconds for a demo.

Story map (`story-map.md`) explicitly justifies why this is one story and not multiple — splitting by layer (grouping then deleting) or by feature (without sidecars then with) would violate the elephant-carpaccio rule.

### 7. Technical notes identify constraints

"Technical Notes" section in US-05c lists:

- Stateless rediscovery (intake Q7)
- Concurrency: detect-and-prompt-then-retry (intake Q5)
- HF plugin only in v1 (intake scope constraint #1)
- Safety rubric per ADR-009: typed-confirmation for irreversible (matches US-05), not `[y/n]`
- Cross-platform: macOS and Linux
- Sidecar enumeration owned by HF plugin, not core

Plus 3 open questions for DESIGN (Q-FGD-1, Q-FGD-2, Q-FGD-3) explicitly tracked and ALLOWED to stay open through DISCUSS handoff — they belong to DESIGN's bounded context.

### 8. Dependencies resolved or tracked

Dependencies listed in US-05c:

- 10 parent stories (all PASSED DoR, in DELIVER or shipped): US-03, US-04, US-05, US-05b, US-06, US-08, US-09, US-11, US-12, US-17
- 3 DESIGN-open questions (do not block DISCUSS handoff; will block DEVOPS handoff): Q-FGD-1, Q-FGD-2, Q-FGD-3
- No external dependencies

### 9. Outcome KPIs defined with measurable targets

Three story-level KPIs defined in `outcome-kpis.md`:

- **K-FGD-1**: `time_to_reclaim_repo_p50_seconds` — Devon completes a folder-delete in p50 ≤ 15 s wall-clock (including typing); p90 ≤ 30 s for a 21-file repo
- **K-FGD-2**: `keystrokes_per_repo_delete` — drops from O(N_files × 20+ keystrokes) under US-05b loop to O(1 hotkey + path-length + 1 Enter); target ≤ 35 keystrokes total for a typical `<author>/<repo>` (~30 chars)
- **K-FGD-3**: `mis_target_rate` (guardrail) — typed-confirmation mismatches that abort < 1% of dialog opens; accidental wrong-folder deletes = 0

All three have measurement methods (instrument the dialog code path; log timing and confirmation outcomes locally).

## Anti-Pattern Scan

| Anti-Pattern | Detected? | Notes |
|---|---|---|
| Implement-X | NO | Story framed from Devon's pain (21-keystroke ceremony, stranded sidecars) |
| Generic Data | NO | All examples use real HF repo names (bartowski/Llama-3.2-1B-Instruct-GGUF), real sizes (14.7 GB), real PIDs (4421) |
| Technical AC | NO | All 19 ACs are observable user outcomes. AC-13 ("uses compute_compatibility()") is an integration-checkpoint AC, not an implementation directive — it enforces no-parallel-implementation, which is observable via code-review/grep |
| Oversized Story | NO | 2-3 days, 7 UAT scenarios, 1 user outcome, demonstrable in one session |
| No Examples | NO | 3 domain examples with real data |
| Tests After Code | N/A | DELIVER wave concern; UAT scenarios defined here, tests will be RED-first |

## Recovery / Remediation

No DoR failures requiring remediation. All 9 items PASS. Anti-pattern scan clean.

## DoR Status: PASSED

Ready for peer review and DESIGN handoff. Open questions Q-FGD-1, Q-FGD-2, Q-FGD-3 are explicitly tracked as DESIGN-must-close items and do NOT block this DoR.
