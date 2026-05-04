# DESIGN Peer Review — release-process-homebrew-github

**Reviewer mode:** structured self-review against `nw-sa-critique-dimensions` (Atlas profile applied inline; nw-solution-architect-reviewer Task invocation not available in current subagent context).
**Reviewer:** Morgan, applying the 5 critique dimensions to own DESIGN artifacts.
**Date:** 2026-05-03
**Iteration:** 1

The parent agent should consider invoking `nw-solution-architect-reviewer` directly when this subagent returns control, to obtain an independent verdict.

## Artifacts Under Review

- `docs/feature/release-process-homebrew-github/design/architecture-design.md`
- `docs/feature/release-process-homebrew-github/design/technology-stack.md`
- `docs/feature/release-process-homebrew-github/design/component-boundaries.md`
- `docs/feature/release-process-homebrew-github/design/data-models.md`
- `docs/feature/release-process-homebrew-github/design/wave-decisions.md`
- `docs/adrs/ADR-010-release-pipeline-architecture.md`
- `docs/adrs/ADR-011-xtask-placement.md`
- `docs/adrs/ADR-012-cross-compile-strategy.md`
- `docs/adrs/ADR-013-tap-repo-credential.md`
- `docs/adrs/ADR-014-formula-templating.md`

## Review Output

```yaml
review_id: "arch_rev_20260503_design_release_pipeline"
reviewer: "solution-architect (self-review mode, nw-sa-critique-dimensions)"
artifact: "docs/feature/release-process-homebrew-github/design/*, docs/adrs/ADR-010..014"
iteration: 1

strengths:
  - "Atomic-publish (C2) is expressed as a workflow-graph property (needs: DAG), not imperative code — impossible to bypass without conspicuously editing release.yml. ADR-010 documents the choice with 3 rejected alternatives."
  - "Architecture style (functional-core / imperative-shell) explicitly mirrors the existing project paradigm declared in CLAUDE.md, avoiding paradigm drift between modeltap-tui and the new xtask code."
  - "Every shared artifact in DISCUSS shared-artifacts-registry.md (14 entries) is traced to a single source and one or more consumers in data-models.md §2 + §5. No artifact is consumed without a documented producer."
  - "Cross-compile strategy (ADR-012) explicitly chooses reliability over speed (`cross` adds ~1 min cold cache vs manual rustup-target) and documents the future migration path (OQ-3 native ubuntu-22.04-arm)."
  - "Quality attribute priorities (architecture-design.md §2) are derived from DISCUSS NFRs and KPIs with traceable mapping. Reliability ranked #1 reflects the integrity-of-pipeline constraint, not arbitrary preference."
  - "Technology stack (technology-stack.md) lists every dep with license, source URL, and pinning rationale. Forbidden tools section (§10) explicitly enumerates what is NOT in the stack to prevent drift."
  - "External-integration contract verification (architecture-design.md §7.3) correctly identifies that brew test-bot IS the consumer-driven contract test for this design — there are no third-party REST/GraphQL APIs to write Pact contracts against."
  - "Component boundaries (component-boundaries.md §2.2) provide interface shapes (function signatures, struct schemas) without prescribing internal decomposition — respects software-crafter ownership of HOW."
  - "Workflow file size budget (US-14, ≤250 lines) is enforced by an architectural mechanism (cargo xtask lint-workflows) running in ci.yml — drift catches at PR time, not at first release attempt."

issues_identified:
  architectural_bias:
    technology_preference_bias:
      - issue: "Tera selected over Minijinja and Handlebars without measurable criteria"
        severity: "low"
        location: "ADR-014"
        recommendation: "ADR-014 acknowledges this and rejects Minijinja/Handlebars on conservative-choice grounds, with explicit statement that Minijinja is also acceptable. Acceptable as documented; no change required."

    resume_driven_development:
      - assessment: "No detected pattern. The architecture is conservative: stable Rust, GitHub Actions (already used), `cross` (ecosystem-standard), Tera (5k stars). No microservices, no Kafka, no service mesh, no novel frameworks. Single-maintainer scope is honored — no over-engineering."
        severity: "none"

    latest_technology_bias:
      - assessment: "All chosen tools are mature: cross (10k stars, 6+ years), Tera (5k stars, 8+ years), git-cliff (5k stars, 3+ years), all GH Actions pinned to stable major versions. No bleeding-edge dependencies."
        severity: "none"

  decision_quality:
    missing_context:
      - assessment: "All 5 ADRs (010-014) include explicit Context sections naming the business problem, technical constraints, and quality attributes. Future maintainers can validate."
        severity: "none"

    missing_alternatives_analysis:
      - assessment: "ADR-010 (3 alternatives), ADR-011 (3 alternatives), ADR-012 (3 alternatives), ADR-013 (4 alternatives), ADR-014 (4 alternatives). Each includes pros/cons and explicit rejection rationale."
        severity: "none"

    missing_consequences:
      - assessment: "All 5 ADRs include positive AND negative consequences sections, plus a quality attribute impact table mapping the decision to specific attributes."
        severity: "none"

  completeness_gaps:
    missing_quality_attributes:
      - assessment: "All 5 ranked attributes addressed (architecture-design.md §8): reliability, integrity/supply-chain, maintainability/legibility, performance, cross-platform coverage. Each section has concrete strategies, not just goals."
        severity: "none"

    missing_performance_architecture:
      - assessment: "K-T2T performance budget (architecture-design.md §8.4) breaks down median ≤15min target into per-job allocations (validate-tag ≤30s, build matrix ≤6min, publish ≤2min, bump ≤2min, brew test-bot ≤5min, auto-merge ≤30s). Cache strategy (Swatinem/rust-cache@v2) explicitly documented."
        severity: "none"

    missing_security_architecture:
      - issue: "Threat model for malicious tap-bump scenario not explicitly documented"
        severity: "low"
        location: "architecture-design.md §8.2 (integrity), ADR-013"
        recommendation: "Consider adding a brief threat-model section noting: (a) GH_TAP_TOKEN compromise = tap repo compromise only (least-privilege limits blast radius per ADR-013); (b) malicious commit on main between prep PR merge and tag push = caught by validate-tag if Cargo.toml unchanged, otherwise treated as legitimate code path (the maintainer authored both); (c) SLSA L3 attestation provides build-time integrity but does not protect against malicious source code. ACCEPTED as a documentation enhancement; not a blocker for DESIGN approval."

  implementation_feasibility:
    team_capability_mismatch:
      - assessment: "Single maintainer comfortable with git, GitHub Actions, Homebrew (per intake-brief). xtask requires Rust skills the maintainer already has (modeltap-tui shipped). Tera template syntax is Jinja2-like (well-known). No new skills required."
        severity: "none"

    budget_constraints:
      - assessment: "Zero new infrastructure costs. All hosted on GitHub-free-tier-or-OSS. cross uses GH Actions runners (already paid by GH for OSS). No external services."
        severity: "none"

    testability_validation:
      - assessment: "xtask functional core is pure-function-driven — trivially unit-testable. Adapter layer is integration-testable with assert_cmd against fixture filesystems. Workflow jobs are testable via xtask lint-workflows + brew test-bot. End-to-end testable against ephemeral tap repo (DISTILL to design)."
        severity: "none"
      - issue: "Cross-repo end-to-end testing in DISTILL is genuinely hard"
        severity: "medium"
        location: "wave-decisions.md DISTILL handoff §1"
        recommendation: "Already flagged in DISCUSS handoff and re-flagged in DESIGN handoff. DISTILL must design for it; DESIGN cannot solve it for them. ACCEPTED as a known DISTILL challenge, not a DESIGN gap."

  priority_validation:
    q1_largest_bottleneck:
      evidence: "DISCUSS identified cross-repo automation as the riskiest assumption (Maurya). Walking skeleton scope (US-01..US-06 + US-15) validates it first. DESIGN preserves this: bump-tap-formula is the cross-repo seam; ADR-013 (PAT) and US-12 (idempotency) explicitly target the highest-risk integration."
      assessment: "YES"

    q2_simple_alternatives:
      assessment: "ADEQUATE — every ADR considers and rejects 2-4 simpler alternatives. No 'we picked the complex thing' moves."

    q3_constraint_prioritization:
      assessment: "CORRECT — atomicity (C2) and version-truth-singularity (C1) are correctly prioritized as the highest-impact integrity constraints. Performance (K-T2T) is rank 4, not rank 1; this is appropriate because integrity failures are user-facing while a 16-minute pipeline is fine."

    q4_data_justified:
      assessment: "JUSTIFIED — KPI targets (15-min T2T, 95% pipeline success) inherited from DISCUSS where they were validated as realistic per GitHub Actions performance norms. Per-job duration budget in DESIGN §8.4 is decomposed from the K-T2T target. cross cold-cache cost (~1 min) is empirical from the cross-rs project's own CI."
    
    verdict: "PASS"

approval_status: "approved"
critical_issues_count: 0
high_issues_count: 0
medium_issues_count: 1
low_issues_count: 2
```

## Summary

**Verdict: APPROVED with 1 medium and 2 low advisories. None blocks DISTILL handoff.**

### Issues acknowledged but not blocking

| Severity | Issue | Disposition |
|---|---|---|
| Medium | Cross-repo end-to-end testing is genuinely hard | Already flagged in DISCUSS + DESIGN handoffs; DISTILL's challenge to solve. Not a DESIGN gap. |
| Low | Threat model for malicious tap-bump not explicit | Documentation enhancement; integrity strategy in §8.2 + ADR-013 covers the substantive defenses (PAT scoping, validate-tag, SLSA). Could be expanded in a future doc; not blocking. |
| Low | Tera vs Minijinja choice has marginal subjective component | ADR-014 acknowledges; states either is acceptable. No change required. |

### What would change the verdict to rejected

- Discovery that the `needs:` DAG does NOT enforce atomicity in some edge case → would invalidate ADR-010 and US-08 design. (Verified: GitHub Actions documentation explicitly says jobs with unsatisfied `needs:` are skipped, status "skipped", does not run.)
- Discovery that Tera cannot represent the conditional WS-vs-R1 platform-block logic → would invalidate ADR-014. (Verified: Tera supports `{% if %}` blocks natively.)
- Discovery that fine-grained PATs cannot be scoped to a single repo → would invalidate ADR-013. (Verified: GitHub fine-grained PAT documentation supports this exact scoping.)

None of these scenarios materialized in review.

## Iteration Plan

This is iteration 1. No critical or high issues require iteration 2. Approval stands.

The Medium-severity DISTILL testing-infrastructure note is carried forward in `wave-decisions.md` DISTILL handoff §1. The Low-severity threat-model note is captured here for the maintainer to consider as a future documentation enhancement.

## Next Step

DISTILL wave (acceptance-designer / Quinn) is ready to begin. Inputs in `wave-decisions.md` "Wave Handoff Package — To DISTILL".
