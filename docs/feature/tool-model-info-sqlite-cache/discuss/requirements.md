# Requirements — tool-model-info-sqlite-cache

## Feature Identity

- **Feature ID:** `tool-model-info-sqlite-cache`
- **Wave:** DISCUSS (wave 2 of 6 for this feature)
- **Source brief:** `docs/feature/tool-model-info-sqlite-cache/intake/intake-brief.md`
- **Parent feature:** `modeltap-tui` (DELIVER wave, US-01..US-20 + US-05b shipped)
- **Related in-flight feature:** `folder-group-bulk-delete` (US-05c, DELIVER wave) — see `prioritization.md` for sequencing recommendation
- **Type:** cross-cutting (introduces a new bounded context `modeltap-store`, extends the `Tool` trait, supersedes ADR-003)

## Supersedes prior constraint (CRITICAL — load-bearing)

The parent feature's **ADR-003 — State Model: Stateless Rediscovery, No Persistent Index** is **superseded by this feature**. The intake brief is unambiguous:

> "Let's also refactor this code so it stores information about all the data collected from the tools and models in a local sqlite database."

The user is explicitly directing modeltap to add a persistent index. This reverses Q7 (intake), which read:

> "Stateless rediscovery on every launch. No persistent index file. Each launch walks every tool's directory."

### Rationale for the reversal (now warranted)

The reversal is warranted now (not at original DISCUSS) because:

1. **Real-world dogfooding revealed cumulative launch cost.** modeltap is opened many times per day; the ~1.15 s discovery cost per launch is acceptable in isolation but cumulative across a workflow.
2. **Inspection feature (J1, J2) raises the bar for metadata freshness across launches.** Detail screens that re-introspect on every open feel slow; persisted metadata feels instant.
3. **SHA256 lazy-compute (ADR-002) thrown away on quit is wasteful** for users with large libraries. Persisting hashes is a J5 opportunity that was rejected for v1 partly because of cache-invalidation complexity — that complexity is now justified by the inspection feature's value.
4. **The cache's safety risks are addressed by an explicit design rule** ("cache is paint-only; filesystem is authoritative on mutate") that did not exist when ADR-003 was written.

### What the new ADR (DESIGN owns) must close

A new ADR-NNN superseding ADR-003 must enumerate:

1. **Cache location:** `$XDG_DATA_HOME/modeltap/cache.sqlite` via `dirs::data_dir()` (Q-INFO-2).
2. **Schema migration strategy:** `rusqlite_migration` recommended (Q-INFO-3 closed in `prioritization.md`).
3. **Refresh policy:** warm-start paint + background reconcile per-tool + per-tool TTL (default 24h) + manual `[r]` / `[Shift+R]` + pre-mutate revalidation (closed in this document; ADR confirms).
4. **Cache safety rule:** filesystem is authoritative on mutate; cache re-validation via `(mtime, size, inode_dev)` before any destructive action (closed in this document; ADR confirms).
5. **Concurrency model:** SQLite WAL + `busy_timeout=5000` (Q-INFO-6).
6. **Corruption recovery:** auto-detect → rename to `.corrupt-<timestamp>` → cold-start fallback → banner + log (closed in this document; ADR confirms).
7. **Opt-out:** `--no-cache` CLI + `cache.enabled = false` config; CLI wins (closed in this document; ADR confirms).
8. **Tool trait extension:** required vs default-impl for `inspect_tool()` and `inspect_model()` (Q-INFO-1).
9. **SHA256 cache validity check:** `(mtime, size, inode_dev)` tuple match for cache reuse; any drift → rehash (for US-27 in Release 3).

## Domain Glossary (new terms; extends parent)

| Term | Definition |
|---|---|
| **cache** | The SQLite file at `$XDG_DATA_HOME/modeltap/cache.sqlite` (or the equivalent macOS path) that persists tool and model metadata across launches. NOT a source of truth — tool directories remain the source of truth. |
| **warm start** | Launch where the cache file exists, is valid, and contains usable inventory data. First paint reads from cache; background reconcile updates the cache. |
| **cold start** | Launch where the cache file is empty, missing, or recovering from corruption. Behaves identically to ADR-003 (skeleton paint + parallel discovery). |
| **reconcile** | Per-tool re-discovery pass that compares filesystem state against the cache and updates cached entries. Runs in background after warm-start paint; runs on `[r]` / `[Shift+R]`. |
| **provenance** | The "as of <timestamp>" line in the summary bar that tells Devon when the displayed inventory was last reconciled with the filesystem. |
| **TTL (per-tool)** | The maximum age of a cache entry that is still eligible for warm-start paint. Default 24 hours. Older entries are NOT painted from cache; their tool cold-starts. |
| **introspect** | The per-plugin act of calling `Tool::inspect_tool()` or `Tool::inspect_model()` to gather tool-native metadata (Ollama manifest fields, GGUF header KVs, HF `config.json` excerpts) beyond what `discover()` already returns. |
| **re-introspect** | The action of pressing `[r]` on a detail screen to re-run `inspect_*()` for the current target and refresh its metadata. |
| **pre-mutate revalidation** | The cache-safety rule: before any destructive filesystem action (unify, zap, delete-one, folder-delete), the targeted file's `(mtime, size, inode_dev)` is re-checked against the cache. Drift → re-introspect; file gone → abort + refresh. |
| **tool detail screen** | The screen entered by pressing Enter on a left-pane row. Shows install path, version, model count, disk usage, last scan, plugin version, configured search paths, and last error if any. |
| **model detail screen (extended)** | The existing US-13 detail screen with an additional Metadata section that surfaces tool-native introspection. Entered by pressing Enter on a right-pane row. |
| **cache recovery** | The act of detecting a corrupted or schema-mismatched cache, renaming it to `.corrupt-<timestamp>`, logging the event, and falling back to cold-start. |
| **schema version** | `PRAGMA user_version` value in the cache file; compared against the binary's `EXPECTED_SCHEMA_VERSION` at launch. |

## Stakeholders

| Stakeholder | Role | Engagement |
|---|---|---|
| Devon Park (primary user) | Multi-tool local-AI power user (parent persona) | Validates user-facing stories US-21, US-22, US-24; drives J1, J2, J3, J4 KPIs |
| Maintainer / solo developer (Jeff Bailey) | Project owner; this feature's developer | Owns trait extension stability; owns ADR superseding ADR-003 |
| Contributor (Riley Chen — parent persona) | Open-source contributor adding new tools | Affected by Q-INFO-1 (Tool trait extension); needs minimum-friction default-impl |
| Tool authors (Ollama, llama-cpp, HF, LM Studio) | Out-of-band stakeholders | Inspection feature reads their manifest/header formats; format drift is a maintenance risk (low probability, captured in risks) |

## Functional Requirements (Summary)

The complete user stories are in `user-stories.md`. Summary by release:

### Release 1: Inspection (US-21, US-22)

**US-21 — Tool detail screen.** Enter on a left-pane row opens a new screen showing the tool's discovery root, detected version (if available), configured search paths, model count, disk usage, last scan time, scan duration, last error, plugin version, and largest model. `[r]` refreshes this tool; `[Esc]` returns.

**US-22 — Model detail screen with tool-native metadata.** Extends the existing US-13 detail screen with a Metadata section that surfaces tool-native introspection: GGUF header KVs (for `.gguf` files), Ollama manifest JSON fields (for Ollama models), HF `config.json` excerpts (for HF-hosted models). `[r]` re-introspects; provenance line shows "introspected <N> ago."

### Release 2: SQLite-backed cache (US-23, US-24, US-25, US-26)

**US-23 — Cache schema + persistence + recovery + concurrency.** New `modeltap-store` crate. SQLite file at `$XDG_DATA_HOME/modeltap/cache.sqlite` with `PRAGMA journal_mode=WAL` and `busy_timeout=5000`. Schema versioned via `PRAGMA user_version`; `rusqlite_migration` framework runs forward migrations. Corruption detected on open → file renamed to `.corrupt-<timestamp>` → cold-start fallback → recovery banner. `--no-cache` CLI flag bypasses the cache entirely.

**US-24 — Manual refresh + provenance line.** Bottom bar gains `[r] refresh tool` and `[Shift+R] refresh all`. Summary bar always shows "as of <timestamp>, [reconciling...]". Refresh updates the cache and the provenance.

**US-25 — Warm-start cache read.** Launch reads cache; paints inventory within 100 ms of process start when cache is valid. Cold-start fallback unchanged from ADR-003.

**US-26 — Background reconcile + pre-mutate revalidation.** After warm-start paint, parallel-per-plugin reconcile updates the cache. Per-tool TTL (default 24h) excludes stale entries from warm paint. Pre-mutate revalidation: every destructive action re-stats target files against cache; drift → re-introspect; gone → abort + auto-refresh.

### Release 3 (deferred): SHA256 persistence (US-27)

**US-27 — Persisted SHA256 cache across launches.** Adds `cache.sha256` table. `(path, mtime, size, inode_dev) → ContentHash` mapping survives launches. Pre-unify re-stats to validate; mismatch → background rehash. `modeltap cache verify` developer command rehashes everything and reports drift.

## Non-Functional Requirements

### Performance

| NFR | Target | Verification |
|---|---|---|
| Warm-start first paint | ≤ 100 ms from process start when cache is valid (parent K3a equivalent) | Built-in startup timing log; CI alert if > 200 ms |
| Cold-start first paint | ≤ 150 ms skeleton + ≤ 1.15 s full inventory (parent K3, unchanged) | Built-in startup timing log |
| Background reconcile completion | ≤ 1.15 s for typical 4-plugin inventory (matches parent ADR-003 §7 budget) | Built-in timing log |
| Manual refresh latency (`[r]` per-tool) | ≤ 1 s wall-clock for typical tool inventory (Ollama 12 models, HF 31 models) | Built-in timing log |
| Tool detail screen open (US-21) | ≤ 100 ms from Enter keypress to screen render (data is in-process / cache) | Snapshot test + manual UAT |
| Model detail screen open (US-22) | ≤ 100 ms when metadata is cached; ≤ 1 s when re-introspecting | Snapshot test + manual UAT |
| Cache writes per action | ≤ 50 ms additional latency post-action (write transaction) | Integration test |
| Cache file size | ≤ 1 MB for typical user (200 models × ~4 KB per row); ≤ 5 MB for power user (1000 models) | Manual check |

### Safety

| NFR | Target | Verification |
|---|---|---|
| Pre-mutate revalidation | 100% of destructive actions (unify, zap, delete-one, folder-delete) re-stat target files against cache before mutation | Integration test `tests/acceptance/cache_safety.rs::pre_mutate_revalidation_invoked` |
| Cache never blocks launch | Cache corruption, schema mismatch, or open failure NEVER prevents modeltap from reaching the inventory view | Integration test `tests/acceptance/cache_recovery.rs::corrupted_cache_does_not_block_launch` |
| Cache writes are atomic per action | Partial writes never visible to peer processes; SQLite WAL transactions wrap each action's cache update | Integration test |
| `--no-cache` is a true bypass | Zero bytes written to the cache file or its location for the duration of the launch | Integration test `tests/acceptance/cache_disabled.rs::no_cache_writes` |
| Schema migrations are idempotent | Re-running a failed migration produces the same end state OR fails identically (no partial state) | Migration unit tests |
| Accidental data loss attributable to cache | 0 reports across first 90 days post-release (extends parent K5 guardrail) | Issue tracker review |

### Privacy / Data locality

| NFR | Target | Verification |
|---|---|---|
| Cache stays local | Cache file is never uploaded; no network I/O introduced by this feature | Code review |
| Cache contents | Contains model paths, sizes, mtimes, sha256s, tool-native metadata KVs — NEVER user PII | Code review |
| Cache location respects XDG | Default to `$XDG_DATA_HOME/modeltap/cache.sqlite`; falls back to `dirs::data_dir().join("modeltap/cache.sqlite")` if XDG not set; overridable via `MODELTAP_CACHE_PATH` env var | UAT |
| Opt-out works | `--no-cache` and `cache.enabled = false` both bypass the cache entirely | UAT |

### Cross-platform

| NFR | Target | Verification |
|---|---|---|
| macOS support | Cache location resolves to `~/Library/Application Support/modeltap/cache.sqlite` via `dirs::data_dir()` | UAT on macOS Sonoma+ |
| Linux support | Cache location resolves to `~/.local/share/modeltap/cache.sqlite` via `dirs::data_dir()` (or `$XDG_DATA_HOME/modeltap/cache.sqlite` if set) | UAT on Ubuntu 22.04+ |
| Windows support | WSL-only (parent constraint); cache uses Linux path resolution under WSL | Same as parent |
| SQLite portability | Cache files written on macOS open identically on Linux (and vice versa) — same SQLite library | Integration test on both runners |

### Reliability

| NFR | Target | Verification |
|---|---|---|
| Cache corruption recovery | 100% of detected corruption events result in successful cold-start fallback + recovery banner + log entry | Integration test |
| Schema migration success | All shipped migrations apply cleanly to a fresh DB; migrations from each prior version apply cleanly | Migration test matrix |
| Per-tool reconcile failure isolation | Failure in one plugin's reconcile does not block other plugins (parent US-18 invariant extends) | Integration test |
| Concurrent process safety | Two modeltap processes can read concurrently and write serially via SQLite WAL + `busy_timeout`; no crash, no corruption | Integration test |

### Accessibility / UX (extends parent)

| NFR | Target | Verification |
|---|---|---|
| Provenance line readable | Summary-bar provenance text uses the same readability rules as the rest of the summary bar (parent NFR) | UAT |
| Recovery banner dismissable | `[Esc]` dismisses the recovery banner; banner does not auto-dismiss | UAT |
| Bottom-bar shortcuts | `[r]`, `[Shift+R]`, `[Enter] tool detail` follow parent US-08 dim/bright discipline | UAT |
| Tool detail screen Esc-back | `[Esc]` returns to the main view, preserving cursor position in the left pane | UAT |
| Model detail screen Esc-back | `[Esc]` returns to the main view, preserving cursor position in the right pane | UAT |

## Architectural Constraints (hard, for DESIGN)

### C-INFO-1 — Cache is paint-only; filesystem is authoritative on mutate

**This is the central safety rule.** Every destructive filesystem action MUST re-stat the targeted file(s) before mutation. Cache is read for display, never trusted for mutation. Violations of this rule are P0 bugs — they would regress K5 (no accidental data loss). The rule is verified by integration tests, code review, and a grep-able invariant: no code path that calls `std::fs::hard_link`, `std::fs::remove_file`, or `std::fs::rename` may be reached without an immediately-preceding `std::fs::metadata()` call against the same path.

### C-INFO-2 — Cache failure NEVER blocks launch

Cache corruption, schema mismatch, file permissions errors, or any other cache-layer failure MUST result in successful cold-start (ADR-003 baseline). The cache is an optimisation, never a dependency. Verified by integration test.

### C-INFO-3 — `--no-cache` is a true bypass

The `--no-cache` CLI flag and `cache.enabled = false` config option MUST result in zero bytes written to the cache file or its location for the duration of the launch. Verified by integration test that monitors filesystem writes to the cache path.

### C-INFO-4 — Tool trait extension stays additive and minimal

`Tool::inspect_tool()` and `Tool::inspect_model()` are added to the trait. Q-INFO-1 (default-impl vs required) is DESIGN's call. Either way, the addition MUST be source-compatible with the 4 existing plugin implementations — a contributor who builds against modeltap-core 0.2.x should be able to rebuild against the new version with no changes if they don't want to implement the new methods. (If Q-INFO-1 chooses "required," a no-op default-impl in the trait achieves the same.)

### C-INFO-5 — Cross-platform cache path via `dirs::data_dir()`

No hardcoded `~/.local/share/...` or `~/Library/...` paths. Use the `dirs` crate. Overridable via `MODELTAP_CACHE_PATH` for testing and power-users.

### C-INFO-6 — Schema migrations are forward-only, additive, idempotent

No down migrations. Schema changes are additive (new tables, new nullable columns) where possible; destructive changes (column rename, type change) are deferred or paired with a corruption-recovery-style rebuild that re-discovers from scratch.

### C-INFO-7 — Concurrent process safety via WAL + busy_timeout (no file locking)

Match the parent's intake Q5 philosophy: no PID detection, no file locks beyond what SQLite provides natively. WAL + `busy_timeout=5000` is sufficient for v1; revisit only if dogfooding surfaces issues.

### Parent constraints carry forward unchanged

- C1 (Plugin trait extensibility) — extended additively; new plugins can no-op the new methods.
- C2 (Cross-platform from v1) — cache path resolution is cross-platform.
- C3 (MLX out of scope) — unchanged.
- C5 (Privacy by default) — cache stays local; no telemetry.
- C6 (Cleanup-first framing) — unchanged; inspection is read-only.

## Open Questions — Disposition

The intake brief flagged three open questions plus several that DESIGN must close in the new ADR.

### From intake brief

| ID | Question | Resolution in DISCUSS | Action for DESIGN |
|---|---|---|---|
| Intake #1 | Is this **one feature or two**? | **RESOLVED: one feature.** Info-views as user-visible motivator, persistence as enabler. Rationale and counter-argument in `story-map.md` "Why one feature, not two." Internal split into 2 releases (R1 inspection, R2 cache). | None — proceed as one feature through DESIGN/DEVOPS/DISTILL/DELIVER. |
| Intake #2 | Does this gate/pause `folder-group-bulk-delete` (in-flight)? | **RECOMMENDED: Option C — queue this feature behind folder-group.** Complete this DISCUSS now (in-progress); folder-group DELIVER continues; this feature's DESIGN wave starts after folder-group merges. Rationale in `prioritization.md`. User can override. | Confirm sequencing with maintainer at DESIGN handoff. |
| Intake #3 | Backwards-compat with dev-build `cache.sqlite` files? | **RESOLVED: none.** Any pre-v1 cache file is treated as a version mismatch → recovery → cold-start. Documented in the corruption-recovery banner. | None — handled by the recovery framework. |

### New for this feature (DESIGN-OPEN)

| ID | Artifact | Open question |
|---|---|---|
| Q-INFO-1 | `Tool::inspect_tool()` / `Tool::inspect_model()` | Required trait methods OR default-impl returning `NotSupported`? Recommendation: default-impl returning `NotSupported` so contributors aren't forced to migrate. |
| Q-INFO-2 | `cache.location` | Confirm `dirs::data_dir().join("modeltap/cache.sqlite")` resolves correctly on macOS and Linux. |
| Q-INFO-3 | migration tooling | Recommendation: `rusqlite_migration` crate. DESIGN owns final call. |
| Q-INFO-4 | `cache.tool_ttl_seconds` | Default 24h. Per-tool override via `[cache.tool_overrides.<tool>] ttl_seconds = ...` — optional for v1. |
| Q-INFO-5 | `cache.enabled` default | Recommendation: ON by default; `--no-cache` opt-out. The user explicitly asked for the cache. |
| Q-INFO-6 | concurrent write contention | Recommendation: `busy_timeout=5000` only. Revisit if dogfooding surfaces issues. |
| Q-INFO-7 | `tool.detected_version` source per-plugin | Best-effort per-plugin; "(not detectable)" is the safe fallback. May need light spike per-plugin in DESIGN. |
| Q-INFO-8 | `cache.models.metadata_kv` schema | Recommendation: JSON column for v1; relational schema if needed later. |

## Risks

| Risk | Category | Probability | Impact | Mitigation |
|---|---|---|---|---|
| Pre-mutate revalidation rule is bypassed by future code paths (regresses K5) | Technical | MEDIUM | CRITICAL | Integration test asserts every mutation site goes through the revalidator; code review checklist item; grep-able invariant per C-INFO-1 |
| Cache corruption recovery has bugs (cache doesn't actually fall back to cold-start) | Technical | LOW | CRITICAL | Integration tests exercise multiple corruption modes (SQLITE_CORRUPT, schema mismatch, partial write, full disk); dogfood for 2 weeks before declaring stable |
| Schema migration fails on real-world data after a release ships | Technical | LOW | HIGH | Test matrix covers migrations from every prior shipped version; corruption-recovery handles any failure transparently |
| `dirs::data_dir()` returns unexpected path on some macOS configurations | Technical | LOW | MEDIUM | `MODELTAP_CACHE_PATH` env override available; documented in release notes |
| Tool trait extension breaks contributor PRs in-flight | Project | LOW | MEDIUM | Default-impl returning `NotSupported` (Q-INFO-1 recommendation) keeps the trait backwards-source-compatible |
| Concurrent process scenario causes silent data drift between two open instances | Technical | MEDIUM | LOW | Acceptable per design — each process sees cache state at its own background-refresh time; pre-mutate revalidation gates destructive actions |
| Per-tool TTL of 24h causes user to act on 23h-old data | Project | LOW | MEDIUM | Pre-mutate revalidation catches it; user-configurable TTL for power users; default conservative |
| SHA256 cache (US-27) lets stale hash through via mtime-preserving file replacement | Technical | LOW | HIGH | Cache key includes `inode_dev`; pre-unify re-stats; `modeltap cache verify` developer command |
| User opt-out flag (`--no-cache`) is missed by future contributors adding cache writes | Project | MEDIUM | MEDIUM | All cache writes go through `cache.write_queue` channel; channel respects `cache.enabled`; integration test asserts no cache writes when disabled |
| Folder-group-bulk-delete DELIVER timeline slips because of context-switching with this feature's DESIGN | Project | MEDIUM | MEDIUM | `prioritization.md` Option C explicitly recommends queueing this feature's DESIGN behind folder-group |

## Wave Handoff Package

### To DESIGN (solution-architect)

**Inputs:**
- Journey artifacts: `journey-info-and-cache-visual.md`, `journey-info-and-cache.yaml`, `journey-info-and-cache.feature`
- Story map and prioritization: `story-map.md`, `prioritization.md`
- Requirements: this file, `user-stories.md`, `acceptance-criteria.md`
- Outcome KPIs: `outcome-kpis.md`
- Shared artifacts: `shared-artifacts-registry.md`
- DoR validation: `dor-checklist.md`
- JTBD analysis: `jtbd-job-stories.md`, `jtbd-four-forces.md`, `jtbd-opportunity-scores.md`

**Required ADR outputs from DESIGN:**

1. **ADR-NNN — State Model: SQLite-Backed Cache With Pre-Mutate Revalidation (supersedes ADR-003).** Must enumerate the 9 items listed in this document's "What the new ADR must close" section.
2. **ADR-NNN — Tool Trait Extension: `inspect_tool()` and `inspect_model()`.** Closes Q-INFO-1. Likely choice: default-impl returning `NotSupported`.
3. **ADR-NNN — Schema Migration Strategy.** Closes Q-INFO-3. Likely choice: `rusqlite_migration`.

**Architecture seam to design:**

- New `modeltap-store` crate. Public surface: `Cache::open(path) -> Result<Cache>`, `Cache::tools() -> Vec<CachedTool>`, `Cache::write_tool(...)`, `Cache::write_models(...)`, `Cache::write_action(...)`, `Cache::verify_against_fs(model_id)` (pre-mutate hook), `Cache::recover()` (corruption fallback).
- Extension to `modeltap-core`'s `Tool` trait: `inspect_tool()`, `inspect_model()`.
- Extension to `modeltap-app`'s orchestrator: warm-start path, background reconcile orchestrator, per-tool TTL eligibility check, `--no-cache` plumbing.
- Extension to `modeltap-tui`: new tool detail screen, extended model detail screen, provenance line in summary bar, `[r]`/`[Shift+R]` keymap entries, recovery banner widget.

**Hard constraints (must not be designed away):**

- C-INFO-1 (cache paint-only, filesystem authoritative on mutate)
- C-INFO-2 (cache failure never blocks launch)
- C-INFO-3 (`--no-cache` is a true bypass)
- C-INFO-4 (Tool trait extension stays additive)
- C-INFO-5 (cross-platform cache path via `dirs`)
- C-INFO-6 (forward-only, additive, idempotent migrations)
- C-INFO-7 (concurrency via WAL + busy_timeout only)
- All parent constraints (C1-C6) carry forward.

**Open questions all RESOLVED post-DESIGN:**

Per the table in "Open Questions — Disposition" section above, intake questions are RESOLVED in DISCUSS; Q-INFO-1..Q-INFO-8 are DESIGN's to close with recorded ADRs.

### To DEVOPS (platform-architect)

`outcome-kpis.md` defines new KPIs `K-INFO-1` (warm-start latency), `K-INFO-2` (manual refresh latency), `K-INFO-3` (cache hit ratio), `K-INFO-4` (corruption recovery rate). Key items:

- Add new local log line schemas for `cache_recovery`, `cache_migration`, `cache_verify`, `reconcile_failed` tags.
- CI alert: warm-start first-paint > 200 ms = regression.
- CI alert: cold-start first-paint > 200 ms skeleton = regression (parent K3a).
- No telemetry uploaded by default (parent C5 carries forward).

### To DISTILL (acceptance-designer)

`journey-info-and-cache.feature` plus US-21..US-27 UAT scenarios in `user-stories.md` are the source. Add scenarios to `docs/feature/modeltap-tui/distill/features/master-acceptance.feature` under `@us_21` ... `@us_27` tags. Integration checkpoints in `shared-artifacts-registry.md` are the cross-step invariants.

## DoR Status

See `dor-checklist.md` for per-story validation. Summary: **7 stories defined (US-21..US-27); all 9 DoR items pass for all 7 stories**.

## Peer Review Summary

See `peer-review.md`. Self-review using the 5-dimension critique from `nw-po-review-dimensions`; pass with one MEDIUM and zero CRITICAL/HIGH issues.

---

## Handoff Summary

**Next wave: DESIGN (solution-architect)**

**ADR to be written by DESIGN:** `ADR-NNN — State Model: SQLite-Backed Cache With Pre-Mutate Revalidation (supersedes ADR-003)`. Must enumerate the 9 items in "What the new ADR must close" above. Additional ADRs likely: `ADR-NNN — Tool Trait Extension: inspect_*()`, `ADR-NNN — Schema Migration Strategy`.

**Sequencing recommendation:** Queue DESIGN behind the in-flight `folder-group-bulk-delete` DELIVER (Option C in `prioritization.md`). User to confirm or override at handoff.

**DoR Status: 9/9 PASSED across all 7 stories.**
