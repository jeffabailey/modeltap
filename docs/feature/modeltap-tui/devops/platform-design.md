# Platform Design — modeltap-tui

**Wave:** DEVOPS (4 of 6)
**Author:** Apex (nw-platform-architect)
**Date:** 2026-04-28
**Authoritative inputs:** intake-brief.md, requirements.md, outcome-kpis.md, architecture-design.md, ADR-001..009.

## 1. Scope Statement (what DEVOPS owns vs does not)

modeltap is a **single-binary, local-only desktop CLI**. There are no services to deploy, no infra to provision, no runtime observability stack to operate. DEVOPS scope is therefore narrower than a typical web/microservices platform.

### In scope (this wave)

1. CI/CD pipeline (GitHub Actions, macOS + Linux matrix)
2. Release packaging and distribution (cargo-dist + Homebrew + crates.io)
3. KPI instrumentation (local JSONL log; opt-in upload deferred)
4. Quality gates: format, clippy, test, license/security, K3 latency benchmark, architecture rule R1
5. Branch protection and contributor onboarding
6. Production-readiness checklist for v1.0.0 sign-off
7. Documentation deliverables: RELEASE.md, CONTRIBUTING.md, SECURITY.md, INSTALL.md

### Explicitly NOT in scope (justified non-decisions)

| Item | Why not applicable |
|---|---|
| Kubernetes / container orchestration | Not applicable: ships as a tarball binary |
| Terraform / IaC for cloud resources | Not applicable: no cloud resources in v1 |
| Prometheus / Grafana / Loki | Not applicable: local CLI, not a service |
| Distributed tracing | Not applicable: single-process, single-machine |
| SLO/SLI dashboards, error-budget burn alerts | Not applicable: open-source desktop tool, no SRE on-call |
| SSO / OIDC / secrets manager | Not applicable: no auth, no network in v1 |
| Multi-environment promotion (dev → staging → prod) | Not applicable: "production" is a tagged GitHub release |
| Canary / blue-green / rolling deployment | Not applicable: user installs binary at their own cadence; release IS the rollout |
| On-call rotation, PagerDuty | Not applicable: GitHub issues are the support channel |
| Network DAST, API security testing | Not applicable: no network surface in v1 |
| Runtime admission controllers | Not applicable: no runtime to admit |

The DEVOPS-wave skill files steer toward many of the above. They are documented here as intentional non-decisions per `nw-cicd-and-deployment` "Simplest Solution Check" guidance.

## 2. Quality Attribute Re-priorities for DEVOPS

Inheriting from architecture-design §2 (safety > maintainability > responsiveness > testability > portability), DEVOPS adds:

| Rank | Attribute | Driver |
|---|---|---|
| 1 | **Privacy** | C5 — no telemetry by default, ever. Local-AI users are privacy-sensitive by selection. Telemetry design exists for forward-compat, not for v1 shipment. |
| 2 | **Contributor friction** | K4 — at least one community plugin within 6 months. Riley persona (US-18, US-20). Contributing must be obvious: clone, run `make ci-local` (or equivalent), green. |
| 3 | **Cross-platform parity** | C2 — macOS + Linux must pass the same CI; release artifacts cover both. WSL is Linux. |
| 4 | **Reproducibility** | Releases must be reproducible from a tag. cargo-dist + lock file + pinned action versions. |
| 5 | **Latency observability (K3)** | First-paint < 1 s is a release gate. CI benchmark catches regressions before merge. |

## 3. Pipeline Design Summary

See `ci-pipeline.md` for the full GitHub Actions design. Summary:

| Stage | What runs | Gate |
|---|---|---|
| Local pre-commit | `cargo fmt --check`, `cargo clippy -D warnings` (workspace) | Blocking (developer) |
| Local pre-push | `cargo test --workspace --lib` (fast subset) | Blocking (developer) |
| PR — CI commit stage | fmt, clippy, build, unit tests, integration tests, plugin contract tests, architecture rule R1, cargo-deny, K3 benchmark | Blocking (merge) |
| PR — matrix | macOS-latest + ubuntu-latest, both must be green | Blocking (merge) |
| Tag push (`v*`) — release stage | cargo-dist build for 4 targets, sign-not-required, generate checksums, publish to GitHub Releases, update Homebrew tap, optionally `cargo publish` | Blocking (release) |
| Post-release — smoke | Manual verification: `brew install`, `cargo install`, run binary, verify version | Advisory |

**Quality gate categories** (per skill taxonomy):

- Local: formatting, linting, unit tests (mirror commit stage)
- PR: status checks (matrix CI), CODEOWNERS approval (US-18 plugin trait stability)
- CI: build, all tests, security/license scan, K3 benchmark, architecture rule
- Release: tag-driven; cargo-dist generates artifacts; checksums attached
- Production: post-release smoke is advisory only — no automated rollback (users decide when to upgrade)

Branching strategy: **GitHub Flow** (single main branch, feature branches via PR). Fits a small OSS project with a single binary release cadence. Trunk-based was considered but PR-mediated review is appropriate given the plugin-trait stability commitment (US-18 SemVer).

## 4. Release Strategy Summary

See `release-strategy.md`. Summary:

- **Tool:** `cargo-dist` (axodotdev/cargo-dist) — Rust-native, generates GitHub Actions workflow, builds for all 4 targets, attaches tarballs + checksums to GitHub Releases, publishes Homebrew formula automatically.
- **Targets:** `x86_64-apple-darwin`, `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. WSL users install Linux x86_64. Native Windows is not targeted (per C2).
- **Channels:**
  - GitHub Releases (canonical) — tarballs, SHA256SUMS, source tarball
  - Homebrew tap (`<org>/homebrew-modeltap`) — auto-published by cargo-dist
  - crates.io (`cargo install modeltap`) — published in release workflow
  - Deferred: AUR, Nix, .deb, .rpm — community-driven post-v1
- **Versioning:** SemVer. Plugin trait is the SemVer contract — any breaking change to `trait Tool` in `modeltap-core` = MAJOR bump. New trait method with default impl = MINOR. Bug fixes = PATCH. Documented in RELEASE.md.
- **Code signing:** deferred to v1.x. Documented as known limitation in INSTALL.md (macOS users may see Gatekeeper warning; instructions provided).
- **Release cadence:** as-needed (not time-boxed). v1.0.0 is the first tagged release; pre-1.0 lives on `main` with no release tags.

## 5. KPI Instrumentation Summary

See `kpi-instrumentation.md`. Summary:

| KPI | Mechanism | Default state |
|---|---|---|
| K1 (bytes reclaimed) | JSONL line per zap/unify in `~/.modeltap/launch.log` | Local-only, no opt-in needed (it's a local file) |
| K2 (dedupable %) | JSONL line per launch with inventory composition | Local-only |
| K3 (first paint latency) | JSONL line per launch with timing breakpoints; CI benchmark asserts < 1 s headless on fixture | Local-only + CI gate |
| K4 (community plugins) | GitHub PR count (manual) | N/A |
| K5 (accidental loss) | GitHub issues tagged `accidental-loss` | N/A |

**Privacy rule (C5, NFR Privacy):** the local log NEVER leaves the machine. Even when the user runs `modeltap telemetry enable` (deferred to v1.x), only aggregated counts and timings would upload — never paths, never model names, never PII. The opt-in upload is designed in `telemetry-design.md` for forward-compat but is not implemented in v1.

**Log rotation (per ADR-003 stateless model):** size-based, 10 MB cap per file, keep 3 rotations. JSONL stream is append-only; rotation triggered on launch when current file exceeds cap. Implemented via `tracing-appender::rolling` or equivalent. No daily rotation (sessions are bursty, not continuous).

**JSONL schema, log path, rotation policy** — all specified in `kpi-instrumentation.md`.

## 6. Local Quality Gates (pre-commit / pre-push)

Mirroring CI commit stage, fast feedback (< 30 s for pre-commit, < 5 min for pre-push). Tool: `lefthook` (polyglot, fast, parallel; no Python dep).

Pre-commit:
```yaml
pre-commit:
  parallel: true
  commands:
    fmt:
      run: cargo fmt --all -- --check
    clippy:
      run: cargo clippy --workspace --all-targets -- -D warnings
    secrets:
      run: gitleaks protect --staged --redact
```

Pre-push:
```yaml
pre-push:
  commands:
    test:
      run: cargo test --workspace --lib
    deny:
      run: cargo deny check
```

`--no-verify` allowed for emergencies. CI is the authoritative gate.

## 7. Branch Protection (main)

- Require PR with at least 1 review approval
- Require status checks to pass: `ci / macos`, `ci / linux`, `ci / arch-rule`, `ci / cargo-deny`, `ci / k3-benchmark`
- Require branches to be up to date before merging
- Require linear history (squash-merge default)
- Restrict force pushes and deletions
- CODEOWNERS:
  - `crates/modeltap-core/**` → maintainer (the trait is the SemVer contract)
  - `plugins/**` → maintainer + plugin-area contributors (when they emerge)
  - `.github/workflows/**` → maintainer
  - `docs/adrs/**` → maintainer

## 8. Stateless = No Operational State

Per C7 + ADR-003, modeltap writes only:

| Path | Purpose | Lifecycle |
|---|---|---|
| `~/.modeltap/launch.log` | KPI JSONL stream (K1, K2, K3) | Rotating, 10 MB cap, keep 3 |
| `~/.modeltap/diagnostics.log` | Errors, panics, warnings (tracing output) | Rotating, 10 MB cap, keep 3 |
| `~/.modeltap/config.toml` (optional, user-owned) | Extra search paths, preferences | User-managed; modeltap reads only |

No DB. No migrations. No `~/.modeltap/index.json`. No central store. No SQLite. The "cleanup tooling" needed is just log rotation.

If the user deletes `~/.modeltap/` entirely, modeltap recreates the log files on next launch and behaves identically. Removing config.toml restores defaults.

## 9. Production-Readiness Checklist (v1.0.0)

See `production-readiness-checklist.md` for the full sign-off list. Summary count: **15 items**, broken down as:

- Build / CI health (3 items)
- Acceptance criteria (2 items, including HF + LM Studio spike completion)
- Release tooling (3 items)
- Distribution (2 items)
- Documentation (1 item, covering 4 docs)
- License + security (2 items)
- Manual end-to-end validation (2 items)

DELIVER wave owns checking off these items. Apex provides the checklist; DELIVER provides the evidence.

## 10. Stakeholder Demonstration Plan

For an OSS desktop CLI, "stakeholder demonstration" is:

1. **Internal (to the user / project owner):** record an asciinema cast of: launch on macOS → see Ollama models → zap one → unify one across two tools. Attach to the v1.0.0 GitHub Release notes.
2. **External (community):** README has a 30-second animated GIF showing the same flow. Reduces time-to-understanding for new contributors (Riley persona).

No formal demo presentation needed. Audience is technical (terminal users, OSS contributors). Success criteria: README + asciinema + working `brew install` makes the value obvious in < 5 minutes.

## 11. Outcome Measurement Plan

Without runtime telemetry, outcome measurement is:

| KPI | First-month plan | Quarterly plan |
|---|---|---|
| K1 | First-100-users survey: "how much disk did you reclaim in your first session?" | Re-survey at 90 days |
| K2 | Same survey: "what fraction of your models showed `*` or `o`?" | Re-survey |
| K3 | CI benchmark trend (regression alarms in CI) | CI benchmark median over time |
| K4 | GitHub PR review under `plugins/` directory | Quarterly count |
| K5 | GitHub issue search for `accidental-loss` label | Quarterly count |

Aggregate opt-in telemetry uploads are designed but not shipped in v1 (see `telemetry-design.md`).

## 12. ADR Index (DEVOPS additions)

| ADR | Title | Status |
|---|---|---|
| ADR-010 | CI platform — GitHub Actions | Accepted |
| ADR-011 | Release tooling — cargo-dist | Accepted |
| ADR-012 | Telemetry approach — opt-in, local-by-default | Accepted |

## 13. Open Items Handed to DELIVER

1. HF + LM Studio linking spikes (per peer-review.md M-1) — must complete in build week 1 of US-10. DEVOPS provides the CI infrastructure to verify spike outputs (the plugin contract test running against fixture directories).
2. DISCUSS back-edits BE-1..BE-12 — non-blocking for DEVOPS, should be applied during DELIVER story refinement.
3. `modeltap stats` subcommand decision — recommended in v1 (one screenful of summary stats from launch.log). If DELIVER finds it bloats walking skeleton, defer to v1.x.
4. License choice — **MIT recommended** for max reuse and ecosystem compatibility (HF tooling, llama.cpp are both MIT/Apache). Apache-2.0 dual-license is also acceptable. The `cargo-deny` policy in CI assumes MIT/Apache-2.0/BSD allowed; GPL forbidden. Final decision can be made by maintainer at the time of `cargo init`; default to MIT in `LICENSE` and `Cargo.toml`.

## 14. Definition of Done (DEVOPS wave)

- [x] CI pipeline designed for both macOS and Linux runners
- [x] All quality gates classified (local / PR / CI / release / production-advisory)
- [x] K3 latency benchmark designed and gated in CI
- [x] Architecture rule R1 codified as a CI test
- [x] Release tooling chosen (cargo-dist) and distribution channels enumerated
- [x] KPI instrumentation: JSONL schema, log path, rotation policy specified
- [x] Telemetry: local-by-default honored; opt-in upload designed for forward-compat (not shipped v1)
- [x] Branch protection and CODEOWNERS specified
- [x] Production-readiness checklist authored
- [x] Documentation deliverables enumerated (RELEASE.md, CONTRIBUTING.md, SECURITY.md, INSTALL.md to be authored in DELIVER)
- [x] ADR-010, ADR-011, ADR-012 authored
- [x] Non-applicable items documented as such (no over-engineering)
- [ ] Peer review: scheduled by parent agent; Apex does not invoke for this scope-narrow run
