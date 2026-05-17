# Peer Review — tool-model-info-sqlite-cache DESIGN

**Wave:** DESIGN (3 of 6), self-review
**Reviewer:** Morgan (nw-solution-architect)
**Date:** 2026-05-17
**Artifacts reviewed:** `architecture-design.md`, `technology-stack.md`, `component-boundaries.md`, `data-models.md`, `ADR-015`, `ADR-016`, `ADR-017`, `ADR-018`, `ADR-003` (superseded header)

This is the self-review pass before handing off to `solution-architect-reviewer`. The review uses the 5-dimension `nw-sa-critique-dimensions` skill plus the antipattern checklist requested in the parent task brief.

## Dimension 1: Architectural Bias Detection

### Technology Preference Bias

**Detected?** No.

**Evidence:**
- `rusqlite` chosen with ADR-017 alternatives analysis (`sqlx` rejected with quantitative reasons: 3× dep footprint, async-first when we need sync, compile-time SQL checking hostile to CI).
- `rusqlite_migration` chosen over hand-rolled migrator with cost analysis (50 LoC framework calls vs 200+ LoC hand-rolled migrator + tests).
- `dirs` chosen over `directories` (similar API) and over hardcoded paths (wrong on macOS).
- Default-body trait method pattern chosen over capability subtrait per ADR-016 — same reasoning as the precedent ADR-010.

**Severity:** 0 / clean.

### Resume-Driven Development

**Detected?** No.

**Evidence:**
- Solo developer, 4 tables, 1 cache file. No microservices. No event bus. No CQRS. No serverless. No GraphQL. No service mesh.
- `modeltap-store` is one new sync Rust crate with 3 dependencies — the simplest reasonable shape for "embedded SQLite cache."
- ADR-015 explicitly considered JSON/TOML alternatives (Alt B, Alt C) and rejected SQLite would have been the resume-driven choice if alternatives sufficed; they didn't (no transactional writes, no WAL, no native indexed queries).

**Severity:** 0 / clean.

### Latest Technology Bias

**Detected?** No.

**Evidence:**
- `rusqlite` v0.31 — established since 2017, 2.9K+ stars, monthly releases.
- `rusqlite_migration` v1.2 — single-maintainer risk explicitly flagged with mitigation (small API surface, inlinable in <100 LoC if abandoned).
- `dirs` v5 — semver-stable since 2020.
- No bleeding-edge or pre-1.0 deps. Tokio version unchanged.

**Severity:** 0 / clean.

## Dimension 2: ADR Quality Validation

### Missing Context

| ADR | Context section | Verdict |
|---|---|---|
| ADR-015 | Quotes intake brief, lists 4 reasons for reversal, references ADR-003's own §"Migration trigger" anticipating this | Strong |
| ADR-016 | Explains the inspection feature requirement, sets up Q-INFO-1, references precedent ADR-010 | Strong |
| ADR-017 | Lists 4 constraints from C-INFO-6; references DISCUSS deferral | Strong |
| ADR-018 | Explains the seam between ADR-013 / ADR-015 / future US-27 | Strong |

**Severity:** 0 / clean.

### Missing Alternatives Analysis

| ADR | Alternatives count | Each evaluated against requirements? | Verdict |
|---|---|---|---|
| ADR-015 | 4 (A: keep ADR-003, B: JSON, C: per-tool JSON, D: SQLite CHOSEN) | Yes — each with pros/cons mapped to constraints | Strong |
| ADR-016 | 3 (A: required methods, B: capability subtrait, C: default-body CHOSEN) | Yes — references ADR-010's same analysis | Strong |
| ADR-017 | 4 (A: rusqlite_migration CHOSEN, B: hand-rolled, C: sqlx::migrate!, D: refinery) | Yes | Strong |
| ADR-018 | 4 (A: defer everything, B: only file-level, C: only model-level, D: both tiered CHOSEN) | Yes | Strong |

**Severity:** 0 / clean.

### Missing Consequences

All 4 ADRs include Positive/Negative/Neutral consequences and enforcement sections.

**Severity:** 0 / clean.

## Dimension 3: Completeness Validation

### Missing Quality Attributes

ISO 25010 checklist against this design:

| Attribute | Addressed? | Where |
|---|---|---|
| Performance Efficiency (time behavior) | YES | K-INFO-1/2/7 in §8.1; warm-start path budget; index strategy in data-models.md |
| Performance Efficiency (resource utilization) | YES | Cache size budget in data-models.md §"Storage estimates"; bundled SQLite ~1 MB |
| Compatibility (interoperability) | YES | Cross-platform paths via `dirs`; SQLite file portable across macOS/Linux/WSL |
| Reliability (fault tolerance) | YES | Corruption recovery §7.4; per-tool reconcile failure isolation; "cache failure never blocks launch" |
| Reliability (recoverability) | YES | K-INFO-4 strategy; three failure modes → one recovery path |
| Reliability (availability) | YES | Cache is optimization, not dependency |
| Security (confidentiality, privacy) | YES | Cache stays local; no network I/O; parent C5 carries forward |
| Security (integrity) | YES | Pre-mutate revalidation invariant + R9 architecture-lint |
| Maintainability (modularity) | YES | Modular monolith preserved; new crate respects R7/R8 |
| Maintainability (testability) | YES | §8.4 — `Cache::open_in_memory()`, repo-level unit tests, plugin contract test extension |
| Portability | YES | Cross-platform path resolution; `bundled` SQLite removes system version skew |

**Severity:** 0 / clean.

### Missing Performance Architecture

**Detected?** No.

K-INFO-1 (warm-start ≤100 ms) has a written budget (§7.2): cache reads <50 ms via spawn_blocking + indexed queries; first paint at T0+100ms. K-INFO-7 (cache overhead ≤50 ms) has a guardrail. Indexes on `(tool_id, last_seen_at)` and partial index on `sha256` map directly to the warm-paint queries.

**Severity:** 0 / clean.

## Dimension 4: Implementation Feasibility

### Team Capability Mismatch

**Detected?** No.

**Evidence:**
- Solo developer; CLAUDE.md says rust-idiomatic multi-paradigm is the prescribed style.
- The work is straightforward Rust: one new crate, one trait extension (precedent in ADR-010), SQL DDL, a migration framework, three architecture-lint rules.
- No async/sync concurrency novelty — `spawn_blocking` is already used by ADR-013's hash pool.
- No new architectural pattern. All seams are existing patterns (hexagonal ports, Elm-style update loop, default-body trait extension).

**Severity:** 0 / clean.

### Budget Constraints

**Detected?** No.

**Evidence:**
- No infrastructure costs. SQLite is embedded; the cache file lives on the user's local disk.
- No telemetry uploaded by default (parent C5).
- Binary size grows by ~1 MB (bundled SQLite) — within reasonable bounds for a CLI tool.

**Severity:** 0 / clean.

### Testability Validation

**Detected?** No.

**Evidence:**
- `Cache::open_in_memory()` is the explicit test seam. Unit tests can populate any cache state without touching the filesystem.
- The pre-mutate revalidator is testable end-to-end with `tempfile` fixtures (parent feature already uses this pattern).
- Plugin contract test under `crates/modeltap-core/tests/inspect_contract.rs` extends the existing pattern.
- Architecture-lint R7-R9 are testable as part of `cargo test --workspace`.

**Severity:** 0 / clean.

## Dimension 5: Priority Validation

### Q1: Largest bottleneck?

**Evidence:** K-INFO-1 (warm-start ≤100 ms p90) is the user-facing North Star. The parent K3 (first paint <1 s) is REDEFINED into K3a (warm-start) and K3b (cold-start) — Devon Park's dogfooded complaint is the latency tax of cold-start on every launch. This is the largest user-experience problem the feature solves.

**Assessment:** YES.

### Q2: Simpler alternatives considered?

**Evidence:** ADR-015 considered: (A) keep stateless, (B) JSON file index, (C) per-tool JSON files, (D) SQLite. Each rejected with reasoning. ADR-016 considered: (A) required methods, (B) capability subtrait, (C) default-body. Each rejected/accepted with reasoning consistent with ADR-010 precedent.

**Assessment:** ADEQUATE.

### Q3: Constraint prioritization correct?

**Evidence:** Quality attribute ranking (§2 of architecture-design.md):
1. Latency (K-INFO-1 is North Star)
2. Trust/correctness (K5 extension; load-bearing R9 invariant)
3. Recoverability (K-INFO-4; explicit failure-mode taxonomy)
4. Testability
5. Maintainability

The largest user-visible value (latency) and the largest safety risk (cache enabling stale-data destructive action) are the top two priorities. No prioritization inversion detected.

**Assessment:** CORRECT.

### Q4: Data-justified?

**Evidence:** Performance NFRs come from DISCUSS measurements (parent K3 ~1.15 s full inventory measured; K-INFO-1 derived from this baseline; cache overhead K-INFO-7 ≤50 ms is the differential budget). Cache size estimates (data-models.md §"Storage estimates") derived from typical-Devon profile data.

**Assessment:** JUSTIFIED.

## Antipattern Checklist (from task brief)

### Big Ball of Mud

**Q:** Does `modeltap-store` have clear seams?

**A:** Yes. Public API is 12 methods on the `Cache` struct, all returning `Result<T, CacheError>`. Internal modules are repository-per-table plus migrate/recovery/open. The crate has no `pub use` re-exports beyond the typed row data structures. No circular deps (R7/R8 enforce).

**Severity:** 0 / clean.

### Premature Optimization

**Q:** Is rusqlite + WAL warranted vs in-memory KV?

**A:** Yes:
- Multi-process concurrency (US-23 Scenarios 4-5) requires WAL.
- Transactional writes are required for partial-write recovery.
- Indexed queries (warm-start, dedup-group lookup) are O(log N) in SQL; in-memory KV requires a separate index structure.
- Embedded SQLite is ~1 MB; no operational footprint beyond the cache file itself.

**Severity:** 0 / clean.

### Distributed Monolith

**Q:** Does pre-mutate revalidation create hidden coupling between modeltap-store and the orchestrator?

**A:** Minor concern. `revalidate::pre_mutate` in `modeltap-app::orchestration` is the single choke point that depends on `Cache::verify_against_fs`. The coupling is explicit and intentional (it's the safety invariant), enforced by R9 architecture-lint. However, the coupling means that **changes to the validation result enum (`ValidationResult`)** propagate from `modeltap-store` to every mutation orchestrator. This is acceptable for v1 because:
- The enum is small and stable (`Match | Drift | Gone`).
- Architecture-lint R9 statically catches mutation sites that bypass the revalidator.

**Mitigation already in design:** the enum is a pure-data type in `modeltap-store::types`; consumers `match` on it exhaustively, so the compiler enforces correctness if the enum grows.

**Severity:** LOW.

### Vendor Lock-in

**Q:** Is rusqlite-specific code escaped to one adapter?

**A:** Yes. Only `modeltap-store` imports `rusqlite`. Architecture-lint R7 enforces. The public API of `Cache` returns typed Rust values, not `rusqlite::Row` or connection types. Swapping the backend (hypothetically to `redb` or any other embedded KV/SQL) requires changing only `modeltap-store` internals — the `Cache::open`, `Cache::tools`, `Cache::write_*` API stays stable.

**Severity:** 0 / clean.

### Test Seam Clarity

**Q:** Can each layer be unit-tested without a real SQLite file?

**A:** Yes:
- `modeltap-store` repository methods: `Cache::open_in_memory()` for in-process tests.
- `modeltap-app::orchestration::revalidate`: in-memory cache + `tempfile`-backed fixtures for the filesystem side.
- `modeltap-tui` view of detail screens: snapshot-tested with ratatui's `TestBackend` against an `AppState` literal containing `ToolDetail` / `ModelDetail` values.
- `modeltap-core` pure types: trivially unit-testable.

**Severity:** 0 / clean.

## Summary

| Dimension | Severity | Notes |
|---|---|---|
| Architectural bias | 0 | Clean across all 3 sub-dimensions |
| ADR quality | 0 | All 4 new ADRs have context, alternatives, consequences, enforcement |
| Completeness | 0 | ISO 25010 attributes addressed; performance architecture explicit |
| Implementation feasibility | 0 | Solo dev capability matched; no budget concerns; testability strong |
| Priority validation | 0 | Q1-Q4 all pass |
| Antipattern: Big Ball of Mud | 0 | Clean |
| Antipattern: Premature optimization | 0 | Clean |
| Antipattern: Distributed monolith | LOW | One acceptable coupling (revalidate ↔ Cache::verify_against_fs); mitigated by R9 + enum stability |
| Antipattern: Vendor lock-in | 0 | rusqlite isolated to one crate per R7/R8 |
| Antipattern: Test seam clarity | 0 | Clean — `:memory:` opener + `tempfile` fixtures + ratatui TestBackend |

**Critical issues:** 0
**High issues:** 0
**Medium issues:** 0
**Low issues:** 1 (the revalidate coupling, mitigated and accepted)

## Approval status

**APPROVED for handoff** with the LOW-severity observation noted in the Distributed Monolith section. No revisions required before invoking `solution-architect-reviewer`.

## Handoff package

Ready for next wave (DEVOPS — `platform-architect`):

- Architecture document: `docs/feature/tool-model-info-sqlite-cache/design/architecture-design.md`
- Component boundaries: `docs/feature/tool-model-info-sqlite-cache/design/component-boundaries.md`
- Data models: `docs/feature/tool-model-info-sqlite-cache/design/data-models.md`
- Technology stack: `docs/feature/tool-model-info-sqlite-cache/design/technology-stack.md`
- ADRs: `docs/adrs/ADR-015-state-model-sqlite-cache.md`, `ADR-016-tool-trait-inspect.md`, `ADR-017-schema-migration.md`, `ADR-018-sha256-persistence.md`
- Superseded ADR: `docs/adrs/ADR-003-state-model.md` (header added)
- This self-review: `docs/feature/tool-model-info-sqlite-cache/design/peer-review.md`

DEVOPS items to consume (per outcome-kpis.md handoff notes):

- New log line schemas: `cache_recovery`, `cache_migration`, `cache_verify`, `reconcile_failed`.
- CI alert: warm-start p90 > 200 ms = regression (K-INFO-1).
- CI alert: cache layer overhead > 100 ms = regression (K-INFO-7).
- No new dashboards needed (parent constraint).
- No new external integrations (no contract tests required beyond parent's existing Ollama plugin contract).

**Recommended next wave: DISTILL (acceptance-designer)**, not DEVOPS. Rationale: this feature does not introduce new CI/CD infrastructure or external integrations (existing CI lints extend with R7-R9 in the same `tests/architecture.rs`; no new pipeline stages). DEVOPS adds value when there is real platform/observability/infrastructure work; for this feature, the platform delta is minimal (a few log line tags). DISTILL is the higher-leverage next step — write the executable BDD scenarios that DELIVER's Outside-In TDD will drive against. DEVOPS can run in parallel or be folded into the DELIVER walking-skeleton step.

Per the nWave standard the user can choose either order; this recommendation is informed by the artifact analysis.
