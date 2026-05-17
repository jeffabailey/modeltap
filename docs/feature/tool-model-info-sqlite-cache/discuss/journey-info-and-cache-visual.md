# Journey: Tool & Model Inspection With SQLite-Backed Cache — Visual

**Feature:** tool-model-info-sqlite-cache
**Persona:** Devon Park — same persona as the parent `modeltap-tui` feature. Multi-tool local-AI power user on macOS/Linux. Runs ≥2 of {Ollama, llama-cli, Hugging Face cache, LM Studio}. Comfortable with vim-style keys. Already shipping the parent feature in DELIVER and the folder-group-bulk-delete extension. Now wants: (a) per-tool and per-model inspection inside the TUI, and (b) launches that don't pay the full discovery cost every single time.
**Goal:** Open modeltap, see a useful inventory *instantly* from cached state, drill into one tool or one model to confirm metadata before acting, and always know how fresh the displayed data is.

## Brownfield context

This journey extends the parent journey (`docs/feature/modeltap-tui/discuss/journey-cleanup-and-unify-visual.md`) at three points:

1. **Step 1 (Launch):** First-paint now reads from a persisted SQLite cache when available (J3). The skeleton-first paint of ADR-003 still applies on cold start (empty DB, first install, recovered-from-corruption).
2. **Step 3 (Browse):** A new "as of <timestamp>" provenance line appears in the summary bar. `[r]` (per-tool) and `Shift+R` (global) refresh hotkeys appear in the bottom bar.
3. **Step 4 (Inspect):** US-13 model detail screen is expanded with tool-native metadata (Ollama manifest fields, GGUF header KVs, HF `config.json` excerpts). A NEW tool detail screen (entered by pressing Enter on a left-pane row) shows per-tool diagnostics (install path, version, last scan, configured search paths, last error).

All vocabulary, indicators, post-action message format, and emotional rules from the parent journey carry forward unchanged. The **constraint reversal of ADR-003** (Q7 stateless rediscovery → SQLite-backed cache with revalidate-before-mutate guardrails) is the load-bearing architectural change; the user-visible payoff is the inspection feature.

## Emotional Arc

Two emotional arcs run in parallel — one per primary job. Both feed the same overall feeling: *modeltap got smarter and now trusts itself enough to remember*.

### Arc A — "Verify a model" (J1)

| Phase | State | What drives it |
|---|---|---|
| Trigger | Uncertain ("is this Q4_K_M or Q5_K_M? does its SHA match the HF blob?") | The right-pane row shows name + size + indicator; mental-model gap |
| Open detail | Curious | Pressing Enter on the model row |
| Read | Reassured ("yes — Q4_K_M, GGUF v3, llama architecture, 4.4 GB matches HF's `config.json`") | Detail view shows tool-native metadata + provenance |
| Decide | Confident | Bottom bar offers `[u] unify`, `[d] delete-from-one`, `[Esc] back` — Devon knows enough to act |

### Arc B — "Open modeltap on a Tuesday morning" (J3)

| Phase | State | What drives it |
|---|---|---|
| Trigger | Routine ("opening modeltap to check disk") | Workflow habit; no pain |
| Launch | Surprised ("oh — it's instant now") | Sub-100 ms paint from cache vs. previous ~1 s skeleton |
| Background reconcile | Trusting (provenance line: "as of just now, refreshed") | Background discovery completes within ~1.15 s; provenance updates |
| Act | In-flow ("nothing's changed since last Friday") | No surprise diffs; cache was accurate |

### Failure-mode emotional rule (inherited from parent)

Every cache-related failure must end with the user feeling **in control**, not surprised:

- Cache corruption → detected on open, renamed to `.corrupt-<timestamp>`, log line written, banner on next launch — Devon understands what happened.
- Stale cache caught at mutate-time (re-stat shows file gone) → action aborted with clear "file no longer exists; refreshing" message and an automatic re-discovery.
- Concurrent process wrote while we read → SQLite WAL handles transparently; Devon never sees a crash.

## Journey Flow (ASCII)

```
[Trigger: open modeltap        | [End state: inventory paints instantly,
 to check Hugging Face]           detail view confirms metadata,
                                  Devon acts with full confidence]
       |                                            ^
       v                                            |
+------+-------+ +-------+-------+ +-------+-------+ +-------+-------+
| Step 1       | | Step 2        | | Step 3        | | Step 4        |
| Launch with  |-| Background    |-| Browse w/     |-| Drill into    |
| cached paint |  | reconcile     |  | provenance    |  | tool / model  |
+--------------+ +---------------+ +---------------+ +---------------+
 Feels:           Feels:            Feels:            Feels:
 "instant"         "honest about    "I know how       "I have the
                    freshness"       fresh this is"    full picture"
                         |                                 |
                         v                                 v
                  [If cache corrupted:                [If stale on mutate:
                   rename + log + empty]               re-stat + abort]
```

## Step-by-Step Detail

### Step 1: Launch with cached paint (warm start) — OR skeleton paint (cold start)

**Context:** Devon types `modeltap` in his shell.

- **Warm start** (cache exists, schema matches, not corrupted): SQLite read pulls the last-known inventory; view paints within ~100 ms.
- **Cold start** (no cache file, fresh install, or post-corruption-recovery): ADR-003 skeleton paint takes over; "discovering..." placeholder appears within ~150 ms, full inventory within ~1.15 s.
- **Recovery start** (cache file present but corrupted or schema-mismatched): rename to `cache.sqlite.corrupt-<timestamp>`, log, fall back to cold-start behaviour, surface "previous cache reset" banner.

**TUI mockup (warm start, default view):**

```
+- modeltap ---------------------------------------------------------------+
| Tools                | Models in Ollama (12, 47.3 GB)                    |
| > Ollama        12   |                                                   |
|   llama-cli     6    | * llama3:8b-instruct-q4_K_M       4.9 GB          |
|   Hugging Face  31   |   (also in: Hugging Face)                         |
|   LM Studio     9    | * mistral:7b-instruct-q4_K_M      4.4 GB          |
|                      |   (also in: llama-cli, Hugging Face)              |
|                      | o qwen2.5:14b-q4_K_M              8.2 GB          |
|                      | ... 9 more models ...                             |
+--------------------------------------------------------------------------+
| Total: 138.4 GB | 58 models | as of 14 min ago, refreshing...            |
+--------------------------------------------------------------------------+
| [<-/->] tools  [up/down] models  [Enter] detail  [r] refresh tool        |
| [Shift+R] refresh all  [u] unify  [z] zap tool  [?] help [q] quit        |
+--------------------------------------------------------------------------+
```

**Shared artifacts introduced here:**

- `${cache.as_of_timestamp}` — when the displayed inventory was last reconciled with the filesystem. Source: `cache.last_full_reconcile_at` column.
- `${cache.state}` — one of `{warm, cold, recovering}`. Source: app boot logic.
- `${total.disk_usage}` / `${total.model_count}` — unchanged from parent; now sourced from cache on warm start, from in-process state on cold start.

**Integration checkpoint:** if `cache.state == recovering`, a banner line appears between the summary bar and the bottom bar: `"Previous cache file was corrupted and reset. See ~/.modeltap/diagnostics.log"`. Banner dismisses with Esc.

**Emotional entry:** routine.
**Emotional exit:** surprised (warm) / oriented (cold) / informed (recovering).

---

### Step 2: Background reconcile

**Context:** Same screen as Step 1 (no UI transition). After paint, modeltap kicks off a parallel-per-plugin `discover()` pass (the existing tokio-based pipeline from ADR-003 step 7). As each plugin completes, the affected rows refresh and the provenance line updates.

**TUI mockup (mid-reconcile, Ollama just finished, others still working):**

```
+- modeltap ---------------------------------------------------------------+
| Tools                | Models in Ollama (12, 47.3 GB)                    |
| > Ollama        12 . | * llama3:8b-instruct-q4_K_M       4.9 GB          |
|   llama-cli     6 .. |   (also in: Hugging Face)                         |
|   Hugging Face  31 . | * mistral:7b-instruct-q4_K_M      4.4 GB          |
|   LM Studio     9 .. | o qwen2.5:14b-q4_K_M              8.2 GB          |
|                      | ... 9 more models ...                             |
+--------------------------------------------------------------------------+
| Total: 138.4 GB | 58 models | reconciling 3 tools (1 done)               |
+--------------------------------------------------------------------------+
| [<-/->] tools  [up/down] models  [Enter] detail  [r] refresh tool        |
+--------------------------------------------------------------------------+
```

The trailing `.` and `..` are spinner indicators per-tool. Once a tool completes reconcile, the dot disappears.

**Per-tool TTL behaviour:** entries with `last_seen_at` older than `cache.tool_ttl_seconds` (default 86400 = 24 h) are NOT painted from cache — they're treated as cold for that tool. Devon's mental model: "if I haven't opened modeltap for 25+ hours, the inventory is fully re-discovered before display." This avoids surprise where the user has been on holiday.

**Reconcile diff behaviour:** if a re-discovered tool's inventory differs from the cached snapshot (new models, deleted models, changed sizes), the row counts update silently and a tiny blue `*` appears next to the tool name in the left pane for 3 seconds — a visual ack that "something changed since you last saw this." No modal, no dialog.

**Shared artifacts:**

- `${cache.reconcile_status}` — one of `{idle, reconciling(n,k), failed}`. Source: app reconcile orchestrator.
- `${tool.last_reconciled_at}` — per-tool reconcile timestamp.

**Integration checkpoint:** post-reconcile, `total.disk_usage == sum(tool.disk_usage)` (existing parent invariant). Reconcile failures per-tool surface as "(error)" in the left pane (unchanged from US-02 behaviour) — the cache for that tool is *not* overwritten; the stale-but-displayed entry stays until the next successful reconcile.

**Emotional entry:** surprised/oriented (from Step 1).
**Emotional exit:** trusting — the provenance line tells Devon when reconcile finished.

---

### Step 3: Browse with provenance always visible

**Context:** Devon scrolls the right pane and switches tools via Left/Right arrows. Everything works as in the parent journey. The ONE new affordance: the summary bar always shows the "as of <timestamp>" line, and `[r]` / `Shift+R` hotkeys are visible in the bottom bar.

**Manual refresh interaction (Devon presses `r` on Ollama):**

```
+- modeltap ---------------------------------------------------------------+
| Tools                | Models in Ollama (12, 47.3 GB)                    |
| > Ollama        12 . | ... (rows shown but refresh spinner active)       |
|   llama-cli     6    |                                                   |
|   Hugging Face  31   |                                                   |
|   LM Studio     9    |                                                   |
+--------------------------------------------------------------------------+
| Total: 138.4 GB | 58 models | refreshing Ollama...                       |
+--------------------------------------------------------------------------+
```

Within ~500 ms (typical Ollama discovery time):

```
+--------------------------------------------------------------------------+
| Total: 138.4 GB | 58 models | as of just now (Ollama refreshed)          |
+--------------------------------------------------------------------------+
```

If Ollama gained a model since last reconcile (e.g., Devon `ollama pull`'d in another terminal), the model count and totals update; the new row appears in the right pane.

**Shared artifacts:**

- `${cache.refresh_target}` — `{None, Tool(id), All}`. Source: keypress handler.
- `${cache.as_of_timestamp}` — updated to "just now" after refresh.

**Integration checkpoint:** `[r]` is a no-op when a dialog is open (unify, zap, delete-one, folder-delete confirmation, detail screen). Bottom bar dims `[r] refresh tool` in those contexts.

**Emotional entry:** trusting.
**Emotional exit:** in-flow — Devon can see freshness without thinking.

---

### Step 4a: Drill into a tool (NEW screen — tool detail)

**Context:** Devon presses Enter on a left-pane row to open the tool detail screen.

**TUI mockup (Ollama detail):**

```
+- modeltap : Ollama -----------------------------------------------------+
| Tool detail                                                             |
|                                                                         |
| Name           : Ollama                                                 |
| Version        : 0.6.4         (detected via http://localhost:11434)    |
| Discovery root : ~/.ollama/models/                                      |
| Search paths   : ~/.ollama/models/manifests/registry.ollama.ai/         |
|                  ~/.ollama/models/blobs/                                |
| Model count    : 12                                                     |
| Disk usage     : 47.3 GB (unique blobs)                                 |
| Largest model  : llama3:70b-instruct-q4_K_M  (39.8 GB)                  |
| Last scan      : 2026-05-16 09:14:22 (4 min ago)                        |
| Scan duration  : 0.42 s                                                 |
| Last error     : (none)                                                 |
| Plugin version : modeltap-plugin-ollama 0.2.6                           |
|                                                                         |
+-------------------------------------------------------------------------+
| [Esc] back  [r] refresh this tool  [?] help                             |
+-------------------------------------------------------------------------+
```

**Edge cases this screen handles:**

- **Version undetectable** (Ollama not running, or llama-cli's static binary doesn't expose `--version` cleanly): `Version : (not detectable)`. No false data.
- **Last error present** (US-02 "(error)" annotation): `Last error : permission denied reading ~/.ollama/models/manifests/ (errno 13)` with the timestamp.
- **Search paths from config**: if user added custom search paths via `~/.modeltap/config.toml`, those are listed and labelled `(user config)` vs `(default)`.

**Shared artifacts (new tool.* fields):**

- `${tool.install_path}` — discovery root. Source: plugin's `discover()` default + user config.
- `${tool.detected_version}` — best-effort version string or `None`. Source: per-plugin `inspect_tool()` impl.
- `${tool.last_scan_at}` — Source: `cache.tools.last_scan_at`.
- `${tool.last_scan_duration_ms}` — Source: same row.
- `${tool.last_error}` — Source: `cache.tools.last_error`.
- `${tool.plugin_version}` — Source: plugin's static metadata.
- `${tool.search_paths[]}` — list of configured paths with provenance (default / config).

**Integration checkpoint:** `tool.model_count` in this screen MUST equal the left-pane count for the same tool. Drift detection is in the shared artifacts registry.

**Emotional entry:** curious / suspecting-trouble.
**Emotional exit:** oriented — Devon knows what modeltap knows about this tool.

---

### Step 4b: Drill into a model (EXPANDED screen — model detail with tool-native metadata)

**Context:** Devon presses Enter on a right-pane model row. The existing US-13 detail screen is expanded with a "Metadata" section that surfaces tool-native introspection.

**TUI mockup (Mistral-7B detail, registered in 3 tools):**

```
+- modeltap : mistral:7b-instruct-q4_K_M --------------------------------+
| Model detail                                                            |
|                                                                         |
| Id              : mistral:7b-instruct-q4_K_M                            |
| Format          : GGUF v3                                               |
| Quantisation    : Q4_K_M                                                |
| Architecture    : llama                                                 |
| Parameters      : 7.24 B                                                |
| Context length  : 32768                                                 |
| Dedup key       : sha256:8f3e9c102a4b...c102  (computed 2 min ago)      |
| Size on disk    : 4.4 GB (per-tool)                                     |
| Total bytes     : 4.4 GB (1 inode, 3 hardlinks)  [UNIFIED]              |
|                                                                         |
| Registered with:                                                        |
|   * Ollama        ~/.ollama/models/blobs/sha256-8f3e9c102a4b...         |
|   * llama-cli     ~/llms/mistral-7b-instruct-q4_K_M.gguf  (hardlink)    |
|   * Hugging Face  ~/.cache/huggingface/.../mistral-7b-q4_K_M.gguf       |
|                                                                         |
| Metadata (from GGUF header, introspected 2 min ago):                    |
|   general.architecture           : llama                                |
|   general.quantization_version   : 2                                    |
|   llama.context_length           : 32768                                |
|   llama.embedding_length         : 4096                                 |
|   llama.block_count              : 32                                   |
|   tokenizer.ggml.model           : llama                                |
|                                                                         |
| Last action     : unify mistral:7b (success, 2 hours ago)               |
| Reclaim status  : Reclaimed: 8.8 GB                                     |
|                                                                         |
+-------------------------------------------------------------------------+
| [Esc] back  [u] unify  [d] delete-one  [r] re-introspect  [?] help      |
+-------------------------------------------------------------------------+
```

**Per-tool metadata format:**

| Tool | Metadata surfaced |
|---|---|
| Ollama | Manifest JSON fields (`config`, `parameters`, `template`, `system`, `messages`) + blob SHA |
| llama-cli (GGUF) | GGUF header KV pairs (general.architecture, quantization_version, model-specific tokens) |
| Hugging Face | Excerpts from `config.json` (model_type, architectures, hidden_size, num_attention_heads, etc.) |
| LM Studio | Whatever the file format exposes (GGUF KV pairs typically) + LM Studio config if present |

**Re-introspect interaction:** Devon presses `r` on this screen → re-runs `inspect_model()` for this model, updates the metadata section, updates the "introspected X ago" provenance.

**Shared artifacts (new model.* fields):**

- `${model.format_version}` — e.g., "GGUF v3", "Ollama manifest v2"
- `${model.quantisation}` — e.g., "Q4_K_M"
- `${model.architecture}` — e.g., "llama"
- `${model.parameters}` — e.g., "7.24 B"
- `${model.context_length}` — integer
- `${model.metadata_kv}` — flat key→value map from tool-native introspection
- `${model.metadata_introspected_at}` — provenance timestamp
- `${model.dedup_key_computed_at}` — SHA256 cache provenance

**Integration checkpoint:** `model.size_on_disk` displayed here MUST equal the size column in the right-pane row that opened this detail. `model.dedup_key` MUST match the SHA256 cache entry (validated against filesystem before any mutate per J3/J5 rule).

**Emotional entry:** uncertain.
**Emotional exit:** confident — Devon has every fact he needs.

---

### Step 5: Act with confidence (existing parent flows, unchanged behaviour, additional guardrails)

**Context:** Devon presses `u` or `d` or `z` from the detail screen or main view. Existing parent flows (US-05, US-05b, US-05c, US-10) run unchanged at the user-visible layer.

**NEW invariant (the cache-driven safety rule):** before any mutating filesystem action, the per-target file is `fstat`'d (or `stat`'d) against the cache entry. If `(mtime, size, inode_dev)` differ from cache:

- For unify: dedup re-grouping is invalidated for affected files; user sees "file changed since last seen; re-checking..." and the action restarts after re-introspection.
- For delete-one / folder-delete: pre-flight refusal "file no longer exists or has changed; refreshing inventory" (mirrors parent US-05c's F-FGD-8 pre-flight check).
- For zap-tool: each file inside the tool is re-stat'd per the same rule.

**Cache write timing:** post-action, the cache is updated with the new state (deleted rows removed, modified sizes updated). On unify success, the new hardlinks are reflected (same inode → cache merges entries).

**Emotional entry:** confident (from Step 4).
**Emotional exit:** satisfied (from parent journey).

---

### Step 6 (failure paths): cache corruption, schema migration

**Context:** Either at launch (Step 1) or mid-operation (very rare with SQLite WAL), the cache is detected as corrupted or schema-mismatched.

**TUI mockup (post-recovery banner, first launch after corruption detected):**

```
+- modeltap ---------------------------------------------------------------+
| Tools                | Models in Ollama (12, 47.3 GB)                    |
| > Ollama        12   | * llama3:8b-instruct-q4_K_M       4.9 GB          |
|   llama-cli     6    | ... rest of inventory ...                         |
+--------------------------------------------------------------------------+
| Previous cache reset (corrupted or schema mismatch).                     |
| Renamed to ~/.modeltap/cache.sqlite.corrupt-2026-05-16T091422.           |
| Cold-start discovery in progress. See ~/.modeltap/diagnostics.log.       |
|                                                              [Esc] dismiss|
+--------------------------------------------------------------------------+
| Total: 138.4 GB | 58 models | reconciling 4 tools (cold start)           |
+--------------------------------------------------------------------------+
| [<-/->] tools  [up/down] models  [Enter] detail  [?] help [q] quit       |
+--------------------------------------------------------------------------+
```

**Schema migration behaviour:** schemas are versioned (`PRAGMA user_version`). On launch, compare cache's `user_version` against the binary's expected version:

- **Match:** proceed.
- **Cache version < binary version:** run forward migrations in order (`migrations/{N}_{N+1}_*.sql`). Log each step. If any migration fails, treat as corruption and recover.
- **Cache version > binary version:** binary is older than the cache (user downgraded). Refuse to migrate backwards; rename cache to `.future-version-<n>` and start cold. Log the situation; the banner explains.

**Shared artifacts:**

- `${cache.schema_version}` — `PRAGMA user_version`. Source: SQLite metadata.
- `${cache.expected_schema_version}` — compiled into binary. Source: `crates/modeltap-store/src/schema.rs` constant.
- `${cache.recovery_reason}` — `{None, Corrupted, SchemaTooOld, SchemaTooNew}`. Source: app boot logic.

**Integration checkpoint:** corruption recovery MUST always succeed (the recovered state is empty cache + cold-start discovery, which is the ADR-003 baseline). The cache is never load-bearing for correctness — only for warm-start latency.

**Emotional entry:** confused (something happened).
**Emotional exit:** informed (banner explains what and where to read more).

---

## Cross-tool integration validation (carried forward from parent)

| Invariant | Steps involved | Failure mode |
|---|---|---|
| `total.disk_usage == sum(tool.disk_usage)` | 1, 2, 3 | Summary-bar drift; cache out of sync with reconciled state |
| `tool.model_count` on left pane == `tool.model_count` on detail screen | 1, 4a | User sees conflicting counts |
| `model.size` on right-pane row == `model.size_on_disk` on detail screen | 3, 4b | Per-row vs per-detail disagreement |
| `cache.as_of_timestamp` always reflects the *most recent* per-tool reconcile | 1, 2, 3 | Misleading freshness claim |
| Pre-mutate `(mtime, size, inode_dev)` check uses **filesystem**, not cache | 5 | THE critical rule — cache acting as source of truth for mutation = data loss |
| Cache corruption never blocks launch | 1, 6 | "modeltap won't start" — the failure mode ADR-003 specifically avoided |
| Schema migrations are idempotent and forward-only | 1, 6 | Partial migration state on retry |

## Open questions for DESIGN

| ID | Question |
|---|---|
| Q-INFO-1 | Does the `Tool` trait grow `inspect_tool()` and `inspect_model()` as required methods, or as a default-impl returning `NotSupported`? Implications for plugin author migration (parent's 4 plugins must update or no-op). |
| Q-INFO-2 | Cache location: `$XDG_DATA_HOME/modeltap/cache.sqlite` (preferred per XDG basedir spec, falls back to `~/.local/share/modeltap/` on Linux, `~/Library/Application Support/modeltap/` on macOS via `dirs` crate). Confirm and document. |
| Q-INFO-3 | Migration tooling: `rusqlite_migration`, hand-rolled SQL files run by an embedded migrator, or `sqlx::migrate!`? The `rusqlite_migration` crate is minimal and aligns with the project's preference for small dependency footprints; recommended but DESIGN owns the call. |
| Q-INFO-4 | Per-tool TTL: default 24 h reasonable? Per-tool override in config? Inheritance from a global `cache.default_ttl_seconds`? |
| Q-INFO-5 | Cache is on by default OR off by default in v1? Recommendation: **on by default** (the user explicitly asked for it; opt-out via `--no-cache` and `cache.enabled = false` in config). Confirm with DESIGN. |
| Q-INFO-6 | Concurrent-process behaviour during a mutating action: per intake Q5 the parent uses detect-and-prompt-then-retry for *tool* processes. Should SQLite cache writes use a similar "detect-and-prompt" if a peer modeltap process holds the WAL? Or rely entirely on `busy_timeout`? Recommendation: `busy_timeout` is sufficient for v1; revisit only if seen in dogfooding. |
