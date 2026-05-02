# Adversarial Review — cross-tool-model-unify

**Reviewer**: nw-software-crafter-reviewer
**Date**: 2026-05-02
**Scope**: 28 commits between `a9ae78c` and `HEAD` (cross-tool-model-unify feature DELIVER wave)

## Summary

- **Verdict: APPROVED**
- **0 critical, 0 major, 0 minor findings**
- All 26 DELIVER steps verified; all 568 workspace tests passing; clippy clean

## Findings

### Critical

(none)

### Major

(none)

### Minor

(none)

## TDD Phase Compliance

All 26 steps completed all 5 phases (PREPARE / RED_ACCEPTANCE / RED_UNIT / GREEN / COMMIT).
- 19 RED_ACCEPTANCE EXECUTED, 7 SKIPPED with valid `NOT_APPLICABLE` reasons
- 23 RED_UNIT EXECUTED, 3 SKIPPED (activation/unignore steps)
- One proper escalation (01-08 queue.rs regression — re-dispatched, fixed forward, no test corruption)

## Acceptance Criteria Coverage

All 43 ACs from roadmap.json are exercised by passing tests. Spot-check verifies driving-port entry (assert_cmd binary launches, no internal-class testing).

## ADR Adherence

| ADR | Decision | Status |
|-----|----------|--------|
| ADR-002 | SHA256 conservative-when-uncertain (failed → Unique) | ✓ PASS |
| ADR-003 | No persistent state (in-process cache only) | ✓ PASS |
| ADR-013 | Background hash pool (`min(num_cpus, 4)`, 250ms throttle, 200ms shutdown) | ✓ PASS |
| ADR-013 | tokio-util in modeltap-app only (modeltap-core has no tokio dep) | ✓ PASS |
| ADR-014 | Synthetic slots are render-only (never `impl Tool`, never in plugin registry) | ✓ PASS |

## Architecture Lint

| Rule | Status |
|------|--------|
| R1: modeltap-core has NO tokio dep | ✓ PASS |
| R2: Plugin registry stays `Vec<Box<dyn Tool>>` | ✓ PASS |
| R3: Tool trait frozen at 6 methods | ✓ PASS |
| R4: No domain-layer unit tests bypassing driving ports | ✓ PASS |
| R5: Acceptance tests use real I/O (real fs, real plugins) | ✓ PASS |
| R6: All steps have full TDD phases or justified SKIPs | ✓ PASS |

## Testing Theater Scan (7-Pattern)

| Pattern | Found |
|---------|-------|
| Zero-assertion test | NONE |
| Tautological assertion | NONE |
| Mock-dominated SUT | NONE |
| Circular verification | NONE |
| Always-green (suppressed failures) | NONE |
| Fully-mocked SUT | NONE |
| Assertion-free smoke test | NONE |

All tests would fail if production logic were removed.

## RPP Smell Scan (L1-L2)

- No dead code, no LLM-residue comments, no magic numbers (all named or ADR-referenced).
- Longest new method ~40 lines (worker.rs), single-responsibility.
- Production-code duplication: minimal (one acceptable case in headless.rs/interactive.rs noted but not flagged).
- Complex conditionals: `compute_dedup_glyph` has 5 levels but flat (table-driven).

## External Validity

Features invocable end-to-end through composition root, not just present as test artifacts:
- Walking-skeleton scenario (us_u1) launches real binary via `assert_cmd`, performs full discover→hash→dispatch→unify→verify-inode flow.
- Synthetic [All Unified] slot navigable via j/k keys.
- Hash pool wired post-first-paint per ADR-013 (verified at 01-08 + 01-12).

## Mutation Testing Readiness

Test suite estimated to achieve ≥85% kill rate against:
- Arithmetic mutations (byte counts asserted in us_u2, us_u5, us_u6, us_u7)
- Boolean mutations (display-state assertions in us_u2, us_u3)
- Comparison mutations (glyph classifier tests, plan builder tests)
- Constant mutations (some are env-var driven; not fully exercised — acceptable for v1)

## Recommendations

1. **Production-ready.** Proceed to Phase 5 mutation testing then Phase 7 finalize.
2. **Pre-release**: `cargo-mutants` against `modeltap-core` logic to confirm ≥80% kill rate.
3. **Future**: Document `MODELTAP_HASH_WORKERS` in `--help` if users on slow HDDs report performance issues.

## Verdict

**APPROVED** — Zero blocking issues. Feature ready for finalize.
