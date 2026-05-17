# Shared Artifacts Registry — tool-model-info-sqlite-cache

Brownfield extension of `docs/feature/modeltap-tui/discuss/shared-artifacts-registry.md` and `docs/feature/folder-group-bulk-delete/discuss/shared-artifacts-registry.md`. This file lists **only the new artifacts** introduced by US-21..US-27 plus **updated rows** where parent artifacts now have additional consumers in the inspection / cache flows.

## Conventions

- **Source of truth** is the canonical producer (a function, file, SQLite column, or process).
- **Consumers** are every place the value is displayed or referenced.
- **Integration risk** is the impact of inconsistency.

## New artifacts — cache.* (SQLite metadata)

| Artifact | Source of Truth | Consumers | Integration Risk |
|---|---|---|---|
| `cache.location` | `dirs::data_dir().join("modeltap/cache.sqlite")` — overridable via `MODELTAP_CACHE_PATH` env var | open-on-launch, --no-cache check, diagnostics log | HIGH — wrong location = silent two-cache split where launches use different files |
| `cache.schema_version` | `PRAGMA user_version` (SQLite metadata) | migrator, boot validation, diagnostics log | CRITICAL — wrong version comparison triggers spurious migration or false-positive corruption |
| `cache.expected_schema_version` | compile-time constant in `crates/modeltap-store/src/schema.rs` | migrator, boot validation | CRITICAL — drift between code and constant breaks migration logic |
| `cache.state` | app boot logic: `{warm, cold, recovering}` | banner display, internal pathway selection, diagnostics log | HIGH — wrong state = wrong paint path |
| `cache.recovery_reason` | app boot logic: `{None, Corrupted, SchemaTooOld, SchemaTooNew}` | recovery banner text, diagnostics.log entry | HIGH — silent recovery without user-visible explanation erodes trust |
| `cache.as_of_timestamp` | `cache.last_full_reconcile_at` column (MAX across `cache.tools.last_scan_at` per-launch) | summary-bar provenance line, tool detail screen "last scan" | CRITICAL — provenance lying = user acts on stale data thinking it's fresh |
| `cache.reconcile_status` | app reconcile orchestrator: `{idle, reconciling(n,k), failed}` | summary-bar text, per-tool spinner indicators | MEDIUM — visual lag, not data corruption |
| `cache.refresh_target` | keypress handler at `[r]` / `[Shift+R]` dispatch | reconcile orchestrator, spinner display | MEDIUM |
| `cache.tool_ttl_seconds` | `[cache]` section of `~/.modeltap/config.toml`; default `86400` (24h) | per-tool TTL eligibility check at warm-start paint | HIGH — TTL too long = stale data acted on; TTL too short = warm-start benefit lost |
| `cache.enabled` | `[cache]` section of `~/.modeltap/config.toml` or `--no-cache` CLI flag (CLI wins) | open-on-launch, write-after-action | HIGH — disabled cache must NEVER touch SQLite file |
| `cache.write_queue` | post-action cache update logic; in-process channel to writer task | SQLite write transaction | HIGH — dropped queue entries = cache drift |

## New artifacts — tool.* (per-tool inspection / detail)

| Artifact | Source of Truth | Consumers | Integration Risk |
|---|---|---|---|
| `tool.install_path` | plugin's `discover()` default + user config in `~/.modeltap/config.toml` (e.g., `~/.ollama/models/`) | tool detail screen "Discovery root", per-plugin `discover()` invocation | HIGH — wrong path = wrong inventory |
| `tool.detected_version` | per-plugin `Tool::inspect_tool()` impl; `Option<String>` (None if undetectable) | tool detail screen "Version" line | MEDIUM — false data is worse than no data; "(not detectable)" is the safe fallback |
| `tool.detection_source` | per-plugin metadata describing HOW the version was detected (e.g., "http://localhost:11434", "ollama --version", "(not detectable)") | tool detail screen "(detected via X)" annotation | LOW |
| `tool.last_scan_at` | `cache.tools.last_scan_at` column | tool detail screen, summary-bar provenance, per-tool TTL eligibility | CRITICAL — central freshness contract |
| `tool.last_scan_duration_ms` | `cache.tools.last_scan_duration_ms` column | tool detail screen "Scan duration" | LOW — diagnostic only |
| `tool.last_error` | `cache.tools.last_error` column (text + timestamp) | tool detail screen "Last error", left-pane "(error)" annotation | HIGH — must clear on successful next scan |
| `tool.plugin_version` | plugin's static metadata (Cargo.toml version compiled in) | tool detail screen | LOW |
| `tool.search_paths` | plugin defaults + user config; list of `(path, source)` where source ∈ `{default, user_config}` | tool detail screen "Search paths" | MEDIUM |
| `tool.largest_model` | `core::find_largest_model(tool, inventory)` (pure function) | tool detail screen | LOW |
| `tool.model_count` | `cache.tools.model_count` (mirrors left-pane count) | tool detail screen, left-pane row, summary bar aggregate | HIGH — drift visible immediately to user |
| `tool.disk_usage` | `cache.tools.disk_usage_bytes` (mirrors left-pane bytes) | tool detail screen, left-pane row, `total.disk_usage` rollup | HIGH (same as parent) |

## New artifacts — model.* (per-model inspection / detail)

| Artifact | Source of Truth | Consumers | Integration Risk |
|---|---|---|---|
| `model.format_version` | per-plugin `Tool::inspect_model()` impl (e.g., "GGUF v3", "Ollama manifest v2", "safetensors v2") | model detail screen "Format" | MEDIUM |
| `model.quantisation` | `Tool::inspect_model()` (parses GGUF header `general.file_type` or HF config) | model detail screen "Quantisation", potentially indicator computation if quant-locked | MEDIUM |
| `model.architecture` | `Tool::inspect_model()` (GGUF `general.architecture` or HF `config.json` `architectures`) | model detail screen "Architecture" | MEDIUM |
| `model.parameters` | `Tool::inspect_model()` (computed from architecture KVs — e.g., for llama: `block_count * (4*embd^2 + 3*embd*ffn)` heuristic) | model detail screen "Parameters" | MEDIUM — heuristic approximation acceptable for v1 |
| `model.context_length` | `Tool::inspect_model()` (GGUF `<arch>.context_length` or HF `config.json` `max_position_embeddings`) | model detail screen "Context length" | LOW |
| `model.metadata_kv` | `Tool::inspect_model()` returns `BTreeMap<String, String>` of selected KVs | model detail screen "Metadata" section | MEDIUM |
| `model.metadata_introspected_at` | `cache.models.metadata_introspected_at` column | model detail screen provenance line | HIGH — same freshness contract as cache.as_of_timestamp |
| `model.dedup_key_computed_at` | `cache.sha256.computed_at` column (US-27) | model detail screen dedup-key line | HIGH — pre-mutate revalidation depends on this |
| `model.fs_stat` | live `std::fs::metadata()` call at action time (NOT from cache) | pre-mutate validator | CRITICAL — bypassing this rule = data loss |
| `model.size_on_disk` | `cache.models.size_bytes` (mirrors right-pane row size) | model detail screen, right-pane row, per-tool disk aggregate | HIGH (extends parent) |

## Updated parent artifacts (new consumers from this feature)

| Artifact (parent registry) | New consumer this feature adds | Integration Risk |
|---|---|---|
| `keyboard_shortcuts` | gains `[r] refresh tool` and `[Shift+R] refresh all` entries on main view; `[r] re-introspect` on detail screens; `Enter` for tool detail (new); existing `Enter` for model detail unchanged | HIGH (same as parent) |
| `total.disk_usage` | now sourced from cache on warm start; reconcile updates trigger the same refresh path as US-11 | HIGH (same as parent) |
| `total.model_count` | now sourced from cache on warm start | HIGH (same as parent) |
| `model.dedup_key` | now persisted across launches via cache.sha256 table (US-27); pre-mutate revalidation rule applies | CRITICAL — drift = wrong dedup grouping = data loss |
| `model.compatible_tools` | computed from cached + reconciled inventory; recomputed after any mutation (US-09 invariant) | HIGH (same as parent) |
| `last_action.bytes_reclaimed` | unchanged; written to cache.actions log table when cache enabled | LOW |
| `cli_vocabulary` | gains new terms: "warm start", "cold start", "reconcile", "provenance", "TTL", "cache recovery", "tool detail", "introspect", "re-introspect" | HIGH — terminology drift erodes trust |
| `~/.modeltap/diagnostics.log` | gains new log tags: `cache_recovery`, `cache_migration`, `cache_verify`, `reconcile_failed` | LOW |

## Open Source-of-Truth Questions (DESIGN must close)

| ID | Artifact | Open question |
|---|---|---|
| Q-INFO-1 | `Tool::inspect_tool()` / `Tool::inspect_model()` | Required trait methods (all 4 existing plugins must implement) OR default-impl returning `NotSupported`? Implications for plugin author migration. |
| Q-INFO-2 | `cache.location` | `$XDG_DATA_HOME/modeltap/cache.sqlite` on Linux, `~/Library/Application Support/modeltap/cache.sqlite` on macOS via `dirs::data_dir()`. Confirm exact path resolution and document. |
| Q-INFO-3 | migration tooling | `rusqlite_migration` (minimal dep, recommended), hand-rolled SQL+migrator, or `sqlx::migrate!` (requires async runtime change)? |
| Q-INFO-4 | `cache.tool_ttl_seconds` | Default 24h reasonable? Per-tool override via `[cache.tool_overrides.<tool>] ttl_seconds = ...`? |
| Q-INFO-5 | `cache.enabled` default | ON by default (recommended; user explicitly asked for it) with `--no-cache` opt-out, OR OFF by default opt-in? |
| Q-INFO-6 | concurrent cache writes | `PRAGMA busy_timeout=5000` sufficient for v1? Or detect-and-prompt-then-retry pattern (parent intake Q5)? Recommendation: busy_timeout for v1. |
| Q-INFO-7 | `tool.detected_version` source per-plugin | Ollama: hit `http://localhost:11434/api/version` if running else None; llama-cli: no canonical method; HF: read `~/.cache/huggingface/version.txt` or skip; LM Studio: no canonical method. Confirm with DESIGN spike per-plugin if needed. |
| Q-INFO-8 | `cache.models.metadata_kv` schema | Stored as JSON blob in a single column, or as separate `cache.model_metadata (model_id, key, value)` rows? JSON is simpler; relational is queryable. Recommendation: JSON for v1. |

## Validation plan (during DESIGN review)

1. Every `${variable}` in `journey-info-and-cache-visual.md` and `journey-info-and-cache.yaml` MUST appear in either this registry or the parent registry.
2. Pre-mutate revalidation (`model.fs_stat`) MUST be invoked at every mutation site — verified by code review and by an integration test (`tests/acceptance/cache_safety.rs::pre_mutate_revalidation_invoked`).
3. Cache-write paths MUST go through the single `cache.write_queue` channel — no direct SQLite writes from plugin code.
4. Schema constants MUST be in one place (`crates/modeltap-store/src/schema.rs`) — verified by grep.
5. `--no-cache` MUST bypass every cache touch — verified by integration test (`tests/acceptance/cache_disabled.rs::no_cache_writes`).
6. Open questions Q-INFO-1..Q-INFO-8 must be closed before any code is written for the corresponding artifacts.

## Integration checkpoints (cross-step invariants)

| Invariant | Steps involved | Failure mode |
|---|---|---|
| `tool.model_count` on detail screen == left-pane count == `cache.tools.model_count` | 1, 2, 4 | Three-way drift; user sees inconsistent numbers |
| `model.size_on_disk` on detail screen == right-pane row size == `cache.models.size_bytes` | 3, 5 | Per-row vs per-detail disagreement |
| `cache.as_of_timestamp` shown in summary bar == `MAX(cache.tools.last_scan_at)` at render time | 1, 2, 3 | Provenance lying about freshness |
| `model.dedup_key` displayed in detail == result of pre-mutate `model.fs_stat`-validated recomputation | 5, 6 | Cache hit acted on without revalidation = potential data-loss case |
| `total.disk_usage == sum(tool.disk_usage)` (parent invariant) holds during reconcile mid-flight | 1, 2 | Summary-bar arithmetic visibly wrong mid-update; aesthetic but undermines trust |
| Pre-mutate `(mtime, size, inode_dev)` check MUST use `model.fs_stat` (live), NEVER `cache.models.*` | 6 | THE critical rule — bypass = data loss |
| Cache corruption recovery NEVER blocks launch (cold-start fallback always succeeds) | 1, 7 | "modeltap won't start" failure mode — explicitly prevented by ADR-003 baseline |
| Schema migrations are idempotent and forward-only — re-running a failed migration produces same end state or fails identically | 7 | Partial migration state on retry leaves cache in undefined shape |
| `keyboard_shortcuts` displayed in bottom bar matches the actual key handler dispatch table, including new `[r]` and `[Shift+R]` (extends parent US-08 invariant) | 1, 2, 3 | App feels buggy / undiscoverable |
| `--no-cache` writes ZERO bytes to the cache file or its location for the duration of the launch | 1, 6, 7 | Silent cache pollution; opt-out doesn't actually opt out |
