# Wave Decisions Summary — release-process-homebrew-github (DISTILL)

DISTILL wave (wave 5 of 6) decisions, deviations from defaults, and handoff state.

## Wave Configuration

| Setting | Value | Rationale |
|---|---|---|
| Format | feature files + step skeletons + traceability + adapter coverage + WS strategy | Per nw-acceptance-designer default for an infrastructure feature with cross-repo seam |
| Auto mode | active | Inherited from `/nw:new` wizard via DISCUSS / DESIGN |
| Peer review | nw-acceptance-designer-reviewer (Sentinel) | Hard gate per workflow |

## Personas (inherited from DISCUSS / DESIGN)

- **Jeff Bailey** — single maintainer; primary persona.
- **Devon Park** — end-user installer (reused from `modeltap-tui`).
- **Riley Chen** — open-source contributor (reused from `modeltap-tui`).

No new personas introduced in DISTILL.

## Decisions Resolved in DISTILL

These extend the DESIGN decisions (DESIGN-01..08) and resolve test-design choices.

| ID | Decision | Choice | Rationale |
|---|---|---|---|
| **DWD-01** | **Walking-skeleton strategy** | **Strategy C — Real local resources** | All adapters in DESIGN are local subprocess + filesystem + git. WS uses real `tmp_path`, real `git init`, real subprocess invocation, real Tera render. Costly externals (`gh release create`, `gh pr create`, `gh attestation verify`, `cross` Docker, `brew test-bot`) are tagged `@requires_external` or `@requires_docker` and substituted via local-fake-remote (`file://` URL pointing at ephemeral tap-repo). |
| DWD-02 | Cross-repo seam test architecture | Two ephemeral git repos in `tempfile::tempdir()`: one as fake "modeltap repo" with seeded `Cargo.toml` + `CHANGELOG.md` + `release/templates/modeltap.rb.tera`; one as fake "tap repo" initialized as bare-or-non-bare with `--bare` so push-with-lease works. `bump-tap-formula` is exercised by pointing `GH_TAP_TOKEN`-bearing checkout at the local `file://` URL. Real `gh pr create` is bypassed by a thin shim (or skipped under `@requires_external`). | Avoids live GitHub. Faithful enough — proves git operations, formula content, branch state, idempotent retry. |
| DWD-03 | Workflow execution | `release.yml` end-to-end execution is OUT OF SCOPE for acceptance tests. Workflow correctness is verified by `xtask lint-workflows` (line count, purpose comments, `needs:` graph well-formedness via YAML parse) + a `@requires_external` smoke runnable via `nektos/act`. | Per Mandate 1 (driving ports): `release.yml` IS a driving port shape — but exercising it requires GH Actions runners. The xtask lint is the boundary we own; the workflow itself is a configuration artifact. |
| DWD-04 | DEVOPS-missing graceful degradation | Default environment matrix from instructions: macOS-14 (ARM), macOS-13 (Intel), ubuntu-22.04 x86_64, ubuntu-22.04 aarch64-cross + tap-repo state matrix (fresh / existing / stale). Mandate 4 (Pure Function Extraction) reduces fixture parametrization to the adapter layer only. Recorded as a warning at the top of `test-scenarios.md`. | Per skill graceful degradation rule: log + proceed; reconcile in DELIVER if DEVOPS contradicts. |
| DWD-05 | Test crate placement | New crate `tests/acceptance/release_process/` at repo root (not under `xtask/tests/`). xtask owns its unit tests; acceptance tests live where the journey crosses `xtask` + `release.yml` + cross-repo seam. | Mirrors `modeltap-tui`'s acceptance placement convention; keeps acceptance suite portable across crates. |
| DWD-06 | Property-based scenarios | 4 scenarios tagged `@property`: monotonic version invariant; sha256 length/charset invariant; idempotent bump roundtrip; render-formula determinism. DELIVER software-crafter implements as proptest generators. | Universal invariants ("for any valid X, Y holds"); single-example scenarios would underspecify. |
| DWD-07 | RED scaffolds | All 5 xtask subcommand entrypoints + 5 lib modules created with `panic!("Not yet implemented — RED scaffold")` and `// SCAFFOLD: true` marker (Rust convention). `xtask` declared as workspace member, EXCLUDED from default-members per ADR-011 via `default-members` enumeration. | Per Mandate 7: imports must succeed (no BROKEN classification); first acceptance run produces RED, not BROKEN. |

No DISTILL decision contradicts any DISCUSS, DESIGN, or constraint (C1-C8).

## Phase Execution Summary

| Phase | Activity | Status |
|---|---|---|
| Phase 1 | Skill load + prior-wave reading + WS strategy detection | COMPLETE |
| Phase 2 | Scenario design (WS + R1 + R2 + integration + property) | COMPLETE |
| Phase 3 | Test infrastructure scaffolding (xtask RED + step skeletons) | COMPLETE |
| Phase 4 | Self-review + peer review + handoff prep | COMPLETE |

## Wave Handoff Package

### To DELIVER (software-crafter)

**Inputs:**
- All DISTILL artifacts under `docs/feature/release-process-homebrew-github/distill/`
- All DESIGN + DISCUSS artifacts (already loaded)
- xtask scaffolds at `xtask/src/{main,lib,formula,cargo_toml,changelog,lint,tag}.rs`

**Implementation order (one-at-a-time, outside-in):**
1. WS scenario `xtask validate-tag accepts matching tag` — easiest pure-function red→green.
2. WS scenario `xtask render-formula produces single-platform formula` — drives Tera adapter wiring.
3. WS scenario `xtask extract-changelog returns the section text` — drives changelog adapter.
4. WS scenario `release-prep refuses dirty tree` — drives git adapter via real tempdir repo.
5. WS scenario `bump-tap-formula opens PR against ephemeral tap` — drives the cross-repo seam.
6. R1 scenarios (matrix, atomic-publish guard, SLSA, 4-block formula).
7. R2 scenarios (auto-merge, idempotent retry, runbook, lint).

**Mandate compliance evidence enclosed:**
- CM-A: every step file imports `xtask::*` (driving port — the CLI library API), no internal-component imports.
- CM-B: feature files contain zero technical jargon (see `acceptance-review.md` grep results).
- CM-C: 6 walking-skeleton scenarios + 18 focused + 6 integration + 5 infrastructure-failure + 4 property = 39 scenarios; ≥40% error/edge.
- CM-D: pure-function extraction inventory: `parse_workspace_version`, `assert_monotonic`, `assert_tag_matches`, `render` (Tera), `extract_section`, `lint`. All take strings/structs in, return strings/Results out. Adapter parametrization confined to `git_adapter`, `cargo_adapter`, `gh_adapter`, `cliff_adapter`, `fs_adapter`.

### Mutation testing target

Per `CLAUDE.md`: ≥80% kill rate against `xtask` pure-function modules (`xtask::version`, `xtask::formula`, `xtask::changelog`, `xtask::workflow_lint`). Adapter modules and `main.rs` excluded.

## Risks Surfaced

| Risk | Probability | Impact | Owner | Status |
|---|---|---|---|---|
| `nektos/act` does not perfectly emulate `actions/attest-build-provenance@v2` (OIDC) | Medium | Low | DELIVER | Acceptable — `@requires_external` smoke covers this path with real GH |
| Local `file://` git push semantics differ subtly from GH HTTPS push (e.g., `--force-with-lease`) | Low | Low | DELIVER | Mitigated by also running `@requires_external` cross-repo smoke |
| Tera template syntax error introduced via PR breaks render in production but not in unit tests | Low | Medium | DELIVER | Mitigated by `xtask::formula::render` round-trip property test (`@property`) |
| `cross` Docker image unavailable in test environments without Docker | High (in CI without Docker layer) | Low | DELIVER | `@requires_docker` tag + skip logic; native ubuntu cells cover the build correctness |

## DEVOPS Reconciliation Note

DEVOPS ran in parallel and was missing at DISTILL start. If DEVOPS produces an environment matrix or KPI instrumentation requirements that contradicts DWD-04 defaults, reconcile in DELIVER:
- New environments → add Given clauses to relevant WS scenarios.
- New observability requirements → add Then clauses asserting log output (mirror modeltap-tui's `@kpi-instrumentation` pattern).

## Cross-Feature Coupling Notes

| Coupling | Direction | Notes |
|---|---|---|
| `modeltap-app` binary | This pipeline ships it; US-15 verifies `modeltap --version` | The binary's version-string contract is owned by `modeltap-app` (clap `CARGO_PKG_VERSION`). |
| `modeltap-tui` `--version` behavior | US-15 delegates to it | `modeltap-tui` US-01 already shipped per `CLAUDE.md`. |
| `tests/acceptance/release_process/` test crate | New | Sibling to any future `tests/acceptance/modeltap_tui/` re-organization. |

No vocabulary conflicts (this feature: tag, release, tap, tap-bump, atomic-publish, CI parity gates, SLSA attestation, bump branch). No constraint conflicts.
