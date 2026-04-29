# ADR-010: CI Platform — GitHub Actions

## Status

Accepted (2026-04-28).

## Context

modeltap is an OSS Rust project hosted on GitHub. CI must:

- Run on macOS and Linux (per C2; Windows is WSL-only and uses Linux runners)
- Run a Rust toolchain matrix-friendly workload (fmt, clippy, test, deny)
- Gate merges via branch protection
- Drive the release tooling (`cargo-dist` generates a workflow file in this same CI system)
- Be free for OSS use
- Be the path of least friction for contributors (Riley persona) — no separate sign-up, no credentials to provision, no foreign UI to learn

We need a CI platform.

## Decision

**Use GitHub Actions.** Workflows live under `.github/workflows/`. Hosted macOS-latest and ubuntu-latest runners. Branch protection requires `ci / ci-success` (an aggregate job) to pass.

## Alternatives considered

### Alternative A — GitLab CI

**Pros:** mature; powerful pipeline DSL; built-in container registry.

**Cons:** the project is on GitHub. Mirroring to GitLab purely for CI is friction without payoff. GitHub Actions is sufficient. **Rejected on locality.**

### Alternative B — CircleCI

**Pros:** historically faster than GHA on Linux jobs; nice config language.

**Cons:** separate account; macOS minutes are paid even for OSS in some plans; cargo-dist's first-class CI target is GitHub Actions. **Rejected on integration overhead.**

### Alternative C — Buildkite / Drone / self-hosted

**Pros:** maximum control; dedicated runners avoid noisy-neighbor flakiness.

**Cons:** infrastructure cost; maintenance burden; defeats the "no infra" stance for a local CLI project. **Rejected as overkill.**

### Alternative D (CHOSEN) — GitHub Actions

GitHub-native, free for OSS, hosted macOS + Linux runners, first-class cargo-dist support, tight branch-protection integration.

## Consequences

### Positive

- Zero CI infrastructure to operate.
- Contributors fork the repo and CI runs against their fork's PR — no credential setup.
- `cargo dist generate` writes the release workflow directly; no translation layer.
- Hosted macOS runners eliminate the need for a Mac on the maintainer's desk for releases.
- CODEOWNERS and branch protection are GitHub-native; consistent with the rest of the project's tooling.

### Negative

- macOS-latest runners are slower than Linux runners (typical 2-3x). K3 benchmark may need different thresholds per OS if Linux-only is too tight a guardrail. Mitigation: K3 benchmark currently runs only on Linux (per `ci-pipeline.md` §5); macOS runs the test suite but not the benchmark.
- Hosted runner concurrency limits apply (10 concurrent jobs on free tier). Likely irrelevant for a single-maintainer project; if PR throughput grows, can buy more or self-host.
- Vendor lock-in to GitHub Actions YAML. Mitigation: the workflows are simple (fmt/clippy/test/deny/build) and easily portable — no GitHub-only constructs in critical paths.

### Neutral

- Workflow YAML lives in-repo under `.github/workflows/`, version-controlled like everything else.
- Action pinning policy: pin to major version (`@v4`), allow minor/patch updates via Dependabot or maintainer review. Stricter SHA-pinning is a security best practice that may be worth adopting in v1.x but adds maintenance friction; deferred.

## Enforcement

- `.github/workflows/ci.yml` per `ci-pipeline.md` §2.
- Branch protection on `main` requires `ci / ci-success` status check.
- CODEOWNERS routes `.github/workflows/**` to maintainer review.
