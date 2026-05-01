# Definition of Ready: cross-tool-model-unify

DoR is a hard gate. Each story must pass all 9 items before handoff to DESIGN. This document records pass/fail per story with evidence.

DoR items:

1. Problem statement clear, in domain language
2. User/persona identified with specific characteristics
3. >=3 domain examples with real data
4. UAT in Given/When/Then (3-7 scenarios)
5. AC derived from UAT
6. Right-sized (1-3 days, 3-7 scenarios)
7. Technical notes identify constraints
8. Dependencies resolved or tracked
9. Outcome KPIs defined with measurable targets

---

## US-U1: Background SHA256 hashing with progress

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | "v1 summary bar shows 0 because hashing is on-demand" — domain language, references real bug |
| User/persona identified | PASS | Devon, small-team developer with 2+ tools |
| 3+ domain examples | PASS | Devon (typical), Maya (slow disk), Devon (quit during hash) |
| UAT scenarios (3-7) | PASS | 5 scenarios |
| AC derived from UAT | PASS | 6 AC items, each maps to a scenario |
| Right-sized | PASS | ~3 days; 5 scenarios — within envelope |
| Technical notes | PASS | ADR-002, Q7, ADR-001 dependency, hash-queue placement deferred to DESIGN |
| Dependencies tracked | PASS | None upstream; foundation story |
| Outcome KPIs defined | PASS | KPI-1 with 95% target, 60 s window, baseline 0% |

**Status: PASSED**

---

## US-U2: Wire dedup-able bytes from classifier to summary bar

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | Names the file+line of the v1 bug; uses domain language |
| User/persona identified | PASS | Devon, with concrete 9.4 GB scenario |
| 3+ domain examples | PASS | Devon (with dups), Riley (no dups), Devon (mid-hash) |
| UAT scenarios (3-7) | PASS | 4 scenarios |
| AC derived from UAT | PASS | 5 AC items |
| Right-sized | PASS | ~1 day; trivial wiring |
| Technical notes | PASS | Identifies exact bug location, references shared-artifacts-registry |
| Dependencies tracked | PASS | US-U1 |
| Outcome KPIs defined | PASS | KPI-1 |

**Status: PASSED**

---

## US-U3: Row glyph reflects dedup state

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | Domain language; describes scanning need |
| User/persona identified | PASS | Devon scanning 19+ rows |
| 3+ domain examples | PASS | Mixed states, all-unique, hash failure |
| UAT scenarios (3-7) | PASS | 6 scenarios |
| AC derived from UAT | PASS | 6 AC items |
| Right-sized | PASS | ~2 days |
| Technical notes | PASS | Single-source pattern, NO_COLOR compliance |
| Dependencies tracked | PASS | US-U1, US-U2 |
| Outcome KPIs defined | PASS | Contributes to KPI-1, KPI-2 |

**Status: PASSED**

---

## US-U4: `u` from main view opens unify dialog with mates pre-populated

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | "v1 hotkey is a lie" — concrete |
| User/persona identified | PASS | Devon scanning rows, expects direct action |
| 3+ domain examples | PASS | `=` row happy, `#` row info, `?` row hint |
| UAT scenarios (3-7) | PASS | 5 scenarios (one per glyph case + regression) |
| AC derived from UAT | PASS | 5 AC items |
| Right-sized | PASS | ~2 days |
| Technical notes | PASS | Reuses `Msg::OpenUnifyDialog`, references existing canonical/plan code |
| Dependencies tracked | PASS | US-U3 |
| Outcome KPIs defined | PASS | KPI-2 with 60% target |

**Status: PASSED**

---

## US-U5: Unify dialog shows concrete reclaim preview and applies plan

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | Confidence/destructive-feel framing in domain language |
| User/persona identified | PASS | Devon at the confirmation moment |
| 3+ domain examples | PASS | Three-tool unify, toggle-off-one, cancel |
| UAT scenarios (3-7) | PASS | 5 scenarios |
| AC derived from UAT | PASS | 7 AC items |
| Right-sized | PASS | ~2 days; reuses existing apply path |
| Technical notes | PASS | References existing `actions::unify::run()`, ADR-008, Q5 |
| Dependencies tracked | PASS | US-U4 |
| Outcome KPIs defined | PASS | KPI-2, KPI-3 |

**Status: PASSED**

---

## US-U6: Post-unify row glyph and summary bar update without restart

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | "Restart-to-see-result would gut the payoff" |
| User/persona identified | PASS | Devon needing immediate confirmation |
| 3+ domain examples | PASS | Full success, partial (cross-fs skip), total failure |
| UAT scenarios (3-7) | PASS | 4 scenarios |
| AC derived from UAT | PASS | 7 AC items |
| Right-sized | PASS | ~2 days |
| Technical notes | PASS | JSONL event consumption, full-vs-scoped re-classify deferred to DESIGN |
| Dependencies tracked | PASS | US-U2, US-U3, US-U5 |
| Outcome KPIs defined | PASS | KPI-3 |

**Status: PASSED**

---

## US-U7: `[All Unified]` pseudo-tool slot in left pane

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | Audit-without-scanning, in domain language |
| User/persona identified | PASS | Devon as auditor / cumulative-savings tracker |
| 3+ domain examples | PASS | Five unified, count-parity, zero count |
| UAT scenarios (3-7) | PASS | 5 scenarios |
| AC derived from UAT | PASS | 6 AC items |
| Right-sized | PASS | ~2 days |
| Technical notes | PASS | NOT a `Tool` impl (BR-4); ADR-001 respected |
| Dependencies tracked | PASS | US-U3 |
| Outcome KPIs defined | PASS | KPI-4 |

**Status: PASSED**

---

## US-U8: `[All Unified]` empty state with onboarding guidance

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | Empty-state-as-dead-end in domain terms |
| User/persona identified | PASS | Devon (or new user) on first visit |
| 3+ domain examples | PASS | Fresh install, post-delete-empty, mid-hash |
| UAT scenarios (3-7) | PASS | 3 scenarios |
| AC derived from UAT | PASS | 3 AC items |
| Right-sized | PASS | ~1 day; pure rendering |
| Technical notes | PASS | No core changes |
| Dependencies tracked | PASS | US-U7 |
| Outcome KPIs defined | PASS | KPI-4 contributor |

**Status: PASSED**

---

## US-U9: Detail screen for unified model shows shared inode and paths

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | "Skeptical Devon needs filesystem-level proof" |
| User/persona identified | PASS | Riley as auditor primary, Devon secondary |
| 3+ domain examples | PASS | `#` happy, `=` multi-inode, missing-inode |
| UAT scenarios (3-7) | PASS | 3 scenarios |
| AC derived from UAT | PASS | 4 AC items |
| Right-sized | PASS | ~2 days |
| Technical notes | PASS | `stat()` filesystem-dependence flagged for DESIGN; ADR-001 respected |
| Dependencies tracked | PASS | US-U7 |
| Outcome KPIs defined | PASS | KPI-4 |

**Status: PASSED**

---

## US-U10: Partial-success reporting (per-target outcome in toast)

| DoR Item | Status | Evidence/Issue |
|---|---|---|
| Problem statement clear | PASS | "JSONL exists but toast is non-specific" — concrete |
| User/persona identified | PASS | Devon at partial-success moment |
| 3+ domain examples | PASS | Single failure + retry, total failure, low-level OS error |
| UAT scenarios (3-7) | PASS | 3 scenarios |
| AC derived from UAT | PASS | 5 AC items |
| Right-sized | PASS | ~2 days |
| Technical notes | PASS | Reuses existing JSONL events |
| Dependencies tracked | PASS | US-U5 |
| Outcome KPIs defined | PASS | KPI-3 contributor |

**Status: PASSED**

---

## Aggregate

| Story | DoR Status |
|---|---|
| US-U1 | PASSED |
| US-U2 | PASSED |
| US-U3 | PASSED |
| US-U4 | PASSED |
| US-U5 | PASSED |
| US-U6 | PASSED |
| US-U7 | PASSED |
| US-U8 | PASSED |
| US-U9 | PASSED |
| US-U10 | PASSED |

**Aggregate DoR: 10/10 PASSED. Feature is ready for DESIGN handoff.**
