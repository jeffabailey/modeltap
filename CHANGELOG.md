# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added

- tool-model-info-sqlite-cache feature — US-21..US-26 closure (Phase 06)
- US-21 per-tool detail screen + plugin `inspect_tool` overrides (Ollama HTTP /
  HF cache-dir detection, llama-cli / lm-studio / atomic-chat / gpt4all
  return `InspectError::Unsupported` via trait default)
- US-22 per-model detail screen + `inspect_model` overrides (llama-cli GGUF
  header, lm-studio + Ollama + HF metadata variants), graceful-degradation
  panic isolation at orchestrator boundary (INT-INFO-8)
- US-23 SQLite-backed inventory cache (`modeltap-store` crate) — schema v0→v1
  migration, WAL + `busy_timeout=5000` PRAGMAs, opt-out via `--no-cache` and
  `[cache] enabled = false`, corruption / future-version / migration-failed
  recovery banner (AC-23-7), concurrent-process safety (AC-23-10)
- US-24 manual refresh hotkeys: `[r]` (selected tool, scope=Tool) and
  `[Shift+R]` (all tools, scope=All); dialog-gated as silent no-op
- US-25 warm-start cache paint (`launch.warm_paint_ms ≤ 150 ms` per K-INFO-1),
  per-tool TTL eligibility (24 h default), summary-bar provenance suffix
  ("as of 2 min ago", "reconciling...", "refreshing <tool>...")
- US-26 background reconcile orchestrator with atomic per-tool writes +
  last-known-good rollback on plugin failure; silent-ack indicator (3s blue
  `*`); inventory-diff pure function in `modeltap-core::logic::inventory_diff`;
  pre-mutate revalidator (K5) gating all four destructive sites
- Architecture lints R7 + R8 + R9 in `crates/modeltap-app/tests/architecture.rs`
  (Step 06-01)
- Mutation-kill tests for `modeltap-core::domain::inspect`,
  `modeltap-core::logic::inventory_diff`, and `modeltap-store::repo::tools`
  (Step 06-02) — 100% kill rate on the targeted core modules; ≥80% on the
  targeted store modules
- Phase 06 finalize regression gate
  (`tests/acceptance/regression_gate.rs`) — pins parent + sibling scenario
  counts, INT-INFO-2 vocabulary surfaces, and the US-27 `@release-3 @skip`
  invariant

### Changed

- ADR-003 (stateless rediscovery, v0.2.x) is superseded by ADR-015
  (SQLite-backed cache + pre-mutate revalidation). The cache is opt-out per
  AC-23-8 / AC-23-9; `--no-cache` and `[cache] enabled = false` preserve the
  ADR-003 path byte-for-byte (DirManifest invariant)
- `Cache::open` now sets `journal_mode=WAL`, `busy_timeout=5000`, and
  `foreign_keys=ON` PRAGMAs before any other query runs
- `modeltap-tui::screens::detail::DetailScreenState` gained an optional
  `metadata: Option<MetadataSection>` field for US-22 Metadata rendering;
  legacy US-13 callers pass `None`

### Fixed

- `crates/modeltap-store/tests/revalidate.rs` — seed parent `cache_tools` +
  `cache_models` rows before writing `cache_model_files` (FK constraint
  satisfaction; baseline test bitrot uncovered by step 06-02 mutation gate)
- `crates/modeltap-store/src/recovery.rs::compact_utc_timestamp_matches_*`
  test — expected length 15→17 chars (YYYY-MM-DDTHHMMSS with literal dashes
  in the date is 17 chars; the original test had the off-by-2 wrong)
- pre-existing clippy 1.91 lints across `modeltap-store`, `modeltap-tui`,
  `modeltap-app`, `plugins/lm-studio`, and the `tests` acceptance crate
  (doc-list indentation, large-enum-variant, redundant guard, redundant
  closure, cloned-ref-to-slice-refs, derivable-default, iter-any → contains,
  iterator-last → next_back, too-many-arguments)

### Deferred

- US-27 (persistent SHA256) is deferred to Release 3 per ADR-018; the 3
  scenarios in `sha256-persistence.feature` carry `@release-3 @skip` tags
  and the regression-gate test enforces it

### Known issues

- `crates/modeltap-store/tests/concurrent.rs::write_blocks_via_busy_timeout_and_succeeds`
  is `#[ignore]`-marked on macOS 15 / rusqlite 0.31 — BEGIN IMMEDIATE inside
  a single process under WAL does not serialise as the test expected. The
  subprocess-level acceptance test in `tests/acceptance/cache_concurrent.rs`
  exercises the AC-23-10 invariant against real shared-cache contention

## [0.2.6]

### Added

- per-tool PNG icons in left pane via ratatui-image
- add FolderGroup types + Tool::delete_folder default body (Step 01-01)
- add pure folder-group logic (group/classify/plan) (Step 01-02)
- add folder header + Shift+F keymap + folder confirm dialog (Step 01-04)
- implement delete_folder happy path (Step 01-03)
- orchestrate folder-delete + M1 walking-skeleton green (Step 01-05)
- add @property proptests for folder-group invariants (Step 03-03)
- wrong-path + trailing-slash + Esc cancel paths (Step 02-01)
- itemize mixed-folder dialog + post-action accounting (Step 03-01)
- pre-flight refusals for read-only cache + vanished folder (Step 04-03)
- folder-delete plugin-contract tests + M5 boundary (Step 05-01)
- Shift+F is no-op on non-HF tools + dim [F] indicator (Step 02-02)
- mixed-folder delete preserves cross-tool hardlinks (Step 03-02)
- partial-failure handling + EBUSY test seam (Step 04-01)
- idempotent folder-delete retry after busy resolved (Step 04-02)
- KPI instrumentation + keystroke-count + mis-target invariant (Step 06-01)
- integration checkpoints + feature exit gate (Step 06-02)
- show folder grouping in HF right pane (recovery step 01-06)
- folder collapse/expand with default collapsed (Step 01-07)

### Fixed

- keep bottom-bar within 80 cols by shortening [F] label

### Misc

- If a git tree falls in a forest and no one is around to hear it, does it make a sound?
- kill surviving mutants in classify_sidecar / path_starts_with_subdir
- If a git tree falls in a forest and no one is around to hear it, does it make a sound?

## [0.2.5]

### Added

- focus-aware up/down dispatch in left pane

### Misc

- consolidate focus-aware label assertions per reviewer feedback
- kill surviving mutants in keymap focus-aware dispatch
- archive arrow-keys-navigate-tools evolution

## [0.2.4]

### Fixed

- render-formula gate iterates published matrix not all kinds
- render delete-one confirmation dialog (RCA cause A)
- lift DeleteFromOne on Detail in interactive loop (RCA cause B)
- dim [d] on Main when no real tool is selected (RCA cause C)

### Misc

- archive fix-delete-one-hang RCA, roadmap, and DES log

## [0.2.3]

### Fixed

- install cross binutils before strip on aarch64-linux cell

## [0.2.2]

### Fixed

- inherit workspace license in xtask and modeltap-acceptance

### Misc

- Add license scan report and status
- Merge pull request #1 from fossabot/add-license-scan-badge
- Merge branch 'main' of github.com:jeffabailey/modeltap
- If a git tree falls in a forest and no one is around to hear it, does it make a sound?

## [0.2.1]

### Added

- add cut-release.sh one-shot release driver

### Fixed

- use cross-aware strip for aarch64-linux build cell
- drop x86_64-apple-darwin from published matrix

## [0.2.0]

### Added

- scaffold workspace + ratatui foundation (US-01)
- implement Ollama discovery + Tool trait + JSONL events (US-02)
- two-pane navigation + Elm update + plugin linker fix (US-03)
- zap-all with typed-name confirmation modal (US-05)
- post-action message + summary bar refresh (US-06; WS exit gate)
- row metadata indicator + also-in annotation (US-04)
- discover loose .gguf files with header parsing (US-07)
- walk hub/ snapshot symlink farm + linking spike (US-12)
- default + older path conventions + paths spike (US-15)
- format-aware compatibility indicator engine (US-09)
- per-model detail screen with lazy SHA256 + status (US-13)
- format-locked red ! indicator + WCAG NO_COLOR (US-16)
- bottom bar polish + help overlay + INT-6 invariant (US-08)
- per-tool link() impls + canonical selector + OQ-3 spike (US-10 partial)
- complete unify wiring + acceptance + close 03-02 (US-10)
- cross-filesystem fallback with per-target choice (US-19; closes 03-03)
- incremental post-action refresh + INT-5 invariant (US-11; closes 03-04)
- dry-run preview before unify with no-mutation guarantee (US-14; closes 03-05)
- single-model delete dialog state + update wiring (US-05b; closes 03-06)
- per-plugin delete_one + action orchestrator + acceptance (US-05b functional)
- detect-and-prompt-then-retry for running tools (US-17; closes 03-07; closes Phase 03)
- plugin trait certification + atomic-chat fixture (US-18; closes 04-01)
- cross-platform CI matrix + WSL/Windows handling (US-20; closes 04-02; closes Phase 04)
- wire production interactive crossterm event loop
- add 5th tool — real production plugin (Jan fork)
- bind [d] in main pane to delete the highlighted model
- add dedup-glyph and synthetic-slot domain types
- pure dedup-glyph + dedup-summary fns
- add dedup-glyph column to right-pane rows
- add hash pool Msg variants + pure update handlers
- implement background SHA256 hash pool (ADR-013)
- wire hash_pool into composition root with clean shutdown
- add <hash-complete> script harness sentinel
- route u-keypress on main view to glyph-aware dispatch
- implement reclassify_after_unify with summary_delta wiring
- activate cross-tool-model-unify walking skeleton
- unignore US-U2 dedup-able-bytes acceptance tests
- unignore US-U3 dedup-glyph acceptance tests
- unignore US-U4 u-from-main-view acceptance tests
- unify dialog reclaim preview with toggle (US-U5)
- add collect_unified_rows pure function (US-U7)
- render [All Unified] pseudo-slot in left+right panes (US-U7)
- unignore US-U7 [All Unified] acceptance tests
- render transient (was X) summary delta (US-U6)
- handle partial-success in reclassify_after_unify (US-U6)
- unignore US-U6 post-unify-no-restart acceptance tests
- [All Unified] empty-state guidance (US-U8)
- Detail-screen inode proof display (US-U9)
- partial-success toast with retry-failed-only (US-U10)
- scaffold modeltap-plugin-gpt4all crate skeleton
- add paths + config (env-var override per-OS defaults)
- implement discover() walking *.gguf files
- wire gpt4all plugin into composition root
- implement link/delete_one/delete_all (ADR-008 EXDEV fallback)
- implement parse_workspace_version + assert_monotonic
- assert_tag_matches and validate-tag CLI
- extract_section + extract-changelog CLI
- render-formula via Tera (single-platform WS)
- lint pure function + lint-workflows CLI
- release-prep with git, cargo, and cliff adapters
- authoring release.yml validate-tag/build/publish DAG
- bump-tap-formula job + WS exit gate
- expand build job to 4-target matrix with cross for aarch64-linux
- SLSA L3 build provenance attestation per archive
- render 4-platform formula with TargetKind dispatch
- auto-merge tap-bump PR via gh pr merge --auto
- idempotent bump-tap-formula via force-push-with-lease
- follow-up workflows for K-PIPE alerting + token expiry
- close DELIVER Phase 2 — ci.yml lint + README + recovery

### Fixed

- viewport-aware scrolling on both panes
- wire summary_bar Dedup-able to dedup_summary
- update plugin-count assertions and env carving for gpt4all (5th plugin)
- satisfy rustfmt --all and clippy 1.95.0 manual_checked_ops
- clippy unnecessary_sort_by + cargo-deny config discovery
- ignore RUSTSEC-2024-0436 (paste unmaintained, transitive via ratatui)

### Changed

- L1-L4 sweep across modeltap-tui v1 implementation
- AppState.tools → AppState.left_pane_slots
- consolidate 7 duplicate format_size into bytes::format_bytes
- collapse redundant self-skip + harden mutation coverage
- harden mutation coverage to 100% kill rate
- consolidate fixture helpers + fix proptest semver generator

### Documentation

- RELEASING.md runbook with release-log table

### Misc

- snapshot waves DISCUSS/DESIGN/DEVOPS/DISTILL artifacts before DELIVER 01-01
- finalize modeltap-tui v1 — archive evolution + phase artifacts
- If a git tree falls in a forest and no one is around to hear it, does it make a sound?
- ignore cargo-mutants output and coverage artifacts
- wave artifacts — discuss/design/distill baseline
- cargo fmt --all post step 01-02
- add project_id mirror of feature_id for stop-hook compat
- verify US-U5 apply+esc tests pass (no-op marker)
- archive cross-tool-model-unify evolution + adversarial review
- record 100% mutation kill rate after dedup.rs hardening
- If a git tree falls in a forest and no one is around to hear it, does it make a sound?
- add us_gpt4all_discovery acceptance scenarios
- add us_gpt4all_cross_tool_unify acceptance scenario
- archive gpt4all-plugin evolution + adversarial review
- pin macOS linker to /usr/bin/cc + xtask alias
- assert atomic-publish guard via needs DAG + proptest
- multi-arch e2e + cross-artifact version consistency
- close mutation-test kill-rate gap to 100% on xtask pure modules
- archive release-process-homebrew-github wave artifacts
- v0.2.0
- persist proptest seed for version-consistency invalid-semver regression

