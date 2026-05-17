# Acceptance Review — tool-model-info-sqlite-cache

```yaml
review_id: "accept_rev_2026-05-17_tool-model-info-sqlite-cache"
reviewer: "acceptance-designer (self-review mode, Sentinel proxy)"
wave: "DISTILL (5 of 6)"
feature: "tool-model-info-sqlite-cache"
artifacts_reviewed:
  - "docs/feature/tool-model-info-sqlite-cache/distill/features/walking-skeleton.feature"
  - "docs/feature/tool-model-info-sqlite-cache/distill/features/cache-state-model.feature"
  - "docs/feature/tool-model-info-sqlite-cache/distill/features/tool-detail.feature"
  - "docs/feature/tool-model-info-sqlite-cache/distill/features/model-detail.feature"
  - "docs/feature/tool-model-info-sqlite-cache/distill/features/manual-refresh.feature"
  - "docs/feature/tool-model-info-sqlite-cache/distill/features/sha256-persistence.feature"
  - "docs/feature/tool-model-info-sqlite-cache/distill/features/integration-checkpoints.feature"
  - "docs/feature/tool-model-info-sqlite-cache/distill/acceptance-test-plan.md"
  - "docs/feature/tool-model-info-sqlite-cache/distill/step-definitions-skeleton.md"
  - "docs/feature/tool-model-info-sqlite-cache/distill/plugin-contract-spec.md"
  - "docs/feature/tool-model-info-sqlite-cache/distill/wave-decisions.md"

strategy_declaration: "Strategy B (real I/O against fixture-populated temp dirs) — declared in wave-decisions.md §D5. Inherits parent + sibling Strategy B verbatim."

scenario_inventory:
  total_scenarios: 40
  walking_skeleton_count: 1
  error_or_edge_count: 20
  error_path_ratio_pct: 50
  property_tagged_count: 3
  release_1_count: 10
  release_2_count: 27
  release_3_count: 3
  release_3_all_skipped: true

strengths:
  - "Strategy B declared in wave-decisions.md §D5 — walking skeleton uses real on-disk cache.sqlite + real in-process TestTool + real two-process launch; passes Dim 9a/b/c/d/e litmus."
  - "Walking skeleton title is a USER GOAL ('Devon's second launch shows yesterday's inventory instantly from cache'), not a technical flow — passes Dim 5 litmus. Devon can confirm 'yes, that's what I need.'"
  - "Error-path ratio 50% (20 of 40 scenarios) — significantly above 40% minimum per Dim 1. Driven by cache-state-model.feature (73% error/edge) and integration-checkpoints.feature (86% error/edge); these capture infrastructure-failure modes (corruption, downgrade, permission-denied, file-gone, mtime-drift) and cross-feature safety invariants (revalidator always called, --no-cache true bypass)."
  - "Full AC traceability matrix (acceptance-test-plan.md §6) maps every US-21..US-27 AC and every INT-INFO-1..9 to at least one scenario tag. Zero coverage gaps."
  - "Plugin contract spec extends the parent's existing pattern verbatim — adds inspect_tool and inspect_model contract paths for the 6 plugins (3 Supported, 3 Unsupported for inspect_tool; 4 Supported, 2 Unsupported for inspect_model). Reuses the parent's run_full_contract_suite shape; adds panic-isolation harness (3.12.S.3 / 3.13.S.6) explicitly for INT-INFO-8."
  - "ADR traceability via @adr-015 / @adr-016 / @adr-017 / @adr-018 tags on relevant scenarios — every architectural decision is exercised by at least one scenario."
  - "KPI traceability via @k-info-1-warm-100ms / @k-info-2-refresh-1s / @k-info-4-recovery-100 / @k-info-7-overhead-50ms / @k3a-warm-paint / @k3b-cold-start tags — every time-bounded KPI from outcome-kpis.md is encoded as an assertion."
  - "Release-slicing via @release-1 / @release-2 / @release-3 tags honors prioritization.md exactly. US-27 (Release 3) scenarios are uniformly @skip per ADR-018 implementation guidance; DELIVER unblocks them ONE AT A TIME post-Release-2 dogfooding."
  - "Pre-mutate revalidator (the central K5 safety invariant per ADR-015 §3) covered defense-in-depth: cache-state-model.feature drift/gone scenarios + integration-checkpoints Scenario Outline covering all 4 destructive actions (unify, zap, delete_one, folder_delete) + a dedicated INT-INFO-7 scenario for the folder-delete coordination with the sibling feature."
  - "Concurrent-process scenarios (US-23 Scenarios 4 and 5) launch two REAL modeltap processes — no in-process double-Connection simulation. This is the only way to validate that WAL lock files resolve across actual OS process boundaries (per wave-decisions.md §D8 rationale)."
  - "Real-IO walking-skeleton litmus test (Dim 9d): 'if we deleted the real modeltap-store adapter and substituted an InMemoryCache, would the WS still pass?' Answer: NO. Process B starts with no in-memory state; it must read cache.sqlite from disk. WS assertions read cache.sqlite file existence, PRAGMA user_version = 1, cache_models row presence — all fail without a real on-disk SQLite. Strategy B honored."

issues_identified:
  happy_path_bias: []
    # No issues. Error-path ratio 50% significantly exceeds 40% minimum.

  gwt_format:
    - issue: "Walking skeleton has 6 step actions in the When block (Devon runs modeltap, quits, a second process launches, etc.)"
      severity: "low"
      recommendation: "Acceptable for a walking-skeleton scenario per BDD-methodology rule 4 (3-5 step exception relaxed for end-to-end skeletons that necessarily span the full user journey including a process restart). Parent's modeltap-tui M1-equivalent and sibling's folder-group M1 have the same shape. NO ACTION."
    - issue: "cache-state-model.feature 'Production cache path resolves via XDG_DATA_HOME on Linux or Library/Application Support on macOS' uses 'Then ... Or ... on macOS' which is non-standard Gherkin"
      severity: "low"
      recommendation: "Acceptable as a documentation construct; DELIVER's step implementation will branch on `cfg!(target_os = ...)`. Alternative would be two scenarios (one per platform) gated by a `@platform-linux` / `@platform-macos` tag — DELIVER's call. Spec accommodates either. NO ACTION; flag for DELIVER reconfirmation."

  business_language:
    - issue: "Multiple scenarios reference 'PRAGMA user_version', 'SQLITE_CORRUPT', 'WAL', 'busy_timeout', 'cache.sqlite-wal', 'cache.sqlite-shm', '(mtime, size, inode, dev) quad'"
      severity: "low"
      recommendation: "Acceptable: these ARE the user-facing log lines (per ADR-015 §5 — diagnostics.log uses these terms verbatim) and the user-facing filenames in recovery banners (per ADR-015 §5). They are the business language at the cache-recovery boundary. Same convention as parent's K3 / K3a / K3b in TUI text. NO ACTION."
    - issue: "Plugin-contract scenarios reference 'Tool::inspect_tool()', 'InspectError::Unsupported', 'ModelDetail'"
      severity: "low"
      recommendation: "Acceptable: these are the PUBLIC type names that plugin authors (Riley persona, US-18) program against per ADR-016. They ARE the business language at the plugin-port boundary. Same convention as the sibling's ADR-010 Tool::delete_folder references. NO ACTION."
    - issue: "cache-state-model.feature 'permission denied reading ~/.ollama/models/manifests/ (errno 13)' uses 'errno 13'"
      severity: "low"
      recommendation: "Acceptable: this IS the user-facing error text surfaced verbatim to Devon in the Last error field of the tool detail screen per US-21 Scenario 3. Devon (the persona, a power user) reads errno values. Same convention as the sibling's 'EBUSY' / 'permission denied' in folder-delete error scenarios. NO ACTION."

  coverage_gaps: []
    # AC traceability matrix in acceptance-test-plan.md §6 shows every US-21..US-27 AC and every INT-INFO-1..9 traces to >= 1 scenario. AC-22-6 (BTreeMap<String,String>) and AC-23-12 (cache stays local; no network) are explicitly delegated to plugin-contract-spec and code review respectively — not coverage gaps, but explicit out-of-scope-for-E2E with cited alternative coverage. AC-26-8 (architecture-lint R9) is similarly delegated to DELIVER's tests/architecture.rs.

  walking_skeleton_centricity:
    - issue: "Walking skeleton title is 'Devon's second launch shows yesterday's inventory instantly from cache' — passes the Dim 5 litmus"
      severity: "pass"
      recommendation: "PASS. Title describes USER GOAL (instant warm-start, not technical flow). Then steps describe USER OBSERVATIONS (right pane shows the model, summary bar shows 'as of <N> seconds ago'). Non-technical Devon can confirm 'yes, that is what I need — modeltap should remember my inventory between launches'. NO ACTION."

  observable_behavior:
    - issue: "Several scenarios assert on cache.sqlite contents directly via `Then cache_models contains exactly N rows for tool_id` (the @cache-introspection tag) — this reads internal SQLite state, not user-observable behavior"
      severity: "medium"
      recommendation: "PASS-WITH-NOTE. The cache.sqlite file IS a user-observable artifact per ADR-015 §4 — the docstring explicitly says 'users can sqlite3 it'. The @cache-introspection tag marks the scenarios where SQLite-shape assertions are needed to prove the contract (e.g., walking-skeleton must assert PRAGMA user_version = 1 to prove the migration ran; the concurrent-write scenario must assert the final timestamp reflects process B's write to prove serialization correctness). These assertions are paired with user-observable assertions (TUI frame substrings, JSONL events) — they are the 'internal verification' of mechanisms that DO have user-observable consequences. The mandatory Dim 7 litmus 'asserts internal state instead of observable behavior' would reject these only if NO user-observable assertion accompanied them; in this distill, every @cache-introspection assertion is paired. ACCEPTABLE. DELIVER review can re-check during step-def implementation."
    - issue: "Plugin-contract scenarios assert on `metadata_kv.contains_key('general.architecture')` and `metadata_kv['model_type'] == 'llama'` — these are internal struct fields"
      severity: "medium"
      recommendation: "PASS-WITH-NOTE. These assertions are Layer B (plugin contract), not Layer A. The contract test exists to verify the plugin's TRAIT-LEVEL return value. At Layer A, the corresponding model-detail.feature scenarios assert on the TUI's rendered Metadata section substring (e.g., 'Then the Metadata section shows \"general.architecture : llama\"'), which IS user-observable. Defense in depth via two layers. ACCEPTABLE."
    - issue: "integration-checkpoints INT-INFO-4 Scenario Outline asserts 'the JSONL log shows a revalidate.invoked event with source = <action> before the action's outcome event' — JSONL is internal instrumentation"
      severity: "medium"
      recommendation: "PASS-WITH-NOTE. The JSONL log IS user-observable per the parent's instrumentation contract (kpi-instrumentation.md): Devon can `tail ~/.modeltap/launch.log` and see his actions. The revalidate.invoked event is part of the public instrumentation surface. Alternative framing — assert on the dialog text or the file-on-disk consequence — was considered for each destructive-action row. The dialog-text assertion is covered by the per-story scenarios (cache-state-model US-26 drift/gone). The file-on-disk consequence is covered by the parent's destructive-action E2E scenarios. INT-INFO-4 is the 'and the invariant holds for ALL 4 destructive actions' check; expressing it via JSONL events is the most compact way to assert the cross-cutting safety invariant. ACCEPTABLE."

  traceability_coverage:
    # Check A — Story-to-Scenario mapping:
    - issue: "Every US-21, US-22, US-23, US-24, US-25, US-26, US-27 ID has at least one @us-NN tagged scenario"
      severity: "pass"
      recommendation: "PASS — full story-to-scenario coverage per acceptance-test-plan.md §6 traceability matrix. NO ACTION."
    # Check B — Environment-to-Scenario mapping:
    - issue: "No docs/feature/tool-model-info-sqlite-cache/devops/environments.yaml exists (DEVOPS wave was skipped per wave-decisions.md §D4)"
      severity: "high"
      recommendation: "wave-decisions.md §D4 acknowledges skipped DEVOPS context. Per Dim 8 Check B default-fallback list (clean, with-pre-commit, with-stale-config), the equivalents for this feature are inherited from the parent: 'clean' is exercised by `devon-cache-empty` fixture; 'with-existing-cache' (stand-in for 'with-pre-commit') is exercised by `devon-cache-warm`; 'with-stale-config' is exercised by `devon-cache-stale-tool` and `devon-cache-future-v`. The walking skeleton + cache-state-model.feature scenarios collectively exercise all 4 of: no-cache, warm-cache, stale-cache, corrupted-cache. ACCEPTABLE per the brownfield additive precedent set by the sibling feature; downgrade to NOTE pending Sentinel confirmation."

  walking_skeleton_boundary:
    # 9a: WS Strategy Declaration
    - issue: "Strategy B declared in wave-decisions.md §D5"
      severity: "pass"
      recommendation: "PASS. NO ACTION."
    # 9b: WS Strategy-Implementation Match
    - issue: "Walking skeleton uses @real-io @adapter-integration @cache-introspection tags; no @in-memory tags anywhere on @walking-skeleton scenarios. Walking skeleton uses real on-disk cache.sqlite (not :memory:), real two-process launch, real TestTool plugin registration through MODELTAP_TEST_PLUGINS env var."
      severity: "pass"
      recommendation: "PASS. NO ACTION."
    # 9c: Adapter Integration Coverage
    - issue: "Every NEW driven adapter has at least one @real-io @adapter-integration scenario (per wave-decisions.md §D5 audit table)"
      severity: "pass"
      recommendation: "PASS. Cache::open + Cache::write_tool + Cache::write_models + Migrator + dirs::data_dir resolver covered by walking-skeleton + cache-state-model.feature. Cache::verify_against_fs covered by every @us-26 scenario. OllamaPlugin::inspect_*, HfPlugin::inspect_*, LmStudioPlugin::inspect_*, llama-cli inspect_model covered by tool-detail.feature + model-detail.feature. NO ACTION."
    # 9d: WS Fixture Tier
    - issue: "Walking skeleton's process B reads from real on-disk cache.sqlite via real Cache::open()"
      severity: "pass"
      recommendation: "PASS — litmus test 'if we deleted the real modeltap-store adapter and substituted an InMemoryCache, would the WS still pass?' Answer: NO (process B has no in-memory state; assertions read cache.sqlite from disk via test-only rusqlite::Connection and JSONL warm_paint_ms event from real launch.log). NO ACTION."
    # 9e: Strategy Drift Detection
    - issue: "Grep for @in-memory across all 7 feature files returns zero hits on @walking-skeleton scenarios (and zero hits anywhere — this feature does not use @in-memory at all)"
      severity: "pass"
      recommendation: "PASS. NO ACTION."

mandate_compliance:
  CM-A_hexagonal_boundary:
    status: "PASS"
    evidence: "All E2E scenarios invoke through the modeltap binary (driving port). Plugin Contract tests invoke Tool trait method inspect_tool / inspect_model (public plugin port per ADR-016). NO scenario imports modeltap-store::* directly EXCEPT for the @cache-introspection-tagged Then-step assertions that open a READ-ONLY rusqlite::Connection to verify PRAGMA user_version and row counts — this is permitted because the SQLite file IS a user-observable artifact (per ADR-015 §4) AND every such assertion is paired with a user-observable assertion (TUI frame substring or JSONL event). Verified by inspection of features/*.feature and step-definitions-skeleton.md. Acceptance-test-plan.md §1 explicitly documents the cache-introspection seam."

  CM-B_business_language:
    status: "PASS"
    evidence: "Step phrases use Devon's vocabulary: 'Devon runs modeltap', 'the cache contains the inventory from the previous launch', 'the summary bar reads', 'the recovery banner appears', 'Devon presses Shift+R', 'the cache was corrupted', 'the model file no longer exists'. Technical terms (PRAGMA user_version, WAL, busy_timeout, SQLITE_CORRUPT, (mtime, size, inode, dev) quad) are confined to where they ARE the user-facing log lines, recovery-banner contents, or filenames per ADR-015 §5. Plugin-port type names (Tool::inspect_tool, InspectError::Unsupported, ModelDetail) appear at the plugin-contract boundary where they ARE the public contract per ADR-016. No HTTP, database, endpoint, function, method-call, class, assert_eq!, unwrap, or other forbidden terms in feature files. Verified by grep over features/."

  CM-C_user_journey_completeness:
    status: "PASS"
    evidence: "Walking skeleton traces a complete journey: trigger (user runs modeltap) -> business logic (discover + write to cache + quit + relaunch + warm-read) -> observable outcome (right pane shows model, summary bar shows freshness) -> business value (the cache persists; modeltap remembers across launches). All focused scenarios anchored to user observable outcomes (TUI frames, recovery banners, dialog text, exit codes, JSONL events). Pre-mutate revalidation scenarios assert observable consequences (action proceeds / aborts / refreshes), not internal call counts."

  CM-D_pure_function_extraction:
    status: "PASS"
    evidence: "DESIGN (architecture-design.md §5 + component-boundaries.md) already separates pure logic from impure I/O at the seam between modeltap-core / modeltap-store. Architecture-lint R7 (only modeltap-app depends on modeltap-store), R8 (modeltap-store has no tokio/ratatui), R9 (every mutation site preceded by pre_mutate) — DELIVER-owned but specified in design. Acceptance tests (Layer A) exercise impure I/O through the binary; DELIVER's unit tests (Layer D) exercise pure logic directly without fixtures. NO fixture parametrization at the acceptance layer beyond the named-fixture-tree level. Pure functions inventory in acceptance-test-plan.md §9 + step-definitions-skeleton.md §J.5."

approval_status: "approved"

approval_notes: |
  All 9 critique dimensions evaluated. Zero blocker findings. Three medium findings on
  observable_behavior (Dim 7) — all resolved as PASS-WITH-NOTE:
    - The @cache-introspection assertions read SQLite directly because the SQLite file
      IS a user-observable artifact per ADR-015 §4; every such assertion is paired with
      a user-observable TUI/JSONL assertion.
    - Plugin-contract `metadata_kv` field assertions are Layer B, not Layer A; the
      Layer A model-detail.feature scenarios assert on the rendered Metadata section
      substring.
    - INT-INFO-4 JSONL assertion is the most compact cross-cutting safety check;
      individual per-story scenarios cover dialog/file-on-disk consequences.

  One high finding on traceability_coverage (Dim 8 Check B) is environmental — DEVOPS
  artifacts do not exist for this feature per wave-decisions.md §D4. The fallback
  environments (clean, with-existing-cache, with-stale-config, with-corrupted-cache) are
  exercised by the per-scenario fixture choices (devon-cache-empty, devon-cache-warm,
  devon-cache-stale-tool, devon-cache-corrupt, devon-cache-future-v). Acceptable;
  Sentinel proxy reviewer downgrades to NOTE pending PA-reviewer confirmation.

  All four mandates (CM-A through CM-D) pass with cited evidence.

  Walking-skeleton boundary proof (Dim 9): all five sub-dimensions pass.

  Approval is contingent on DELIVER:
  1. Implementing the `MODELTAP_TEST_PLUGINS=test-tool` env-var seam and the in-process
     `TestTool` plugin under `cfg(any(test, feature = "test-harness"))` (acceptance-test-plan.md §3,
     wave-decisions.md §D12).
  2. Implementing the `MODELTAP_CACHE_AGE_OVERRIDE` env-var seam under the same cfg gate
     (acceptance-test-plan.md §3).
  3. Implementing the `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS` flag under the same cfg gate
     for the concurrent-write-contention scenario (wave-decisions.md §D8).
  4. Choosing between `MODELTAP_OLLAMA_API_URL` stub server and `MODELTAP_OLLAMA_VERSION`
     env-var short-circuit for Ollama's `inspect_tool` HTTP call (wave-decisions.md §D12).
     Spec accommodates either choice.
  5. NOT unskipping any `@release-3` scenario until Release 2 has dogfooded and US-27 is
     unblocked per prioritization.md.
  6. Implementing the architecture-lint R9 invariant in `tests/architecture.rs` as
     specified by ADR-015 §"Enforcement". Layer A scenarios complement R9 by proving
     the revalidator is wired correctly at the user-observable level; the lint catches
     static violations.

handoff_to: "nw-software-crafter (DELIVER wave)"
handoff_artifacts:
  - "features/walking-skeleton.feature (1 scenario; WS exit gate)"
  - "features/cache-state-model.feature (15 scenarios; US-23/US-25/US-26 infrastructure)"
  - "features/tool-detail.feature (5 scenarios; US-21)"
  - "features/model-detail.feature (5 scenarios; US-22)"
  - "features/manual-refresh.feature (4 scenarios; US-24)"
  - "features/sha256-persistence.feature (3 scenarios; US-27; all @release-3 @skip)"
  - "features/integration-checkpoints.feature (7 scenarios + 4-row Scenario Outline; INT-INFO-1..9)"
  - "acceptance-test-plan.md (additive over parent + sibling)"
  - "step-definitions-skeleton.md (NEW step phrases only; parent + sibling inherited)"
  - "plugin-contract-spec.md (inspect_tool + inspect_model contract 3.12.U.1 + 3.12.S.1..3 + 3.13.U.1 + 3.13.S.1..6)"
  - "wave-decisions.md (D1..D13)"
  - "this acceptance-review.md"
```
