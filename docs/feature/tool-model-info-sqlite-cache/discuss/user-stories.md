<!-- markdownlint-disable MD024 -->

# User Stories — tool-model-info-sqlite-cache

Seven new stories (US-21..US-27) extending the parent `modeltap-tui` feature. Persona is shared with the parent: **Devon Park**, multi-tool local-AI power user on macOS or Linux. The story IDs continue the parent's numbering (US-01..US-20 + US-05b shipped or in DELIVER; US-05c shipping via folder-group-bulk-delete).

Each story traces to one or more job stories in `jtbd-job-stories.md` (N:1 mapping). Cross-references to parent stories are explicit where this feature extends, depends on, or interacts with prior work.

---

## US-21: Tool detail screen

### Problem

Devon Park has Ollama showing "(error)" in the left pane and needs to know: which path did the plugin scan, when was it last successful, what's the error text, what version of Ollama is installed, and which search paths are configured. Today this requires three separate commands (`ls -la ~/.ollama/models/`, `cat ~/.modeltap/diagnostics.log`, `ollama --version`) in a second terminal pane. The left-pane count "Ollama 12" is sufficient for navigation but useless for diagnosis. The intake brief leads with "Add an ability to get information about each tool," and this is the per-tool half.

### Who

- **Devon Park**, multi-tool local-AI power user, macOS Sonoma or Ubuntu 22.04, runs ≥2 of {Ollama, llama-cli, Hugging Face cache, LM Studio}, comfortable with vim-style keys. Hits this screen when a tool's row looks suspicious (high disk usage, "(error)" annotation, unexpected model count) or when triaging a bug for an issue report.

### Solution

Pressing Enter on a left-pane tool row opens a new tool detail screen showing the tool's discovery root, detected version (if available), configured search paths (default vs user-config-provided), model count, disk usage, largest model, last scan time, scan duration, last error (if any), and the plugin's version. Bottom bar offers `[Esc] back`, `[r] refresh this tool`, `[?] help`.

### Job links

- J2 — Audit a tool's health and inventory cost at a glance (Score 12.0 for O4, 14.0 for O8)

### Domain Examples

#### 1: Happy path — Devon checks Ollama after a routine `ollama pull`

Devon ran `ollama pull qwen2.5:32b-q4_K_M` 5 minutes ago in a separate terminal. He opens modeltap and presses Enter on the Ollama row. The detail screen shows: Discovery root `~/.ollama/models/`, Version `0.6.4 (detected via http://localhost:11434)`, Model count `13` (up from 12 last week), Disk usage `66.5 GB`, Largest model `qwen2.5:32b-q4_K_M (18.2 GB)`, Last scan `2026-05-16 09:14:22 (4 min ago)`, Plugin version `modeltap-plugin-ollama 0.2.6`. Devon confirms the pull is reflected, presses Esc, returns to the main view.

#### 2: Edge — Devon opens a tool with no detectable version

Devon presses Enter on the llama-cli row. The detail screen shows everything except Version, which reads `(not detectable)`. The Discovery root, Search paths (including the user-config-added `/data/models`), and other fields render normally. No false data is shown.

#### 3: Error — Devon opens a tool that errored at last scan

Ollama's `~/.ollama/models/manifests/` directory had its permissions corrupted to `chmod 000` (Devon `sudo`'d once and left it owned by root). The left pane shows "Ollama (error)". Devon presses Enter. The detail screen shows Last error `permission denied reading ~/.ollama/models/manifests/ (errno 13) at 2026-05-16 09:14:22`, Model count `(stale — last successful scan 2 days ago: 12)`. Devon fixes the permissions in a second terminal, returns to modeltap, presses `r`. The error clears.

### UAT Scenarios (BDD)

#### Scenario: Pressing Enter on a left-pane row opens the tool detail screen

Given Devon has Ollama selected in the left pane
When Devon presses Enter
Then the tool detail screen opens
And it shows Ollama's discovery root `~/.ollama/models/`
And it shows model count `12`, disk usage `47.3 GB`, last scan `2026-05-16 09:14:22 (4 min ago)`, and plugin version `modeltap-plugin-ollama 0.2.6`

#### Scenario: Undetectable version is shown as "(not detectable)"

Given a plugin's `inspect_tool()` returns no version (e.g., llama-cli static binary cannot self-introspect)
When Devon opens that tool's detail screen
Then the Version field reads "(not detectable)"
And no false or stale version is shown
And the rest of the detail screen renders normally

#### Scenario: Last error surfaces when discovery failed

Given Ollama's discovery failed at last scan with `permission denied` reading `~/.ollama/models/manifests/`
When Devon opens Ollama's detail screen
Then the Last error field shows `permission denied reading ~/.ollama/models/manifests/ (errno 13)` with the timestamp
And the bottom bar offers `[r] refresh this tool` to retry after fixing permissions

#### Scenario: User-configured search paths are labelled

Given Devon has added `search_paths = ["/data/models"]` to `~/.modeltap/config.toml` under `[plugins.llama-cli]`
When Devon opens the llama-cli detail screen
Then the Search paths section lists `~/llms/ (default)`, `~/models/ (default)`, and `/data/models/ (user config)`

#### Scenario: Esc returns to main view preserving left-pane cursor

Given Devon has the cursor on Ollama in the left pane
When Devon presses Enter to open the detail screen
And then presses Esc
Then the main view returns
And the cursor is still on Ollama in the left pane

### Acceptance Criteria

- [ ] AC-21-1: Pressing Enter on any left-pane tool row opens the tool detail screen within 100 ms
- [ ] AC-21-2: Detail screen shows discovery root, version (or "(not detectable)"), search paths (with default/user-config provenance), model count, disk usage, largest model, last scan time, scan duration, last error (if any), and plugin version
- [ ] AC-21-3: Version field is `Option<String>`; `None` renders as "(not detectable)" — never as empty or as a false value
- [ ] AC-21-4: Last error field shows the error text + timestamp when present; reads "(none)" when absent
- [ ] AC-21-5: Search paths section distinguishes default paths from user-config paths
- [ ] AC-21-6: `[r]` re-runs discovery for this tool, updates the detail screen and the left-pane row
- [ ] AC-21-7: `[Esc]` returns to main view with left-pane cursor preserved
- [ ] AC-21-8: Bottom bar shows `[Esc] back`, `[r] refresh this tool`, `[?] help` on the detail screen
- [ ] AC-21-9: Plugin panic during `inspect_tool()` is caught at the boundary; detail screen shows "(inspection failed — see diagnostics.log)" and other fields render with what `discover()` provided (extends parent US-18 panic-isolation invariant)

### Outcome KPIs

See `outcome-kpis.md`. This story drives:

- **K-INFO-5** — % of "(error)" investigations where Devon resolves the issue without leaving the TUI (target: ≥ 80%)
- **O4** (12.0) — Minimize time to diagnose why a specific tool shows "(error)"
- **O8** (14.0) — Minimize time to discover tool-specific metadata without leaving the TUI

### Technical Notes

- Requires `Tool::inspect_tool()` method addition (Q-INFO-1).
- Version detection per plugin: Ollama HTTP `/api/version`, others best-effort or `None` (Q-INFO-7).
- Renders from in-process state (Release 1 ships without cache); cache integration is automatic once Release 2 lands.
- Layout: single-column, label-aligned-left, value-aligned-right. Use existing ratatui widgets (`Paragraph`, `List`).

### Dependencies

- Parent US-03 (left-pane row selection) — exists.
- Parent US-08 (bottom bar) — extended with `[Enter]` for tool detail on main view.
- Parent US-18 (Tool trait) — extended with `inspect_tool()` (Q-INFO-1; DESIGN-open).
- No new ADRs required beyond Q-INFO-1 closure.

---

## US-22: Model detail screen with tool-native metadata

### Problem

Devon Park is about to run `mistral:7b-instruct-q4_K_M` via Ollama for a real task and wants to confirm: is this the same Mistral he downloaded last week, or did `ollama pull` upgrade it to a newer revision? What's the actual quantisation level (Q4_K_M vs Q4_K_S?)? What's the architecture's context length? Today these answers require leaving modeltap to run `gguf-dump`, `ollama show mistral:7b-instruct-q4_K_M`, or `cat ~/.cache/huggingface/.../config.json`. The existing US-13 detail screen shows paths + dedup key + reclaim estimate — useful for unify decisions but silent on tool-native metadata. The intake brief leads with "get information about each model," and this is the per-model half.

### Who

- **Devon Park**, same as US-21. Hits this screen before running a model for real work, before deciding to `[u] unify` or `[d] delete-one`, or when comparing two superficially-similar models.

### Solution

The existing US-13 model detail screen is extended with a Metadata section that surfaces tool-native introspection: for GGUF files the header KVs (`general.architecture`, `general.quantization_version`, `<arch>.context_length`, `<arch>.embedding_length`, `<arch>.block_count`, `tokenizer.ggml.model`); for Ollama models the manifest JSON fields (`config.architecture`, `parameters`, `template`); for HF-hosted models excerpts from `config.json` (`model_type`, `architectures`, `hidden_size`, `num_attention_heads`). A new `[r]` shortcut on this screen re-introspects; provenance reads "introspected <N> ago".

### Job links

- J1 — Verify a model is what I think it is (Score 15.5 for O1, 14.0 for O8)

### Domain Examples

#### 1: Happy path — Devon confirms Mistral-7B's quant level before running it

Devon presses Enter on the `mistral:7b-instruct-q4_K_M` row (Ollama selected). The extended detail screen opens. The Metadata section shows `general.architecture: llama`, `general.quantization_version: 2`, `llama.context_length: 32768`, `llama.embedding_length: 4096`, `llama.block_count: 32`. The provenance reads "introspected 2 min ago". Devon confirms this is Q4_K_M (quantization_version 2 = K-quants level 4) and proceeds with confidence.

#### 2: Edge — Devon re-introspects after a re-download

Devon ran `ollama pull mistral:7b-instruct-q4_K_M` an hour ago to get a newer build. He opens the detail screen; metadata reads "introspected 6 days ago" (from the old version). He presses `r`. The screen refreshes; `Tool::inspect_model()` re-parses the GGUF header; the metadata KVs update if anything changed; provenance reads "introspected just now". Devon sees that `general.quantization_version` is unchanged, so this is a content tweak not a quant-level change.

#### 3: Error — Corrupt GGUF file degrades gracefully

Devon opens detail for a `.gguf` file that was truncated during a failed download. The Format field reads "GGUF v3 (header partially readable)". The Metadata section shows "(introspection failed — see diagnostics.log)". The Size on disk, Dedup key, Registered with sections still render normally. Devon presses Esc, returns to the main view, and decides to `[d] delete-one` the corrupt file.

#### 4: Edge — HF-hosted model shows config.json excerpts instead of GGUF KVs

Devon opens detail for `meta-llama/Llama-3-8B` (in HF cache only, safetensors format). The Metadata section shows `model_type: llama`, `architectures: ["LlamaForCausalLM"]`, `hidden_size: 4096`, `num_attention_heads: 32`, `num_hidden_layers: 32`. The Format field reads `safetensors v2`.

### UAT Scenarios (BDD)

#### Scenario: Model detail surfaces GGUF header metadata

Given `mistral:7b-instruct-q4_K_M` is registered in 3 tools
And the file format is GGUF v3
When Devon presses Enter on the Mistral row
Then the model detail screen opens
And the Metadata section shows `general.architecture: llama`, `general.quantization_version: 2`, `llama.context_length: 32768`
And the Metadata section provenance reads "introspected <N> ago"

#### Scenario: Model detail surfaces Ollama manifest fields for Ollama models

Given `llama3:8b-instruct-q4_K_M` is registered in Ollama only
When Devon opens its model detail
Then the Metadata section shows excerpts from the Ollama manifest JSON: `config.architecture`, `parameters`, and `template`
And the Format field reads "Ollama manifest v2" (or the actual manifest format version)

#### Scenario: Model detail surfaces HF config.json fields for HF-only models

Given `meta-llama/Llama-3-8B` is in Hugging Face only (16.0 GB safetensors)
When Devon opens its model detail
Then the Metadata section shows excerpts from `config.json`: `model_type`, `architectures`, `hidden_size`, `num_attention_heads`, `num_hidden_layers`
And the Format field reads "safetensors v2"

#### Scenario: Re-introspect updates the metadata provenance

Given Devon is on the Mistral detail screen
And the metadata was introspected 2 hours ago
When Devon presses `r`
Then `Tool::inspect_model()` re-runs against the current file
And the Metadata section updates with new values if any
And the provenance reads "introspected just now"

#### Scenario: Un-introspectable file shows partial info gracefully

Given a model file's format cannot be parsed (corrupt GGUF or unknown)
When Devon opens its model detail
Then the Format field reads what could be detected (e.g., "GGUF v3 (header partially readable)")
And the Metadata section shows "(introspection failed — see diagnostics.log)"
And the screen does not crash
And the other panels (Registered with, Size on disk, Dedup key) still render

#### Scenario: Esc returns to main view preserving right-pane cursor

Given Devon has the cursor on the Mistral row in the right pane
When Devon presses Enter to open the detail screen
And then presses Esc
Then the main view returns
And the cursor is still on the Mistral row in the right pane

### Acceptance Criteria

- [ ] AC-22-1: Pressing Enter on any right-pane model row opens the model detail screen within 100 ms (when metadata is cached / in-process)
- [ ] AC-22-2: Re-introspection (`[r]` on detail screen) completes within 1 second for typical model files (GGUF headers are small; HF `config.json` is small)
- [ ] AC-22-3: Detail screen retains all existing US-13 fields (id, format, size, dedup key, registered tools, status, reclaim estimate) AND adds a Metadata section
- [ ] AC-22-4: Metadata section format is consistent across plugins: aligned key-value pairs, dim section header reading "Metadata (from <source>, introspected <N> ago)"
- [ ] AC-22-5: Per-plugin metadata source: GGUF header (llama-cli, LM Studio for GGUF files); Ollama manifest JSON (Ollama); HF config.json (HF cache); plugin-defined for new plugins
- [ ] AC-22-6: Plugin's `Tool::inspect_model()` returns `BTreeMap<String, String>` with selected KVs; "selected" means tool-relevant subset (not the entire GGUF KV table; not the entire HF config.json)
- [ ] AC-22-7: Un-introspectable files (corrupt headers, unknown formats) show "(introspection failed — see diagnostics.log)" instead of crashing; other panels still render
- [ ] AC-22-8: `[r]` on detail screen re-runs `inspect_model()` and updates the provenance timestamp
- [ ] AC-22-9: `[Esc]` returns to main view with right-pane cursor preserved
- [ ] AC-22-10: Bottom bar on detail screen shows `[Esc] back`, `[u] unify` (dimmed when not unifiable), `[d] delete-one`, `[r] re-introspect`, `[?] help`

### Outcome KPIs

See `outcome-kpis.md`. This story drives:

- **O1** (15.5) — Minimize time to confirm a model's quantisation, format, and dedup identity
- **O8** (14.0) — Minimize time to discover tool-specific metadata without leaving the TUI
- **K-INFO-6** — % of detail-screen opens that result in a downstream action (unify, delete, dismiss-confidently) without leaving the TUI (target: ≥ 90%)

### Technical Notes

- Requires `Tool::inspect_model()` method addition (Q-INFO-1).
- Per-plugin `inspect_model()` impls:
  - **Ollama:** read `~/.ollama/models/manifests/<repo>/<tag>` JSON; extract `config.architecture`, `parameters`, `template`, `system`.
  - **llama-cli:** parse GGUF header via `gguf` crate (or hand-rolled minimal parser); extract `general.*` and `<arch>.*` KVs.
  - **HF:** if model dir contains `config.json`, read it; extract `model_type`, `architectures`, `hidden_size`, `num_attention_heads`, `num_hidden_layers`, `max_position_embeddings`.
  - **LM Studio:** GGUF header parse if file is .gguf; otherwise the model's `model.json` config if present.
- Selected-KV list per plugin lives in plugin code, not core; this respects plugin autonomy per parent C1.
- Caching: in Release 1 ships, metadata is cached in-process for the session. Release 2's cache makes it persistent.

### Dependencies

- Parent US-13 (existing model detail screen) — extended.
- Parent US-18 (Tool trait) — extended with `inspect_model()` (Q-INFO-1; DESIGN-open).
- US-21 — shares the new trait method scaffolding.
- No new ADRs required beyond Q-INFO-1 closure.

---

## US-23: SQLite-backed cache schema, recovery, and concurrency

### Problem

Devon Park's cache file at `~/.local/share/modeltap/cache.sqlite` could be corrupted (failed write during a power loss, bad block on the SSD, accidental rsync over the file). Without explicit handling, modeltap would fail to start the next time he runs it — exactly the kind of "tool broke because of its own state" failure that ADR-003 was written to avoid. He also runs two terminal panes with modeltap open simultaneously sometimes (forgot one was open; comparing two states) and needs both processes to share the cache safely. And when modeltap ships a new schema (say, US-27 adds the `cache.sha256` table), the old cache file needs to migrate forward smoothly. This story delivers the SQLite layer with corruption-recovery, schema migration, concurrent-process safety, and a `--no-cache` opt-out — the infrastructure that all other cache-dependent stories build on.

### Who

- **Devon Park** (the user-facing failure-recovery surface)
- **Jeff Bailey** (the maintainer, who needs schema migrations to ship safely)

### Solution

A new `modeltap-store` crate exposing the cache layer. SQLite file at `$XDG_DATA_HOME/modeltap/cache.sqlite` (or platform equivalent via `dirs::data_dir()`). Opens with `PRAGMA journal_mode=WAL` and `PRAGMA busy_timeout=5000`. Schema versioned via `PRAGMA user_version`; `rusqlite_migration` framework runs forward migrations from cache version to binary's `EXPECTED_SCHEMA_VERSION`. On `SQLITE_CORRUPT` or schema-mismatch-with-no-migration, the file is renamed to `cache.sqlite.corrupt-<timestamp>`, a log line is written, and cold-start proceeds. `--no-cache` CLI flag (and `cache.enabled = false` config) bypasses all cache I/O.

### Job links

- J3 — Trust modeltap's first-paint inventory across launches (Score 13.5 for O2; ENABLER for the J3 jobs)
- J6 — Recover from a corrupted or unreadable cache (mandatory guardrail)
- J7 — Run multiple modeltap processes without corruption (table-stakes)

### Domain Examples

#### 1: Happy path — Fresh install, no cache yet

Devon installs modeltap for the first time. Runs `modeltap`. `cache.sqlite` doesn't exist. The cache layer creates an empty DB at `~/.local/share/modeltap/cache.sqlite`, applies all migrations (creating `cache.meta`, `cache.tools`, `cache.models` tables; setting `PRAGMA user_version = 1`), then hands off to the cold-start path. Devon sees the inventory paint via the parent's ADR-003 skeleton-then-discover flow. On exit, the cache file contains the discovered inventory.

#### 2: Edge — Schema migration from v1 to v3 on launch

Devon upgrades modeltap from version 0.3.0 (cache schema v1) to version 0.5.0 (cache schema v3). The first launch detects `PRAGMA user_version = 1` and `EXPECTED_SCHEMA_VERSION = 3`. `rusqlite_migration` runs `migrations/0002_*.sql` then `migrations/0003_*.sql`. After each migration, `PRAGMA user_version` is bumped. `~/.modeltap/diagnostics.log` records `cache_migration from=1 to=3 status=ok` with per-step timings. Devon doesn't notice anything beyond a brief 200 ms additional launch time.

#### 3: Error — Cache file corrupted by a power loss mid-write

Devon's laptop dies mid-cache-write (likely scenario: low battery during a `cache.write_tool()` transaction). On next launch, SQLite reports `SQLITE_CORRUPT` when reading the file. The cache layer catches this, renames the file to `~/.local/share/modeltap/cache.sqlite.corrupt-2026-05-16T091422`, logs `cache_recovery reason=corrupted renamed_to=<path>`, and proceeds with cold-start. Devon sees a banner in the TUI: "Previous cache reset (corrupted or schema mismatch). Renamed to ~/.local/share/modeltap/cache.sqlite.corrupt-2026-05-16T091422. Cold-start discovery in progress. See ~/.modeltap/diagnostics.log." Devon presses Esc, dismisses the banner, continues working.

#### 4: Edge — Concurrent reads from two modeltap processes

Devon has terminal pane A running `modeltap`. He opens terminal pane B and runs `modeltap`. Process A is mid-reconcile (writing `cache.tools` rows). Process B opens the cache, sees the WAL-mode journal, proceeds to read its snapshot of the tools table. Both processes display consistent inventory (each from its own snapshot — process B sees the state as of when it opened). Neither process crashes; neither displays a `SQLITE_BUSY` error.

#### 5: Edge — Concurrent write contention

Process A and process B both finish their reconciles within ~10 ms of each other and both try to write the updated `cache.tools` rows. Process A's write transaction begins first; process B's `BEGIN IMMEDIATE` blocks. `busy_timeout=5000` gives process B up to 5 seconds to wait. Process A commits in ~30 ms; process B proceeds with its write. Both succeed.

#### 6: Edge — `--no-cache` flag bypasses cache entirely

Devon runs `modeltap --no-cache` for a debugging session. The cache file is not opened, not created if absent, not written to. The launch follows the pure ADR-003 stateless rediscovery path. Performance matches the pre-cache baseline exactly.

#### 7: Error — Downgrade detected (cache from a newer binary)

Devon switches between modeltap versions for testing. The cache file has `PRAGMA user_version = 5` but the current binary's `EXPECTED_SCHEMA_VERSION = 3`. The cache layer detects the downgrade, renames the file to `~/.local/share/modeltap/cache.sqlite.future-version-5`, logs the situation, and starts cold. The recovery banner explains: "Cache was written by a newer modeltap version (schema v5; this binary supports v3). Renamed to <path>. Cold-start in progress."

### UAT Scenarios (BDD)

#### Scenario: Fresh install creates empty cache and applies all migrations

Given the cache file does not exist
When Devon runs `modeltap`
Then the cache layer creates `~/.local/share/modeltap/cache.sqlite` (or the equivalent platform path)
And all schema migrations apply cleanly
And `PRAGMA user_version` matches the binary's `EXPECTED_SCHEMA_VERSION`
And the launch proceeds via cold-start

#### Scenario: Schema migration runs forward when binary expects a newer schema

Given the cache `PRAGMA user_version` is 1
And the binary's `EXPECTED_SCHEMA_VERSION` is 3
When Devon runs `modeltap`
Then the migrator runs migration `0002_*.sql` then `0003_*.sql` in order
And the cache `PRAGMA user_version` becomes 3
And `~/.modeltap/diagnostics.log` records `cache_migration from=1 to=3 status=ok`
And the launch proceeds normally

#### Scenario: Cache corruption is detected on open and recovered

Given the cache file `~/.local/share/modeltap/cache.sqlite` exists but returns `SQLITE_CORRUPT` on open
When Devon runs `modeltap`
Then the cache file is renamed to `~/.local/share/modeltap/cache.sqlite.corrupt-<timestamp>` matching the regex `cache\.sqlite\.corrupt-\d{4}-\d{2}-\d{2}T\d{6}`
And a recovery banner appears
And `~/.modeltap/diagnostics.log` gains a line tagged `cache_recovery reason=corrupted`
And cold-start discovery proceeds without crashing

#### Scenario: Downgrade detected — cache from a newer binary

Given the cache `PRAGMA user_version` is 5
And the current binary's `EXPECTED_SCHEMA_VERSION` is 3
When Devon runs `modeltap`
Then the cache file is renamed to `~/.local/share/modeltap/cache.sqlite.future-version-5`
And the recovery banner explains the downgrade and the rename target
And cold-start discovery proceeds

#### Scenario: `--no-cache` bypasses the cache for one launch

Given a valid cache file exists at `~/.local/share/modeltap/cache.sqlite`
When Devon runs `modeltap --no-cache`
Then the cache file is neither opened nor written
And the launch follows the ADR-003 stateless rediscovery path

#### Scenario: Two modeltap processes can read concurrently (SQLite WAL)

Given a first modeltap process is reading the cache (`PRAGMA journal_mode=WAL`)
When a second modeltap process opens the same cache file
Then both processes coexist without `SQLITE_BUSY` errors during reads
And both processes display consistent inventory data (each from its own snapshot)

#### Scenario: Concurrent cache writes serialise via busy_timeout

Given two modeltap processes are running with cache writes enabled
And process A is mid-transaction writing a `cache.tools` row update
When process B attempts to write a `cache.tools` row update
Then process B waits up to 5 seconds (`PRAGMA busy_timeout=5000`)
And process B's write succeeds after process A commits
And neither process crashes or returns an error to the user

### Acceptance Criteria

- [ ] AC-23-1: Cache file location resolves via `dirs::data_dir().join("modeltap/cache.sqlite")`, overridable via `MODELTAP_CACHE_PATH` env var
- [ ] AC-23-2: SQLite opens with `PRAGMA journal_mode=WAL` and `PRAGMA busy_timeout=5000`
- [ ] AC-23-3: Schema version is stored in `PRAGMA user_version`; compared at launch against compile-time `EXPECTED_SCHEMA_VERSION`
- [ ] AC-23-4: Forward migrations run automatically when cache version < binary expected; each step logged
- [ ] AC-23-5: Cache version > binary expected (downgrade) renames file to `.future-version-<n>` and starts cold; recovery banner explains
- [ ] AC-23-6: `SQLITE_CORRUPT` on open renames file to `.corrupt-<timestamp>` and starts cold; recovery banner explains; log line tagged `cache_recovery reason=corrupted`
- [ ] AC-23-7: Recovery banner appears at top of main view; dismissable with `[Esc]`; never blocks the launch
- [ ] AC-23-8: `--no-cache` CLI flag results in ZERO bytes written to the cache file or its location for the launch (integration-tested)
- [ ] AC-23-9: `[cache] enabled = false` config option in `~/.modeltap/config.toml` has the same effect as `--no-cache`; CLI flag wins when both present
- [ ] AC-23-10: Two concurrent modeltap processes can read and write the cache via SQLite WAL + busy_timeout; neither crashes; writes serialise correctly
- [ ] AC-23-11: Cache failure (corruption, schema mismatch, file permissions) NEVER prevents modeltap from reaching the inventory view — cold-start fallback ALWAYS succeeds
- [ ] AC-23-12: Cache stays local; no network I/O introduced by this feature

### Outcome KPIs

See `outcome-kpis.md`. This story drives:

- **K-INFO-4** — Cache corruption recovery rate (target: 100% of detected corruption events result in successful cold-start)
- **K-INFO-7** — % of launches where cache layer adds < 50 ms to startup time (target: ≥ 95%)
- **O9** (mandatory guardrail) — Minimize likelihood of cache corruption causing data loss
- **O10** (mandatory guardrail) — Minimize likelihood of concurrent-process corruption

### Technical Notes

- New crate: `modeltap-store`. Depends on `rusqlite` and `rusqlite_migration` (Q-INFO-3 recommendation).
- Cache schema (initial migration `0001_initial.sql`):
  ```sql
  CREATE TABLE cache_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
  CREATE TABLE cache_tools (
    tool_id TEXT PRIMARY KEY,
    install_path TEXT NOT NULL,
    detected_version TEXT,
    plugin_version TEXT NOT NULL,
    model_count INTEGER NOT NULL,
    disk_usage_bytes INTEGER NOT NULL,
    last_scan_at TEXT NOT NULL,
    last_scan_duration_ms INTEGER NOT NULL,
    last_error TEXT,
    last_error_at TEXT,
    search_paths_json TEXT NOT NULL
  );
  CREATE TABLE cache_models (
    model_id TEXT NOT NULL,
    tool_id TEXT NOT NULL,
    path TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    format TEXT,
    quantisation TEXT,
    architecture TEXT,
    parameters TEXT,
    context_length INTEGER,
    metadata_kv_json TEXT,
    metadata_introspected_at TEXT,
    mtime INTEGER NOT NULL,
    inode_dev INTEGER NOT NULL,
    PRIMARY KEY (model_id, tool_id),
    FOREIGN KEY (tool_id) REFERENCES cache_tools(tool_id)
  );
  ```
  (DESIGN may refine; this is a baseline.)
- `rusqlite_migration` is preferred over hand-rolled or `sqlx::migrate!` per `prioritization.md`.
- Recovery rename uses a deterministic timestamp format `YYYY-MM-DDTHHMMSS` for sortability.
- `--no-cache` plumbed through `modeltap-app::Config` from `clap`.

### Dependencies

- New crate `modeltap-store` added to workspace.
- DESIGN ADR superseding ADR-003 must be written (closes the 9 items in `requirements.md` "What the new ADR must close").
- DESIGN ADR for schema migration strategy (Q-INFO-3).

---

## US-24: Manual refresh hotkeys and provenance line

### Problem

Devon Park downloads a new Ollama model in a separate terminal (`ollama pull qwen2.5:32b`) and alt-tabs back to modeltap to confirm it landed. Today his only option is to quit and relaunch, which is a ~1.15 s penalty and breaks his flow. With the cache shipping in US-23, Devon also needs to be able to *see* how stale his current view is — "as of 14 minutes ago" tells him whether to trust the indicator or to refresh. This story adds the user-facing surface that makes the cache feel honest.

### Who

- **Devon Park**, same persona. Hits this surface during any download / install workflow where modeltap is open in parallel.

### Solution

The summary bar always shows a provenance line: "Total: N GB | M models | as of <timestamp>[, reconciling...]". The bottom bar gains `[r] refresh tool` and `[Shift+R] refresh all` shortcuts. `[r]` re-runs `discover()` for the currently-selected tool and updates the cache; `[Shift+R]` does it for all tools in parallel. Both are no-ops when a dialog is open (the shortcuts are dimmed in those contexts per parent US-08).

### Job links

- J3 — Trust modeltap's first-paint inventory across launches (provenance line)
- J4 — Diagnose why a model I expected isn't showing up (`[r]` refresh)

### Domain Examples

#### 1: Happy path — Devon refreshes Ollama after `ollama pull`

Devon ran `ollama pull qwen2.5:32b` in another terminal, then alt-tabbed to modeltap. The summary bar reads "Total: 138.4 GB | 58 models | as of 4 min ago". Devon presses `r`. A spinner appears next to the Ollama row; the summary bar reads "refreshing Ollama...". Within 500 ms the spinner clears; the summary bar reads "Total: 156.6 GB | 59 models | as of just now (Ollama refreshed)". The new Qwen2.5 row appears in the right pane.

#### 2: Edge — Devon refreshes all tools at startup

Devon was on holiday for a week; modeltap's cache `last_scan_at` is older than the per-tool TTL (24h) for all tools, so they cold-start anyway. He launches modeltap, waits for the cold-start to complete, then presses `Shift+R` "just to be sure". All four tool spinners activate; within 2 seconds all clear. Devon confirms nothing has changed.

#### 3: Error — Refresh attempted while dialog is open

Devon has the unify dialog open. He absent-mindedly presses `r`. Nothing happens. The `[r] refresh tool` shortcut in the bottom bar is visibly dimmed.

#### 4: Edge — Provenance line during background reconcile

Devon launches modeltap (warm start). The summary bar shows "as of 14 min ago, refreshing...". As each tool's background reconcile completes, the text updates: "as of just now (Ollama refreshed)", then "as of just now (Ollama, llama-cli refreshed)", etc. After all tools complete, the text settles on "as of just now".

### UAT Scenarios (BDD)

#### Scenario: `[r]` refreshes the selected tool

Given Devon has Ollama selected in the left pane
And no dialog is open
When Devon presses `r`
Then a spinner appears next to the Ollama row
And the summary bar reads "refreshing Ollama..."
And within 1 second the spinner clears and the summary bar reads "as of just now (Ollama refreshed)"
And the `cache.tools` row for Ollama updates with the new `last_scan_at`

#### Scenario: `[Shift+R]` refreshes all tools in parallel

Given no dialog is open
When Devon presses Shift+R
Then all four tool rows show the per-tool spinner
And the summary bar reads "refreshing all tools..."
And within 2 seconds all spinners clear
And the `cache.tools` rows for every tool are updated

#### Scenario: `[r]` is a no-op when a dialog is open

Given the unify dialog is open
When Devon presses `r`
Then no refresh is triggered
And the `[r] refresh tool` shortcut in the bottom bar is dimmed
And the unify dialog state is preserved

#### Scenario: Provenance line always shows freshness

Given modeltap has just completed a warm-start paint
When the summary bar renders
Then it shows "Total: <X> GB | <Y> models | as of <Z> ago[, reconciling...]"
And the timestamp updates as reconcile progresses

### Acceptance Criteria

- [ ] AC-24-1: Summary bar shows a provenance line at all times: "as of <X>" where X is human-readable ("just now", "<N> min ago", "<N> hours ago", "<N> days ago")
- [ ] AC-24-2: During reconcile, the provenance line appends ", reconciling..." (or ", refreshing Ollama..." for targeted refresh)
- [ ] AC-24-3: `[r]` keystroke triggers per-tool reconcile of the currently-selected left-pane tool
- [ ] AC-24-4: `[Shift+R]` keystroke triggers parallel reconcile of all tools
- [ ] AC-24-5: Both shortcuts are no-ops when any dialog is open (unify, zap, delete-one, folder-delete, detail screens); bottom bar dims them in those contexts
- [ ] AC-24-6: Bottom bar always shows `[r] refresh tool` and `[Shift+R] refresh all` in the main view
- [ ] AC-24-7: After refresh completes, the provenance line updates to "as of just now (<scope> refreshed)"
- [ ] AC-24-8: Refresh updates the `cache.tools.last_scan_at` for the affected tools
- [ ] AC-24-9: Manual refresh latency is ≤ 1 s for a typical single tool (Ollama 12 models, HF 31 models)

### Outcome KPIs

See `outcome-kpis.md`. This story drives:

- **O5** (12.5) — Minimize time to reflect an out-of-band model change in modeltap
- **K-INFO-2** — Manual refresh wall-clock (target: p50 ≤ 500 ms, p90 ≤ 1 s for typical tool)

### Technical Notes

- New keymap entries in `modeltap-tui::input::keymap::SHORTCUT_TABLE` (parent US-08's single source of truth).
- Provenance line is a computed view-model field; updates on every render based on `cache.last_full_reconcile_at`.
- Reconcile dispatch goes through the same orchestrator as the launch-time background reconcile (US-26) — code reuse.
- Spinner widget: existing ratatui `Spinner` from the parent feature's loading-state pattern.

### Dependencies

- US-23 (cache exists)
- US-26 (reconcile orchestrator)
- Parent US-08 (bottom bar)

---

## US-25: Warm-start cache read

### Problem

Devon Park opens modeltap multiple times per day to check disk. Each launch today takes ~1.15 s for the full inventory to paint (parent K3). With the cache from US-23, modeltap can paint the last-known inventory in <100 ms from process start — an order-of-magnitude improvement that transforms modeltap from "always loading" to "instantly there." This story delivers the warm-start read path: when the cache exists and is valid, paint the cached inventory before any disk I/O against tool directories.

### Who

- **Devon Park**, same persona. Hits this story on every launch after the first.

### Solution

On launch, if `cache.enabled` and the cache file exists and `PRAGMA user_version` matches `EXPECTED_SCHEMA_VERSION`, read the `cache_tools` and `cache_models` tables. Paint the inventory from the cached data within 100 ms of process start. Stamp the summary bar with "as of <cache.last_full_reconcile_at> ago, refreshing..." (the actual reconcile is US-26's responsibility). If the cache is empty (fresh install) or any tool's `last_scan_at` is older than the per-tool TTL, that tool cold-starts via the existing ADR-003 path.

### Job links

- J3 — Trust modeltap's first-paint inventory across launches (Score 13.5 for O2)

### Domain Examples

#### 1: Happy path — Warm-start instant paint

Devon has been using modeltap for a week; the cache contains his full inventory from 4 hours ago (within TTL). He runs `modeltap`. Within ~80 ms, the two-pane layout paints with the cached inventory: 4 tools in the left pane, 12 Ollama models in the right pane (Ollama is the default selection). The summary bar reads "Total: 138.4 GB | 58 models | as of 4 hours ago, reconciling...". The background reconcile (US-26) runs in parallel; within ~1.15 s the provenance updates to "as of just now".

#### 2: Edge — Cold start (no cache)

Devon installs modeltap fresh. No cache exists. The launch falls through to the cold-start path; ADR-003 skeleton paints within 150 ms; full inventory paints within 1.15 s. The summary bar reads "as of just now" from the moment the inventory is built. (US-23 already creates the empty cache; US-25's logic detects the empty state and routes to cold start.)

#### 3: Edge — Mixed warm/cold per-tool (one tool stale)

Devon ran modeltap yesterday at 9 AM and again today at 10:30 AM. Three tools have `last_scan_at` from yesterday (~25h ago, exceeds TTL); one tool was manually refreshed at 8 AM today (~2.5h ago, within TTL). On launch, the in-TTL tool's models paint from cache instantly; the stale tools' rows show the skeleton spinner while their cold-start discovery runs. The summary bar reads "as of <varies per-tool>, reconciling...".

### UAT Scenarios (BDD)

#### Scenario: Warm-start paints cached inventory within 100 ms

Given the cache file exists and is valid
And the cache contains inventory data with `last_scan_at` within the per-tool TTL (24h)
When Devon runs `modeltap`
Then the TUI paints the cached inventory within 100 ms of process start
And the summary bar shows "as of <N> ago, refreshing..."

#### Scenario: Cold start falls back to skeleton paint when no cache

Given the cache exists but contains no inventory rows
When Devon runs `modeltap`
Then the TUI paints the skeleton "discovering..." placeholders within 150 ms
And full inventory paints within 1.15 s (matching parent K3)

#### Scenario: Mixed warm/cold per-tool when some entries are stale

Given the cache contains Ollama inventory with `last_scan_at` 25 hours ago (exceeds TTL)
And the cache contains llama-cli inventory with `last_scan_at` 2 hours ago (within TTL)
When Devon runs `modeltap`
Then llama-cli's models paint from cache instantly
And Ollama's left-pane row shows the cold-start spinner
And Ollama's cold-start discovery proceeds in parallel

### Acceptance Criteria

- [ ] AC-25-1: When cache is valid and contains in-TTL data, first paint completes within 100 ms of process start
- [ ] AC-25-2: Per-tool TTL eligibility: tools with `last_scan_at` older than `cache.tool_ttl_seconds` (default 86400) do NOT paint from cache; they cold-start
- [ ] AC-25-3: Cold-start path (no cache, empty cache, or tool TTL-exceeded) preserves parent K3 (≤ 150 ms skeleton, ≤ 1.15 s full inventory)
- [ ] AC-25-4: Mixed warm/cold per-tool is supported — some tools paint from cache while others cold-start in parallel
- [ ] AC-25-5: Summary bar's provenance line is set at warm-paint time based on `MAX(tool.last_scan_at)` for cache-painted tools
- [ ] AC-25-6: Cache read failure (transient I/O error mid-read) falls back to cold-start for the affected tool; never crashes the launch (C-INFO-2)
- [ ] AC-25-7: `--no-cache` and `cache.enabled = false` skip the warm-paint path entirely; cold-start is used

### Outcome KPIs

- **K-INFO-1** — Warm-start first paint (target: p50 ≤ 80 ms, p90 ≤ 150 ms)
- **O2** (13.5) — Minimize warm-start launch time

### Technical Notes

- Read path: `Cache::tools()` returns `Vec<CachedTool>`; `Cache::models_for_tool(tool_id)` returns `Vec<CachedModel>`. Both are simple SELECT queries indexed on `tool_id`.
- The "valid" check is: cache exists, `PRAGMA user_version` matches, opens cleanly. US-23 owns the corruption-recovery path.
- Per-tool TTL eligibility is decided per-row at warm-paint time; the per-row eligibility result feeds the reconcile orchestrator (US-26).
- Cache read is synchronous and small (KB scale); does not need tokio.

### Dependencies

- US-23 (cache exists; recovery / `--no-cache` handled)
- US-26 (background reconcile picks up after warm paint)

---

## US-26: Background reconcile + pre-mutate revalidation (cache safety rule)

### Problem

Warm-start paint (US-25) shows Devon Park the inventory from his last session. Between then and now, the filesystem may have changed: `ollama pull` added a model, `huggingface-cli delete-cache` removed one, a file's mtime changed because Devon manually replaced it. modeltap must reconcile the cached view with the actual filesystem state — otherwise Devon acts on stale data. Worse, if a destructive action (unify, zap, delete-one, folder-delete) uses cached file metadata, it could target a file that no longer exists, or hardlink to a file whose content has changed since the cache was written. This story delivers the two halves of the cache safety contract: (1) background reconcile after warm paint updates the cache; (2) pre-mutate revalidation re-stats target files before any destructive action — the cache is paint-only, the filesystem is authoritative on mutate.

### Who

- **Devon Park**, same persona. Hits this story implicitly on every launch (background reconcile) and explicitly on every destructive action (pre-mutate revalidation).

### Solution

After warm-start paint, the existing parallel-per-plugin `discover()` orchestrator from ADR-003 runs. Each plugin's reconcile updates the corresponding `cache.tools` and `cache.models` rows atomically. A failing per-tool reconcile keeps the stale cache entries visible with an "(error)" annotation (extends parent US-02 behaviour). When discovered inventory differs from cached inventory (model added, removed, size changed), a silent ack indicator (a blue `*` next to the tool name for 3 seconds) signals the change.

Before any destructive filesystem action (`unify`, `zap`, `delete_one`, `folder_delete`), the affected file paths are re-stat'd against their cache entries via `(mtime, size, inode_dev)`. Drift → re-introspect before action (dialog refreshes). File gone → abort with the parent's pre-flight refusal pattern (F-FGD-8): "file no longer exists; refreshing inventory", auto-trigger a refresh.

### Job links

- J3 — Trust modeltap's first-paint inventory (background reconcile completes the contract)
- O3 (11.0 guardrail) — Don't act on stale data (pre-mutate revalidation)

### Domain Examples

#### 1: Happy path — Background reconcile completes silently

Devon launches modeltap. Warm-start paint shows the inventory from 4 hours ago. Background reconcile runs; all four plugins complete within ~1.15 s. No inventory changes detected. The summary bar updates to "as of just now". No visible indicators beyond the provenance update. Devon doesn't notice.

#### 2: Edge — Inventory diff triggers silent ack indicator

Devon ran `ollama pull qwen2.5:32b-q4_K_M` last night after closing modeltap. He launches modeltap this morning; warm paint shows yesterday's Ollama inventory (12 models). Background reconcile finishes for Ollama; discovers 13 models. The Ollama row updates to "13"; a tiny blue `*` appears next to the Ollama name in the left pane for 3 seconds, then fades. The new model row appears in the right pane.

#### 3: Edge — Per-tool reconcile failure keeps stale cache visible

Ollama's `~/.ollama/models/` was made unreadable (Devon's `sudo chmod 000`). Warm paint shows the cached Ollama inventory. Background reconcile fails for Ollama (`permission denied`). The cache.tools row for Ollama is NOT overwritten (preserves last-known-good); the left-pane Ollama row gains "(error)" annotation; `~/.modeltap/diagnostics.log` records `reconcile_failed tool=ollama reason=permission_denied`. Devon can still see his cached inventory, navigate, and inspect — he just knows it's stale.

#### 4: Happy path — Pre-unify validation passes

Devon presses `u` on a Mistral row. The pre-mutate validator re-stats all 3 paths against cache. All match. The unify dialog opens normally; Devon confirms; unify proceeds. Cache is updated post-action.

#### 5: Edge — Pre-unify validation detects drift

Devon presses `u` on a Mistral row. The pre-mutate validator re-stats; one file's mtime has changed since the cache write (Devon `touch`ed it earlier). The validator triggers a re-introspect on the drifted file. The unify dialog refreshes with the new dedup-key / size; Devon must re-confirm if the reclaim amount changed.

#### 6: Error — Pre-mutate validation aborts when file is gone

Devon presses `d` (delete-one) on a row. The pre-mutate validator re-stats; the file no longer exists (Devon deleted it manually 5 minutes ago). The action aborts with "file no longer exists; refreshing inventory". An automatic refresh runs for that tool. The right pane updates; the row disappears.

### UAT Scenarios (BDD)

#### Scenario: Background reconcile updates the cache after warm-start paint

Given the warm-start paint has completed from cached data
When the background reconcile finishes for Ollama at 09:14:22
Then the `cache.tools` row for Ollama updates with `last_scan_at=2026-05-16T09:14:22`
And the right-pane re-renders if the inventory changed
And the summary bar provenance updates to "as of just now"

#### Scenario: Inventory diff shows silent ack indicator

Given the cache shows Ollama with 12 models
And the user ran `ollama pull qwen2.5:32b-q4_K_M` in another terminal between launches
When Devon runs `modeltap` and the background reconcile completes for Ollama
Then the Ollama left-pane row updates to 13 models
And a tiny blue `*` appears next to the Ollama row name for 3 seconds
And no modal or dialog is shown

#### Scenario: Failed reconcile keeps the stale cache visible

Given Ollama's directory becomes unreadable between launches (chmod 000)
When Devon runs `modeltap` and the Ollama reconcile fails
Then the cached Ollama inventory remains painted
And Ollama's left-pane row shows "(error)" alongside the cached model count
And `~/.modeltap/diagnostics.log` gains a line tagged `reconcile_failed tool=ollama reason=permission_denied`
And the `cache.tools` row for Ollama is NOT overwritten (preserves last-known-good)

#### Scenario: Pre-unify validation passes when cache matches filesystem

Given Mistral-7B is registered in 3 tools per the cache
And all 3 files match the cached `(mtime, size, inode_dev)` tuple
When Devon presses `u` and confirms the dialog
Then the unify proceeds normally
And post-action the cache is updated with the new hardlink state

#### Scenario: Pre-unify validation re-introspects on stat drift

Given Mistral-7B is registered in 3 tools per the cache
And one file's mtime has changed since the last cache write
When Devon presses `u`
Then the validator triggers a re-introspection of the drifted file before opening the confirmation dialog
And the dedup-key / size for the drifted file is recomputed
And Devon is shown the (possibly updated) reclaim estimate
And Devon must re-confirm if the reclaim amount changed by more than rounding

#### Scenario: Pre-mutate validation aborts when a file no longer exists

Given a model is registered in 2 tools per the cache
And one file has been deleted out-of-band between launch and Devon's action
When Devon attempts to unify
Then the pre-flight check refuses with "file no longer exists; refreshing inventory"
And no destructive action occurs
And an automatic per-tool refresh is triggered for the affected tool

### Acceptance Criteria

- [ ] AC-26-1: After warm-start paint, the existing parallel-per-plugin `discover()` orchestrator runs without user action
- [ ] AC-26-2: Successful per-tool reconcile atomically updates `cache.tools` and `cache.models` rows (single transaction per tool)
- [ ] AC-26-3: Failed per-tool reconcile leaves the cache rows unchanged (last-known-good preserved); left-pane row shows "(error)"; log line written
- [ ] AC-26-4: Inventory diff detection: when reconciled inventory differs from cache (rows added, removed, size changed), a blue `*` indicator appears next to the tool name for 3 seconds
- [ ] AC-26-5: Pre-mutate revalidation: every destructive action (`u`, `z`, `d`, `F`) re-stats target files via `std::fs::metadata()` against `cache.models.(mtime, size, inode_dev)`
- [ ] AC-26-6: Pre-mutate drift → re-introspect (via `Tool::inspect_model()`); dialog refreshes; user re-confirms if numbers changed
- [ ] AC-26-7: Pre-mutate file-gone → abort with "file no longer exists; refreshing inventory"; auto-trigger per-tool refresh
- [ ] AC-26-8: Integration test asserts every mutation site goes through the revalidator (no unguarded calls to `hard_link`, `remove_file`, `rename` against model paths)
- [ ] AC-26-9: Background reconcile completes within ~1.15 s for typical 4-plugin inventory (matches parent K3 budget)

### Outcome KPIs

- **K-INFO-3** — Cache hit ratio: % of warm-start tool entries that were within TTL and painted from cache (target: ≥ 80% for active users)
- **O3** (11.0 guardrail) — Don't act on stale data
- **K5** (parent guardrail) — Zero accidental data loss — extended by pre-mutate revalidation invariant

### Technical Notes

- Background reconcile dispatches via the same `Cmd::StartDiscovery` from ADR-006 with a new `ReconcileScope::{All, Tool(id)}` parameter.
- Pre-mutate revalidation is a single function `Cache::verify_against_fs(model_id) -> ValidationResult { Match, Drift(new_stat), Gone }`.
- Every existing destructive code path is augmented with a call to this validator before mutation.
- Integration tests: `tests/acceptance/cache_safety.rs::pre_mutate_revalidation_invoked_on_unify` (and parallel tests for zap, delete-one, folder-delete).
- The silent-ack indicator is a 3-second timer in `AppState` that triggers on diff detection; clears on next render after timeout.

### Dependencies

- US-23 (cache exists)
- US-25 (warm-start paint completes before reconcile starts)
- Parent ADR-006 (Elm-style orchestrator extended with new Msg variants)
- Parent US-05 (zap), US-05b (delete-one), US-05c (folder-delete from folder-group-bulk-delete), US-10 (unify) — all gain pre-mutate revalidation

---

## US-27: Persisted SHA256 cache across launches (Release 3 — DEFERRED)

### Problem

Devon Park has 50+ GB of model files; computing SHA256 for the full library takes ~30-60 s (depending on disk speed). The background hash pool (ADR-013) amortises this cost during a single session, but the hashes are dropped on exit. Every fresh launch pays the full hashing cost again as Devon opens detail screens or runs unify. This story persists the SHA256 cache across launches with strict validity rules (mtime + size + inode_dev) so unchanged files reuse their hashes, and adds a `modeltap cache verify` developer command for drift detection.

### Who

- **Devon Park**, power users with large libraries (50+ GB).

### Solution

A new `cache.sha256` table stores `(path, mtime, size, inode_dev) → ContentHash`. On every SHA256 request, the cache is consulted first; if `(mtime, size, inode_dev)` match, the cached hash is returned. Any drift triggers a fresh hash computation (queued via the existing ADR-013 background pool). Pre-unify revalidation extends to SHA256 — file's stat tuple must match the SHA256 cache entry, otherwise re-hash before acting. A new developer command `modeltap cache verify` rehashes everything and reports drift; safety net for paranoid power users.

### Job links

- J5 — Compare dedup-confidence across launches (Score 11.5 for O6)

### Domain Examples

#### 1: Happy path — Hash persists across launches when file unchanged

Devon computed SHA256 for `~/llms/mistral-7b-instruct-q4_K_M.gguf` last week (during a unify session). The file hasn't been touched since (mtime, size, inode_dev all unchanged). This morning Devon opens the Mistral detail screen; the dedup key displays immediately with provenance "computed 6 days ago". No re-hash needed.

#### 2: Edge — Hash invalidates on mtime change

Devon's editor `touch`ed the file (mtime changed; size and inode_dev unchanged). The cached SHA256 entry no longer matches `(mtime, size, inode_dev)`. On the next detail-screen open, the dedup key shows "(computing...)"; the background hash pool re-hashes; once done, the new hash replaces the old in the cache; provenance reads "computed just now".

#### 3: Error — Developer command detects drift

Devon runs `modeltap cache verify`. The command rehashes all 58 models in his library; for 2 of them, the recomputed hash differs from the cached value (he replaced two files manually with files of the same mtime+size — `cp --preserve=timestamps`). stdout reports the drift; the cache is updated with the recomputed values; `~/.modeltap/diagnostics.log` records `cache_verify drift_count=2`.

### UAT Scenarios (BDD)

#### Scenario: SHA256 persists across launches when the file is unchanged

Given Devon computed SHA256 for `~/llms/mistral-7b-instruct-q4_K_M.gguf` in a previous session
And the file's `(mtime, size, inode_dev)` matches the cached entry
When Devon launches modeltap again and opens the Mistral detail screen
Then the dedup key displays without recomputing the SHA256
And the provenance reads "dedup key computed <N> days ago"

#### Scenario: SHA256 invalidates when mtime, size, or inode_dev differs

Given Devon computed SHA256 for a file in a previous session
And the file's mtime has changed since
When the SHA256 is needed again (detail-screen open or pre-unify)
Then the cached hash is invalidated
And a fresh SHA256 computation is queued via the background hash pool
And the dedup key shows "(computing...)" until the new hash completes

#### Scenario: `modeltap cache verify` rehashes everything and reports drift

When Devon runs `modeltap cache verify`
Then every cached SHA256 entry is recomputed
And entries where the recomputed hash differs from the cached value are listed in stdout
And the cache is updated with the recomputed values
And `~/.modeltap/diagnostics.log` records `cache_verify drift_count=<n>`

### Acceptance Criteria

- [ ] AC-27-1: New table `cache.sha256` schema: `(path TEXT PRIMARY KEY, mtime INTEGER NOT NULL, size INTEGER NOT NULL, inode_dev INTEGER NOT NULL, content_hash TEXT NOT NULL, computed_at TEXT NOT NULL)`
- [ ] AC-27-2: SHA256 cache lookup uses exact tuple match `(mtime, size, inode_dev)`; any drift invalidates
- [ ] AC-27-3: Invalid entries trigger background re-hash via existing ADR-013 hash pool
- [ ] AC-27-4: Pre-unify revalidation includes SHA256 cache check (in addition to stat-only check from US-26)
- [ ] AC-27-5: `modeltap cache verify` developer command rehashes all entries and reports drift
- [ ] AC-27-6: SHA256 cache is opt-in via `[cache] persist_sha256 = true` config flag for v1; default off
- [ ] AC-27-7: Migration `0002_add_sha256_persistence.sql` adds the table cleanly to existing caches
- [ ] AC-27-8: `--no-cache` and `cache.enabled = false` skip the SHA256 cache as well

### Outcome KPIs

- **O6** (11.5) — Minimize the time to re-hash unchanged files across launches
- **K-INFO-8** — % of unify dialogs that open without waiting for a hash compute (target: ≥ 70% after Release 3 ships with `persist_sha256 = true`)

### Technical Notes

- `cache.sha256.computed_at` provides the "computed N ago" provenance on the model detail screen.
- The validity-check rule includes `inode_dev` to defeat most accidental mtime-preserving file replacement.
- `modeltap cache verify` is a CLI subcommand under `modeltap cache <subcommand>`; future subcommands: `verify`, `clear`, `stats`.
- Defer launch by 1-2 versions after Release 2 ships so the cache infrastructure has dogfooded.
- Opt-in flag for v1 of this story; default-on candidate for a later release.

### Dependencies

- US-23 (cache schema infrastructure + migration framework)
- US-26 (pre-mutate revalidation rule — extended with SHA256 validation)
- Parent ADR-013 (background SHA256 hash pool)
- New CLI subcommand surface (clap subcommand `cache verify`)
