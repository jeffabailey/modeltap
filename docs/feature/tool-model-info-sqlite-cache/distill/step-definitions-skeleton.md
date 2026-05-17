# Step Definitions Skeleton — tool-model-info-sqlite-cache

**Wave:** DISTILL (5 of 6) — brownfield extension of `modeltap-tui`
**Author:** Quinn (nw-acceptance-designer)
**Purpose:** specify every NEW Given/When/Then phrase introduced by the 7 `.feature` files in this distill, what each step asserts, and which seam it tests at. DELIVER's software-crafter writes the actual Rust step code.

**Inheritance:** the parent's `step-definitions-skeleton.md` defines all common phrases (launch, navigate, press key, type, snapshot assertions, JSONL assertions). The sibling `folder-group-bulk-delete/distill/step-definitions-skeleton.md` adds folder-specific phrases. This document specifies only DELTAS for the cache and inspect surfaces. Where a phrase appears in the parent or sibling skeleton, this document does not duplicate it — it references the source.

---

## Conventions (inherited)

- **Seam labels** (same as parent): `BIN`, `BIN-HEADLESS`, `APP`, `CORE`, `FS`, `JSONL`, `SNAPSHOT`. NEW seam label for this feature: `CACHE` (read-only `rusqlite::Connection` against the per-scenario `cache.sqlite` to verify schema/rows; used only in `@cache-introspection`-tagged assertions).
- **`World` type** (inherited from parent's `acceptance/world.rs`): carries `temp_dir`, `fixture_name`, `env: HashMap<String,String>`, `cmd`, `output`, `frames`, `script_path`, `log_dir`. The sibling added `hf_cache_root`, `ebusy_paths`, `pre_action_inventory`, `pre_action_disk_usage`.
- **New World fields this feature adds** (DELIVER's `World` extension):
  - `cache_path: PathBuf` — absolute path to the per-scenario `cache.sqlite`. Used by FS, CACHE, and env-var injection.
  - `cache_age_override: Option<Duration>` — populated by `MODELTAP_CACHE_AGE_OVERRIDE`.
  - `xdg_data_home: Option<PathBuf>` — populated by the one `dirs::data_dir()` resolution scenario.
  - `process_a: Option<Child>` — holds the first `modeltap` process for concurrent-process scenarios.
  - `process_b_output: Option<Output>` — captured output from the second process.
  - `pre_mutate_stat_quad: Option<(u64, u64, u64, u64)>` — `(mtime_ns, size, inode, dev)` recorded before a destructive scenario's mutation step.

- **Step file organization** (DELIVER will add): four new files under the parent's `tests/acceptance/steps/`:
  - `cache_lifecycle.rs` — Sections A, B (open/migrate/recover/concurrency).
  - `inspect.rs` — Sections C, D (tool-detail, model-detail).
  - `refresh.rs` — Section E (manual refresh + provenance).
  - `revalidate.rs` — Section F (pre-mutate revalidation).
  - Plus extensions to the parent's `discovery.rs` and `kpi.rs`.

---

## A. Cache lifecycle — Given / When / Then (`cache_lifecycle.rs`)

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given the cache file does not exist` | FS | Asserts `!world.cache_path.exists()`. Asserts the parent directory exists (or creates it). |
| `Given the cache file exists and is valid` | FS + CACHE | Builds `cache.sqlite` via the fixture script's `--warm-cache-seed` mode at `world.cache_path`. Asserts the file exists and `PRAGMA user_version = EXPECTED_SCHEMA_VERSION`. |
| `Given the cache contains inventory data written at the previous launch <duration> ago` | FS + CACHE | As above; then sets `MODELTAP_CACHE_AGE_OVERRIDE=<duration_seconds>` so the binary back-dates `cache_tools.last_scan_at`. |
| `Given the cache contains the test tool's model "<name>" at "<path>"` | FS + CACHE | Builds a minimal cache with one `cache_tools` row (`tool_id = "test-tool"`) and one `cache_models` row. Used by walking skeleton process-B. |
| `Given the cache file "<path>" exists but returns SQLITE_CORRUPT on open` | FS | Writes 16 KB of random bytes to `world.cache_path`. |
| `Given the cache PRAGMA user_version is <n>` | FS + CACHE | Opens the cache, sets `PRAGMA user_version = <n>` via a separate `rusqlite::Connection`, closes. |
| `Given the binary's expected_schema_version is <n>` | (compile-time) | This is documentary — DELIVER's step asserts the compile-time `modeltap_store::EXPECTED_SCHEMA_VERSION == <n>` and SKIPS the scenario if the assertion fails. |
| `Given a valid cache file exists at "<path>"` | FS + CACHE | As `Given the cache file exists and is valid`. |
| `Given --no-cache is passed on the command line` | BIN-HEADLESS | Appends `--no-cache` to the `world.cmd` args. |
| `Given cache.enabled=false in the config` | FS | Writes `[cache] enabled = false` to `${MODELTAP_HOME}/config.toml`. |
| `Given XDG_DATA_HOME is set to "<path>"` | BIN-HEADLESS | Sets `world.env["XDG_DATA_HOME"] = path`. UNSETS `world.env["MODELTAP_CACHE_PATH"]` (the one resolver-proof scenario short-circuits the test override). |
| `Given the cache contains <tool> inventory with last_scan_at <duration> ago` | FS + CACHE | Composite: builds cache, sets the named tool's `last_scan_at` to `now - duration`. |
| `Given cache.tool_ttl_seconds is <n>` | FS | Writes `[cache] tool_ttl_seconds = <n>` to config.toml. Default 86400. |
| `Given the cache shows Ollama with <n> models` | FS + CACHE | Composite: builds cache with N `cache_models` rows where `tool_id="ollama"`. |
| `Given the warm-start paint has completed from cached data` | BIN-HEADLESS | Composite: launch in headless mode against a `devon-cache-warm` fixture, wait_for the warm-paint sentinel frame, hold process. Captures the frame as `world.frames[0]`. |
| `Given two modeltap processes share the same cache.sqlite` | FS + BIN-HEADLESS | Sets up `world.cache_path` once; the When step launches both processes against it. |
| `Given two modeltap processes are running with cache writes enabled` | BIN-HEADLESS | Launches process A with `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS=2000` and a script that triggers a per-tool reconcile write. Process B is the foreground test driver. |
| `Given Devon's laptop has run modeltap N times this week with the cache populated` | (interpretive) | Documentary phrasing; mapped to "the cache file exists and is valid + the cache contains inventory data". |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon runs "modeltap"` | BIN-HEADLESS | Constructs `assert_cmd::Command::cargo_bin("modeltap")`. Adds env, args, `--script` if applicable. Spawns; captures output. (Inherits parent §A; this delta documents the cache-aware variant that always sets `MODELTAP_CACHE_PATH`.) |
| `When Devon runs "modeltap --no-cache"` | BIN-HEADLESS | As above with `--no-cache`. |
| `When Devon runs "modeltap --version"` | BIN-HEADLESS | As above with `--version`. Used by INT-INFO-6. |
| `When a second modeltap process opens the same cache file` | BIN-HEADLESS | Launches process B; captures its output as `world.process_b_output`. |
| `When the background reconcile finishes for <tool> at <timestamp>` | BIN-HEADLESS | Composite: ensures the reconcile JSONL event is present in `world.log_dir/launch.log` with the named tool and ISO timestamp. |
| `When the migration runs` | BIN-HEADLESS | Implicit in `Devon runs modeltap`; this is documentary phrasing for the migration scenarios. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the cache layer creates "<path>"` | FS | `assert!(path.exists())`. |
| `Then the cache file at "<path>" exists with PRAGMA user_version = <n>` | FS + CACHE | `path.exists() == true`; `rusqlite::Connection::open(path)`'s `pragma_query::<u32>("user_version")` returns `n`. |
| `Then cache_models contains exactly <n> row(s)` | CACHE | `SELECT COUNT(*) FROM cache_models` returns `n`. |
| `Then cache_models contains exactly <n> row(s) for tool_id "<id>"` | CACHE | Same with WHERE clause. |
| `Then cache_tools contains a row for tool_id "<id>" with model_count = <n>` | CACHE | `SELECT model_count FROM cache_tools WHERE tool_id = ?`. |
| `Then schema_version is <n>` | CACHE | `PRAGMA user_version` query equals n. |
| `Then the TUI paints the cached inventory within <ms> ms of process start` | JSONL | Reads `${LOG_DIR}/launch.log`; asserts a `launch.first_paint_ms` event exists with value `<= ms`. |
| `Then the TUI paints the skeleton "discovering..." placeholders within <ms> ms` | JSONL | As above; asserts the cold-start skeleton-paint timing event. |
| `Then full inventory paints within <duration>` | JSONL | Asserts `launch.full_inventory_paint_ms <= duration_ms`. |
| `Then the summary bar reads "<text>"` | SNAPSHOT | Substring match in the captured frame's summary-bar region. |
| `Then the summary bar shows "<pattern>"` | SNAPSHOT | Regex match against summary-bar region. Used for "as of <N> ago" where N is variable. |
| `Then the bottom bar shows "<text>" among its shortcuts` | SNAPSHOT | Substring in bottom-bar region. |
| `Then the cache file is renamed to "<path-pattern>"` | FS | Glob match against `world.cache_path`'s parent directory; for the `.corrupt-<timestamp>` and `.future-version-<n>` patterns. Asserts at least one matching file exists. |
| `Then the cache file at "<pattern>" exists` | FS | Same as above. |
| `Then a recovery banner appears reading "<text>"` | SNAPSHOT | Substring in the captured frame's banner region (top-of-main-view per ADR-015 §5). |
| `Then a recovery banner appears explaining "<text>"` | SNAPSHOT | Substring match (less strict). |
| `Then "~/.modeltap/diagnostics.log" gains a line tagged "<text>"` | JSONL | Reads `${MODELTAP_HOME}/diagnostics.log` (NOT the JSONL log; this is the plain-text diagnostics log per ADR-015 §5). Asserts the substring appears in some line. |
| `Then cold-start discovery proceeds without crashing modeltap` | BIN | `process_exit_code == 0` AND `cache.sqlite` exists with `PRAGMA user_version = 1` (cold-start populated it). |
| `Then the launch proceeds normally with warm-start paint` | JSONL | `launch.warm_paint_ms` event present. |
| `Then the launch proceeds normally` | JSONL | `launch.first_paint_ms` event present; `process_exit_code == 0`. |
| `Then the migrator runs migrations "<file1>" and "<file2>" in order` | JSONL | Reads `diagnostics.log`; asserts `cache_migration from=<n> to=<m> status=ok` line. |
| `Then the launch follows the stateless rediscovery path from ADR-003` | JSONL | Asserts no `launch.warm_paint_ms` event; asserts `launch.cold_start_ms` event present. |
| `Then the cache file is neither opened nor written` | FS | After the launch completes, `world.cache_path.exists() == <pre-launch state>`. If the file did not exist before, it does not exist after. If it existed before, its `(size, mtime)` is unchanged. |
| `Then no cache.sqlite, cache.sqlite-wal, or cache.sqlite-shm files exist at "<dir>"` | FS | Verifies all three filenames absent. Used by `--no-cache` scenario per OQ-2. |
| `Then both processes coexist without SQLITE_BUSY errors during reads` | BIN | Both processes' stderr is empty of "SQLITE_BUSY"; both exit code 0. |
| `Then both processes display consistent inventory data` | SNAPSHOT | Compare the two captured TestBackend frames; assert the model counts match. |
| `Then process B waits up to 5 seconds (PRAGMA busy_timeout=5000)` | JSONL | Process B's launch.log includes `cache.write_wait_ms` event with value `>= 0` and `<= 5000`. |
| `Then process B's write succeeds after process A commits` | CACHE | Final `cache_tools.last_scan_at` for the relevant tool reflects process B's write (the more-recent timestamp). |
| `Then neither process crashes or returns an error to the user` | BIN | Both processes' exit code 0; stderr does not contain "panic" or "Error:". |
| `Then "modeltap --version" exits successfully` | BIN | `process_exit_code == 0`; stdout contains version string. |
| `Then the cache file is not touched` | FS | If file existed pre-launch, its `(size, mtime)` is unchanged. If absent, still absent. |

---

## B. Walking-skeleton convenience steps (`cache_lifecycle.rs`)

These compose the multi-step WS journey into discrete phrases for readability.

| Step phrase | Seam | Behavior |
|---|---|---|
| `Given the in-process TestTool plugin is registered` | BIN-HEADLESS | Sets `world.env["MODELTAP_TEST_PLUGINS"] = "test-tool"`. |
| `Given the TestTool will discover one model "<name>" at "<path>"` | FS | Creates a sparse file at `<path>` (relative to `world.temp_dir`); the TestTool's `discover()` returns this single model. |
| `When the first modeltap process completes cold-start discovery and exits` | BIN-HEADLESS | Composite: launches with the WS script (cold-start → wait_for "test-tool" in left pane → `q` to quit); waits for the process to exit cleanly. |
| `When a second modeltap process launches against the same cache file` | BIN-HEADLESS | Launches process B with the same `MODELTAP_CACHE_PATH` and `MODELTAP_TEST_PLUGINS=test-tool`; captures B's output and frames. |
| `Then the second process's TUI shows "<name>" in the right pane` | SNAPSHOT | Process B's first captured frame contains `<name>` in the right-pane region. |
| `Then the second process's warm-paint time is at most <ms> ms` | JSONL | Process B's launch.log `launch.warm_paint_ms` event value `<= ms`. |

---

## C. Tool detail screen — Given / When / Then (`inspect.rs`)

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given Devon has Ollama selected in the left pane` | BIN-HEADLESS | Composite: launch + select Ollama via `[Tab]` or arrow keys. |
| `Given Devon has the cursor on <tool> in the left pane` | BIN-HEADLESS | Same with named tool. |
| `Given a plugin's inspect_tool() returns no version` | FS + APP | Builds the fixture so the named plugin's `inspect_tool` returns `detected_version: None`. For Ollama: sets `MODELTAP_OLLAMA_API_URL=http://127.0.0.1:1` (unreachable) so the version lookup falls back to None within the 500 ms timeout. For llama-cli: no version detection exists — the plugin's `inspect_tool` returns `None` by design. |
| `Given Ollama's discovery failed at last scan with "<error>"` | FS + CACHE | Pre-populates the cache with a `cache_tools` row for Ollama where `last_error = "<error>"` and `last_error_at = <recent ISO timestamp>`. |
| `Given Devon has added 'search_paths = ["<path>"]' to ~/.modeltap/config.toml under [plugins.<plugin>]` | FS | Writes the config snippet to `${MODELTAP_HOME}/config.toml`. |
| `Given the <plugin> plugin reports inspect_tool=Unsupported` | (interpretive) | Documentary; for atomic-chat or gpt4all where the trait default returns `Err(InspectError::Unsupported)`. No fixture changes needed. |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon presses Enter` | BIN-HEADLESS | Appends `key: Enter` to the script. |
| `When Devon opens the tool detail screen for <tool>` | BIN-HEADLESS | Composite: select tool in left pane + press Enter + wait_for detail-screen sentinel. |
| `When Devon opens <tool>'s detail screen` | BIN-HEADLESS | Same. |
| `When Devon presses 'r' on the <tool> pane` | BIN-HEADLESS | Appends `key: r`. (US-21 detail-screen refresh; same key as manual-refresh per parent US-08 dispatch.) |
| `When Devon presses Esc` | BIN-HEADLESS | Appends `key: Esc`. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the tool detail screen opens` | SNAPSHOT | Captured frame contains a detail-screen sentinel header. |
| `Then the tool detail screen opens within <ms> ms` | JSONL + SNAPSHOT | `screen.tool_detail_open_ms <= ms` event present. |
| `Then it shows <tool>'s discovery root "<path>"` | SNAPSHOT | Substring in detail-screen region. |
| `Then it shows the configured search paths under that root` | SNAPSHOT | Substring for "Search paths" label + at least one path. |
| `Then it shows model count <n>, disk usage <size>, last scan <text>, and plugin version "<text>"` | SNAPSHOT | All four substrings present. |
| `Then it shows the largest model: "<text>"` | SNAPSHOT | Substring "Largest model: <text>". |
| `Then the Version field reads "<text>"` | SNAPSHOT | Substring "Version: <text>" in detail-screen region. |
| `Then the Version field reads "(not detectable)"` | SNAPSHOT | Substring "Version: (not detectable)". |
| `Then no false or stale version is shown` | SNAPSHOT | Negative substring assertion: the detail screen does not contain "0.0.0" or "<unknown>" or similar false-positive markers. |
| `Then the rest of the detail screen renders normally` | SNAPSHOT | Asserts all 9 detail-screen field labels (Discovery root, Version, Search paths, Model count, Disk usage, Largest model, Last scan, Plugin version, Last error) are present. |
| `Then the Last error field shows "<text>" with the timestamp` | SNAPSHOT | Substring + ISO-timestamp regex. |
| `Then the Last error field shows "(none)"` | SNAPSHOT | Substring "Last error: (none)". |
| `Then the bottom bar offers "[r] refresh this tool" to retry after fixing permissions` | SNAPSHOT | Substring in bottom-bar region. |
| `Then the bottom bar on the detail screen shows "[Esc] back", "[r] refresh this tool", "[?] help"` | SNAPSHOT | Three substrings present. |
| `Then the Search paths section lists "<path1>", "<path2>", and "<path3>"` | SNAPSHOT | Three substrings present in the Search paths region. |
| `Then "<path>" is labelled "(default)"` | SNAPSHOT | Substring "<path> (default)". |
| `Then "<path>" is labelled "(user config)"` | SNAPSHOT | Substring "<path> (user config)". |
| `Then the main view returns` | SNAPSHOT | Captured frame matches the main-view layout (two-pane); no detail-screen sentinel present. |
| `Then the cursor is still on <tool> in the left pane` | SNAPSHOT | Visual cursor indicator in left-pane at the named tool's row. |
| `Then the detail screen shows "(inspection failed -- see diagnostics.log)"` | SNAPSHOT | Substring assertion. |
| `Then the other detail-screen fields render with what discover() provided` | SNAPSHOT | At minimum, the model count and disk usage labels are populated. |
| `Then the <tool> left-pane row updates after the refresh completes` | SNAPSHOT | Captured frame's left-pane region shows updated count (or "(error)" if the refresh failed). |

---

## D. Model detail screen — Given / When / Then (`inspect.rs`)

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given "<model_id>" is registered in <tool>` | FS | The fixture tree has the model file at the tool's expected path. |
| `Given "<model_id>" is registered in <N> tools per the cache` | FS + CACHE | The cache has `<N>` rows in `cache_models` for `model_id = <model_id>`. |
| `Given the file format is GGUF v<n>` | FS | The fixture writes a minimal GGUF v<n> header + sparse padding. |
| `Given a model file's format cannot be parsed` | FS | Uses `devon-mistral-corrupt-gguf` fixture: 100-byte file with magic bytes only. |
| `Given Devon is on the Mistral detail screen` | BIN-HEADLESS | Composite: launch + select Ollama + navigate to Mistral row + press Enter + wait_for detail-screen sentinel. |
| `Given the metadata was introspected <duration> ago` | FS + CACHE | Cache row's `metadata_introspected_at` is back-dated by `<duration>`. |
| `Given "<model_id>" is in Hugging Face only (<size>)` | FS | HF fixture has the named model file at the named size; no other tool has it. |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon presses Enter on the <name> row` | BIN-HEADLESS | Composite: navigate cursor to row, key: Enter. |
| `When Devon opens its model detail` | BIN-HEADLESS | Same; shorter form. |
| `When Devon opens the Mistral detail screen` | BIN-HEADLESS | Same with the Mistral row. |
| `When Devon presses 'r'` | BIN-HEADLESS | Appends `key: r`. (US-22 detail-screen re-introspect.) |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the model detail screen opens` | SNAPSHOT | Sentinel header present. |
| `Then the model detail screen opens within <ms> ms` | JSONL + SNAPSHOT | `screen.model_detail_open_ms <= ms`. |
| `Then the Metadata section shows "<text>"` | SNAPSHOT | Substring in Metadata section. |
| `Then the Metadata section shows aligned key-value pairs starting with "<text>"` | SNAPSHOT | Substring + format-shape check (left-aligned label, right-aligned value). |
| `Then the Metadata section provenance reads "introspected <text>"` | SNAPSHOT | Substring "introspected <text>" in section header. |
| `Then the Format field reads "<text>"` | SNAPSHOT | Substring "Format: <text>". |
| `Then the Registered with section lists "<text>"` | SNAPSHOT | Substring in "Registered with" region. |
| `Then the Registered with section lists all <n> tool paths` | SNAPSHOT | All n tool-path substrings present. |
| `Then "Tool::inspect_model()" re-runs against the current file` | JSONL | A `inspect.invoked tool=<x> model=<y> source=detail_screen_refresh` event in launch.log. |
| `Then the Metadata section updates with new values if any` | SNAPSHOT | Captured frame post-refresh differs from pre-refresh in the Metadata region (if any KV changed). |
| `Then the provenance reads "introspected just now"` | SNAPSHOT | Exact substring "introspected just now". |
| `Then the cache.models.metadata_introspected_at column updates` | CACHE | `SELECT metadata_introspected_at FROM cache_models WHERE model_id = ?` returns a timestamp within the last 5 seconds. |
| `Then the Metadata section shows "(introspection failed -- see diagnostics.log)"` | SNAPSHOT | Substring. |
| `Then the screen does not crash` | BIN | `process_exit_code == 0` (or process still running per scenario). |
| `Then the other panels (Registered with, Size on disk, Dedup key) still render` | SNAPSHOT | Three substrings present even though Metadata failed. |
| `Then the cursor is still on the <name> row in the right pane` | SNAPSHOT | Visual cursor indicator. |
| `Then the bottom bar on the detail screen shows "[Esc] back", "[u] unify" (dimmed when not unifiable), "[d] delete-one", "[r] re-introspect", "[?] help"` | SNAPSHOT | All five substrings present; `[u]` styled "dimmed" when the model is registered in only one tool. |

---

## E. Manual refresh + provenance — Given / When / Then (`refresh.rs`)

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given no dialog is open` | (precondition) | Documentary; asserts none of the dialog sentinels (unify, zap, delete-one, folder-delete, recovery banner) is in the captured frame. |
| `Given the unify dialog is open` | BIN-HEADLESS | Composite: launch + open unify dialog via `[u]` on a dedup group. |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon presses 'r'` | BIN-HEADLESS | Appends `key: r`. (See also Section C and D — same key, context-dependent dispatch.) |
| `When Devon presses Shift+R` | BIN-HEADLESS | Appends `key: Shift+R`. |
| `When the summary bar renders` | SNAPSHOT | Captures the current frame and inspects the summary-bar region. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then a spinner appears next to the <tool> row` | SNAPSHOT | Substring with a spinner character (DELIVER picks the actual char per parent's existing pattern). |
| `Then the summary bar reads "refreshing <tool>..."` | SNAPSHOT | Substring. |
| `Then within <duration> the spinner clears and the summary bar reads "<text>"` | SNAPSHOT + timing | Captures a later frame; asserts no spinner present AND substring matches. The timing assertion reads `JSONL refresh.wall_clock_ms <= duration_ms`. |
| `Then the cache.tools row for <tool> updates with the new last_scan_at` | CACHE | `SELECT last_scan_at FROM cache_tools WHERE tool_id = ?` returns a timestamp within the last 5 seconds. |
| `Then all four tool rows show the per-tool spinner` | SNAPSHOT | Four spinner substrings present (one per tool). |
| `Then within <duration> all spinners clear` | SNAPSHOT + timing | Later captured frame has no spinner; total elapsed `<= duration`. |
| `Then the cache.tools rows for every tool are updated` | CACHE | `SELECT COUNT(*) FROM cache_tools WHERE last_scan_at > <now - 5s>` equals 4. |
| `Then no refresh is triggered` | JSONL | No new `refresh.*` event appears in launch.log between the keystroke and assertion. |
| `Then the "[r] refresh tool" shortcut in the bottom bar is dimmed` | SNAPSHOT | Bottom-bar cell for "[r]" has the dim style attribute. |
| `Then the unify dialog state is preserved` | SNAPSHOT | Dialog still present in the captured frame; no state change. |
| `Then the provenance line reads "Total: <X> GB | <Y> models | as of <Z> ago[, reconciling...]"` | SNAPSHOT | Regex match against the summary-bar; `<Z>` is one of {"just now", "<N> min ago", "<N> hours ago", "<N> days ago"}. |
| `Then the timestamp updates as reconcile progresses` | SNAPSHOT (multi-frame) | Multiple captured frames across the reconcile show the provenance text changing (e.g., ", reconciling..." appended then removed). |
| `Then the summary bar updates to "as of just now"` | SNAPSHOT | Exact substring. |

---

## F. Pre-mutate revalidation — Given / When / Then (`revalidate.rs`)

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given a model file's mtime/size/inode/dev quad changes between scans` | FS | After the fixture builds the cache, the harness uses `filetime::set_file_mtime` to bump the named file's mtime by 10 seconds. Records the new stat quad in `world.pre_mutate_stat_quad`. |
| `Given <model> is registered in <n> tools per the cache` | FS + CACHE | As §D. |
| `Given all <n> files match the cached (mtime, size, inode, dev) tuple` | (precondition) | Documentary; the fixture is set up so the cache write happened immediately before the scenario started, so the stat quad matches by construction. |
| `Given the <plugin> copy's mtime has changed since the last cache write` | FS | Same as `Given a model file's mtime/size/inode/dev quad changes between scans` but scoped to a named plugin's copy. |
| `Given one file has been deleted out-of-band between launch and Devon's action` | BIN-HEADLESS + FS | Composite: launches modeltap (captures frame with the cached model visible), then `std::fs::remove_file(<path>)`, then the scenario continues with the user-action step. |
| `Given a model row in the right pane comes from the cache` | FS + CACHE | Documentary; precondition for `devon-cache-warm` fixture variants. |
| `Given the file's size has changed since the cache write` | FS | The harness truncates the file to a smaller size after the cache write. |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon presses 'u' on the <name> row and confirms the dialog` | BIN-HEADLESS | Composite: navigate + key:u + wait_for unify dialog + key:Enter (confirm). |
| `When Devon presses 'u'` | BIN-HEADLESS | Just key:u; used by the drift scenario where the dialog should refresh before the user confirms. |
| `When Devon attempts to delete model "<id>"` | BIN-HEADLESS | Composite: navigate + key:d + wait_for delete-one dialog OR abort message. |
| `When Devon attempts to unify` | BIN-HEADLESS | Just key:u. |
| `When Devon opens the detail screen and presses 'd' to delete` | BIN-HEADLESS | Composite: Enter (detail) + key:d. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the unify proceeds normally` | SNAPSHOT + FS | Unify dialog closes; "Last action: unified <N> models" appears in right pane; the post-action hardlink state is verified via `std::fs::metadata().ino()` equality across the unified paths. |
| `Then post-action the cache is updated with the new hardlink state` | CACHE | `cache_model_files` rows for the unified group all share the same `inode`. |
| `Then the validator detects the drift before opening the confirmation dialog` | SNAPSHOT | A "Re-introspecting..." progress indicator appears in the dialog before the final confirmation prompt. |
| `Then the dialog displays "Re-introspecting before proceeding..."` | SNAPSHOT | Substring. |
| `Then the dedup-key / size for the drifted file is recomputed` | JSONL | `inspect.invoked tool=<x> model=<y> source=pre_mutate_drift` event. |
| `Then Devon is shown the (possibly updated) reclaim estimate` | SNAPSHOT | The dialog's "Reclaim" line contains the post-recompute value. |
| `Then Devon must re-confirm if the reclaim amount changed by more than rounding` | SNAPSHOT | If pre-recompute reclaim != post-recompute reclaim (within 1 byte tolerance), the dialog adds "amount changed — re-confirm" annotation. |
| `Then the mutation should be rejected with reason "stale cache"` | SNAPSHOT + FS | Right-pane shows "Action aborted: stale cache"; the fixture filesystem is byte-identical pre/post via the parent's `DirManifest` mechanism. |
| `Then the pre-flight check refuses with "file no longer exists; refreshing inventory"` | SNAPSHOT | Substring. |
| `Then no destructive action occurs` | FS | `DirManifest` equality pre/post. |
| `Then an automatic per-tool refresh is triggered for the affected tool` | JSONL | A `refresh.tool=<x> source=pre_mutate_gone` event in launch.log. |
| `Then the right pane updates to reflect the missing file` | SNAPSHOT | Post-refresh frame: the missing model's row is gone from the right pane. |
| `Then the cache safety rule re-stats the file` | JSONL | A `revalidate.invoked model=<x> result=Drift` (or Match/Gone) event. |
| `Then the delete dialog includes a "WARNING: file has changed since last seen" line` | SNAPSHOT | Substring. |
| `Then the dialog requires explicit re-confirmation before proceeding` | SNAPSHOT | The dialog awaits a second key:Enter; one key:Enter does not close it. |
| `Then for every previously-shared file, the other tool's path still stats to a live inode` | FS | Reuses the sibling's `int-fgd-4` assertion (for INT-INFO-7 cross-feature). |

---

## G. Inventory diff / silent-ack indicator (`refresh.rs` continued)

| Step phrase | Seam | Behavior / Assertion |
|---|---|---|
| `Given the user ran "<command>" in another terminal since the last reconcile` | FS | The harness adds/removes a model file from the fixture tree between the cache write and the relaunch. (E.g., for the qwen2.5 scenario: adds the file to the Ollama fixture dir.) |
| `Then the <tool> left-pane row updates to <n> models` | SNAPSHOT | Substring in left-pane region. |
| `Then a tiny blue "*" appears next to the <tool> row name for <duration> seconds` | SNAPSHOT (multi-frame) | One captured frame within `<duration>s` has the `*` indicator with blue foreground; a later frame after `<duration>s` does not. |
| `Then no modal or dialog is shown` | SNAPSHOT | No dialog sentinel in any captured frame during the indicator-display window. |
| `Then the cache.tools row for <tool> is NOT overwritten` | CACHE | `SELECT last_scan_at FROM cache_tools WHERE tool_id = ?` returns the SAME timestamp as before the failed reconcile. |
| `Then "<tool>"'s left-pane row shows "(error)" alongside the cached model count` | SNAPSHOT | Substring "<tool> <n> (error)". |
| `Then "~/.modeltap/diagnostics.log" gains a line tagged "<text>"` | (see Section A above) | |

---

## H. SHA256 persistence (R3 — `@release-3 @skip` until R3 unblocks) — `sha256.rs`

| Step phrase | Seam | Notes |
|---|---|---|
| `Given Devon computed SHA256 for "<path>" in a previous session` | FS + CACHE | Pre-populates `cache_models.sha256` for the named file with a known hex hash. |
| `Given the file's (mtime, size, inode, dev) matches the cached entry` | (precondition) | Documentary. |
| `Given the file's mtime has changed since` | FS | As §F. |
| `When the SHA256 is needed again` | BIN-HEADLESS | Composite: opens detail screen for the model OR triggers a unify. |
| `When Devon runs "modeltap cache verify"` | BIN | `assert_cmd::Command::cargo_bin("modeltap").arg("cache").arg("verify")`. Captures stdout/stderr. |
| `Then the dedup key displays without recomputing the SHA256` | SNAPSHOT + JSONL | Detail screen shows the dedup key; no `hash.computed` event in launch.log. |
| `Then the provenance reads "dedup key computed <duration> ago"` | SNAPSHOT | Substring. |
| `Then the cached hash is invalidated` | CACHE | `SELECT sha256 FROM cache_models WHERE model_id = ?` returns `NULL`. |
| `Then a fresh SHA256 computation is queued via the background hash pool` | JSONL | `hash.queue tool=<x> model=<y>` event. |
| `Then the dedup key shows "(computing...)" until the new hash completes` | SNAPSHOT (multi-frame) | Early frame shows "(computing...)"; later frame shows the computed hex. |
| `Then every cached SHA256 entry is recomputed` | JSONL | `hash.computed` events for every row in `cache_models` (or `cache_sha256` in R3). |
| `Then entries where the recomputed hash differs from the cached value are listed in stdout` | BIN | Stdout contains "drift: <path>" lines. |
| `Then the cache is updated with the recomputed values` | CACHE | `SELECT sha256 FROM cache_sha256 WHERE path = ?` returns the new hex. |
| `Then "~/.modeltap/diagnostics.log" records "cache_verify drift_count=<n>"` | JSONL/diagnostics | Substring in diagnostics.log. |

These steps are documented for forward planning; DELIVER implements them only when US-27 ships (Release 3).

---

## I. Cross-feature integration steps (`integration.rs`)

| Step phrase | Seam | Notes |
|---|---|---|
| `Given <n> destructive code paths exist in modeltap-app` | (interpretive) | Documentary; mapped to "unify, zap, delete_one, folder_delete" in the AST-walk assertion. |
| `When each destructive code path is invoked` | (parameterised) | Scenario Outline iterates `unify`, `zap`, `delete_one`, `folder_delete`. |
| `Then the pre-mutate revalidator is invoked before the mutation` | JSONL | `revalidate.invoked` event with `source=<action_name>` precedes the action's `outcome` event. |
| `Then the parent's K3 (first paint < 1 s) is satisfied via K3a OR K3b` | JSONL | At least one of `launch.warm_paint_ms <= 150` OR `launch.first_paint_ms <= 150 && launch.full_inventory_paint_ms <= 1150` is true. |
| `Then the keyboard_shortcuts registry includes "[r]" mapped to "refresh tool"` | (DELIVER-owned) | A unit test in `modeltap-tui/tests/` asserts the SHORTCUT_TABLE static contains the entry. At Layer A, the help screen (parent US-08) is captured and the substring "[r] refresh tool" is asserted. |
| `Then for any valid combination of tools, total.disk_usage equals the sum of tool.disk_usage` | SNAPSHOT (@property) | DELIVER may implement as proptest over a generated `Inventory`; Layer A scenario uses the `devon-multi-tool` fixture as a single-example concrete case AND asserts the property holds during a mid-reconcile frame as well as a settled frame. |
| `Then the help screen shows the term "<term>"` | SNAPSHOT | Substring in help-screen region; used for INT-INFO-9 vocabulary check. |

---

## J. Step-Definition File Layout (DELIVER recommendation)

Per `nw-bdd-methodology` "Step Organization by Domain", the new step files extend the parent layout:

```
crates/modeltap-app/tests/acceptance/
├── world.rs                          # MODIFIED — add cache_path, cache_age_override,
│                                       xdg_data_home, process_a, process_b_output,
│                                       pre_mutate_stat_quad
├── steps/
│   ├── ... (existing parent + sibling step files) ...
│   ├── cache_lifecycle.rs            # NEW — Sections A, B (open/migrate/recover/concurrency, WS)
│   ├── inspect.rs                    # NEW — Sections C, D (tool-detail, model-detail)
│   ├── refresh.rs                    # NEW — Sections E, G (manual refresh, silent-ack)
│   ├── revalidate.rs                 # NEW — Section F (pre-mutate revalidation)
│   ├── sha256.rs                     # NEW — Section H (R3; skeletal until US-27 unblocks)
│   ├── integration.rs                # MODIFIED — Section I (cross-feature; sibling already created this file)
│   ├── kpi.rs                        # MODIFIED — add launch.warm_paint_ms, refresh.wall_clock_ms, cache.open_ms, screen.tool_detail_open_ms, inspect.invoked, revalidate.invoked events
│   └── discovery.rs                  # MODIFIED — silent-ack indicator
├── test_tool.rs                      # NEW — in-process TestTool plugin (walking-skeleton ONLY)
└── fixtures/
    ├── build.sh                      # MODIFIED — add the 14 new named fixtures
    ├── devon-cache-empty/            # NEW
    ├── devon-cache-warm/             # NEW
    ├── devon-cache-corrupt/          # NEW
    ├── devon-cache-future-v/         # NEW
    ├── devon-cache-old-v/            # NEW
    ├── devon-cache-stale-tool/       # NEW
    ├── devon-mistral-gguf/           # NEW
    ├── devon-mistral-corrupt-gguf/   # NEW
    ├── devon-hf-with-config-json/    # NEW
    ├── devon-ollama-manifest/        # NEW
    ├── devon-tool-error-ollama/      # NEW
    ├── devon-llamacli-userconfig/    # NEW
    ├── devon-cache-mtime-drift/      # NEW
    └── devon-cache-file-gone/        # NEW
```

Each step file is a self-contained module of `#[given(...)] / #[when(...)] / #[then(...)]` functions over the shared `World` type. cucumber-rs auto-discovers them.

---

## K. Test harness types referenced

For DELIVER's reference, the following harness types are mentioned across this spec:

| Type | Purpose | Where it lives |
|---|---|---|
| `CacheFixture` | Builder for `cache.sqlite` files via `--warm-cache-seed` mode. Methods: `with_tool(id, model_count)`, `with_model(tool_id, name, size, mtime)`, `with_last_scan_at(tool_id, offset)`, `with_corrupt()`, `with_user_version(n)`. Returns a `Drop`-cleaning handle. | `tests/src/fixtures/cache_fixture.rs` (NEW) |
| `TestTool` | In-process `Tool` impl registered via `MODELTAP_TEST_PLUGINS=test-tool`. Walking-skeleton ONLY. | `tests/src/test_tool.rs` (NEW) |
| `CacheVerifier` | Read-only `rusqlite::Connection` wrapper providing `pragma_user_version()`, `count_models(tool_id)`, `count_tools()`, `last_scan_at(tool_id)`. Used in `@cache-introspection`-tagged Then steps. | `tests/src/fixtures/cache_verifier.rs` (NEW) |
| `KeyEventDriver` | Inherited from parent. | Existing |
| `HeadlessTuiHarness` | Inherited from parent. | Existing |
| `DirManifest` | Inherited from sibling. | Existing (sibling-introduced) |
| `JSONLogReader` | Reads `${LOG_DIR}/launch.log`. Methods: `events_of_type("launch.warm_paint_ms")`, `field(event_id, key)`. | Existing (parent) |
| `DiagnosticsLogReader` | Reads `${MODELTAP_HOME}/diagnostics.log` (plain-text, not JSONL). | NEW (cache-recovery uses plain-text per ADR-015 §5) |

---

## L. Implementation discipline notes for DELIVER

1. **One scenario enabled at a time.** Quinn's standard `@skip` discipline: the walking-skeleton is initially the only un-skipped scenario. After WS goes green, DELIVER removes `@skip` from `cache-state-model.feature` scenarios one at a time per the milestone ordering in §J of `acceptance-test-plan.md`. The `@release-3` scenarios stay `@skip` until R3 unblocks.

2. **Step phrases are STABLE.** The phrasing in this skeleton matches the phrasing in the feature files. DELIVER MUST NOT alter phrasing during implementation — that breaks the executable-spec round-trip. If a phrase is awkward in Rust step code, propose a phrase change in a follow-up PR.

3. **All assertions read observable behavior.** Per Mandate 1 and critique Dim 7, every Then step here asserts either (a) a return value from the `modeltap` binary's exit/stdout/JSONL/diagnostics-log, (b) an observable filesystem state (a path or DB row exists or doesn't), or (c) a captured TestBackend frame substring. No assertion reads `_internal_field` or `mock.called`. The `@cache-introspection`-tagged assertions read SQLite directly — this is permitted because the SQLite file IS a user-observable artifact (per ADR-015 §4: users can `sqlite3 cache.sqlite` it). Verified by grep over the implementation during DELIVER review.

4. **Concurrent-process scenarios are slow.** They launch two real `modeltap` binaries. CI runs them in a serial-only test job. Local developers should run `cargo test -- --test-threads=1` for the `@concurrent`-tagged scenarios.

5. **The `MODELTAP_CACHE_AGE_OVERRIDE` env-var seam is well-isolated.** Gated by `cfg(any(test, feature = "test-harness"))` in `modeltap-store::open` (specifically: applied AFTER `Cache::open()` returns, in a `if cfg!(test_harness) { /* back-date */ }` block). DELIVER may choose between `cfg(test)` and the `test-harness` feature flag per `wave-decisions.md` §D12.

6. **The in-process `TestTool` is walking-skeleton-only.** No other scenario registers it. DELIVER documents this in `test_tool.rs` and in `crates/modeltap-app/src/plugin_registry.rs` where the `MODELTAP_TEST_PLUGINS` env-var seam is honored.

7. **The `--debug-hold-write-lock-ms` flag is concurrent-write-only.** Used by exactly one scenario (US-23 Scenario 5). Gated by `cfg(any(test, feature = "test-harness"))`. DELIVER may choose between flag and feature gate.

8. **R9 architecture-lint** (every mutation site preceded by `revalidate::pre_mutate`) is a DELIVER concern, NOT a Layer A scenario. The Layer A scenarios assert the user-visible CONSEQUENCES of the revalidator (action proceeds/aborts/refreshes). The lint catches static violations; the acceptance scenarios catch dynamic-runtime correctness.
