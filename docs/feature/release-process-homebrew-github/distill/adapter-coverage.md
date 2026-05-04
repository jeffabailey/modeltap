# Adapter Coverage Audit — release-process-homebrew-github

**Mandate:** Mandate 6 from `nw-test-design-mandates` — every driven adapter has at least one `@real-io` integration scenario or a documented `@requires_external` smoke.
**Wave:** DISTILL (5 of 6)
**WS Strategy:** C — Real local resources (DWD-01)

## Adapter Inventory (from DESIGN component-boundaries.md §2.3)

| Adapter | Wraps | Used by xtask subcommand(s) |
|---|---|---|
| `fs_adapter` | `std::fs::read_to_string`, `std::fs::write`, `tempfile` | every subcommand |
| `git_adapter` | `git status --porcelain`, `git rev-parse`, `git tag --list`, `git log --format`, `git push --force-with-lease`, `git checkout -B` | release-prep, validate-tag, extract-changelog, bump-tap-formula |
| `cargo_adapter` | `cargo metadata`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked`, `cargo update --workspace` | release-prep |
| `cliff_adapter` | `git-cliff --tag <X.Y.Z> --output CHANGELOG.md` | release-prep |
| `gh_adapter` | `gh release create`, `gh release view --json assets`, `gh pr list --head`, `gh pr create`, `gh pr merge --auto --squash`, `gh attestation verify` | release.yml steps (via xtask OR inline YAML) |
| `tera_adapter` (in-process, not shell-out) | `tera::Tera::new`, `tera::Context::new`, `tera::Tera::render` | render-formula |

Per the DESIGN, the formula renderer is in-process (Tera Rust crate), not a shell-out. It still counts as an adapter because it bridges pure logic (the `FormulaCtx` struct) to a textual side-effect.

## Coverage Matrix

| Adapter | `@real-io` scenario | Tagged scenario name | Source feature file | Notes |
|---|---|---|---|---|
| `fs_adapter` | YES | `xtask reads workspace version from real Cargo.toml` | `walking-skeleton.feature` | Real `tempfile::tempdir()` writes Cargo.toml; xtask reads. |
| `git_adapter` | YES | `release-prep refuses on dirty working tree` | `walking-skeleton.feature` | Real `git init` + `echo > untracked` in tempdir. |
| `git_adapter` (push semantics) | YES | `bump-tap-formula opens PR against ephemeral tap repo` | `walking-skeleton.feature` | Real `git init --bare` tap-fake; real push-with-lease via `file://` URL. |
| `cargo_adapter` | YES | `release-prep runs CI parity gates locally and exits zero on success` | `walking-skeleton.feature` | Real `cargo fmt`/`clippy`/`test` against a fixture workspace inside tempdir. Heavier scenario; tagged `@real-io @slow`. |
| `cliff_adapter` | YES | `release-prep regenerates CHANGELOG.md from conventional commits` | `walking-skeleton.feature` | Real `git-cliff` against seeded commit history in tempdir repo. |
| `tera_adapter` | YES | `xtask render-formula produces single-platform formula for walking skeleton` | `walking-skeleton.feature` | Real `release/templates/modeltap.rb.tera` template; real Tera engine. |
| `tera_adapter` (4 blocks) | YES | `Formula renders all 4 platform blocks with sha256s read from artifact files` | `multi-arch-release.feature` | Real Tera + 4 fixture `.sha256` files. |
| `gh_adapter` (release create) | NO — costly external | `xtask shells out to gh release create with correct arguments` | `walking-skeleton.feature` | Tagged `@requires_external`; covered by `@infrastructure-failure @in-memory` scenario for argument-shape testing. |
| `gh_adapter` (pr create) | NO — costly external | `bump-tap-formula opens PR titled correctly` | `walking-skeleton.feature` | Same pattern as above. |
| `gh_adapter` (auto-merge) | NO — costly external | `auto-merge fires when brew test-bot is green` | `hands-off-automation.feature` | `@requires_external` only; observable outcome on the live tap repo. |
| `gh_adapter` (attestation verify) | NO — costly external | `every published archive carries a verifiable attestation` | `multi-arch-release.feature` | `@requires_external`; verification IS the contract test per DESIGN §7.3. |
| `cross` Docker | NO — needs Docker | `aarch64-linux cell cross-compiles successfully` | `multi-arch-release.feature` | `@requires_docker`; native ubuntu cells cover archive-shape correctness. |
| `brew test-bot` | OUT OF SCOPE | (not tested by this suite) | — | Tap repo's own CI owns this; consumer-driven contract per DESIGN §7.3. |

## Audit Verdict

- **6 of 7 adapters** with a local equivalent have `@real-io` scenarios in the WS (or in R1 for the 4-block render).
- **All 4 costly-external paths** have `@requires_external` smoke scenarios + at least one `@infrastructure-failure @in-memory` scenario for error-shape coverage.
- **Docker** path has `@requires_docker` smoke + skipped-when-unavailable behavior.
- **brew test-bot** is explicitly out of scope per DESIGN §7.3 (consumer-driven contract owned by the tap repo's CI).

No driven adapter lacks coverage. Mandate 6 PASSES.

## Anti-Pattern Check (Mandate 5 + Mandate 7)

- WS scenarios under Strategy C use `@in-memory`? **NO** — grep `@walking_skeleton @in-memory` in `walking-skeleton.feature` returns zero matches.
- Adapter integration tests substitute mocks for real I/O where real I/O is feasible? **NO** — `git_adapter`, `fs_adapter`, `cliff_adapter`, `tera_adapter` all use real local resources; `cargo_adapter` uses real `cargo` against a fixture workspace; `gh_adapter` is the only one mocked, and ONLY because live GH operations are costly externals.
- Walking-skeleton fixtures set up the EXPECTED OUTPUT instead of preconditions? **NO** — Given clauses set up `Cargo.toml` content, commit history, template files, ephemeral tap repos. Then clauses assert on rendered formulas, branch state, exit codes — observable outcomes from the xtask binary.
