# Wave Decisions Summary — release-process-homebrew-github (DEVOPS)

DEVOPS wave (wave 4 of 6) decisions, defaults applied via auto-mode, and handoff state.

## Wave Configuration

| Setting | Value | Rationale |
|---|---|---|
| Format | platform-architecture + ci-cd-pipeline + observability + monitoring/alerting + infra-integration + branching + kpi-instrumentation + environments.yaml + this file | Per nw-platform-architect default; release-pipeline shape forces deviation from "deployed-service" defaults (D1, D2, D6, D7) |
| Architecture style | GitHub-Actions-as-platform; no servers, no orchestration, distribution via GH Releases + Homebrew tap | Inherited from DESIGN ADR-010 (single-workflow DAG) and DISCUSS D1 (Homebrew + GH Releases) |
| Auto mode | active | User chose continuous, autonomous execution |
| Peer review | nw-platform-architect-reviewer | Hard gate per workflow |

## Personas (inherited)

- **Jeff Bailey** — single maintainer; primary persona for the entire pipeline
- **Devon Park** — end-user installer
- **Riley Chen** — open-source contributor reading workflow + runbook

No new personas in DEVOPS.

## Decisions Resolved

The 9 standard DEVOPS decisions, all answered via auto-mode defaults supplied by parent agent. No deviations from those defaults; rationale below confirms each is correct for this feature.

| ID | Decision | Choice | Rationale |
|---|---|---|---|
| D1 | Deployment target | **Distribution channels: GitHub Releases + Homebrew tap (`jeffabailey/homebrew-modeltap`)** | This feature does not deploy a service. It ships a binary artifact via two distribution channels. Standard cloud/on-prem options are inapplicable. |
| D2 | Container orchestration | **None** | `cross` uses Docker for build-time cross-compile only (ADR-012). No runtime container orchestration; the artifact is a stripped Rust binary. |
| D3 | CI/CD platform | **GitHub Actions** | Already in use (`.github/workflows/ci.yml`). DESIGN ADR-010 commits to single-workflow DAG. C7 mandates the same toolchain pinning conventions as `ci.yml`. |
| D4 | Existing infrastructure | **Existing CI/CD only** | `ci.yml` is the only existing infra. Release pipeline extends CI conventions (toolchain pin, action versions, rust-cache key shape). No managed-service migration. |
| D5 | Observability and logging | **GitHub-native (Actions logs, release-log table, gh CLI for queries)** | Per DISCUSS C5 (privacy by default) and `outcome-kpis.md` ("GitHub-native KPIs, no external telemetry"). Forbids Datadog / ELK / Sentry / etc. |
| D6 | Deployment strategy | **Recreate (each tag = new release; previous tags remain in GitHub Releases as historical record)** | The "deployment unit" is a tag/release, not a running service. Rolling/blue-green/canary do not apply to a binary download. Rollback = delete release + tap formula revert (see monitoring-alerting.md §4). |
| D7 | Continuous learning | **No** | Release pipeline is not a deployed app subject to A/B testing or feature flags. `continuous-learning.md` skipped per parent agent direction. |
| D8 | Git branching strategy | **Trunk-Based Development** | Recent commit history (`facddef`, `4c8428f`, `bc1695e`) shows direct commits to main; CI gates on every push. Matches single-maintainer reality. See branching-strategy.md. |
| D9 | Mutation testing strategy | **per-feature** | Already declared in `CLAUDE.md` ("Mutation Testing Strategy: per-feature — kill-rate gate of ≥80% must pass before finalize"). Applies to xtask functional core (`xtask::version`, `xtask::formula`, `xtask::changelog`, `xtask::workflow_lint`). No new CLAUDE.md section needed. |

No DEVOPS decision contradicts any DISCUSS decision (D1-D8) or DESIGN decision (DESIGN-01..08).

## Decisions Flagged for User Reconsideration

None. All 9 defaults align with feature shape and prior-wave decisions.

## Phase Execution Summary

| Phase | Activity | Status |
|---|---|---|
| Phase 1 | Requirements analysis (re-read DISCUSS outcome-kpis + DESIGN architecture/component-boundaries/wave-decisions/ADR-010/ADR-013) | COMPLETE |
| Phase 2 | Existing infrastructure analysis (`.github/workflows/ci.yml`, `Cargo.toml`) | COMPLETE |
| Phase 3 | Platform design (8 design artifacts) | COMPLETE |
| Phase 4 | Quality validation (DoD checklist below) | COMPLETE |
| Phase 5 | Peer review (nw-platform-architect-reviewer) | TRIGGERED at end of writeup |

## Artifacts Produced

All under `docs/feature/release-process-homebrew-github/devops/`:

| Artifact | Purpose |
|---|---|
| `platform-architecture.md` | The system as runtime infrastructure: GH Actions + tap repo + release artifacts; no servers, no orchestration. |
| `ci-cd-pipeline.md` | Workflow file structure, job DAG specification, secret/permission matrix, pinned action versions, cache keying. Includes K-PIPE alerting follow-up workflow. |
| `observability-design.md` | GitHub-native KPI collection: how K-T2T, K-PIPE, K-COVER, K-TOIL, K-PROV, K-CONTRIB are extracted from GH Actions API + release-log table. Queries / scripts / dashboard mockup. |
| `monitoring-alerting.md` | Workflow-failure alerting design: `workflow_run: completed` follow-up; `GH_TAP_TOKEN` expiry warning; rate-limiting advisory; how the maintainer is notified; rollback procedures for a published release. |
| `infrastructure-integration.md` | How the new `release.yml` coexists with `ci.yml`; shared cache keys; toolchain version sync; secret scoping; branch protection on tap repo. |
| `branching-strategy.md` | Trunk-based development: short-lived branches, PR with CI gates, tag-on-main triggers release, tap repo branch protection requiring `brew test-bot`. |
| `kpi-instrumentation.md` | Per-KPI: source = GH Actions API endpoint or RELEASING.md log table column; collection method = ad-hoc gh CLI query or scheduled workflow; review cadence. |
| `environments.yaml` | Per skill mandate. Schema: target_environments, coexistence_matrix, platform_coverage, deployment_assumptions. |
| `wave-decisions.md` | (this file) |

`continuous-learning.md` is INTENTIONALLY OMITTED per D7 = No.

## Story-to-Pipeline-Stage Traceability

| Story | DEVOPS-wave instrumentation |
|---|---|
| US-01 release-prep | Local quality gate (pre-tag) — covered by `kpi-instrumentation.md` K-TOIL |
| US-02 validate-tag | CI commit gate — covered by `ci-cd-pipeline.md` job DAG |
| US-03 CI parity gates | CI commit gate — covered by `infrastructure-integration.md` toolchain-sync section |
| US-04 single-target build | CI build gate — covered by `ci-cd-pipeline.md` matrix spec |
| US-05 publish-github-release | CI publish gate — covered by `ci-cd-pipeline.md` |
| US-06 bump-tap-formula | CI cross-repo gate — covered by `ci-cd-pipeline.md` + `monitoring-alerting.md` token-expiry |
| US-07 4-target matrix | CI matrix — covered by `environments.yaml` platform_coverage |
| US-08 atomic-publish guard | CI atomicity — covered by `ci-cd-pipeline.md` `needs:` DAG |
| US-09 SLSA attestation | CI provenance gate — K-PROV in `kpi-instrumentation.md` |
| US-10 4 platform blocks | CI render gate — covered by `ci-cd-pipeline.md` |
| US-11 auto-merge | CD gate (tap repo) — covered by `infrastructure-integration.md` branch protection |
| US-12 idempotent retry | CD recovery — covered by `monitoring-alerting.md` rollback section |
| US-13 RELEASING.md | Operational doc + KPI source — covered by `observability-design.md` |
| US-14 ≤250-line workflow | CI lint gate — covered by `ci-cd-pipeline.md` |
| US-15 end-user install | E2E verification — K-COVER in `kpi-instrumentation.md` |

## Definition of Done (DEVOPS wave)

- [x] Every DESIGN component has a runtime/operational story (platform-architecture.md)
- [x] CI/CD pipeline fully specified with job DAG, permissions, caches, action pins (ci-cd-pipeline.md)
- [x] All 6 outcome KPIs from DISCUSS have collection method, source, frequency, owner (kpi-instrumentation.md)
- [x] Workflow-failure alerting designed (monitoring-alerting.md §1) — closes DEVOPS handoff item #1
- [x] README troubleshooting copy drafted (monitoring-alerting.md §3) — closes DEVOPS handoff item #2
- [x] `GH_TAP_TOKEN` expiry monitoring designed (monitoring-alerting.md §2) — closes DEVOPS handoff item #3
- [x] `RELEASING.md` release-log helper designed (kpi-instrumentation.md §K-T2T) — closes DEVOPS handoff item #4
- [x] Workflow-failure routing decision documented (monitoring-alerting.md §1.3) — closes DEVOPS handoff item #5
- [x] Rate-limiting advisory documented (monitoring-alerting.md §5) — closes Eclipse-flagged item
- [x] `environments.yaml` produced per skill mandate
- [x] Coexistence with `ci.yml` documented (infrastructure-integration.md)
- [x] Branching strategy aligned to D8 (branching-strategy.md)
- [x] Local quality gates designed (ci-cd-pipeline.md §6 — pre-commit / pre-push mirroring CI commit stage)
- [x] Rollback rehearsal added to first-release checklist (infrastructure-integration.md §6 — Scenario A + Scenario B drills) — addresses Forge review Issue #4
- [x] Peer review (Forge / nw-platform-architect-reviewer) — APPROVED iteration 2 after Issue #4 fix; 0 blockers, 0 critical, 0 medium remaining, 4 low accepted for v1

## Risks Surfaced

Inherited from DESIGN risk register; DEVOPS adds:

| Risk | Probability | Impact | Owner | Mitigation |
|---|---|---|---|---|
| `GH_TAP_TOKEN` silent expiry breaks the next release | Medium (annual cycle) | High (release pipeline halts at bump-tap-formula) | Maintainer | `token-expiry-warning.yml` scheduled workflow (monitoring-alerting.md §2) |
| GH API rate-limit hit during retries | Low | Low (gh client retries handled by `gh` itself) | Maintainer | Documented as advisory (monitoring-alerting.md §5); not actively mitigated |
| K-PIPE alerting workflow itself fails silently | Low | Medium (failures unnoticed; K-PIPE measurement degraded) | Maintainer | Self-test on first deploy: deliberate `release.yml` failure to confirm issue creation (covered in monitoring-alerting.md §1.5) |
| `workflow_run` event delivery latency / drops | Low | Low (occasional missed alert; manual review catches via monthly K-PIPE audit) | Maintainer | Acceptable; release frequency is low (~1-4/month) |
| Maintainer ignores opened `release-pipeline-failure` issues | Low | Medium (stale K-PIPE signal degrades trust) | Maintainer | Process discipline; quarterly K-PIPE review per `kpi-instrumentation.md` |

## Wave Handoff Package

### To DISTILL (acceptance-designer / Quinn) — RUNNING IN PARALLEL

**Inputs:**
- All DEVOPS artifacts under `docs/feature/release-process-homebrew-github/devops/`
- Particular attention: `environments.yaml` enumerates 8 environments DISTILL must parametrize against

**Cross-wave coordination notes:**
- DISTILL is running in parallel; if DISTILL began before `environments.yaml` was visible, DISTILL must reconcile its environment assumptions against this file. The 8 environments listed are: `ci-release-runner`, `tap-repo-fresh`, `tap-repo-with-prior-formula`, `tap-repo-with-stale-bump-branch`, `end-user-macos-14-arm`, `end-user-macos-13-intel`, `end-user-ubuntu-22-x86_64`, `end-user-ubuntu-22-aarch64` (also covers WSL2).

### To DELIVER (software-crafter, when DISTILL completes)

**DEVOPS-implementation deliverables:**
1. `.github/workflows/release.yml` — implements `ci-cd-pipeline.md` job DAG
2. `.github/workflows/release-pipeline-alert.yml` — implements `monitoring-alerting.md` §1 (K-PIPE alerting follow-up)
3. `.github/workflows/token-expiry-warning.yml` — implements `monitoring-alerting.md` §2 (`GH_TAP_TOKEN` expiry warning)
4. `RELEASING.md` release-log helper section — implements `kpi-instrumentation.md` K-T2T
5. README troubleshooting section additions — implements `monitoring-alerting.md` §3

**Local quality gate setup** (per `ci-cd-pipeline.md` §6):
- Add `lefthook.yml` or `.git/hooks/` setup script that mirrors CI commit-stage checks
- Document in `RELEASING.md` "First-time setup" section

## Cross-Feature Coupling Notes

| Coupling | Direction | Notes |
|---|---|---|
| `ci.yml` toolchain pin (`dtolnay/rust-toolchain@stable`) | `release.yml` mirrors | C7 enforced by `infrastructure-integration.md`; drift will break K-PIPE |
| `Swatinem/rust-cache@v2` keys | `ci.yml` ↔ `release.yml` share where possible | `infrastructure-integration.md` documents intentional overlap |
| `cargo-deny` ALL | `release.yml` runs deny check before publish | Per DISCUSS C3 (CI parity) — release MUST gate on `cargo deny check` because new workspace deps (`tera`, `toml_edit`, `cargo_metadata`, `regex`) introduce license-audit surface |
| `modeltap-tui` `--version` flag | release pipeline depends on it | Consumed by `brew test-bot` test stanza per US-15 |
| `GH_TAP_TOKEN` lifecycle | maintained per ADR-013 | DEVOPS adds expiry-warning workflow (this wave's deliverable) |

## Peer Review

To be executed after artifacts written. Hard gate per workflow. APPROVED required before DELIVER hand-off.
