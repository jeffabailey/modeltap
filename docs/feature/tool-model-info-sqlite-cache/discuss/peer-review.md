# Peer Review — tool-model-info-sqlite-cache (DISCUSS wave)

Self-review using the 5-dimension critique from `nw-po-review-dimensions`. Performed inline as a second-pass critique by the same product-owner persona (Luna) — not a second-opinion review by `nw-product-owner-reviewer`. Recommendation: a second-opinion review BEFORE DESIGN wave starts; nothing in this document blocks that handoff.

Reviewer persona: independent requirements reviewer. Mindset: fresh perspective, assume nothing, challenge assumptions.

```yaml
review_id: "req_rev_20260516_inline"
reviewer: "product-owner (self-review mode, Luna persona)"
artifact: "docs/feature/tool-model-info-sqlite-cache/discuss/*"
iteration: 1

strengths:
  - "Constraint reversal of ADR-003 (stateless rediscovery) is called out explicitly, with explicit rationale, in requirements.md 'Supersedes prior constraint' section. The 9 items the new ADR must close are enumerated."
  - "The 'cache is paint-only; filesystem is authoritative on mutate' rule (C-INFO-1) is the central safety contract and is captured as both a hard constraint AND as an integration test invariant (AC-26-8) — design rule + enforcement together."
  - "JTBD analysis surfaces 7 jobs (J1..J7) including the operational J7 that would otherwise be invisible; opportunity scores anchor story priority in user-visible value, not in architectural elegance."
  - "Story map honestly grades scope assessment as BORDERLINE on bounded contexts, explains why anyway, and offers Elephant-Carpaccio counter-argument — shows independent thinking, not stenography."
  - "Sequencing recommendation vs in-flight folder-group-bulk-delete (Option C: queue this feature's DESIGN behind folder-group DELIVER) is the actually-recommended lowest-risk path for a solo dev — and the user has explicit override options A/B documented."
  - "Refresh policy is specified concretely: warm-paint + background reconcile + per-tool TTL (24h default) + manual [r] / [Shift+R] + pre-mutate revalidation. Cache lifecycle is closed end-to-end."
  - "K3 redefinition into K3a (warm) + K3b (cold) preserves the parent's contract while extending it — no silent regression."
  - "Cache failure mode (J6) is treated as a v1 mandatory guardrail (US-23 corruption-recovery + recovery banner), not a v1.x add-on. The ADR-003 failure-class that's being re-introduced is explicitly mitigated."

issues_identified:
  confirmation_bias:
    technology_bias: []
    happy_path_bias: []
    availability_bias:
      - issue: "Reliance on `rusqlite_migration` recommendation may be confirmation-driven ('we've used migration crates before')."
        severity: "low"
        location: "prioritization.md 'Schema versioning strategy'; requirements.md Q-INFO-3"
        recommendation: "DESIGN should sanity-check by looking at the rusqlite_migration crate's maintenance status, dep tree, and test coverage. Hand-rolled fallback is documented as acceptable; no blocker."

  completeness_gaps:
    missing_stakeholder_perspectives:
      - issue: "No explicit consideration of users who exclusively use `--no-cache` (e.g., for security audits, ephemeral containers)."
        severity: "low"
        location: "Stakeholders table; requirements.md NFRs"
        recommendation: "Document that the `--no-cache` path is a first-class supported workflow, not a degraded mode. Already covered by AC-23-8/AC-25-7/AC-27-8 in spirit; could surface in user-facing docs."
    missing_error_scenarios:
      - issue: "What happens when the cache database file exists but is on a read-only filesystem (e.g., Docker container with read-only volume mount)?"
        severity: "medium"
        location: "US-23 examples; US-23 ACs"
        recommendation: "Add a UAT scenario: 'Read-only cache file falls back to in-memory-only cache for the launch'. Captured in `requirements.md` Open Questions as effectively a Q-INFO-9; flag for DESIGN."
      - issue: "What happens when the cache file exists but the *directory* containing it is read-only (cache cannot be written but can be read)?"
        severity: "medium"
        location: "Same as above"
        recommendation: "Same fallback: open cache read-only, skip writes, log the situation, proceed. Flag for DESIGN to specify exact behaviour."
    missing_non_functional_requirements:
      - issue: "Storage size growth over time: cache size for a power user with 1000 models is estimated at ~5 MB; but if cache.actions log table grows unbounded, file could grow indefinitely."
        severity: "low"
        location: "requirements.md NFR Performance row 'Cache file size'"
        recommendation: "Specify a cache.actions retention policy: keep last N days or last M entries. DESIGN should close this. Add Q-INFO-9 for cache size growth bounds."

  clarity_issues:
    vague_performance_requirements: []
    ambiguous_requirements:
      - issue: "AC-26-4 'inventory diff detection' — what counts as a 'diff'? Just model count change, or size changes too, or new mtimes?"
        severity: "medium"
        location: "user-stories.md US-26 AC-26-4; acceptance-criteria.md"
        recommendation: "Specify: diff = added or removed model_id from `(model_id, tool_id)` set, OR change in `size_bytes`, OR change in `(mtime, size, inode_dev)` tuple. Mtime-only change is NOT a diff (file content unchanged); the silent ack indicator is only for content-affecting changes."

  testability_concerns:
    - issue: "K-INFO-6 ('decisive action from detail screen ≥ 90%') is hard to instrument across processes. The doc acknowledges this and proxies via survey, but no concrete metric is defined for in-test measurement."
      severity: "low"
      location: "outcome-kpis.md K-INFO-6"
      recommendation: "Already documented as 'hard to instrument' with a survey proxy. Acceptable; DESIGN/DEVOPS will treat this as a no-CI-alert KPI."

  priority_validation:
    q1_largest_bottleneck: "YES — O1 (15.5) is the highest-scoring opportunity and US-22 is the primary carrier; O2 (13.5) is the architectural-refactor justification and US-25 is the carrier; both are addressed by Release 1 and Release 2 respectively."
    q2_simple_alternatives: "ADEQUATE — story-map.md considers and documents the 'two features instead of one' counter-argument; prioritization.md considers three sequencing options (A/B/C) against folder-group-bulk-delete; the per-feature decision tree is captured."
    q3_constraint_prioritization: "CORRECT — the central constraint (ADR-003 stateless rediscovery → cache with paint-only + filesystem-authoritative rule) is foregrounded across requirements.md, journey artifacts, and shared-artifacts-registry. The user-explicit ask (intake brief) drives the reversal, not internal preference."
    q4_data_justified: "JUSTIFIED — opportunity scores are team-estimate (N=1, single rater) but rationale is documented per outcome; baselines exist for K-INFO-1 (parent K3), K-INFO-2 (quit+relaunch), K-INFO-3 (n/a, first-30-days baseline period), K-INFO-7 (ADR-003 cold-start baseline). K-INFO-6 has anecdotal baseline only; documented honestly. No quantitative-data gap that blocks DESIGN."
    verdict: "PASS"

approval_status: "approved"
critical_issues_count: 0
high_issues_count: 0
medium_issues_count: 3
low_issues_count: 4
```

## Action items before DESIGN starts (none blocking; reviewer-judgement)

1. **(MEDIUM)** Add a Q-INFO-9 to `requirements.md` for "cache on read-only filesystem / read-only directory" behaviour. Both scenarios (file read-only; directory read-only) need DESIGN to specify: open read-only + skip writes + log; never crash.

2. **(MEDIUM)** Clarify AC-26-4 inventory diff definition in `user-stories.md` and `acceptance-criteria.md`: diff = added/removed `(model_id, tool_id)` OR `size_bytes` change OR `(mtime, size, inode_dev)` tuple change. Mtime-only change without size change is NOT a diff (no silent ack indicator).

3. **(MEDIUM)** Add Q-INFO-10 for cache.actions retention policy (cache size growth bounds). Default recommendation: keep last 30 days OR last 10000 entries, whichever is smaller; DESIGN owns the final number.

4. **(LOW)** Note in user-facing docs that `--no-cache` is a first-class supported workflow.

5. **(LOW)** DESIGN should sanity-check `rusqlite_migration` crate health (dep tree, maintenance, test coverage) before adopting.

These items are non-blocking — they're refinements DESIGN can close as part of writing the supersession-of-ADR-003 ADR. None of them break DoR (the affected stories still PASS all 9 items because the gaps are at the constraint/AC-clarification level, not the per-story DoR level).

## Recommendation

**APPROVED for handoff to DESIGN wave.** Open items above are documented for DESIGN to close in the new ADR; none block handoff. A second-opinion review by `nw-product-owner-reviewer` (or equivalent) is encouraged but not required.

## Review confidence

- **Method:** self-review by the same persona (Luna). The 5-dimension critique was applied; the strengths and gaps reflect a deliberate adversarial pass on this DISCUSS wave's own output.
- **Risk of confirmation bias in the review itself:** documented. The biggest risk is that the reviewer (same persona as the author) shares blindspots with the author. Mitigation: the surfaced issues are documented honestly (3 MEDIUM, 4 LOW); none of them are flattering.
- **Confidence level:** HIGH for DoR pass; MEDIUM for completeness of edge-case coverage; HIGH for sequencing decision (Option C is defensible regardless of perspective).
