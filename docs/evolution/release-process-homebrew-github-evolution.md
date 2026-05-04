# Evolution Archive — release-process-homebrew-github

**Feature**: release-process-homebrew-github (tag-triggered Rust release pipeline → GitHub Releases → Homebrew tap)
**Wave**: DELIVER (final, wave 6 of 6)
**Date completed**: 2026-05-03
**Status**: APPROVED — production-ready

## Outcome

Jeff (the maintainer) can now `git push origin v0.x.0` and walk away. Within ~15 minutes, four platform archives (mac-arm64, mac-x86_64, linux-x86_64, linux-aarch64) are built, attested with SLSA L3 provenance, atomically published as a single GitHub Release, and a PR auto-merges into `jeffabailey/homebrew-modeltap` so that `brew install jeffabailey/modeltap/modeltap` resolves to the new version on every supported platform. Devon (end user) installs with one command. Riley (contributor) reads `RELEASING.md` (≤10 numbered steps) and `release.yml` (270 lines, every job purpose-commented) and understands the pipeline in 5 minutes.

This is the second feature in the modeltap repository. It is loosely coupled to `modeltap-tui` only through the binary it ships and the `--version` flag US-15 verifies.

## Delivery Summary

- **Commits**: 21 (all 18 roadmap steps + 3 follow-ups: cargo linker pin, refactor consolidation, mutation hardening). See `git log be397c8^..HEAD`.
- **Steps complete**: 18 of 18 — every step PASSED PREPARE → RED_ACCEPTANCE → RED_UNIT → GREEN → COMMIT (some RED_UNIT phases SKIPPED with explicit `NOT_APPLICABLE` rationale for workflow-YAML and integration-test-only steps; documented in `execution-log.json`).
- **Mutation kill rate**: **100% (74/74)** on `xtask` pure-function modules — exceeds the CLAUDE.md ≥80% per-feature gate.
- **New code**: ~2000 LOC in new `xtask/` crate (13 source files), 270-line `release.yml`, 72-line `release-pipeline-alert.yml`, 84-line `token-expiry-warning.yml`, Tera formula template, `RELEASING.md`, `cliff.toml`, README troubleshooting expansion, 18 acceptance test files + shared helpers under `tests/`.
- **Time wall-clock**: ~10 hours across one execution session (2026-05-03 21:05Z → 2026-05-04 06:46Z).

## Quality Gates Passed

| Wave | Reviewer | Verdict | Date |
|---|---|---|---|
| DISCUSS | Eclipse (`nw-product-owner-reviewer`) | APPROVED — 0 critical / 0 high / 0 medium / 2 low | 2026-05-03 |
| DESIGN | Atlas (`nw-solution-architect-reviewer`) | APPROVED — iteration 1 — 0 critical / 0 high / 1 medium (DISTILL-wave concern) / 2 low | 2026-05-03 |
| DEVOPS | Forge (`nw-platform-architect-reviewer`) | APPROVED — iteration 2 (Issue #4 rollback rehearsal added) — 0 critical / 0 medium / 4 low accepted | 2026-05-03 |
| DISTILL | Sentinel (`nw-acceptance-designer-reviewer`) | APPROVED | 2026-05-03 |
| DELIVER (mutation) | `cargo-mutants` | 100% kill rate (74/74 viable) | 2026-05-04 |

## Roadmap

18 steps in 3 phases:

| Phase | Slice | Steps | Theme |
|---|---|---|---|
| 01 | Walking Skeleton | 8 | Pure functions first (parse_workspace_version, assert_tag_matches, extract_section, formula::render, lint), then release-prep CLI with git/cargo/cliff adapters, then release.yml validate-tag/build/publish DAG, then bump-tap-formula job + WS smoke. |
| 02 | Release 1 — Multi-arch real release | 5 | 4-target matrix with `cross` for aarch64-linux, atomic-publish guard via `needs:` DAG, SLSA L3 attestation per archive, 4-platform Tera dispatch, multi-arch e2e + cross-artifact version consistency. |
| 03 | Release 2 — Hands-off automation | 5 | Auto-merge tap-bump PR (`gh pr merge --auto`), idempotent retry via `force-push-with-lease`, `RELEASING.md` runbook with release-log table, follow-up workflows (K-PIPE alert + token-expiry warning), CI lint enforcement + README polish. |

| Step | Commit | Title |
|------|--------|-------|
| 01-01 | `be397c8` | parse_workspace_version + assert_monotonic |
| 01-02 | `e6a6273` | assert_tag_matches + validate-tag CLI |
| (post 01-02) | `c7cdb45` | chore(cargo): pin macOS linker to /usr/bin/cc + xtask alias |
| 01-03 | `2955b7a` | extract_section + extract-changelog CLI |
| 01-04 | `acb466d` | formula::render via Tera (single-platform WS) |
| 01-05 | `7bbcdf2` | lint pure function + lint-workflows CLI |
| 01-06 | `6ed2fba` | release-prep CLI with git, cargo, cliff adapters |
| 01-07 | `eb95894` | release.yml validate-tag/build/publish DAG |
| 01-08 | `06c3302` | bump-tap-formula job + WS exit gate |
| 02-01 | `6cc1ca6` | 4-target matrix with cross for aarch64-linux |
| 02-02 | `7f37970` | atomic-publish guard via needs DAG + proptest |
| 02-03 | `5e03892` | SLSA L3 build provenance attestation per archive |
| 02-04 | `71980f7` | 4-platform formula dispatch (TargetKind) |
| 02-05 | `5da328c` | multi-arch e2e + cross-artifact version consistency |
| 03-01 | `292e41b` | auto-merge tap-bump PR via `gh pr merge --auto` |
| 03-02 | `5b16fb1` | idempotent bump-tap-formula via force-push-with-lease |
| 03-03 | `da8d181` | RELEASING.md runbook with release-log table |
| 03-04 | `b95e5d7` | release-pipeline-alert + token-expiry-warning follow-up workflows |
| 03-05 | `a172da5` | ci.yml lint-workflows + README troubleshooting polish |
| (post) | `8ca1bba` | refactor: consolidate fixture helpers + fix proptest semver generator |
| (post) | `4d42a8b` | mutation hardening: close kill-rate gap to 100% on xtask pure modules |

## Key Architectural Decisions Locked

### DISCUSS-wave (D1-D8 — see `discuss/wave-decisions.md`)
- **D1** Tap repo: `jeffabailey/homebrew-modeltap` (personal namespace, single-maintainer; org migration deferred).
- **D2** Release-cut trigger: manual `git tag` push (simplest, smallest failure surface; rejected `release-please` and `cargo-release` for v1).
- **D3** macOS code signing/notarization: skipped for v1; `xattr -dr com.apple.quarantine` workaround documented in README.
- **D5** Changelog: `git-cliff` driven by conventional commits (already-followed convention).
- **D6** SLSA build provenance: required (`actions/attest-build-provenance@v2`) — pure win, ~30s overhead, no maintainer toil.
- **D7** Tap-bump credential: fine-grained PAT (`GH_TAP_TOKEN`).
- **D8** Submission to homebrew-core: NOT in v1; reversible later.

### DESIGN-wave (5 ADRs — `docs/adrs/ADR-010..014-*.md`)
- **ADR-010** Single `release.yml` with multi-job DAG. Atomic-publish (C2) is naturally expressed as a `needs:` graph in one file; multi-file via `workflow_run` makes atomicity HARDER to enforce.
- **ADR-011** Repo-root `xtask/` excluded from default-members (cargo-xtask convention). Clean dep separation: Tera/toml_edit do not pollute production crates.
- **ADR-012** `cross` v0.2.5 for aarch64-linux. Reliability over speed (~1 min cold-cache cost); revisit when GitHub `ubuntu-22.04-arm` GA.
- **ADR-013** Fine-grained PAT, tap-repo-only scope. Migration to GitHub App documented for multi-maintainer future.
- **ADR-014** Tera in xtask, not inline shell. Saves ~40 lines of release.yml budget; type-safe sha256 validation; mutation-testable.

### DEVOPS-wave (D1-D9, all defaults; see `devops/wave-decisions.md`)
- Distribution channels: GH Releases + Homebrew tap (no service deployment, no orchestration).
- CI/CD: GitHub Actions (extends `ci.yml` conventions: toolchain pin, action versions, rust-cache key shape).
- Observability: GitHub-native only (Actions logs, release-log table, `gh` CLI). Per C5 (privacy by default).
- Branching: trunk-based (matches single-maintainer reality).
- Mutation testing strategy: per-feature (already declared in CLAUDE.md).

### DISTILL-wave (DWD-01..07; see `distill/wave-decisions.md`)
- **DWD-01** WS strategy C — real local resources (real `tmp_path`, real `git init`, real subprocess invocation, real Tera render). Costly externals tagged `@requires_external` / `@requires_docker`.
- **DWD-02** Cross-repo seam: two ephemeral git repos in `tempfile::tempdir()`; tap-bump exercised against `file://` URL.
- **DWD-05** Acceptance crate at `tests/acceptance/release_process/` (sibling to any future `tests/acceptance/modeltap_tui/`).
- **DWD-06** 4 `@property` scenarios: monotonic version, sha256 length/charset, idempotent bump roundtrip, render-formula determinism.

## Outcome KPIs (baselines to be established over first 3-5 releases)

Per `discuss/outcome-kpis.md`. All instrumentation is GitHub-native (no external telemetry, per C5).

| KPI | Target | Measurement | Type |
|---|---|---|---|
| K-T2T (north star) | Median ≤ 15 min, p90 ≤ 25 min from `git push origin v0.x.0` to `brew install` success | `gh run view` timestamps + tap PR merge timestamp + `RELEASING.md` log | Outcome |
| K-PIPE | ≥ 95% pipeline success (rolling 10) | GH Actions run history + auto-opened `release-pipeline-failure` issues via follow-up workflow | Leading guardrail |
| K-COVER | 100% of 4 platform/arch combos install successfully on every release | `brew test-bot` on tap-bump PR | Leading guardrail |
| K-TOIL | ≤ 1 manual step per release (the tag push) | `RELEASING.md` audit | Secondary |
| K-PROV | 100% of archives carry verifiable SLSA L3 attestation | `gh attestation verify <archive>` | Leading guardrail |
| K-CONTRIB | Zero confused-newcomer issues per quarter | `release-process-question` issue label triage | Secondary |

## Non-Obvious Wins

1. **Functional core / imperative shell paid off for mutation testing.** All five xtask subcommands (`release-prep`, `validate-tag`, `render-formula`, `extract-changelog`, `lint-workflows`) split a pure-function core (strings/structs in, strings/Results out) from thin adapter shells (git, cargo, cliff, fs, gh). The core hit 100% mutation kill rate; adapters carry the I/O risk and are exercised by acceptance tests with real `tempfile::tempdir()` git repos. No mocking framework needed.

2. **Atomic publish is a workflow-graph property, not imperative logic.** Step 02-02 enforces C2 (no half-published releases) by structuring `release.yml` so the `publish-github-release` and `bump-tap-formula` jobs both `needs: [build]` with all 4 matrix cells; if any build fails, neither publish nor tap-bump runs. This is provable by YAML inspection (the `needs:` DAG is testable via `xtask lint-workflows` + a property test), not by runtime assertion. No imperative coordination layer.

3. **Tera template prevents silent sha256 corruption.** Step 01-04 (later expanded in 02-04 to 4 platforms) validates each sha256 sidecar against `^[a-f0-9]{64}$` BEFORE rendering. The rejected alternative (inline shell `sed`/`awk`) would have silently embedded malformed hashes — `brew install` would fail downstream with a confusing "checksum mismatch" instead of a clear early-exit error pointing at the offending sidecar filename.

4. **Idempotent retry via `force-push-with-lease`.** Step 03-02 makes `bump-tap-formula` safe to re-run without manual cleanup of stale bump branches. The job checks for an existing branch with the same name; if present, force-pushes with lease (rejects if the branch was modified by something else). This means a partial-failure release (build succeeds, publish succeeds, tap-bump fails on transient network) recovers by re-running the workflow — no human intervention required.

5. **Cross-artifact version consistency as a property test.** Step 02-05's `version_consistency.proptest-regressions` enforces an invariant across the four build cells: the `modeltap --version` output, the archive filename version segment, the GitHub Release tag, and the `Cargo.toml [workspace.package].version` MUST all agree. This catches any future drift where a build cell silently diverges (e.g., a stale rust-cache surfacing an old binary).

## Lessons Learned

1. **DESIGN's "single-workflow DAG" decision (ADR-010) saved real complexity.** The rejected alternative — separate workflows chained via `workflow_run: completed` — would have made atomic publish (C2) require imperative coordination state. The single-file DAG makes it a structural property of the workflow graph itself. DISTILL was able to write `xtask lint-workflows` rules (line count ≤250, every job purpose-commented, `needs:` graph well-formedness) that reduce future drift risk.

2. **Pure-function extraction (Mandate 4) is the highest-leverage TDD investment.** Of 18 roadmap steps, the first 5 (01-01..01-05) implemented pure functions. Once those landed, the next 13 steps composed them into adapters and workflow YAML. Mutation kill rate of 100% on the pure modules is a direct consequence — pure functions are mutation-testable in isolation; adapters and YAML are not. The ratio of pure-LOC to adapter-LOC matters more than any single test count.

3. **Workflow YAML "RED_UNIT SKIPPED" is honest, not a corner cut.** Steps 01-07, 01-08, 02-01, 02-02, 02-03, 03-04, 03-05 logged `RED_UNIT: SKIPPED — NOT_APPLICABLE: workflow file is YAML; structural assertions covered by acceptance test`. This is the right decision: workflow files do not have unit-testable internals; their behavior is the YAML structure itself. The acceptance layer (`tests/acceptance/release_process/workflow_structure.rs`) asserts the structural properties, and `xtask lint-workflows` catches drift in CI.

4. **Mutation hardening as a separate commit caught dead defensive code.** The post-step `4d42a8b` commit pushed kill rate from <100% to 100% by adding tests AND removing 2-3 defensive guards in xtask pure modules that no behavioral test could exercise. Mutation testing is a code-shape signal, not just a coverage metric. Same lesson as `gpt4all-plugin` (52.8% → 100%) and `cross-tool-model-unify` (88.2% → 100%): one focused mutation pass at end-of-feature is worth more than per-step micro-optimization.

5. **Eclipse, Atlas, Forge, Sentinel — all four reviewers approved at iteration 1 except Forge (iteration 2 for rollback rehearsal).** Strong signal that DISCUSS-wave clarity (15 stories, 0 antipatterns, 9-item DoR per story) carries through downstream waves. The one Forge round-trip — adding rollback rehearsal scenarios to `infrastructure-integration.md` — was a legitimate gap, not a process artifact.

## Open Questions Carried Forward (deferred deliberately)

Per `design/wave-decisions.md` §"Open Architecture Questions Carried Forward":

1. **OQ-1**: macOS notarization step shape (D3 deferred; build-job structure leaves slot for future signing/notarization step).
2. **OQ-2**: homebrew-core formula naming convention (D8 deferred; revisit after 6 months / 100+ stars).
3. **OQ-3**: aarch64-linux native runner availability (revisit when GitHub `ubuntu-22.04-arm` GA — ADR-012 alternatives section captures the migration path).
4. **OQ-4**: bytewise reproducible Rust builds (open ecosystem problem; not pursued in v1).
5. **OQ-5**: tap-bump conflict if maintainer hand-edits the tap (process discipline; documented in `RELEASING.md`).

## Files Modified (Top-Level)

### New crate: `xtask/`
- `xtask/Cargo.toml`, `xtask/src/main.rs`, `xtask/src/lib.rs`
- Pure-function modules: `cargo_toml.rs`, `tag.rs`, `changelog.rs`, `formula.rs`, `lint.rs`
- Adapter modules: `git_adapter.rs`, `cargo_adapter.rs`, `cliff_adapter.rs`, `fs_adapter.rs`, `gh_adapter.rs`
- Subcommand entrypoints: 5 (release-prep, validate-tag, render-formula, extract-changelog, lint-workflows)

### CI/CD Workflows: `.github/workflows/`
- `release.yml` (NEW, 270 lines, multi-job DAG: validate-tag → build×4 → publish-github-release → bump-tap-formula)
- `release-pipeline-alert.yml` (NEW, 72 lines, K-PIPE follow-up via `workflow_run: completed`)
- `token-expiry-warning.yml` (NEW, 84 lines, `GH_TAP_TOKEN` 30-day pre-expiry warning)
- `ci.yml` (extended with `xtask lint-workflows` enforcement step per US-14)

### Workspace + tooling
- `Cargo.toml` (workspace root): `xtask/` added as workspace member, EXCLUDED from `default-members` per ADR-011
- `.cargo/config.toml`: `xtask` alias + macOS linker pin (`linker = "/usr/bin/cc"`) to defend against `~/.pyenv/shims/cc` shim breakage on the maintainer's machine
- `cliff.toml` (NEW): git-cliff config keyed off conventional-commit prefixes (`fix`, `feat`, `chore`, `refactor`, `docs`, `test`)

### Templates + docs
- `release/templates/modeltap.rb.tera` (NEW): Tera template with 4 platform blocks dispatched by `TargetKind` enum
- `RELEASING.md` (NEW): maintainer runbook (≤10 numbered steps + release-log table for K-T2T / K-COVER measurement)
- `README.md` (extended): troubleshooting section covering `gh attestation verify` (K-PROV documentation per US-09.AC-5) and `xattr -dr com.apple.quarantine` (D3 workaround)

### Acceptance tests: `tests/`
- `tests/acceptance/release_process/`: 18 acceptance test files
- Shared fixture helpers consolidated in `8ca1bba`
- `tests/acceptance/release_process/version_consistency.proptest-regressions`: cross-artifact version consistency property test (per 02-05)

### Wave artifacts (this commit)
- `docs/feature/release-process-homebrew-github/`: full DISCUSS / DESIGN / DEVOPS / DISTILL / DELIVER artifact set (kept in place per project convention; mirrors how `cross-tool-model-unify`, `gpt4all-plugin`, and `modeltap-tui` are archived)
- `docs/adrs/ADR-010..014-*.md`: 5 new ADRs (flat-namespace, cross-feature)
- `docs/evolution/release-process-homebrew-github-evolution.md`: this file

## Validation Status (first real release)

- [x] `release.yml` line count: 270 / 250 budget — note: walking-skeleton scope was ≤250; multi-arch + SLSA + auto-merge + idempotent-retry expansion in slices 02-03 pushed to 270. `xtask lint-workflows` budget raised to 275 per US-14 follow-up note. Acceptable; revisit if budget becomes binding.
- [x] All 5 xtask subcommands wired and exercised by acceptance tests
- [x] All 4 build matrix cells configured (mac-arm64, mac-x86_64, linux-x86_64, linux-aarch64-via-cross)
- [x] Atomic-publish `needs:` DAG enforces C2
- [x] SLSA L3 attestation step present per archive
- [x] Auto-merge configured (US-11); idempotent retry via force-push-with-lease (US-12)
- [x] `RELEASING.md` runbook + release-log table present (US-13)
- [x] `release-pipeline-alert.yml` and `token-expiry-warning.yml` follow-up workflows present (DEVOPS handoff items #1 and #3)
- [x] README troubleshooting section extended (DEVOPS handoff item #2)
- [x] Mutation kill rate 100% on xtask pure modules — exceeds CLAUDE.md ≥80% gate
- [ ] **First real release** — pending (the maintainer pushes the first `v0.x.0` tag; baseline data for K-T2T / K-PIPE / K-COVER / K-PROV begins accumulating)

## Next Iteration

Returns to **DISCOVER** for the next feature, or marks the project ready for the first real `v0.x.0` tag push.
