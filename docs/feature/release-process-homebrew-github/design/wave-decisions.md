# Wave Decisions Summary — release-process-homebrew-github (DESIGN)

DESIGN wave (wave 3 of 6) decisions, deviations from defaults, and handoff state.

## Wave Configuration

| Setting | Value | Rationale |
|---|---|---|
| Format | architecture-design + technology-stack + component-boundaries + data-models + 5 ADRs | Per nw-solution-architect default for a feature whose architecture surface includes both code and CI workflows |
| Architecture style | Functional-core / imperative-shell (xtask) + workflow-DAG (release.yml) | Mirrors project's established paradigm (CLAUDE.md: pure-functional core, async I/O at edges); functional-core fits the largely-stateless render/validate/extract logic |
| Stress analysis | NOT executed | `--residuality` flag not set; default behavior |
| Auto mode | active | Inherited from /nw:new wizard via DISCUSS |
| Peer review | nw-solution-architect-reviewer (Atlas) | Hard gate per workflow |

## Personas (inherited from DISCUSS)

- **Jeff Bailey** — single maintainer; primary persona for the entire pipeline
- **Devon Park** — end-user installer (reused from modeltap-tui)
- **Riley Chen** — open-source contributor reading workflow + runbook (reused from modeltap-tui)

No new personas introduced in DESIGN.

## Decisions Resolved in DESIGN

These extend or operationalize DISCUSS decisions D1-D8:

| ID | Decision | Choice | ADR | Reasoning |
|---|---|---|---|---|
| DESIGN-01 | Workflow file layout | **Single `release.yml` with multi-job DAG** | ADR-010 | Atomic-publish (C2) is naturally expressed as a `needs:` DAG in one file. Multi-file via `workflow_run` makes atomicity HARDER to enforce. |
| DESIGN-02 | xtask placement | **Repo-root `xtask/` excluded from default-members** | ADR-011 | Standard Rust community convention (cargo-xtask pattern). Clean dep separation: Tera/toml_edit don't pollute production crates. |
| DESIGN-03 | Cross-compile mechanism for aarch64-linux | **`cross` v0.2.5** | ADR-012 | Reliability over speed (~1 min cold-cache cost). Mature tool; broader glibc compat than manual rustup-target. Native ubuntu-22.04-arm GA → revisit (OQ-3). |
| DESIGN-04 | Tap-bump credential | **Fine-grained PAT (`GH_TAP_TOKEN`), tap-repo-only scope** | ADR-013 | Confirms DISCUSS D7. Single-maintainer; least-privilege. Migration to GitHub App documented for multi-maintainer future. |
| DESIGN-05 | Formula templating | **Tera in xtask, not inline shell** | ADR-014 | Saves ~40 lines of release.yml budget; type-safe sha256 validation; mutation-testable. Inline shell silently embeds malformed sha256s. |
| DESIGN-06 | Subcommand catalog | **5 xtask subcommands**: release-prep, validate-tag, render-formula, extract-changelog, lint-workflows | (in component-boundaries.md §2.1) | Each subcommand has one purpose; pure-function core + thin shell adapters. |
| DESIGN-07 | Build-cache strategy | **`Swatinem/rust-cache@v2` keyed per (target, Cargo.lock hash)** | (in technology-stack.md §2) | Matches ci.yml. Shared cache where keys overlap. First release cold; subsequent warm. |
| DESIGN-08 | Release artifact naming | **`modeltap-{version}-{target}.tar.gz` (single `modeltap` binary inside)** | (in data-models.md §4) | Confirms DISCUSS US-04.AC-3. Bare-hex `.sha256` sidecar (NOT GNU `sha256sum` two-field format) — documented to prevent format bugs. |

No DESIGN decision contradicts any DISCUSS decision (D1-D8) or any constraint (C1-C8).

## Phase Execution Summary

| Phase | Activity | Status |
|---|---|---|
| Phase 1 | Requirements analysis (re-read DISCUSS in full) | COMPLETE |
| Phase 2 | Existing system analysis (`.github/workflows/ci.yml`, root `Cargo.toml`, modeltap-tui design) | COMPLETE |
| Phase 3 | Constraint and priority analysis (8 hard constraints C1-C8 + 6 KPIs) | COMPLETE |
| Phase 4 | Architecture design (5 design docs + 5 ADRs) | COMPLETE |
| Phase 4.5 | Stress analysis (residuality) | SKIPPED — `--residuality` not set |
| Phase 5 | Quality validation (DoD checklist in architecture-design.md §12) | COMPLETE |
| Phase 6 | Peer review and handoff | **APPROVED by `nw-solution-architect-reviewer` (Atlas) 2026-05-03 — iteration 1** |

## Artifacts Produced

All under `docs/feature/release-process-homebrew-github/design/`:

| Artifact | Purpose |
|---|---|
| `architecture-design.md` | Master document. C4 L1+L2 (Mermaid), L3 for xtask. Quality attribute strategies. ADR index. |
| `technology-stack.md` | Pinned versions for every external tool, action, crate. Licenses. Forbidden tools. |
| `component-boundaries.md` | Component-by-component contracts. xtask subcommand catalog. release.yml job-by-job specs. |
| `data-models.md` | FormulaCtx schema. Shared-artifact flow diagram (Mermaid). Rendered formula shape. Archive naming. |
| `wave-decisions.md` | (this file) |

Under `docs/adrs/`:

| ADR | Title |
|---|---|
| ADR-010 | Release pipeline architecture — single workflow file, multi-job DAG |
| ADR-011 | xtask placement — repo-root xtask/ excluded from default workspace |
| ADR-012 | Cross-compile strategy — `cross` for aarch64-linux |
| ADR-013 | Tap-repo credential — fine-grained PAT (GH_TAP_TOKEN) |
| ADR-014 | Formula templating — Tera in xtask, not inline shell |

## Story-to-Component Traceability

Confirms every DISCUSS user story has a designed home:

| Story | Implementing component(s) |
|---|---|
| US-01 release-prep | `xtask::release-prep` subcommand |
| US-02 validate-tag | `xtask::validate-tag` subcommand + `release.yml` `validate-tag` job |
| US-03 CI parity gates | `release.yml` `build` job steps 5-7 (fmt, clippy, test before cargo build) |
| US-04 single-target build | `release.yml` `build` job steps 8-13 (build, strip, package, sha256, attest, upload) |
| US-05 publish-github-release | `release.yml` `publish-github-release` job + `xtask::extract-changelog` |
| US-06 bump-tap-formula opens PR | `release.yml` `bump-tap-formula` job + `xtask::render-formula` + `GH_TAP_TOKEN` (ADR-013) |
| US-07 4-target matrix | `release.yml` `build` matrix.target with 4 entries; aarch64-linux via `cross` (ADR-012) |
| US-08 atomic-publish guard | `release.yml` `needs:` DAG (ADR-010) |
| US-09 SLSA attestation | `release.yml` `build` step 12 (`actions/attest-build-provenance@v2`) |
| US-10 4 platform blocks | `xtask::render-formula` + `release/templates/modeltap.rb.tera` (ADR-014) |
| US-11 auto-merge | `release.yml` `bump-tap-formula` step 10 (`gh pr merge --auto --squash`) + tap repo branch protection |
| US-12 idempotent retry | `release.yml` `bump-tap-formula` step 5 (check existing branch, force-push-with-lease) |
| US-13 RELEASING.md runbook | `RELEASING.md` (component-boundaries.md §7) |
| US-14 ≤250-line workflow | `xtask::lint-workflows` subcommand + `ci.yml` invocation |
| US-15 end-user install | (no new component — verifies the entire pipeline works end-to-end) |

Every cross-story integration AC (INT.AC-1 .. INT.AC-6) is traced in `data-models.md` §2 invariants table.

## Quality Gates Status

| Gate | Status |
|---|---|
| Requirements traced to components | PASS (story table above) |
| Component boundaries with clear responsibilities | PASS (`component-boundaries.md`) |
| Technology choices in ADRs with alternatives (≥2) | PASS (ADR-010..014; each has 2-4 rejected alternatives) |
| Quality attributes addressed | PASS (architecture-design.md §8: reliability, integrity, maintainability, performance, coverage) |
| Dependency-inversion compliance | PASS (xtask functional core / imperative shell; pure functions take strings, return strings) |
| C4 diagrams (L1+L2 minimum, Mermaid) | PASS (architecture-design.md §4.1, §4.2; L3 for xtask in §4.3) |
| Integration patterns specified | PASS (architecture-design.md §7) |
| OSS preference validated | PASS (technology-stack.md — every dep MIT/Apache-2.0 or BSD; no proprietary) |
| AC behavioral, not implementation-coupled | PASS (DESIGN inherits DISCUSS AC; introduces no implementation-coupled AC) |
| External integrations annotated for contract testing | PASS (architecture-design.md §7.3 — brew test-bot IS the contract gate; SLSA verify IS the attestation contract) |
| Architectural enforcement tooling recommended | PASS (`cargo xtask lint-workflows` for US-14; `cargo deny` already in CI) |
| Peer review completed | PENDING (next phase) |

## Risks Surfaced (managed in downstream waves)

Inherited from DISCUSS risk register; DESIGN adds:

| Risk | Probability | Impact | Owner | Status |
|---|---|---|---|---|
| `cross` Docker image pull failure on cold-cache release | Low | Low (retry succeeds) | Maintainer | Acceptable; documented |
| Tera template syntax error introduced via PR | Low | Medium (release fails at render step) | Software-crafter (DELIVER tests) | Mitigated by xtask unit tests on `render()` |
| `default-members` drift when new runtime crate added | Medium (over time) | Low (CI catches via `cargo build --workspace`) | Maintainer | Acceptable; future xtask subcommand could lint |
| `tera` crate becomes unmaintained | Low | Low (swappable for minijinja) | Maintainer | Acceptable; ADR-014 lists alternatives |
| GH Actions deprecates `actions/attest-build-provenance@v2` | Medium (annual majors) | Medium (must bump pin) | Maintainer | Acceptable; pinned to `@v2`; visible breaks |

## Wave Handoff Package

### To DISTILL (acceptance-designer / Quinn)

**Inputs:**
- All DESIGN artifacts under `docs/feature/release-process-homebrew-github/design/`
- All DISCUSS artifacts under `docs/feature/release-process-homebrew-github/discuss/`
- ADRs ADR-010..014 under `docs/adrs/`

**Test design challenges:**
1. **Cross-repo testing**: acceptance scenarios involve modeltap repo + tap repo. DISTILL must design test infrastructure that can stand up an ephemeral tap repo OR mock the tap-repo seam for fast iteration. Already flagged in DISCUSS handoff.
2. **GH Actions execution**: most acceptance scenarios involve workflow runs. Options: (a) act (run GH Actions locally), (b) ephemeral test repo with real GH Actions runs, (c) per-step xtask unit + integration tests with workflow YAML linted by `xtask lint-workflows`. Recommendation: combination of (a) + (c).
3. **Pure-function unit tests on xtask core** are easiest to design (just fixture inputs + expected outputs). Adapter and end-to-end tests need fixture repositories.

### To DEVOPS (platform-architect)

**Inputs:**
- DESIGN artifacts (architecture, technology-stack, component-boundaries, data-models)
- ADR-013 (PAT credential model — DEVOPS owns the expiry-warning workflow per DISCUSS handoff)
- DISCUSS `outcome-kpis.md` "Handoff Notes for DEVOPS" section (already prepared)

**External integrations requiring contract-style verification (per DESIGN §7.3):**

```
External Integrations Requiring Contract-Style Verification:

- Homebrew formula DSL (consumed by `brew install` and `brew test-bot`):
  Already covered — `brew test-bot` runs in the tap-repo PR and is the canonical
  consumer-driven contract test. Auto-merge gated on its success (US-11.AC-3).

- SLSA attestation envelope (consumed by `gh attestation verify`):
  Already covered — `gh attestation verify` runs in CI per US-09.AC-4 and is
  documented for end users in README troubleshooting (US-09.AC-5).

- GitHub Releases asset URL stability (consumed by Homebrew formula `url` field):
  Verified by `brew test-bot install` step in the tap-repo PR. Any URL change
  fails the test-bot before auto-merge fires.

- GitHub Actions API (`gh`, `actions/*`): pinned by major version in workflow
  files. Annual review recommended; not a contract-test concern.

No third-party REST/GraphQL APIs require Pact-style contracts.
```

**DEVOPS work items (carried forward from DISCUSS):**
1. K-PIPE alerting: `workflow_run: completed` follow-up workflow opening a `release-pipeline-failure`-tagged issue on `release.yml` failure
2. README troubleshooting section: `gh attestation verify <archive> --owner jeffabailey` documentation
3. `GH_TAP_TOKEN` expiry monitoring: workflow that warns 30 days before expiry (out of scope this feature; tracked separately)
4. Release log table maintenance helper in `RELEASING.md` (data source for K-T2T, K-COVER)
5. Rate-limiting note for GitHub API (low advisory inherited from DISCUSS reviewer)

### To DELIVER (software-crafter, when DISTILL completes)

**Inputs:**
- All DESIGN artifacts (component contracts, interface shapes)
- DISTILL acceptance test designs

**Implementation notes:**
- xtask functional core MUST achieve ≥80% mutation kill rate (project policy per CLAUDE.md)
- xtask functional core uses thiserror; CLI dispatcher uses anyhow (project paradigm convention)
- `release.yml` and `bump-tap-formula.yml` (if extracted) must be ≤250 lines combined per US-14
- Composite actions (`.github/actions/<name>/action.yml`) are an acceptable extraction tool if line budget becomes binding

## Cross-Feature Coupling Notes

| Coupling | Direction | Notes |
|---|---|---|
| `modeltap-app` binary | This pipeline ships it | The release pipeline depends on `modeltap-app` building successfully. No reverse coupling. |
| `modeltap-tui` `--version` behavior | US-15 verifies it | Delegates to modeltap-tui US-01 (already shipped per CLAUDE.md "DELIVER wave"). |
| `ci.yml` toolchain pinning | C7 mirrors it | Drift between ci.yml and release.yml breaks K-PIPE; both files pin `dtolnay/rust-toolchain@stable`. |
| `Cargo.toml [workspace.package].version` | C1 single-source | Shared with `clap` `CARGO_PKG_VERSION` for `modeltap --version`; shared with this pipeline's tag-validation. |

No conflicting constraints with `modeltap-tui`. No vocabulary conflicts (this feature introduces "tag", "release", "tap", "tap-bump", "atomic publish", "CI parity gates", "SLSA attestation"; modeltap-tui introduces "tool", "model", "indicator", "unify" — disjoint).

## Open Architecture Questions Carried Forward

Per architecture-design.md §11. Not blockers; deferred deliberately:

1. **OQ-1**: macOS notarization step shape (D3 deferred; build-job structure leaves slot)
2. **OQ-2**: homebrew-core formula naming convention (D8 deferred)
3. **OQ-3**: aarch64-linux native runner availability (revisit when GitHub `ubuntu-22.04-arm` GA)
4. **OQ-4**: bytewise reproducible Rust builds (open ecosystem problem; not pursued v1)
5. **OQ-5**: tap-bump conflict if maintainer hand-edits the tap (process discipline; documented in RELEASING.md)

## Peer Review (Independent — `nw-solution-architect-reviewer` "Atlas")

**Verdict: APPROVED → hand off to DISTILL + DEVOPS.** Confirmed 2026-05-03, iteration 1.

Atlas independently validated all five hard-gate dimensions and confirmed the producer's self-review issue counts:

- **Architectural bias detection**: PASS (no technology preference, no resume-driven, no latest-tech bias)
- **ADR quality**: PASS (5 ADRs, all with context + ≥2 alternatives + consequences + quality-attribute impact)
- **Completeness**: PASS (all 5 ranked quality attributes have concrete strategies; constraints C1-C8 operationalized)
- **Implementation feasibility**: PASS (team capability, zero new infra costs, testability adequate)
- **Priority validation**: PASS (riskiest assumption tackled first; integrity ranked above speed; data-justified targets)

Issue counts confirmed: **0 critical, 0 high, 1 medium, 2 low**. The medium (cross-repo end-to-end test infrastructure) is a known DISTILL-wave challenge, not a DESIGN gap. The two lows (optional threat-model section, Tera-vs-Minijinja subjective margin) are documentation enhancements, not blockers.

**Critical gate closure verified:** atomic publish (C2) is provable as a workflow-graph property; version integrity (C1) has a single source enforced by `validate-tag`; CI parity (C3) is preserved by step ordering before `cargo build`.
