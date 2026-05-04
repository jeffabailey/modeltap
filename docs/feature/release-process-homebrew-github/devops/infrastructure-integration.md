# Infrastructure Integration — release-process-homebrew-github

**Wave:** DEVOPS (4 of 6)
**Author:** Apex (nw-platform-architect)
**Date:** 2026-05-03

How the new `release.yml` (and its two follow-up workflows) coexists with the existing `ci.yml`, the workspace `Cargo.toml`, the cross-repo tap, and the secret store.

## 1. Coexistence with `.github/workflows/ci.yml`

### 1.1 Boundary

| | `ci.yml` (existing) | `release.yml` (new) |
|---|---|---|
| Triggers | `pull_request: [main]`, `push: [main]` | `push: tags: [v*.*.*]` |
| Concurrency | `ci-${{ github.ref }}`, cancel-in-progress | (none — distinct tags are independent releases) |
| Jobs | fmt, clippy, test, deny, k3-bench, ci-success | validate-tag, build (matrix x4), publish-github-release, bump-tap-formula |
| Outputs | Pre-merge "all checks pass" status | GitHub Release + tap PR |
| Failure mode | Block PR merge | Block release publish (atomic per US-08) |

The two files have **disjoint trigger sets** — there is no event that fires both. This eliminates concurrency conflicts: a tag push runs only release.yml; a PR or main-branch push runs only ci.yml.

### 1.2 Toolchain version sync (C7 enforcement)

Per DISCUSS C7: "Same toolchain pinning as ci.yml". Drift between the two files would silently break K-PIPE.

Both files MUST use the identical action+pin combination:

```yaml
- uses: dtolnay/rust-toolchain@stable
```

No `with: toolchain: 1.75.0` override in either file. No `toolchain.toml` introduction. Stable + workspace `rust-version = "1.75"` in `Cargo.toml` is the version contract.

**Drift detection**: a future enhancement to `cargo xtask lint-workflows` could grep both YAML files and assert the toolchain step is byte-identical. Not in v1; flagged as v2 enhancement in `kpi-instrumentation.md`.

### 1.3 Cache key sharing

`Swatinem/rust-cache@v2` keys per (target, Cargo.lock hash). Where `release.yml` and `ci.yml` build the same target with the same lockfile, cache entries are mutually accessible.

| Target | ci.yml job | release.yml job | Cache shared? |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` (default) | `test (ubuntu-latest)`, `clippy`, `k3-bench` | `build` (cell B3) | YES — same key shape |
| `aarch64-apple-darwin` (default on macos-14) | `test (macos-latest)` | `build` (cell B1) | YES if `macos-latest` resolves to macos-14 (currently does) |
| `x86_64-apple-darwin` | (none — ci.yml doesn't target Intel macOS specifically) | `build` (cell B2) | NO (independent) |
| `aarch64-unknown-linux-gnu` (cross) | (none — ci.yml doesn't cross-compile) | `build` (cell B4) | NO (cross's Docker layer cache is its own surface) |

Effect: 2 of 4 build cells benefit from CI's pre-warmed cache on the first release after a feature merge. Cold-cache release time impact: ~3-5 min savings.

### 1.4 Shared deny.toml

`cargo-deny check` runs in:

- `ci.yml` `deny` job (existing)
- `release.yml` `build` cells (per C3 CI parity gates step 4 — actually inline as step 7c rather than a separate job; see `ci-cd-pipeline.md` §3 for ordering)

Both reference the SAME `deny.toml` at repo root. Adding new workspace deps in this feature (`tera`, `toml_edit`, `cargo_metadata`, `regex`) requires updating `deny.toml` allow-list. The PR adding the xtask must also update `deny.toml`; the PR-stage `ci.yml` `deny` job catches missing entries before merge.

Note: a recent CI fix (commit `facddef` per git log) added an ignore for `RUSTSEC-2024-0436` (paste unmaintained, transitive via ratatui). The release pipeline inherits this ignore via the shared `deny.toml`.

### 1.5 Workflow lint coexistence

`cargo xtask lint-workflows --workflow .github/workflows/release.yml --max-lines 250` runs in `ci.yml` (per US-14). This is the drift-catching gate: any PR that pushes `release.yml` over 250 lines or removes a `# Purpose:` comment from any job FAILS in CI before merge.

Could the lint also enforce `ci.yml` constraints? Out of scope for this feature; the lint targets `release.yml` only per US-14. Future enhancement: extend to a multi-file invocation.

### 1.6 The `ci-success` job stays unchanged

`ci.yml` ends with a `ci-success` job that depends on all five primary jobs. This is the merge-required status check. Adding `cargo xtask lint-workflows` to ci.yml means it must be added to `ci-success: needs:`. Documented as crafter task in DELIVER wave handoff (`wave-decisions.md`).

## 2. Workspace `Cargo.toml` Integration

### 2.1 New workspace member

Adding `xtask` to the workspace per DESIGN ADR-011:

```toml
[workspace]
resolver = "2"
default-members = [
    "crates/modeltap-core",
    "crates/modeltap-tui",
    "crates/modeltap-app",
    "plugins/ollama",
    "plugins/hf",
    "plugins/lm-studio",
    "plugins/atomic-chat",
    "plugins/gpt4all",
    "plugins/atomic-chat-fixture",
]
members = [
    "crates/modeltap-core",
    # ... (same as default-members)
    "plugins/atomic-chat-fixture",
    "xtask",   # NEW — included in members but excluded from default-members
]
```

**Why explicit `default-members`**: without it, `cargo build` / `cargo test` at workspace root build EVERY member, including xtask. This would slow down CI's `test` job. Adding `default-members` (which currently isn't set, so cargo defaults to "all members") forces only production crates into the default build, while keeping `xtask` available for `cargo run -p xtask`.

Per DESIGN ADR-011 — the standard cargo-xtask community convention.

### 2.2 New workspace dependencies

Per DESIGN technology-stack.md §5, the xtask adds these to `[workspace.dependencies]`:

```toml
[workspace.dependencies]
# ... (existing)

# xtask (release pipeline)
tera = "1.20"
toml_edit = "0.22"
semver = "1"
cargo_metadata = "0.18"
regex = "1"
```

Existing workspace deps reused by xtask: `clap`, `anyhow`, `thiserror`, `serde`, `serde_json`.

`cargo deny check` (in `ci.yml` and in `release.yml`) gates new licenses; all 5 new deps are MIT/Apache-2.0 — already on the project allow-list.

### 2.3 The `[alias]` entry

A `.cargo/config.toml` entry makes `cargo xtask <subcommand>` work as if xtask were a first-class cargo subcommand:

```toml
# .cargo/config.toml (new file or amended)
[alias]
xtask = "run --package xtask --"
```

Per DESIGN ADR-011 §Decision. Implementation owned by software-crafter in DELIVER.

## 3. Cross-Repo Coexistence (Tap Repo)

### 3.1 Tap repo branch protection

Per DESIGN component-boundaries.md §6 and US-11.AC-5: the tap repo `main` branch MUST require `brew test-bot` status check before any merge. One-time setup (documented in RELEASING.md "First-time setup"):

```bash
gh api -X PUT \
  /repos/jeffabailey/homebrew-modeltap/branches/main/protection \
  -f required_status_checks[strict]=true \
  -f required_status_checks[contexts][]='brew test-bot' \
  -f enforce_admins=true \
  -f required_pull_request_reviews=null \
  -f restrictions=null
```

This means even the maintainer cannot push directly to tap-repo main; everything goes through PR + brew test-bot. The auto-merge from `bump-tap-formula` works because `gh pr merge --auto` waits for required status checks.

### 3.2 Tap repo `test-bot.yml` initial content

Owned by tap repo. Recommended content (provided here as a draft for first-time setup, but the file lives in jeffabailey/homebrew-modeltap, not in this source repo):

```yaml
# tap-repo .github/workflows/test-bot.yml
name: brew test-bot

on:
  pull_request:

jobs:
  test-bot:
    strategy:
      fail-fast: false
      matrix:
        os: [macos-14, macos-13, ubuntu-22.04]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: Homebrew/actions/setup-homebrew@master
      - run: brew test-bot --only-formulae --tap jeffabailey/modeltap
        env:
          HOMEBREW_DEVELOPER: 1
          HOMEBREW_NO_AUTO_UPDATE: 1
          HOMEBREW_NO_ANALYTICS: 1   # privacy parity with C5
```

The aarch64-linux platform is NOT covered natively here (no ubuntu-22.04-arm runner in this matrix); QEMU emulation by Homebrew on the ubuntu-22.04 cell handles it best-effort per ADR-012. Future migration to native arm runner tracked as OQ-3.

### 3.3 Cross-repo authentication topology

```
jeffabailey/modeltap repo                    jeffabailey/homebrew-modeltap repo
─────────────────────                        ─────────────────────────────────
GH Actions secrets:                          GH Actions secrets:
  GITHUB_TOKEN  (auto)                         GITHUB_TOKEN  (auto, scoped to tap)
  GH_TAP_TOKEN  (PAT)                          (no GH_TAP_TOKEN; tap repo never reads back into source)

Releases:                                    Branch protection (main):
  source of release artifacts                  required: brew test-bot
                                               enforce_admins: true

bump-tap-formula job ────[GH_TAP_TOKEN]────► tap repo PR creation
                                                ↓ (auto-merge after status pass)
brew test-bot ◄──────[brew/setup-homebrew]── tap repo CI
                                                ↑
brew install ◄──────────[end users]─────────── tap repo Formula/modeltap.rb
```

Token flows ONE direction: source repo → tap repo. Tap repo never pushes anything back to source repo. This is the minimum-coupling topology; if `GH_TAP_TOKEN` is compromised, the blast radius is the tap repo (and the formula could be made to point at a malicious URL, but `brew test-bot` would catch sha256 mismatch).

### 3.4 Tap repo `README.md`

Owned by tap repo (hand-maintained by maintainer). Recommended boilerplate:

```markdown
# homebrew-modeltap

Homebrew tap for [modeltap](https://github.com/jeffabailey/modeltap), a Rust TUI
for managing local AI models.

## Install

```sh
brew install jeffabailey/modeltap/modeltap
```

## Updating

The formula in this repo is **automatically updated** by GitHub Actions in the
[modeltap source repo](https://github.com/jeffabailey/modeltap) on every release.
Do not edit `Formula/modeltap.rb` directly — changes will be overwritten on the
next release. To change the build, update the source repo's
`release/templates/modeltap.rb.tera`.

## Verification

To verify a release archive's build provenance:

```sh
gh attestation verify <archive> --owner jeffabailey
```

See [main repo's troubleshooting docs](https://github.com/jeffabailey/modeltap#installation-troubleshooting) for more.
```

## 4. Secret Scoping

### 4.1 `GITHUB_TOKEN`

Auto-provided per job. Scope = the repo running the workflow. Cannot cross repos.

`release.yml` per-job permissions are explicit (per `ci-cd-pipeline.md` §2.3) — narrower than the default. This is least-privilege.

### 4.2 `GH_TAP_TOKEN` (PAT)

Per ADR-013. Scoped to ONE repo (`jeffabailey/homebrew-modeltap`) with TWO permissions (Contents:RW, PRs:RW).

**Storage**: GH Actions repository secret on `jeffabailey/modeltap`. No environment scoping (no environments configured for this feature); accessible by any job that references it.

**Reference discipline**: ONLY `bump-tap-formula` references `${{ secrets.GH_TAP_TOKEN }}`. Code-review enforces; a `lint-workflows` future enhancement could assert this.

### 4.3 No other secrets

This feature introduces NO other secrets. No SLSA signing key (provided by GH OIDC). No notarization keys (D3 deferred). No webhook URLs (D5 forbids).

## 5. Branch Protection on Source Repo

Per D8 (Trunk-Based Development) and DEVOPS handoff for `branching-strategy.md`:

Source repo `main` branch should have:

- **Required status checks**: `ci-success` (the existing aggregator job) — already present
- **Branch protection**: enforce admins (so the maintainer also can't bypass) — recommended add per first-time setup
- **Allow force pushes**: disabled — prevents accidental history rewrite
- **Allow deletions**: disabled — prevents accidental main-branch deletion

`release.yml` is NOT a required status check on main (it doesn't run on main pushes; it runs on tag pushes). Tag protection is handled differently:

- **Tag pattern protection** (`v*.*.*`): GH Actions does not provide tag-pattern protection. The defense is the `validate-tag` job — any tag mismatching `Cargo.toml` version fails fast within ~30s. This is a runtime check, not a push-time gate. Acceptable for single-maintainer.

For a future multi-maintainer scenario: GitHub now offers "tag protection rules" in repo settings; a rule restricting `v*.*.*` tag pushes to maintainer accounts would add a push-time gate.

## 6. Conflict-Free First Release Checklist

A consolidated "is everything wired" check, runnable BEFORE the first tag push:

- [ ] `release.yml` lints clean: `cargo run -p xtask -- lint-workflows --workflow .github/workflows/release.yml --max-lines 250`
- [ ] `ci.yml` `deny` job passes locally: `cargo deny check`
- [ ] `xtask` builds: `cargo build -p xtask --locked`
- [ ] `xtask` unit tests pass: `cargo test -p xtask --locked`
- [ ] `cargo run -p xtask -- validate-tag --tag v0.1.0` passes (matches workspace version)
- [ ] `cargo run -p xtask -- render-formula ...` produces sane Ruby
- [ ] Tap repo `jeffabailey/homebrew-modeltap` exists with empty `Formula/` dir
- [ ] Tap repo `.github/workflows/test-bot.yml` committed (per §3.2)
- [ ] Tap repo `main` branch protection requires `brew test-bot` (per §3.1)
- [ ] `GH_TAP_TOKEN` secret set on source repo with correct scope (per ADR-013 / §4.2)
- [ ] `release-pipeline-failure` label exists: `gh label create release-pipeline-failure ...`
- [ ] `tap-token-expiry` label exists: `gh label create tap-token-expiry ...`
- [ ] `release-pipeline-alert.yml` self-test executed (per `monitoring-alerting.md` §1.6)
- [ ] `token-expiry-warning.yml` first run via workflow_dispatch returns `valid`
- [ ] **Rollback rehearsal — Scenario A** (per `monitoring-alerting.md` §4.2): cut a throwaway pre-release tag (e.g., `v0.0.0-rollback-drill-A`) that is intentionally version-mismatched so `validate-tag` fails. Confirm: no GH Release created; no tap PR opened; `release-pipeline-failure` issue auto-opens. Delete tag and close issue.
- [ ] **Rollback rehearsal — Scenario B** (per `monitoring-alerting.md` §4.2): cut another throwaway tag (e.g., `v0.0.0-rollback-drill-B`) and after `publish-github-release` succeeds but `bump-tap-formula` is in-flight, intentionally revoke `GH_TAP_TOKEN` to force a bump-tap-formula failure mid-flight. Then: rotate the token, re-run the failed `bump-tap-formula` job (US-12 idempotent retry), confirm tap PR opens correctly. Then exercise `gh release delete v0.0.0-rollback-drill-B --yes --cleanup-tag` to confirm the drafting/deletion command works as documented. (This is a heavier drill; can be staged as a one-time exercise after pipeline first-deploy.)
- [ ] `RELEASING.md` "First-time setup" section completed by maintainer

This checklist is incorporated into `RELEASING.md` "First-time setup" section per US-13.AC-4 and the DELIVER wave handoff.

## 7. Cross-Reference

- DESIGN ADR-010 (single-workflow DAG)
- DESIGN ADR-011 (xtask placement)
- DESIGN ADR-013 (PAT credential)
- DEVOPS `ci-cd-pipeline.md` (per-job permissions, action pins)
- DEVOPS `monitoring-alerting.md` (alert workflows)
- DEVOPS `branching-strategy.md` (D8, branch protection details)
- Existing `.github/workflows/ci.yml`
- Existing `Cargo.toml`
