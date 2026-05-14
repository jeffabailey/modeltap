# Peer Review — folder-group-bulk-delete DESIGN

**Reviewer:** Atlas (nw-solution-architect-reviewer, invoked via Task protocol)
**Iteration:** 1
**Date:** 2026-05-11
**Artifacts reviewed:**

- `docs/feature/folder-group-bulk-delete/design/architecture-design.md`
- `docs/feature/folder-group-bulk-delete/design/technology-stack.md`
- `docs/feature/folder-group-bulk-delete/design/component-boundaries.md`
- `docs/feature/folder-group-bulk-delete/design/data-models.md`
- `docs/adrs/ADR-010-folder-group-delete-hf-capability.md`

## Review Output (YAML)

```yaml
review_id: "arch_rev_2026-05-11_fgd"
reviewer: "solution-architect-reviewer"
artifact: "docs/feature/folder-group-bulk-delete/design/*, docs/adrs/ADR-010-*.md"
iteration: 1

strengths:
  - "ADR-010 has three alternatives (A/B/C), each with a worked code sketch and explicit rejection rationale grounded in component-boundaries §R5 and ADR-001's object-safety contract."
  - "Single-engine invariant (D-FGD-4) is enforced architecturally: classify_unique_vs_shared depends on compute_indicator; no parallel dedup logic possible by construction."
  - "Q-FGD-3 closed correctly toward Option B with a written rationale; no premature artifact materialization."
  - "Default-method trait extension preserves R5 (only app composes plugins) and ADR-001's 'add a 5th tool = zero changes outside the plugin crate'."
  - "Zero new dependencies (technology-stack.md is honest about this); transitive footprint unchanged."
  - "Reuse of existing DeleteOutcome rather than parallel FolderDeleteOutcome — correct simplest-solution choice."
  - "Partial-failure semantics traced from D-FGD-6 through orchestrator aggregation to LastAction rendering; no rollback machinery added."
  - "C4 L1+L2 delta diagrams + two L3 diagrams (folder_group pure subsystem + hf folder_delete plugin subsystem) — justified, not over-applied."

issues_identified:
  architectural_bias:
    - issue: "Default-body trait method does extend the 'FROZEN SURFACE' comment in ADR-001's tool.rs. The ADR notes this and proposes a clarifying note, but the parent ADR-001 is not updated."
      severity: "low"
      location: "ADR-010 §Negative Consequences"
      recommendation: "DELIVER step: add a one-line note to tool.rs doc comment when the new method is added: 'FROZEN against breaking changes; extensions via default-body methods are permitted per ADR-010.' No ADR-001 amendment required."

  decision_quality:
    - issue: "ADR-010 covers Q-FGD-1 and Q-FGD-2 in one ADR. Q-FGD-2's decision is 'inherit ADR-009' which is sub-architectural and does not warrant a separate ADR, but the combined treatment is slightly unusual."
      severity: "low"
      location: "ADR-010 §Decision Q-FGD-2"
      recommendation: "Acceptable as-is. The cross-reference to ADR-009 + the explicit 'no new ADR' statement makes the inheritance traceable. No change needed."

  completeness_gaps:
    - issue: "Performance: classification under Tentative dedup keys (SHA256 not yet computed at dialog open) is acknowledged (architecture-design.md §8.3 + Risk R1) but the mitigation depends on 'compute_indicator already returns Compatible (not Shared) when either side's hash is missing'. Verify this claim against the actual compute_indicator source."
      severity: "medium"
      location: "architecture-design.md §8.3 and §10 Risk R1"
      recommendation: "Reviewer-verified against crates/modeltap-core/src/logic/compatibility.rs §'Decision rules' #2: 'when the SHA256 is None for either side, the engine MUST NOT classify as Shared.' Claim is accurate. NO ACTION REQUIRED but a citation in architecture-design.md §8.3 would strengthen traceability."

  implementation_feasibility:
    - issue: "DELIVER step count is implicit. No explicit list of crafter steps or test plan owned by the architect; DISTILL/DELIVER infer them from the design."
      severity: "low"
      location: "architecture-design.md (overall)"
      recommendation: "ACCEPTABLE: architecture owns WHAT, crafter owns HOW (Morgan's principle #1). The design's §5 module-delta table + §13 DoD provide sufficient WHAT. DISTILL will materialize the Gherkin and DELIVER will sequence the steps. No change needed."

  priority_validation:
    q1_largest_bottleneck:
      evidence: "Outcome KPIs K-FGD-1 (15-30s wall-clock) and K-FGD-2 (35 keystrokes vs ~440) quantify the user-facing bottleneck the feature targets. Architecture's primary investment (single-engine classification + plan-then-execute) directly serves these."
      assessment: "YES"
    q2_simple_alternatives:
      assessment: "ADEQUATE — ADR-010 considers three options with code sketches and explicit rejection rationale. Simplest-solution check passes: chose default-body (the simpler additive option) over capability subtrait or plugin-private API."
    q3_constraint_prioritization:
      assessment: "CORRECT — quality attributes inherited from parent without re-ordering; this is an additive feature, not a re-architecture. Constraints quantified (≤200 ms dialog open, ≤500 ms summary refresh, zero new dependencies) and each maps to a strategy."
    q4_data_justified:
      assessment: "JUSTIFIED — performance NFRs reference parent benchmarks; classification cost is O(N×M) with concrete N (typical 1-30 files per folder) and M (typical <500 inventory size); HF cache layout assumptions cite the actual cache_walk.rs source."

approval_status: "approved"
critical_issues_count: 0
high_issues_count: 0
medium_issues_count: 1
low_issues_count: 3
```

## Revisions Applied in This Iteration

Issues found are all low/medium severity and consist of optional traceability improvements rather than corrections. The reviewer recommends approval as-is. The one medium-severity item (citation for the conservative-when-uncertain claim) is addressed by inline acknowledgment that the claim was reviewer-verified against the source — no document change is required for correctness.

## Quality Gate Status

- [x] Requirements traced to components — `architecture-design.md` §5
- [x] Component boundaries with clear responsibilities — `component-boundaries.md`
- [x] Technology choices in ADRs with alternatives — ADR-010 (three alternatives with code sketches)
- [x] Quality attributes addressed — `architecture-design.md` §8 (5 attributes, strategies)
- [x] Dependency-inversion compliance — default-body trait method, plugin override, orchestrator composition
- [x] C4 diagrams Mermaid (L1+L2 minimum, L3 where warranted) — 4 diagrams: L1 context, L2 container delta, L3 core folder_group, L3 HF folder_delete
- [x] Integration patterns specified — `architecture-design.md` §9 (none new; plugin contract test extended)
- [x] OSS preference validated — zero new dependencies (`technology-stack.md`)
- [x] AC behavioral, not implementation-coupled — design does not introduce implementation-coupled AC; type signatures are contract, bodies are crafter-owned
- [x] External integrations annotated with contract-test recommendation — N/A (no external integrations)
- [x] Architectural enforcement tooling recommended — existing `tests/architecture.rs` covers without modification
- [x] Peer review completed — this document

## Handoff Package for acceptance-designer (DISTILL wave)

Artifacts at `docs/feature/folder-group-bulk-delete/design/`:

1. `architecture-design.md` — overall design + C4 diagrams + risks + quality attributes
2. `technology-stack.md` — zero-new-dependency confirmation
3. `component-boundaries.md` — module deltas + closure of Q-FGD-3 + Q-FGD-2
4. `data-models.md` — algebraic type sketches for `FolderGroup`, `Sidecar`, `FolderClassification`, `FolderDeletePlan`, `DeleteError::Unsupported`, `Tool::delete_folder` signature
5. `peer-review.md` — this document

ADR at `docs/adrs/`:

- `ADR-010-folder-group-delete-hf-capability.md`

Open architecture questions: NONE. All three (Q-FGD-1, Q-FGD-2, Q-FGD-3) closed.
