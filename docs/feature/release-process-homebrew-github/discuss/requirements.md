# Requirements — release-process-homebrew-github

## Feature Identity

- **Feature ID:** `release-process-homebrew-github`
- **Wave:** DISCUSS (wave 2 of 6)
- **Source:** `/nw:new` wizard (auto mode, lightweight research depth, JTBD skipped)
- **Reference:** Companion to the `modeltap-tui` feature, which delivers the binary this feature publishes.

## Domain Glossary

| Term | Definition |
|---|---|
| **release** | A versioned, published set of binaries available for end-user install. Identified by a `v0.x.0` tag. Matches the `gh release` vocabulary. |
| **tag** | An annotated git tag matching the pattern `v*.*.*`. Pushing a tag is the only manual step that triggers the release pipeline. |
| **archive** | A `tar.gz` file containing one stripped release binary, named `modeltap-${version}-${target}.tar.gz`. One archive per supported target. |
| **target** | A Rust target triple identifying a platform/arch combination. v1 supports four: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. |
| **workspace version** | The `[workspace.package].version` field in the root `Cargo.toml`. The single source of truth for `${version}` across the entire pipeline. |
| **the tap** / **tap repo** | The separate GitHub repository hosting the Homebrew formula. v1: `jeffabailey/homebrew-modeltap` (decision D1). |
| **tap-bump** | The act of updating `Formula/modeltap.rb` in the tap repo to reference a new release's URLs and sha256s. Done by an automated PR opened by `release.yml`. |
| **CI parity gates** | The set `{cargo fmt --all -- --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace --locked}` mirroring `.github/workflows/ci.yml`. The release workflow MUST run these before any artifact is built. |
| **SLSA build provenance** | A signed attestation produced by `actions/attest-build-provenance@v2` proving an archive was built by a specific GitHub Actions workflow run. |
| **brew test-bot** | The Homebrew CI action that runs `audit`, `install`, and `test` stanzas on a formula PR. Auto-merge is conditional on its success. |
| **tap-bump token** | A GitHub fine-grained PAT with `Contents: Read+Write` and `Pull Requests: Read+Write` on the tap repo, stored as the modeltap repo's `GH_TAP_TOKEN` Actions secret. |
| **release-prep** | A `cargo xtask release-prep` subcommand that bumps the workspace version, regenerates the changelog, and runs CI gates locally before the maintainer opens a prep PR. |
| **walking skeleton** | The thinnest end-to-end pipeline slice: one target, one archive, one platform, no auto-merge — proving the cross-repo flow works at all. |

## Stakeholders

| Stakeholder | Role | Engagement |
|---|---|---|
| Jeff Bailey (maintainer, primary persona) | Cuts releases, owns the tap repo, fields install issues | Validates K-T2T, K-PIPE, K-TOIL |
| Devon Park (end-user installer, secondary persona; reused from `modeltap-tui`) | Installs modeltap on macOS or Linux via Homebrew | Validates K-COVER, K-PROV |
| Riley Chen (open-source contributor; reused from `modeltap-tui`) | Reads workflow + runbook to understand the release process | Validates K-CONTRIB |
| GitHub Actions runners (operational dependency) | Execute the workflow | Their availability is a hard dependency; outages defer releases |
| Homebrew project (out-of-band stakeholder) | Provides `brew`, `brew test-bot`, formula DSL conventions | We do not depend on a specific maintainer relationship; we follow the formula DSL conventions |

## Functional Requirements (Story Map Summary)

The complete user stories are in `user-stories.md`. The story map is in `story-map.md`. Summary:

### Walking Skeleton (Release 0) — 7 stories

US-01 (`release-prep` tool), US-02 (validate-tag job), US-03 (CI parity gates in release.yml), US-04 (single-target build + sha256), US-05 (publish-github-release), US-06 (bump-tap-formula opens PR), US-15 (end-user installs and verifies).

### Release 1: Multi-arch real release — 4 stories

US-07 (4-target build matrix), US-08 (atomic-publish guard), US-09 (SLSA build provenance), US-10 (formula renders 4 platform blocks).

### Release 2: Hands-off automation — 4 stories

US-11 (auto-merge tap-bump PR), US-12 (idempotent retry of bump-tap-formula), US-13 (`RELEASING.md` runbook), US-14 (workflow file ≤250 lines, every job commented).

## Non-Functional Requirements

### Performance / Latency

| NFR | Target | Verification |
|---|---|---|
| Tag-to-release latency (median) | ≤ 12 minutes from tag push to GitHub Release publication | `gh run view` timestamps; UAT US-15 SLA scenario |
| Tag-to-tap latency (median) | ≤ 14 minutes from tag push to tap-bump PR merged | Tap repo PR merge timestamp; UAT end-to-end SLA |
| Tag-to-install latency (median) | ≤ 15 minutes from tag push to clean machine `brew install` succeeding | Manual reference-machine verification; UAT end-to-end SLA |
| Single-target build duration | ≤ 5 minutes per target on the standard GitHub-hosted runner | `gh run view` step durations |
| Build matrix parallelism | All 4 targets run concurrently (no serialization) | release.yml matrix configuration |

### Reliability

| NFR | Target | Verification |
|---|---|---|
| Pipeline success rate | ≥ 95% over rolling last 10 releases | `gh run view` history; KPI K-PIPE |
| Atomic publish guarantee | If ANY build matrix cell fails, NO GitHub Release is created and NO tap PR is opened | UAT US-08 |
| Idempotent retry | Re-running `bump-tap-formula` updates the existing PR rather than opening a duplicate | UAT US-12 |
| Tap-bump auth resilience | Token expiry surfaces as a clear job failure, not a silent drop | UAT US-12 + DEVOPS-side monitoring |

### Cross-Platform Coverage

| NFR | Target | Verification |
|---|---|---|
| macOS Apple Silicon (`aarch64-apple-darwin`) | Build + install + run on macOS Sonoma 14.x or later | UAT US-15 + brew test-bot on macos-14 |
| macOS Intel (`x86_64-apple-darwin`) | Build + install + run on macOS Ventura 13.x or later | UAT US-15 + brew test-bot on macos-13 |
| Linux x86_64 (`x86_64-unknown-linux-gnu`) | Build + install + run on Ubuntu 22.04 or later, glibc 2.35+ | UAT US-15 + brew test-bot on ubuntu-22.04 |
| Linux aarch64 (`aarch64-unknown-linux-gnu`) | Build + install + run on Ubuntu 22.04 aarch64 (e.g., RPi 5, AWS Graviton) | UAT US-15; brew test-bot via QEMU on ubuntu-22.04 |
| Native Windows | NOT supported in this pipeline. WSL users install the Linux binary. | Documented in README; matches modeltap-tui C2 |

### Security / Supply Chain

| NFR | Target | Verification |
|---|---|---|
| Build provenance | Every archive carries a SLSA L3 attestation via `actions/attest-build-provenance@v2` | `gh attestation verify <archive>` succeeds; UAT US-09 |
| Sha256 integrity | Every archive's sha256 is computed in the build job and consumed verbatim by the formula bump (no rehashing) | UAT US-10 + Code review |
| Tap-bump credential storage | `GH_TAP_TOKEN` stored only in repo Actions secrets; never logged; scoped to the tap repo only | Code review; secret scope audit |
| CI parity in release.yml | Release workflow runs the SAME `cargo fmt`/`clippy`/`test` invocations as `ci.yml`, on the same `dtolnay/rust-toolchain@stable` toolchain | UAT US-03 + workflow file diff vs ci.yml |
| Code signing (macOS) | NOT in scope for v1. Documented `xattr -dr com.apple.quarantine` workaround. Tracked as a future feature. | README troubleshooting section |

### Observability

| NFR | Target | Verification |
|---|---|---|
| Workflow-step naming | Every step in release.yml has a descriptive `name:` (no anonymous run blocks) | Code review |
| Job naming | Every job in release.yml has a one-sentence purpose comment above its definition | Code review; UAT US-14 |
| Failure surface | A workflow failure is visible in `gh run watch` AND surfaces a `release-pipeline-failure`-labeled GitHub issue (DEVOPS to wire follow-up workflow) | UAT US-08 + DEVOPS handoff |
| Release log | `RELEASING.md` contains a release log table updated per release with version, timestamps, and verification status | UAT US-13 + KPI handoff |

### Maintainability / Contributor Experience

| NFR | Target | Verification |
|---|---|---|
| Workflow file size | `release.yml` ≤ 250 lines including comments and blank lines | UAT US-14 |
| Runbook size | `RELEASING.md` ≤ 10 numbered steps, ≤ 1 page printed | UAT US-13 |
| Single-source version | The string `${version}` appears in `Cargo.toml` ONCE (workspace) and is read everywhere else from there or from the tag | UAT US-02 + Code review |
| Conventional commits | Repo MUST use conventional commit prefixes (`feat:`, `fix:`, `chore:`, `refactor:`, `docs:`) for `git-cliff` to generate changelog correctly | Already in use per recent history; documented in CONTRIBUTING.md |

### Privacy

| NFR | Target | Verification |
|---|---|---|
| No telemetry collected | Pipeline does not collect or transmit any data outside GitHub-native logging | Code review |
| End-user privacy | `brew install` does not phone home to modeltap; only Homebrew's standard analytics (which the user controls via `HOMEBREW_NO_ANALYTICS`) | N/A — this is a Homebrew property, not modeltap's |

## Architectural Constraints (hard, for DESIGN)

These are constraints on the design space — DESIGN must respect them.

### C1 — Single source of version truth: `Cargo.toml [workspace.package].version`

The workspace version is the only place `${version}` is authored. Tag, archive name, formula version field, and `modeltap --version` output all derive from it. The validate-tag job enforces the tag-vs-Cargo.toml relationship as the primary integrity check.

### C2 — Atomic releases (all-or-nothing)

A release is fully published or not at all. The `publish-github-release` and `bump-tap-formula` jobs MUST NOT run if any build matrix cell failed. No partial uploads, no half-bumped formulas. This is enforced by GitHub Actions `needs:` DAG.

### C3 — CI parity in release.yml

`release.yml` MUST run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --locked` on stable Rust BEFORE any `cargo build --release`. A release that skipped the CI gates is not a release. (See `CLAUDE.md` "CI Lint Discipline" section.)

### C4 — Cross-repo via thin tap

The tap repo (`jeffabailey/homebrew-modeltap`) contains only `Formula/modeltap.rb` (auto-rewritten), `README.md` (hand-maintained, points back to main repo), and `.github/workflows/test-bot.yml` (runs `brew test-bot`). No source code, no docs. The tap is a manifest, not a project.

### C5 — Walking skeleton in 2-3 days

Release 0 (US-01..US-06 + US-15) MUST be completable as the first deliverable. This forces:
- The cross-repo authentication to be solved on day one.
- The single-target build path to work end-to-end before multi-arch is tackled.
- The end-user install verification (US-15) to exist as the success criterion.

### C6 — No native Windows support in the pipeline

Per `CLAUDE.md` C2 and `modeltap-tui` C2, modeltap is WSL-only on Windows. The release pipeline reflects this: NO `windows-latest` runner, NO `*.msi` artifact, NO `.exe` archive. WSL users install the Linux x86_64 binary. The README documents this clearly.

### C7 — Same Rust toolchain pinning convention as ci.yml

`release.yml` uses `dtolnay/rust-toolchain@stable` matching `ci.yml`. If/when ci.yml pins a specific MSRV or toolchain version, release.yml MUST pin the same. Drift between the two breaks K-PIPE.

### C8 — No silent skips

If `actions/attest-build-provenance@v2` fails, the build job fails. If `brew test-bot` fails, auto-merge withholds. If `validate-tag` fails, the workflow aborts. Silent skips are forbidden — every guard must visibly fail rather than emit a warning.

## Open Questions — Disposition

| ID | Question | Resolution in DISCUSS | Action for DESIGN |
|---|---|---|---|
| D1 | Tap repo location | **Default: `jeffabailey/homebrew-modeltap` (personal namespace).** Revisit if a `modeltap` GitHub org is created later (treated as one-way decision; document migration path in `RELEASING.md`). | Use this name in all formulas, README references, and workflow secrets. |
| D2 | Release-cut trigger | **Default: manual `git tag` push.** Defer `release-please`, `cargo-release`, or other PR-driven cut tooling to a future feature. | Implement `release.yml` with `on: push: tags: ['v*.*.*']` trigger. |
| D3 | macOS code signing/notarization | **DEFERRED to a future feature.** v1 ships unsigned. README documents `xattr -dr com.apple.quarantine $(which modeltap)` as the user workaround. | DESIGN must NOT add notarization steps; structure the build job so a future notarize step can be inserted before the tar.gz step without restructuring. |
| D4 | Binaries shipped | **Just `modeltap` (the `modeltap-app` binary).** `modeltap-cli` is listed in `CLAUDE.md` as future and does not yet exist. | Build `cargo build --release --package modeltap-app --locked`. When `modeltap-cli` ships, add a second build step in the same job. |
| D5 | Changelog generation | **`git-cliff` driven by conventional commits.** Recent commit history (`fix(ci):`, `chore(docs):`, `refactor(gpt4all):`) already follows the convention. | Add `git-cliff.toml` with sections for `feat`, `fix`, `chore`, `refactor`, `docs`, `perf`. `cargo xtask release-prep` invokes git-cliff. |
| D6 | SLSA build provenance | **Required.** `actions/attest-build-provenance@v2` per archive. ~30s overhead per build, no maintainer toil. | Add the action to the build job after the tar.gz step. |
| D7 | Tap-bump credential mechanism | **Default: fine-grained PAT** (`GH_TAP_TOKEN` Actions secret) scoped to the tap repo with `Contents: RW` + `PRs: RW`. Migrate to a GitHub App if multiple maintainers join. | Document token rotation in `RELEASING.md`. DEVOPS to add a workflow that warns 30 days before expiry (out of scope for this feature). |
| D8 | Submission to homebrew-core | **NOT in v1.** Revisit after 6 months / 100+ stars. The custom tap is the v1 distribution channel. | DESIGN to NOT design around homebrew-core acceptance criteria (e.g., versioned formula naming) yet. |

All decisions are defaults proposed in the journey artifact's "Open Decisions" section. The maintainer will be asked to confirm via a single batched AskUserQuestion (or implicitly accept by silence in auto mode) before DESIGN begins.

## Risks (surfaced; managed downstream)

| Risk | Category | Probability | Impact | Mitigation |
|---|---|---|---|---|
| Cross-repo PAT (`GH_TAP_TOKEN`) expires silently between releases | Technical | Medium | High | Document rotation in `RELEASING.md`; DEVOPS to add expiry-warning workflow (follow-up) |
| GitHub Actions outage on a release day | Project | Low | Medium | Defer release; nothing to do — outages clear |
| Cross-compile of `aarch64-unknown-linux-gnu` breaks due to dependency adding C-FFI without aarch64 support | Technical | Medium | Medium | Test cross-compile in CI; pin runner image versions |
| Homebrew DSL changes break the formula template | Technical | Low | Medium | `brew test-bot audit` catches DSL violations; pin `brew test-bot` action version |
| Maintainer pushes a tag with mismatched `Cargo.toml` | Project | Medium | Low (caught fast) | C1 + validate-tag job; documented in `RELEASING.md` |
| Half-published release confuses users | Project | Low | High | C2 atomic-publish guard; UAT US-08 |
| macOS Gatekeeper blocks unsigned binary on first launch | Project | High (every install) | Low (workaround documented) | README troubleshooting; track notarization for future feature |
| End user installs during the 1-2 minute tap-update window | Project | Low (rare) | Low (informative failure) | UAT US-15 covers this; brew tells the user to retry |
| Tap repo accidentally accumulates non-formula files | Process | Medium (drift over time) | Low | Documented in C4; periodic audit |
| Release workflow grows unwieldy | Process | Medium (over time) | Medium | C5 walking-skeleton constraint + US-14 size limit |

## Wave Handoff Package

### To DESIGN (solution-architect)

**Inputs:**
- Journey artifacts: `journey-tag-to-brew-install-visual.md`, `journey-tag-to-brew-install.yaml`, `journey-tag-to-brew-install.feature`
- Story map and prioritization: `story-map.md`, `prioritization.md`
- Requirements: this file, `user-stories.md`, `acceptance-criteria.md`
- Outcome KPIs: `outcome-kpis.md`
- Shared artifacts: `shared-artifacts-registry.md`
- DoR validation: `dor-checklist.md`
- Decisions summary: `wave-decisions.md`

**Walking skeleton scope to design first:**
US-01, US-02, US-03, US-04, US-05, US-06, US-15. Approximately 2-3 days of build effort. Successful walking skeleton means: pushing `git push origin v0.0.1-rc1` triggers `release.yml`, builds `modeltap` for `x86_64-unknown-linux-gnu`, publishes a GitHub Release with the archive + sha256, opens a PR against the tap repo with a one-platform formula, and a clean Linux x86_64 box can `brew install jeffabailey/modeltap/modeltap` and `modeltap --version` prints `0.0.1-rc1`. Auto-merge, multi-arch, SLSA, and atomic-publish guard are NOT in WS — they land in Release 1.

**Hard constraints (must not be designed away):**
1. **C1** — single source of `${version}` truth = `Cargo.toml`. Tag, archive, formula, binary `--version` all derive from it.
2. **C2** — atomic publish (no half-released). `publish-github-release` and `bump-tap-formula` gated by `needs:` DAG on the build matrix.
3. **C3** — release.yml runs CI parity gates (fmt/clippy/test) before any release build.
4. **C4** — tap repo is a thin manifest (formula + README + test-bot workflow only).
5. **C5** — walking skeleton in 2-3 days.
6. **C6** — no native Windows; WSL uses the Linux binary.
7. **C7** — same toolchain pinning as ci.yml (`dtolnay/rust-toolchain@stable`).
8. **C8** — no silent skips (every guard fails visibly).

**Open decisions — all RESOLVED post-DESIGN review:**
D1 (tap-name), D2 (manual tag), D3 (skip notarization), D4 (just `modeltap`), D5 (git-cliff), D6 (SLSA required), D7 (PAT), D8 (no homebrew-core in v1) — see Open Questions table above. Any disagreement to be raised during the DESIGN review or batched into a single user-facing AskUserQuestion before this feature progresses.

### To DEVOPS (platform-architect)

**Inputs:**
- KPI definitions: `outcome-kpis.md` (Handoff Notes section)
- NFR observability requirements: this file, "Observability" section
- Risk register: this file, "Risks" section

**DEVOPS work items derived from KPIs:**
1. K-PIPE alerting: `workflow_run: completed` follow-up workflow that opens a `release-pipeline-failure`-labeled issue when `release.yml` fails.
2. K-PROV verification helper: README troubleshooting section documenting `gh attestation verify <archive> --owner jeffabailey`.
3. `GH_TAP_TOKEN` expiry monitoring: a workflow that warns 30 days before expiry. Out of scope for this feature; tracked separately.
4. Release log table maintenance in `RELEASING.md` (per-release row).

### To DISTILL (acceptance-designer / Quinn)

**Inputs:**
- Journey schema: `journey-tag-to-brew-install.yaml`
- Gherkin scenarios: `journey-tag-to-brew-install.feature`
- Integration checkpoints: `shared-artifacts-registry.md` (Integration Checkpoints section)
- Outcome KPIs: `outcome-kpis.md`

The Gherkin scenarios in the .feature file are the source for acceptance tests in DISTILL. Some scenarios require integration tests across two repos (modeltap + homebrew-modeltap); DISTILL must design test infrastructure that can stand up an ephemeral tap repo or use a mock tap for fast iteration.
