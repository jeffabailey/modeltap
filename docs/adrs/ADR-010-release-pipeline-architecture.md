# ADR-010: Release pipeline — single workflow file with multi-job DAG

## Status

Proposed (2026-05-03 — DESIGN wave for `release-process-homebrew-github`).

## Context

The release pipeline must enforce **atomic publishing** (C2 / US-08): a release is fully published or not at all. If 3 of 4 build matrix cells succeed and 1 fails, no GitHub Release may be created and no tap PR may be opened. Half-published releases would 404 for users on the failing platform.

Two architectural approaches exist for orchestrating a multi-step pipeline in GitHub Actions:

1. **Single workflow file with a multi-job DAG using `needs:`**. All jobs (validate-tag, build matrix, publish-github-release, bump-tap-formula) live in `.github/workflows/release.yml`. GitHub Actions natively skips a `needs:`-declared job if any prerequisite failed.
2. **Multiple workflow files chained via `workflow_run`**. e.g., `release-build.yml` triggers `release-publish.yml` on completion, which triggers `release-bump.yml`. Cross-workflow communication via artifact passing or repository_dispatch.

Other constraints:

- C7: same toolchain pinning as `ci.yml` (`dtolnay/rust-toolchain@stable`) — both approaches comply.
- C8: no silent skips — every guard fails visibly.
- US-14: ≤250 lines for `release.yml`. The single-workflow approach must fit.
- US-08: atomic-publish guarantee.
- K-PIPE: ≥95% pipeline success rate; debug-friendly UI is a soft requirement.

## Decision

**Single `release.yml` workflow file containing all jobs in a multi-job DAG with explicit `needs:` declarations.**

```text
on: push: tags: ['v*.*.*']

jobs:
  validate-tag:           # ~30s
  build:                  # matrix × 4, ~5 min slowest cell
    needs: validate-tag
  publish-github-release: # ~2 min
    needs: [validate-tag, build]
  bump-tap-formula:       # ~2 min
    needs: publish-github-release
```

`fail-fast: false` on the `build` matrix so a single failure does not cancel still-running cells (helpful for diagnosis). Atomicity is enforced by the `needs:` chain regardless of `fail-fast`.

No `if: always()` or `if: failure()` overrides on `publish-github-release` or `bump-tap-formula` (US-08.AC-3).

## Alternatives Considered

### Alternative 1: Multiple workflow files chained via `workflow_run`

- **Pros**: each file smaller; clearer separation of concerns; downstream workflow can be re-run independently.
- **Cons**:
  - **Atomicity is harder to express**: `workflow_run` triggers fire on completion regardless of outcome by default; adding `if: ${{ github.event.workflow_run.conclusion == 'success' }}` is required and easy to forget — exactly the kind of silent-skip C8 forbids.
  - **Cross-workflow artifact passing** requires `actions/upload-artifact` + `actions/download-artifact` with the run-id from the triggering workflow — an extra layer of indirection that obscures the data flow.
  - **Permissions** must be re-declared per workflow file; secret scope debugging becomes harder.
  - **Single-pane visibility lost**: `gh run watch` shows one workflow run; chained workflows produce N separate runs that maintainers must manually correlate.
  - **K-PIPE measurement** (success rate over rolling last 10) becomes ambiguous — does a partial chain count as success?
- **Rejection rationale**: introduces complexity that buys nothing. The atomic-publish guarantee is HARDER to enforce, not easier. The 250-line limit (US-14) is not binding on the single-file approach (estimated ~180 lines).

### Alternative 2: Single workflow file with a single mega-job

- **Pros**: literally one job; trivial to read top-to-bottom.
- **Cons**:
  - **No matrix parallelism**: 4 targets serialized would push tag-to-tap latency from ~16 min to ~25 min, blowing K-T2T median target.
  - **No fail-fast on validate-tag**: a tag mismatch wastes 5 minutes of build time before the failure surfaces.
  - **No clear retry surface**: if `bump-tap-formula` fails after `publish-github-release` succeeds (e.g., transient PAT auth), the maintainer cannot re-run only the bump step; the whole release would re-run and try to re-publish (fails because the release already exists).
- **Rejection rationale**: violates K-T2T budget; defeats US-12 (idempotent retry of just bump-tap-formula).

### Alternative 3: Reusable workflow pattern (`workflow_call`)

- **Pros**: composable; future migration to homebrew-core pipeline could reuse the build job.
- **Cons**: adds a layer of indirection (parent workflow calling child); doesn't change the atomicity story (still need `needs:` on the parent); marginal benefit for a single-maintainer project.
- **Rejection rationale**: premature factoring. If/when a second pipeline needs to reuse the build matrix (e.g., an alpha-tag-on-PR pipeline), refactor then.

## Consequences

### Positive

- **Atomic publish is a workflow-graph property**, not imperative code. `publish-github-release: needs: [validate-tag, build]` is the entire mechanism. Pure declarative; impossible to accidentally bypass without conspicuously editing `release.yml`.
- **Single-pane debugging**: `gh run watch` shows the full pipeline; "skipped" status on downstream jobs makes failure mode obvious.
- **Selective re-run of failed jobs** via the GitHub UI ("Re-run failed jobs") works naturally — `bump-tap-formula` can be re-run independently after the publish job succeeded (US-12 idempotent retry).
- **K-PIPE measurement is unambiguous**: one `release.yml` run = one pipeline outcome.
- **US-14 ≤250-line constraint is satisfiable**: estimated ~180 lines in initial implementation.

### Negative

- **The single file grows over time**. Future additions (notarization, multiple binaries, additional targets) push line count toward the 250-line cap. Mitigation: `cargo xtask lint-workflows` enforces the cap; composite actions (`.github/actions/<name>/action.yml`) can extract repeated step sequences.
- **Re-running a single matrix cell is awkward**: GH Actions UI can re-run a failed cell, but if the cell's failure was non-deterministic (flaky test), the rest of the matrix has already finished — re-running the cell alone produces a partial state. Acceptable: per US-08, the maintainer simply retags rather than partial-re-running.
- **Cross-workflow reuse requires copy-paste** until/unless we adopt `workflow_call`. Acceptable for v1.

### Quality attribute impact

| Attribute | Impact |
|---|---|
| Reliability | **Positive** — atomicity is declarative, not imperative |
| Performance | **Neutral** — matrix parallelism preserved; same wall-clock time as multi-file approach |
| Maintainability | **Positive** — single file reads top-to-bottom; ≤250 lines |
| Observability | **Positive** — single pane, clear "skipped" status on downstream jobs |
| Operability | **Positive** — single retry surface; idempotent re-run of bump step |

## References

- DISCUSS `requirements.md` C2 (atomic publish), C8 (no silent skips)
- DISCUSS `user-stories.md` US-08 (atomic-publish guard), US-12 (idempotent retry), US-14 (≤250 lines)
- DESIGN `architecture-design.md` §4.2, §8.1
- DESIGN `component-boundaries.md` §3
