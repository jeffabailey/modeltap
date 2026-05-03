# Adversarial Review — gpt4all-plugin

**Reviewer**: nw-software-crafter-reviewer
**Date**: 2026-05-03
**Scope**: 9 commits between `1fb2bd7` and `HEAD` (gpt4all-plugin DELIVER wave)

## Verdict

**APPROVED** — 0 critical, 0 major, 1 minor (non-blocking observation)

## Summary

- All 10 acceptance criteria (AC-G1.1 through AC-G5.1) exercised and passing
- 100% mutation kill rate (53/53 viable killed; 16 unviable)
- 627+ workspace tests green; zero failing
- ADRs 001 / 005 / 008 / 013 / 014 all adhered to
- Zero testing-theater patterns
- Architecture lint clean (modeltap-core stays plugin-free; inter-plugin deps clean; `inventory::submit!` correctly placed in lib.rs)
- Cross-tool dedup value proposition proven end-to-end (GPT4All + Ollama unify to single inode)

## Findings

### Critical

(none)

### Major

(none)

### Minor

**M1 — Step execution sequence reordering (non-blocking observation)**

Step 01-06 (plugin-count assertion sweep) executed BEFORE step 01-05 (link/delete impl) per execution-log timestamps, despite roadmap declaring 01-05 listed first lexically. Both steps depend on 01-04; orchestrator chose 01-06 first to restore green workspace immediately after 01-04's wiring deliberately broke 3 v1 count assertions. This is valid Outside-In TDD discipline (RED-then-GREEN at the workspace level) and respects all physical (commit-output) dependencies.

**Recommendation**: For future roadmaps, document that lexical step ordering is advisory; physical dependency satisfaction is the binding constraint.

## Testing Theater Scan (7-Pattern)

| Pattern | Found |
|---------|-------|
| Zero-assertion test | NONE |
| Tautological assertion | NONE |
| Mock-dominated SUT | NONE |
| Circular verification | NONE |
| Always-green (suppressed failures) | NONE |
| Fully-mocked SUT | NONE |
| Assertion-free smoke test | NONE |

The mutation-hardening pass (`f82cfd5`) added 19 tests; each pins a specific mutant (e.g., "EXDEV errno 18 must map to CrossFilesystem, not Io") with structural-guard assertions, not implementation mirrors. All would fail if the production logic were removed or stubbed.

## ADR Adherence

| ADR | Decision | Status |
|-----|----------|--------|
| ADR-001 | Tool trait frozen at 6 methods; gpt4all is `impl Tool` only | ✓ PASS |
| ADR-005 | Sync I/O wrapped in `tokio::task::spawn_blocking` | ✓ PASS |
| ADR-008 | EXDEV → `LinkError::CrossFilesystem` (no auto-copy in plugin) | ✓ PASS |
| ADR-013 | GPT4All blobs flow through hash pool unchanged | ✓ PASS (verified by `us_gpt4all_cross_tool_unify`) |
| ADR-014 | No new synthetic slots introduced | ✓ PASS |

## Architecture Lint

| Rule | Status |
|------|--------|
| modeltap-core has NO plugin-crate deps | ✓ PASS |
| Plugin crate inter-deps clean (no path-deps on sibling plugins) | ✓ PASS |
| `inventory::submit!` lives in lib.rs (not a submodule) | ✓ PASS |
| `us_18_plugin_trait` count assertion correctly bumped 5→6 (not weakened) | ✓ PASS |
| `tools_registered` ordering: `["Atomic Chat", "atomic-chat", "gpt4all", "hf", "lm-studio", "ollama"]` | ✓ PASS |

## Acceptance Coverage

| AC | Test | Status |
|----|------|--------|
| AC-G1.1 | us_gpt4all_discovery::gpt4all_models_are_discovered | ✓ |
| AC-G1.2 | us_gpt4all_discovery (size_bytes assertions) | ✓ |
| AC-G1.3 | us_gpt4all_discovery::gpt4all_not_installed_shows_benign_message | ✓ |
| AC-G1.4 | discover.rs::discover_skips_non_gguf_files_and_dotfiles | ✓ |
| AC-G1.5 | us_gpt4all_discovery::gpt4all_two_dirs_both_walked | ✓ |
| AC-G2.1 | us_gpt4all_cross_tool_unify (post-unify inode equality) | ✓ |
| AC-G2.2 | mutation_test::link_creates_hardlink_so_canonical_and_target_share_inode | ✓ |
| AC-G3.1 | mutation_test::delete_one_removes_file_and_reports_freed_bytes + delete_all_removes_every_gguf | ✓ |
| AC-G4.1 | config tests (env_paths_replace_defaults) | ✓ |
| AC-G5.1 | us_18_plugin_trait (registry includes gpt4all in alphabetical order) | ✓ |

## External Validity

- Driving port: acceptance tests invoke the production `modeltap` binary via `Command::cargo_bin("modeltap")`
- Driven port: assertions read JSONL (launch.log, models.log) + filesystem state (inode equality)
- No internal-class testing in acceptance tests; unit tests test helpers via the `Tool` public API
- Hardcoded defaults resolved via paths.rs unit tests; integrated via headless discovery in acceptance tests with env-var override

## Recommendations

1. **Documentation**: Note that lexical step order in roadmap.json is advisory; physical dependencies are binding (relevant to future roadmaps).
2. **WSL**: Paths cover macOS + Linux branches; WSL works by construction (no additional testing needed unless a CI WSL environment becomes available).
3. **Future**: LoRA support could be added as a second discovery root or separate format flag — current architecture supports without core changes.

## Verdict

**APPROVED** — Zero blocking issues. Plugin is production-ready.
