# Acceptance Criteria — tool-model-info-sqlite-cache

Consolidated, testable acceptance criteria for US-21..US-27. Each AC traces back to a UAT scenario in `user-stories.md` and/or an integration-checkpoint invariant in `shared-artifacts-registry.md`. Cross-feature integration ACs (INT-INFO-*) capture invariants that span this feature and the parent `modeltap-tui` (plus `folder-group-bulk-delete`).

## Per-story ACs

### US-21 Tool detail screen (9 ACs)

| ID | Acceptance Criterion | UAT trace |
|---|---|---|
| AC-21-1 | Pressing Enter on any left-pane tool row opens the tool detail screen within 100 ms | Scenario 1 |
| AC-21-2 | Detail screen shows discovery root, version (or "(not detectable)"), search paths (with default/user-config provenance), model count, disk usage, largest model, last scan time, scan duration, last error (if any), and plugin version | Scenario 1 |
| AC-21-3 | Version field is `Option<String>`; `None` renders as "(not detectable)" — never as empty or as a false value | Scenario 2 |
| AC-21-4 | Last error field shows the error text + timestamp when present; reads "(none)" when absent | Scenario 3 |
| AC-21-5 | Search paths section distinguishes default paths from user-config paths | Scenario 4 |
| AC-21-6 | `[r]` re-runs discovery for this tool, updates the detail screen and the left-pane row | Scenario 3 |
| AC-21-7 | `[Esc]` returns to main view with left-pane cursor preserved | Scenario 5 |
| AC-21-8 | Bottom bar shows `[Esc] back`, `[r] refresh this tool`, `[?] help` on the detail screen | Scenarios 1, 5 |
| AC-21-9 | Plugin panic during `inspect_tool()` is caught at boundary; "(inspection failed — see diagnostics.log)" displayed; other fields render from `discover()` data | (parent US-18 panic-isolation extends) |

### US-22 Model detail with metadata (10 ACs)

| ID | Acceptance Criterion | UAT trace |
|---|---|---|
| AC-22-1 | Pressing Enter on any right-pane model row opens the model detail screen within 100 ms (cached metadata) | Scenario 1 |
| AC-22-2 | Re-introspection (`[r]`) completes within 1 second for typical model files | Scenario 4 |
| AC-22-3 | Detail screen retains all existing US-13 fields AND adds a Metadata section | Scenarios 1, 2, 3 |
| AC-22-4 | Metadata section format is consistent across plugins: aligned key-value pairs, dim section header reading "Metadata (from <source>, introspected <N> ago)" | Scenarios 1, 2, 3 |
| AC-22-5 | Per-plugin metadata source: GGUF header, Ollama manifest JSON, HF config.json, plugin-defined for new plugins | Scenarios 1, 2, 3 |
| AC-22-6 | `Tool::inspect_model()` returns `BTreeMap<String, String>` with selected (tool-relevant) KVs | (implementation-checkpoint) |
| AC-22-7 | Un-introspectable files show "(introspection failed — see diagnostics.log)"; other panels still render | Scenario 5 |
| AC-22-8 | `[r]` on detail screen re-runs `inspect_model()` and updates provenance timestamp | Scenario 4 |
| AC-22-9 | `[Esc]` returns to main view with right-pane cursor preserved | Scenario 6 |
| AC-22-10 | Bottom bar shows `[Esc] back`, `[u] unify` (dimmed when not unifiable), `[d] delete-one`, `[r] re-introspect`, `[?] help` | Scenarios 1-6 |

### US-23 Cache schema, recovery, concurrency (12 ACs)

| ID | Acceptance Criterion | UAT trace |
|---|---|---|
| AC-23-1 | Cache file location resolves via `dirs::data_dir().join("modeltap/cache.sqlite")`, overridable via `MODELTAP_CACHE_PATH` env var | (implementation-checkpoint) |
| AC-23-2 | SQLite opens with `PRAGMA journal_mode=WAL` and `PRAGMA busy_timeout=5000` | Scenarios 4, 5 |
| AC-23-3 | Schema version stored in `PRAGMA user_version`; compared at launch against compile-time `EXPECTED_SCHEMA_VERSION` | Scenario 2 |
| AC-23-4 | Forward migrations run automatically when cache version < binary expected; each step logged | Scenario 2 |
| AC-23-5 | Cache version > binary expected (downgrade) renames file to `.future-version-<n>` and starts cold; recovery banner explains | Scenario 7 |
| AC-23-6 | `SQLITE_CORRUPT` on open renames file to `.corrupt-<timestamp>` and starts cold; banner explains; log line tagged `cache_recovery reason=corrupted` | Scenario 3 |
| AC-23-7 | Recovery banner appears at top of main view; dismissable with `[Esc]`; never blocks launch | Scenarios 3, 7 |
| AC-23-8 | `--no-cache` CLI flag results in ZERO bytes written to cache file or location (integration-tested) | Scenario 6 |
| AC-23-9 | `[cache] enabled = false` config has same effect as `--no-cache`; CLI wins when both present | (implementation-checkpoint) |
| AC-23-10 | Two concurrent modeltap processes read/write the cache via WAL + busy_timeout; neither crashes; writes serialise | Scenarios 4, 5 |
| AC-23-11 | Cache failure NEVER prevents modeltap from reaching the inventory view — cold-start ALWAYS succeeds (C-INFO-2) | Scenarios 3, 7 |
| AC-23-12 | Cache stays local; no network I/O introduced by this feature | (implementation-checkpoint) |

### US-24 Manual refresh + provenance (9 ACs)

| ID | Acceptance Criterion | UAT trace |
|---|---|---|
| AC-24-1 | Summary bar shows provenance line at all times: "as of <X>" (human-readable: "just now", "<N> min ago", "<N> hours ago", "<N> days ago") | Scenario 4 |
| AC-24-2 | During reconcile, the provenance line appends ", reconciling..." (or ", refreshing Ollama..." for targeted refresh) | Scenarios 1, 4 |
| AC-24-3 | `[r]` triggers per-tool reconcile of the currently-selected tool | Scenario 1 |
| AC-24-4 | `[Shift+R]` triggers parallel reconcile of all tools | Scenario 2 |
| AC-24-5 | Both shortcuts are no-ops when any dialog is open; bottom bar dims them | Scenario 3 |
| AC-24-6 | Bottom bar always shows `[r] refresh tool` and `[Shift+R] refresh all` in main view | (per US-08 parent invariant) |
| AC-24-7 | After refresh completes, provenance line updates to "as of just now (<scope> refreshed)" | Scenarios 1, 2 |
| AC-24-8 | Refresh updates `cache.tools.last_scan_at` for affected tools | Scenarios 1, 2 |
| AC-24-9 | Manual refresh latency ≤ 1 s for typical single tool | (NFR — performance) |

### US-25 Warm-start cache read (7 ACs)

| ID | Acceptance Criterion | UAT trace |
|---|---|---|
| AC-25-1 | When cache is valid and contains in-TTL data, first paint completes within 100 ms of process start | Scenario 1 |
| AC-25-2 | Per-tool TTL eligibility: tools with `last_scan_at` older than `cache.tool_ttl_seconds` (default 86400) do NOT paint from cache | Scenario 3 |
| AC-25-3 | Cold-start path preserves parent K3 (≤ 150 ms skeleton, ≤ 1.15 s full inventory) | Scenario 2 |
| AC-25-4 | Mixed warm/cold per-tool supported — some tools paint from cache while others cold-start in parallel | Scenario 3 |
| AC-25-5 | Summary bar's provenance line is set at warm-paint time based on `MAX(tool.last_scan_at)` for cache-painted tools | (implementation-checkpoint) |
| AC-25-6 | Cache read failure (transient I/O) falls back to cold-start for affected tool; never crashes launch (C-INFO-2) | (implementation-checkpoint) |
| AC-25-7 | `--no-cache` and `cache.enabled = false` skip warm-paint path entirely; cold-start is used | (cross-trace AC-23-8) |

### US-26 Background reconcile + pre-mutate revalidation (9 ACs)

| ID | Acceptance Criterion | UAT trace |
|---|---|---|
| AC-26-1 | After warm-start paint, parallel-per-plugin `discover()` orchestrator runs without user action | Scenario 1 |
| AC-26-2 | Successful per-tool reconcile atomically updates `cache.tools` and `cache.models` rows (single transaction per tool) | Scenario 1 |
| AC-26-3 | Failed per-tool reconcile leaves cache unchanged (last-known-good preserved); "(error)" shown; log line written | Scenario 3 |
| AC-26-4 | Inventory diff detection: blue `*` indicator appears next to tool name for 3 seconds when reconciled inventory differs | Scenario 2 |
| AC-26-5 | Pre-mutate revalidation: every destructive action re-stats target files via `std::fs::metadata()` against `cache.models.(mtime, size, inode_dev)` | Scenarios 4, 5, 6 |
| AC-26-6 | Pre-mutate drift → re-introspect; dialog refreshes; user re-confirms if numbers changed | Scenario 5 |
| AC-26-7 | Pre-mutate file-gone → abort with "file no longer exists; refreshing inventory"; auto-trigger per-tool refresh | Scenario 6 |
| AC-26-8 | Integration test asserts every mutation site goes through revalidator (no unguarded calls to `hard_link`, `remove_file`, `rename` against model paths) | (implementation-checkpoint; CRITICAL safety rule per C-INFO-1) |
| AC-26-9 | Background reconcile completes within ~1.15 s for typical 4-plugin inventory (matches parent K3 budget) | (NFR — performance) |

### US-27 SHA256 persistence — DEFERRED to Release 3 (8 ACs)

| ID | Acceptance Criterion | UAT trace |
|---|---|---|
| AC-27-1 | New `cache.sha256` table schema as specified in US-27 Technical Notes | Scenario 1 |
| AC-27-2 | SHA256 cache lookup uses exact tuple match `(mtime, size, inode_dev)`; any drift invalidates | Scenario 2 |
| AC-27-3 | Invalid entries trigger background re-hash via existing ADR-013 hash pool | Scenario 2 |
| AC-27-4 | Pre-unify revalidation includes SHA256 cache check (in addition to stat-only check from US-26) | (cross-trace AC-26-5) |
| AC-27-5 | `modeltap cache verify` developer command rehashes all entries and reports drift | Scenario 3 |
| AC-27-6 | SHA256 cache is opt-in via `[cache] persist_sha256 = true` for v1; default off | (implementation-checkpoint) |
| AC-27-7 | Migration `0002_add_sha256_persistence.sql` adds the table cleanly to existing caches | (implementation-checkpoint) |
| AC-27-8 | `--no-cache` and `cache.enabled = false` skip the SHA256 cache as well | (cross-trace AC-23-8) |

## Cross-feature integration ACs (INT-INFO-*)

These ACs capture invariants that span this feature plus the parent `modeltap-tui` and (where applicable) `folder-group-bulk-delete`. They MUST pass alongside the per-story ACs.

| ID | Integration Acceptance Criterion |
|---|---|
| INT-INFO-1 | The parent's K3 (first paint < 1 s) is **REDEFINED** to two sub-KPIs: K3a (warm-start ≤ 100 ms) and K3b (cold-start ≤ 150 ms skeleton + ≤ 1.15 s full inventory, unchanged from ADR-003). Both must pass in CI. |
| INT-INFO-2 | The parent's `keyboard_shortcuts` registry (`modeltap-tui::input::keymap::SHORTCUT_TABLE`) is the single source of truth; new entries `[r]`, `[Shift+R]`, `[Enter] tool detail` (left pane) MUST be added there, not duplicated. (Parent US-08 invariant extended.) |
| INT-INFO-3 | The parent's `total.disk_usage == sum(tool.disk_usage)` invariant holds during reconcile mid-flight; transient inconsistency is acceptable visually only if the summary bar shows "reconciling..." simultaneously. |
| INT-INFO-4 | Cross-cutting safety: every destructive action (`[u]`, `[z]`, `[d]`, `[F]`) runs the pre-mutate revalidator (AC-26-5..AC-26-7) BEFORE any filesystem mutation. Verified by integration test. |
| INT-INFO-5 | Cross-cutting compatibility: `--no-cache` ALL of: skip warm paint, skip writes, skip reconcile cache updates, skip SHA256 persistence — verified by integration test asserting zero bytes written to cache path. |
| INT-INFO-6 | Cross-cutting recovery: `modeltap --version` succeeds even if cache file is unreadable / corrupted (cache layer not touched for `--version`). |
| INT-INFO-7 | Cross-cutting parent dependency: folder-group-bulk-delete (US-05c) `[F]` action also goes through the pre-mutate revalidator (AC-26-5). If US-05c ships before US-26, this integration AC is added retroactively when US-26 lands. |
| INT-INFO-8 | Cross-cutting parent invariant: plugin panic in `inspect_tool()` or `inspect_model()` MUST be caught at the plugin boundary; one bad plugin does not crash the TUI (parent US-18 invariant extends to the new trait methods). |
| INT-INFO-9 | Cross-cutting cli_vocabulary: new terms (warm start, cold start, reconcile, provenance, TTL, cache recovery, tool detail, introspect, re-introspect) MUST be used consistently in TUI text, help output, error messages, and documentation (parent invariant). |

## AC count summary

| Story | Per-story ACs |
|---|---|
| US-21 | 9 |
| US-22 | 10 |
| US-23 | 12 |
| US-24 | 9 |
| US-25 | 7 |
| US-26 | 9 |
| US-27 | 8 |
| INT-INFO-* | 9 |
| **Total** | **73** |

## Trace summary

- Per-story ACs trace to UAT scenarios in `user-stories.md` (each AC cited in the UAT scenarios that exercise it).
- Cross-feature INT-INFO-* ACs trace to integration checkpoints in `shared-artifacts-registry.md` and to parent feature invariants.
- All `@us_21` ... `@us_27` tagged scenarios in `journey-info-and-cache.feature` map to one or more per-story or INT-INFO ACs (DISTILL wave validates the trace).
