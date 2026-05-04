# Walking Skeleton Strategy — release-process-homebrew-github

**Wave:** DISTILL (5 of 6)
**Author:** Quinn (nw-acceptance-designer)
**Date:** 2026-05-03

## 1. Strategy: C — Real Local Resources (DWD-01)

The DESIGN of this feature is dominated by local-resource adapters: the `xtask` Rust binary reads/writes real files, shells out to real `git` / `cargo` / `git-cliff`, renders real Tera templates, and (for cross-repo) pushes to a `file://` URL pointing at an ephemeral local "tap repo." None of these require network or paid services.

The remaining costly externals (`gh release create`, `gh pr create`, `gh attestation verify`, `cross` Docker pull, `brew test-bot`) are tagged and isolated:

- `@requires_external` — needs live GitHub API; runnable on demand, skipped in default CI.
- `@requires_docker` — needs Docker daemon (cross-compile, brew test-bot harness).

The walking-skeleton scenarios use REAL local I/O for every adapter that has a local equivalent. The litmus test from the skill — "if I deleted the real adapter, would the WS still pass?" — answers NO for every WS scenario in this design.

## 2. Walking-Skeleton Scope

The DISCUSS-defined walking skeleton is the WS slice of the story map (US-01..US-06 + US-15 single-target path). The thinnest end-to-end thread that delivers observable user value:

> **Jeff prepares v0.0.1-rc1 with `cargo xtask release-prep`; he tags and pushes; the workflow validates the tag, runs CI gates, builds one Linux x86_64 archive, publishes a GitHub Release, opens a tap-bump PR; Devon `brew install`s on a Linux box and `modeltap --version` prints `modeltap 0.0.1-rc1`.**

Per Mandate 5 (Walking Skeleton Strategy) and the DESIGN constraints, the WS exit gate is **6 user-value scenarios** (one per backbone activity + one end-user verification):

1. **PREP** — `xtask release-prep --version 0.0.1-rc1` bumps `Cargo.toml`, regenerates `CHANGELOG.md`, exits zero with next-step message.
2. **TAG** — `xtask validate-tag --tag v0.0.1-rc1` accepts when `Cargo.toml` matches; rejects when mismatched.
3. **BUILD** — `xtask` orchestration of CI parity gates passes against a clean fixture workspace (gates run before any artifact is built).
4. **PUBLISH** — `xtask extract-changelog --version 0.0.1-rc1` produces release-notes content from a fixture `CHANGELOG.md`; a downstream `gh release create` invocation is shaped correctly (smoke verified under `@requires_external`).
5. **TAP-BUMP** — `xtask render-formula` against an ephemeral tap-repo at `file://${TMPDIR}/tap-fake` produces a single-platform `Formula/modeltap.rb` and a `bump/v0.0.1-rc1` branch is created and pushed.
6. **USER-INSTALL** — End-to-end smoke tagged `@requires_external` (real Homebrew install on a clean container) — observable user outcome: `brew install` succeeds, `modeltap --version` prints `modeltap 0.0.1-rc1`.

## 3. Adapter Coverage Strategy

See `adapter-coverage.md` for the full audit. Summary: every locally-runnable adapter has a `@real-io` integration scenario in the WS (or in `walking-skeleton.feature` if the WS scenario itself doesn't exercise the adapter realistically). Costly adapters have a `@requires_external` smoke + `@infrastructure-failure @in-memory` scenarios for unit-level coverage of error paths.

## 4. WS Strategy Decision Justification

Why C and not B (mocks at network boundary)?

- The "network boundary" for this feature is `gh` CLI invocations against `api.github.com`. We could mock `gh` with a shim that returns canned JSON — but the cost is a brittle mock that doesn't catch CLI invocation typos or argument-order changes. The benefit is dubious because the actual integration point is well-tested by GitHub's own `gh release` smoke + our `@requires_external` smoke.
- Cross-repo seam is git-protocol (push, pull, branch). `file://` git remotes are FAITHFUL — they exercise the same git commands and hooks as `https://`. Using a mock here would lose the test value entirely (idempotent retry, force-push-with-lease semantics depend on real git behavior).

Why C and not D (full live stack)?

- A live tap repo on GitHub for testing would require a separate `jeffabailey/homebrew-modeltap-test` repo + token + cleanup. Per-test cleanup of GitHub state is racy. The local `file://` ephemeral repo is faster and deterministic.
- Live `gh release create` per test would pollute the modeltap repo's release page. The `@requires_external` smoke is invoked manually post-merge for confidence; it's not the WS gate.

## 5. Mandate Compliance Statement (CM-A through CM-D)

- **CM-A (Hexagonal boundary)**: WS scenarios invoke the `xtask` CLI binary (driving port) via `assert_cmd`. No direct calls into `xtask::version::parse_workspace_version` etc. from acceptance tests — those calls are made by the CLI dispatcher, which the test invokes by running `cargo xtask <subcommand>`. Pure-function modules ARE unit-tested directly in DELIVER (per Mandate 4); that is INSIDE the inner loop, not the acceptance suite.
- **CM-B (Business language)**: Gherkin uses `maintainer prepares a release`, `tag matches workspace version`, `tap-bump PR opens`, `auto-merge fires`, `clean machine installs`. Zero `HTTP`, `JSON`, `200`, `database`, `mock` in WS scenarios.
- **CM-C (User journey completeness)**: Every WS scenario has a user trigger (Given/When), business logic (When), observable outcome (Then), and business value (Then — "next-step message printed", "release notes match the section", "PR is opened against the tap repo's main branch", "Devon's `modeltap --version` prints `modeltap 0.0.1-rc1`").
- **CM-D (Pure function extraction)**: 6 pure functions extracted (per DESIGN component-boundaries §2.2: `parse_workspace_version`, `assert_monotonic`, `assert_tag_matches`, `render`, `extract_section`, `lint`). Adapter parametrization confined to `git_adapter`, `cargo_adapter`, `gh_adapter`, `cliff_adapter`, `fs_adapter` — and only at integration test layer.

## 6. WS Exit Criteria (DELIVER ships when these pass)

1. All 6 WS scenarios green using `@real-io` (no `@in-memory` substitutions for adapters Strategy C requires real).
2. RED scaffolds replaced by working implementations.
3. Mutation testing ≥80% kill rate on `xtask::version`, `xtask::formula`, `xtask::changelog`, `xtask::workflow_lint`.
4. `@requires_external` end-to-end smoke (live `gh release create` + `brew install` on a clean container) verified once per maintainer hand-check during the DELIVER review.
