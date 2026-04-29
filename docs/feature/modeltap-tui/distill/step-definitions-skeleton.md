# Step Definitions Skeleton — modeltap-tui

**Wave:** DISTILL (5 of 6)
**Author:** Quinn (nw-acceptance-designer)
**Purpose:** specify every Given/When/Then phrase used in `features/master-acceptance.feature`, what each step asserts, and which seam it tests at. DELIVER's software-crafter writes the actual Rust step code.

## Conventions

- **Seam labels:**
  - `BIN` — Binary E2E. Step drives the `modeltap` binary as a subprocess (`assert_cmd`) or under PTY (`expectrl`).
  - `BIN-HEADLESS` — Binary E2E in headless mode (`MODELTAP_HEADLESS=1`); step uses `--script` to drive synthetic input and reads the captured frame buffer + stdout JSON.
  - `APP` — Composition root (`modeltap-app::orchestration::*`). Used for component-level acceptance under the binary surface; not used in the master file but available for performance-critical inner tests.
  - `CORE` — Pure logic in `modeltap-core::logic::*`. Used by Layer C unit tests, not by the acceptance suite.
  - `FS` — Filesystem assertion against the fixture tree (e.g., `assert!(path.exists())`, `assert_eq!(stat(a).st_ino, stat(b).st_ino)`).
  - `JSONL` — Reads `${LOG_DIR}/launch.log` or `diagnostics.log`; parses each line as JSON; asserts on field values.
  - `SNAPSHOT` — Insta snapshot of captured frame buffer.

- **`World` type contract:** the cucumber-rs `World` carries:
  - `temp_dir: TempDir` — per-scenario tmp root
  - `fixture_name: String` — currently-active fixture
  - `env: HashMap<String, String>` — env vars built up by Given steps
  - `cmd: Option<assert_cmd::Command>` — pending binary invocation
  - `output: Option<assert_cmd::assert::Assert>` — captured exit + stdout + stderr
  - `frames: Vec<TerminalFrame>` — captured TestBackend frames from headless mode
  - `script_path: Option<PathBuf>` — generated input script
  - `log_dir: PathBuf` — `${temp_dir}/.modeltap/`

- **Step ordering invariant:** every scenario opens with a `Given Devon has ...` (fixture + env setup), one or more `When Devon ...` (action through the binary), and one or more `Then ...` (assertion). No `Then` step modifies state. No `When` step asserts.

---

## A. TUI Launch / Quit / Terminal-Size Guards

### Given steps

| Step phrase | Seam | Assertion / setup |
|---|---|---|
| `Given Devon's terminal is <N> columns wide` | BIN-HEADLESS | Sets `MODELTAP_TERM_COLS=<N>` env. The headless backend uses this for its TestBackend size. Production crossterm reads from real terminal — the env override only affects headless. |
| `Given a clean modeltap log directory at "<path>"` | BIN-HEADLESS | Creates the directory with mode 0700; sets `MODELTAP_LOG_DIR=<path>` env. Captures the canonical log dir for later JSONL assertions. |
| `Given Devon has launched modeltap in headless mode against fixture "<name>"` | BIN-HEADLESS | Builds the fixture tree via `tests/fixtures/build.sh <name>`, sets all relevant per-plugin env vars to point at it, spawns `modeltap` with `MODELTAP_HEADLESS=1`, waits for first `launch.started` event in the JSONL log, stores the process handle in World. |
| `Given Devon's "<path>" directory exists with mode <octal>` | FS | Creates path; chmods. |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon runs "modeltap" in headless mode` | BIN-HEADLESS | `assert_cmd::Command::cargo_bin("modeltap").env("MODELTAP_HEADLESS", "1").envs(&world.env).timeout(Duration::from_secs(5)).assert()` and stores result. |
| `When Devon presses "<key>"` | BIN-HEADLESS | Appends `key: <key>` to the input script; if the binary is already running, sends via PTY (interactive only); otherwise writes to script file before launch. |
| `When Devon presses Ctrl+C` | BIN-HEADLESS | Sends SIGINT to the running modeltap process. |
| `When Devon presses Enter` | BIN-HEADLESS | Appends `key: Enter` to the input script. |
| `When Devon presses Esc` | BIN-HEADLESS | Appends `key: Esc` to the input script. |
| `When Devon types "<text>"` | BIN-HEADLESS | Appends `type: <text>` to the input script. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then within 1 second the TUI renders the two-pane layout` | BIN-HEADLESS + JSONL | `launch.timing` event has `process_start_to_first_paint_ms < 1000`. The first captured frame contains both "Tools" left-pane header AND a right-pane header. |
| `Then the left pane lists "<tool>" with its model count` | SNAPSHOT | Captured frame has a row in cells [1..30, header_row+i] containing the tool name and a numeric count. |
| `Then the bottom bar shows "<text>"` | SNAPSHOT | Captured frame's last row matches `<text>` exactly (whitespace-tolerant). |
| `Then modeltap exits cleanly with code 0` | BIN | `world.output.unwrap().success()` — exit code 0. |
| `Then modeltap exits with code <N>` | BIN | `world.output.unwrap().get_output().status.code() == Some(<N>)`. |
| `Then the terminal is restored to normal cursor and color state` | BIN | Captured stderr ends with the cursor-show + color-reset escape sequences (`\x1b[?25h\x1b[0m`). In headless mode this is asserted on the captured "exit teardown" buffer, not on real terminal state. |
| `Then modeltap prints "<text>" to stderr` | BIN | `predicate::str::contains(<text>)` on captured stderr. |
| `Then no partial TUI is rendered` | BIN | Captured stdout contains zero ratatui frame markers. |
| `Then modeltap renders the two-pane layout` | SNAPSHOT | Same as "within 1 second" without the timing assertion. |

---

## B. Tool Discovery (per plugin)

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given Devon has Ollama installed in fixture "<name>" containing <N> models totaling <S> GB` | FS | Builds fixture; verifies count and total size. Sets `MODELTAP_OLLAMA_DIR` to the fixture's `~/.ollama/models/` equivalent. |
| `Given Devon has only Ollama installed in fixture "<name>"` | FS | As above; ensures other tool dirs do not exist. |
| `Given fixture "<name>" contains "<path>" of size <S> GB` | FS | Creates a sparse file at fixture-relative path with apparent size <S> GB. |
| `Given fixture "<name>" contains "<path>" with a valid GGUF header` | FS | Writes the GGUF magic + minimal header followed by sparse padding to apparent size matching the filename's quant label. |
| `Given fixture "<name>" contains a truncated file "<path>"` | FS | Writes 100 bytes of GGUF magic prefix only. |
| `Given fixture "<name>" contains a model file with an unrecognized format` | FS | Writes a file with extension `.weird` in `~/llms/`. |
| `Given fixture "<name>" registers <model> in <toolA>, <toolB>, ... with identical SHA256` | FS | Builds the model file once, copies (or reflinks) into each tool's directory with appropriate naming. Verifies SHA256 equality post-setup. |
| `Given fixture "<name>" has Ollama in <fixture> with 2 manifest entries pointing at the same blob "<hash>"` | FS | Creates one blob file; creates two manifest files referencing it. |
| `Given Devon's Ollama directory in fixture "<name>" has mode <octal>` | FS | chmods. |
| `Given fixture "<name>" contains "<env-path>" set to <S> GB` | FS | Plus sets HF_HOME / MODELTAP_LMSTUDIO_DIR / etc. as needed. |
| `Given fixture "<name>" contains <N> model directories under "<path>"` | FS | Builds N HF-style `models--*--*` directories. |
| `Given fixture "<name>" contains a model directory with a broken snapshot symlink` | FS | Creates `models--foo--bar/snapshots/abc/file.gguf` as `ln -s /nonexistent`. |
| `Given fixture "<name>" contains models under "<path>" (older convention)` | FS | Builds at `~/.lmstudio/models/` not `~/.cache/lm-studio/`. |
| `Given the environment variable "<name>" is set to "<value>"` | BIN-HEADLESS | Adds to `world.env`. |
| `Given the user has set "<toml-line>" in config` | FS + BIN-HEADLESS | Writes `${LOG_DIR}/config.toml`; sets `MODELTAP_CONFIG=${LOG_DIR}/config.toml`. |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon selects "<tool>" in the left pane` | BIN-HEADLESS | Appends script: `wait_for: "Models in"; key: until tool name matches`. |
| `When selects the tool containing the unknown-format model` | BIN-HEADLESS | Looks up which tool has a `?` row in the current frame; navigates to it. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the right pane lists <N> models with their tags` | SNAPSHOT | Counts rows in right-pane region; asserts == N. |
| `Then each row shows the model size in GB` | SNAPSHOT | Each right-pane row contains a float-followed-by-"GB" substring. |
| `Then the right-pane header reads "<text>"` | SNAPSHOT | Cell row 1 of right pane matches text. |
| `Then the row for "<id>" begins with "<char>"` | SNAPSHOT | First non-space character of that row equals `<char>`. |
| `Then the row shows "also in: <list>"` | SNAPSHOT | Row substring matches. |
| `Then the row for "<id>" shows "<text>"` | SNAPSHOT | As above. |
| `Then the row shows no "also in:" annotation` | SNAPSHOT | Row substring does not contain "also in:". |
| `Then the model "<filename>" appears with size "<S> GB"` | SNAPSHOT | Combined check. |
| `Then "<id>" appears in the llama-cli model list` | SNAPSHOT | Row exists. |
| `Then the row for "<id>" shows "[format: corrupt]"` | SNAPSHOT | Substring check. |
| `Then the row's display label includes "<text>"` | SNAPSHOT | Substring. |
| `Then modeltap continues to render the other 5 llama-cli models` | SNAPSHOT | Row count >= 5 (in addition to the corrupt one). |
| `Then the affected model row shows "[broken: missing blob]"` | SNAPSHOT | Substring. |
| `Then its size does not contribute to the Hugging Face disk usage shown in the header` | SNAPSHOT | Header total < sum of all listed model sizes. |
| `Then the row's id reads "<text>"` | SNAPSHOT | First text-column cell range matches. |
| `Then 31 models are listed with their org/repo ids and sizes` | SNAPSHOT | Row count + each row contains "/" in the id. |
| `Then 5 models are listed under "Hugging Face"` | SNAPSHOT | Row count == 5. |
| `Then the older-path models are listed` | SNAPSHOT | At least one row in LM Studio pane. |
| `Then 9 models are listed with their ids and sizes` | SNAPSHOT | Row count == 9. |
| `Then the diagnostics log contains an event with level "ERROR" and target "<target>"` | JSONL | Reads `${LOG_DIR}/diagnostics.log`, scans for matching event. |
| `Then the other tools render normally` | SNAPSHOT | Other tool rows in left pane have non-empty model counts. |
| `Then the right-pane header reports the blob's size exactly once in the total GB` | SNAPSHOT + arithmetic | Header total == sum of unique blob sizes (deduplicated by hash). |

---

## C. Two-Pane Navigation

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given Ollama is highlighted in the left pane` | BIN-HEADLESS | After launch, navigates with arrow keys until Ollama row has the highlight style. |
| `Given Devon has selected "Hugging Face" with 31 models` | BIN-HEADLESS | Combined navigation step. |
| `Given the visible window holds 28 rows` | BIN-HEADLESS | Sets `MODELTAP_TERM_ROWS=32` (header + 28 + footer). |
| `Given Devon has fixture "devon-multi-tool" with all four tools installed` | FS | Builds full fixture. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the left pane highlights "<tool>" (alphabetically first)` | SNAPSHOT | Row with tool name has the highlight style attribute. |
| `Then the highlight moves to "<tool>"` | SNAPSHOT | Same on next captured frame. |
| `Then the right pane shows the new tool's models` | SNAPSHOT | Right-pane header matches the new tool. |
| `Then the bottom-right indicator shows "<position>"` | SNAPSHOT | Last 5 cells of bottom row match. |
| `Then no action is taken` | SNAPSHOT | Two consecutive frames are identical (excluding the brief highlight flash). |
| `Then the bottom bar briefly highlights as a visual reminder` | SNAPSHOT | One frame in the next 100 ms has the bottom-bar style changed. |
| `Then modeltap exits cleanly when Devon then presses "q"` | BIN | Combined behavior + assertion. |
| `Then the right pane shows the Hugging Face model list` | SNAPSHOT | Right-pane header reads "Models in Hugging Face". |

---

## D. Indicator Computation (o / * / ! / ?)

| Step phrase | Seam | Assertion |
|---|---|---|
| `Given fixture "<name>" has a model registered with 2+ tools matching by SHA256` | FS | As in B. |
| `Given Ollama and llama-cli both declare GGUF in accepted_formats` | CORE-config | Verifies plugin metadata at startup (assertion against `Tool::accepted_formats()`). |
| `Given no other supported tool declares AWQ in accepted_formats` | CORE-config | Same. |
| `Then the model's row indicator is "<char>" in every tool's pane` | SNAPSHOT | Navigates to each tool, captures frame, asserts indicator. |
| `Then the Llama-3-8B row indicator is "<char>"` | SNAPSHOT | Single-pane assertion. |
| `Then the AWQ row indicator is "<char>"` | SNAPSHOT | Same. |
| `Given any populated inventory built from fixture "<name>"` | BIN-HEADLESS | Combined fixture + launch. |
| `Then every row begins with one of "o", "*", "!", "?"` | SNAPSHOT | Iterates all rows in the captured frame; asserts first non-space char matches. (`@property` tag — DELIVER may implement as proptest over generated inventories.) |

---

## E. Zap-All (US-05) Flow

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given Devon has fixture "<name>" with llama-cli holding <N> models (<U> shared, <V> unique) totaling <S> GB` | FS + arithmetic | Verifies counts and totals. |
| `Given Devon has opened the zap dialog for "<tool>"` | BIN-HEADLESS | Combined navigation + key press. |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon zaps llama-cli successfully` | BIN-HEADLESS | High-level convenience step: select, press z, type, Enter. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the <N> unique model files are removed from the llama-cli fixture directory` | FS | For each unique-id, asserts `path.exists() == false`. |
| `Then the <N> shared model registrations are removed from llama-cli` | FS | For each shared-id, asserts the llama-cli copy is removed (file or registration entry depending on plugin). |
| `Then the <N> shared model files remain in their other tools' directories` | FS | For each shared-id, asserts the files in OTHER tool dirs still exist. |
| `Then the right pane shows "<text>"` | SNAPSHOT | Substring check on right pane. |
| `Then the dialog closes with no models deleted` | FS + SNAPSHOT | Pre-state == post-state file inventory; dialog is gone. |
| `Then the llama-cli fixture directory is unchanged` | FS | Walks the dir; computes a manifest of (path, size, mtime); compares to pre-state. |
| `Then the dialog reads "<text>"` | SNAPSHOT | Dialog area substring. |
| `Then only "[Esc] close" is offered` | SNAPSHOT | Bottom of dialog area lists exactly one shortcut. |
| `Then no destructive action is performed when Devon presses Esc` | FS | Pre/post fixture manifest equal. |

---

## F. Zap-One (US-05b) Flow

| Step phrase | Seam | Assertion |
|---|---|---|
| `Given fixture "<name>" registers <model> in both <toolA> and <toolB> with identical SHA256` | FS | As in B. |
| `Given Devon is on the Mistral detail screen viewing the llama-cli registration` | BIN-HEADLESS | Combined navigation: select Mistral, Enter, navigate registrations to llama-cli's. |
| `Given Devon is on the detail screen for "<id>"` | BIN-HEADLESS | Combined navigation. |
| `Given fixture "<name>" contains an AWQ model registered only in Hugging Face` | FS | As in B. |
| `Given Devon has opened the single-model delete dialog for any model` | BIN-HEADLESS | Generic. |
| `Given Devon is on the detail screen for a unique-to-llama-cli model "<id>"` | BIN-HEADLESS | Combined. |
| `When Devon deletes the llama-cli copy of Mistral via the [d] shortcut` | BIN-HEADLESS | High-level convenience step. |
| `Then the llama-cli copy of Mistral is removed from disk` | FS | path.exists == false. |
| `Then the Ollama copy of Mistral remains in its directory` | FS | path.exists == true. |
| `Then the AWQ file is removed from the Hugging Face fixture directory` | FS | path.exists == false. |
| `Then the dialog requires the user to type "<id>"` | SNAPSHOT | Dialog text contains "Type <id>". |
| `Then the dialog closes with no file deleted` | FS + SNAPSHOT | Combined. |
| `Then the oddball-model file remains in the llama-cli fixture directory` | FS | path.exists == true. |
| `Then no file is deleted` | FS | Walks fixture; pre/post equal. |
| `Then the detail screen returns` | SNAPSHOT | Frame shows detail-screen header. |

---

## G. Unify (US-10) Flow

### Given steps

| Step phrase | Seam | Setup |
|---|---|---|
| `Given fixture "<name>" has <model> with 3 separate copies of <S> GB across 3 tools on the same filesystem` | FS | Builds. Asserts same st_dev for all 3 paths. |
| `Given fixture "<name>" has a model whose 3 registered paths all stat to the same inode` | FS | Builds, then hardlinks 2 paths to the 3rd. |
| `Given fixture "<name>" has Mistral registered in Ollama as a manifest pointing at a blob` | FS | Standard Ollama layout. |
| `Given Devon has opened the unify dialog for Mistral against fixture "<name>"` | BIN-HEADLESS | Combined navigation. |

### When steps

| Step phrase | Seam | Behavior |
|---|---|---|
| `When Devon unifies Mistral with the llama-cli copy as canonical` | BIN-HEADLESS | High-level: open detail, press u, select canonical = llama-cli, Enter. |
| `When Devon presses "u" on a Mistral model against fixture "<name>"` | BIN-HEADLESS | Combined. |
| `When Devon presses "u" on a model registered in Ollama against fixture "<name>"` | BIN-HEADLESS | Combined. |
| `When Devon proceeds with unify` | BIN-HEADLESS | Press Enter on currently-open unify dialog. |

### Then steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then one of the existing tool-owned Mistral paths is chosen as canonical` | FS | Asserts the canonical path equals one of the original tool paths (NOT under `${LOG_DIR}/store/` — Q1 invariant). |
| `Then the other 2 paths stat to the same inode as the canonical` | FS | `assert_eq!(stat(p1).st_ino, stat(canonical).st_ino)` for each non-canonical path. |
| `Then no file is created under "${LOG_DIR}/store"` | FS | `${LOG_DIR}/store/` does not exist (per Q1: stateless, no central store). |
| `Then hardlinks are created for all targets` | FS | All target paths share st_ino with canonical. |
| `Then no fallback prompt appears` | SNAPSHOT | Dialog area shows no "[s] skip / [c] copy" line. |
| `Then the dialog reads "<text>"` | SNAPSHOT | Substring. |
| `Then no [Enter] proceed action is offered` | SNAPSHOT | Dialog bottom shortcuts do not include "Enter". |
| `Then the [u] shortcut is dimmed` | SNAPSHOT | Bottom-bar cell for "[u]" has the dim style attribute. |
| `Then the screen shows "single tool — unify not applicable"` | SNAPSHOT | Substring. |
| `Then the Ollama blob path "<path>" stats to the same inode as the llama-cli "<filename>"` | FS | st_ino equality. |
| `Then the Ollama manifest at "<path>" still references the blob hash` | FS | Reads manifest JSON; asserts blob hash unchanged. |
| `Then the dialog reads "all targets on different filesystems — unify cannot proceed"` | SNAPSHOT | Substring. |
| `Then no action is performed` | FS | Pre/post fixture equal. |

---

## H. Cross-FS Fallback (US-19) Flow

| Step phrase | Seam | Assertion |
|---|---|---|
| `Given fixture "<name>" places all Mistral copies on the same filesystem` | FS | st_dev equal for all 3. |
| `Given fixture "<name>" has 1 of 3 Mistral copies on a different filesystem` | FS | Builds with the FakeFsProbe configured to report cross-fs for the llama-cli path. (See acceptance-test-plan §3 for cross-fs fixture mechanics.) |
| `Given fixture "<name>" places all Mistral copies on mutually different filesystems` | FS | Same with FakeFsProbe for all targets. |
| `When Devon presses "u" then Enter, then "<key>" to <action>` | BIN-HEADLESS | Multi-key script. |
| `Then the 2 same-fs targets become hardlinks to the canonical` | FS | st_ino equality. |
| `Then the cross-fs target remains an independent file at its original path` | FS | path.exists, st_ino != canonical's st_ino. |
| `Then the cross-fs target's file contents match the canonical (SHA256-equal) but with a different inode` | FS | Compute SHA256 of both; assert equal. Assert st_ino different. |
| `Then the right pane shows "Reclaimed: <S> GB" and "Skipped: <N> cross-fs target"` | SNAPSHOT | Substring. |
| `Then the right pane shows "Reclaimed: <S> GB" and "Copied: <N> cross-fs target (no reclaim)"` | SNAPSHOT | Substring. |
| `Then the dialog offers "[s] skip cross-fs / [c] copy / [x] cancel"` | SNAPSHOT | Substring. |

---

## I. Running-Tool Detection (US-17) Flow

| Step phrase | Seam | Assertion |
|---|---|---|
| `Given the FsProbe adapter is configured with fake-lsof script "<name>" reporting "<text>"` | BIN-HEADLESS | Sets `MODELTAP_LSOF=tests/fakes/<name>.sh`; verifies script exists. The script writes the text to stdout when invoked. |
| `Given the FsProbe adapter reports no holding processes` | BIN-HEADLESS | Sets `MODELTAP_LSOF=tests/fakes/lsof-empty.sh`. |
| `Given the FsProbe adapter returns LsofResult::Unavailable` | BIN-HEADLESS | Sets `MODELTAP_LSOF=tests/fakes/lsof-missing.sh` (script exits 127). |
| `Given the running-tool dialog is showing for Ollama` | BIN-HEADLESS | Combined: launch + open unify + assert dialog. |
| `When the FsProbe adapter is reconfigured with fake-lsof script "<name>"` | BIN-HEADLESS | At-runtime swap by writing a new symlink target for the lsof binary path. |
| `Then the dialog offers "[r] retry" and "[Esc] cancel"` | SNAPSHOT | Substring. |
| `Then no filesystem mutation occurs while the dialog is open` | FS | Snapshots fixture state before opening dialog; re-snaps after dialog open without action; asserts equal. |
| `Then the unify proceeds normally` | BIN-HEADLESS | After the retry key, the unify completes; verify by checking inode equality. |
| `Then the dialog has no running-tool warning section` | SNAPSHOT | Dialog text does not contain "Running tools detected". |
| `Then the dialog proceeds directly to the unify plan` | SNAPSHOT | Dialog shows canonical/targets layout. |
| `Then the dialog includes "Running-tool detection unavailable on this system"` | SNAPSHOT | Substring. |
| `Then Devon can still proceed at his own risk` | SNAPSHOT | Dialog still offers Enter to proceed. |

---

## J. Plugin Trait Contract (US-18)

| Step phrase | Seam | Assertion |
|---|---|---|
| `Given the workspace contains a 5th plugin "<name>" implementing the Tool trait per fixture "<name>"` | FS | The fixture-builder script for `riley-fifth-plugin` creates `plugins/atomic-chat/{Cargo.toml, src/lib.rs}` with a minimal Tool impl, then adds it to the workspace `Cargo.toml`. The acceptance test `cargo build`s the workspace before launch. |
| `Given no source file under "crates/modeltap-core/src/" was modified` | FS | `git diff --quiet crates/modeltap-core/src/` returns 0 against the pre-test commit. |
| `Then the left pane includes "Atomic Chat" alongside the original four tools` | SNAPSHOT | Five rows in left pane. |
| `Given a registered plugin "<name>" panics inside its discover() method` | FS | Builds a test plugin in `plugins/broken-plugin/` whose discover() does `panic!("intentional test panic")`. Acceptance test launches with this plugin registered. |
| `Then the left pane shows "<plugin>" with "(error)"` | SNAPSHOT | Substring. |
| `Then the diagnostics log contains an event with level "ERROR" and field "panic_message"` | JSONL | Reads diagnostics.log; matches. |
| `Then no plugin crate appears in modeltap-core's direct dependencies` | BIN | Runs `cargo metadata --format-version 1`, parses, checks per `architecture-design.md` § 8.2 lint rule. |
| `Then no plugin crate depends on another plugin crate` | BIN | Same. |
| `Then no concrete plugin crate appears in modeltap-tui's direct dependencies` | BIN | Same. |
| `Then the JSONL log "launch.inventory" event has tools_registered listing all 5 plugin names` | JSONL | Asserts `event.tools_registered.len() == 5` and contains all 5. |

---

## K. Cross-Platform (US-20)

| Step phrase | Seam | Assertion |
|---|---|---|
| `Given the environment variable "MODELTAP_FORCE_PLATFORM" is set to "<platform>"` | BIN-HEADLESS | Adds env. The launch.started JSONL event uses this instead of compile-time platform. |
| `Given fixture "<name>" contains all four tools at their <platform> default paths` | FS | Per-platform fixture variant. |
| `Given the modeltap binary is executed on native Windows (not WSL)` | BIN | Sets `MODELTAP_FORCE_PLATFORM=windows-x86_64`. |
| `Given the user is running modeltap inside WSL2 against fixture "<name>"` | BIN-HEADLESS | Tests run on Linux CI; WSL uses identical paths to Linux. |
| `Then all four tools are discovered with non-zero model counts` | SNAPSHOT | All four rows have count > 0. |
| `Then the JSONL log "launch.started" event has platform == "<platform>"` | JSONL | Field equality. |
| `Then it prints "Windows is supported only via WSL — see <url>" to stderr` | BIN | Substring. |
| `Then it exits with code 64` | BIN | Exit code. |
| `Then discovery succeeds with the same paths as native Linux` | SNAPSHOT | Same as Linux discovery scenario. |
| `Then the JSONL log platform field reads "linux-x86_64"` | JSONL | Field equality. |

---

## L. KPI Assertions (JSONL log)

These steps read `${LOG_DIR}/launch.log` (one JSON object per line) and assert on parsed fields.

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then the JSONL log contains exactly one "<event_type>" event` | JSONL | Count event matches; assert == 1. |
| `Then the event has <field> == <value>` | JSONL | Field equality. |
| `Then the event has <field> > <value>` | JSONL | Numeric comparison. |
| `Then the event has <field> < <value>` | JSONL | Numeric comparison. |
| `Then the event has <field> matching <regex>` | JSONL | Regex match (e.g., ULID format `^[0-9A-HJKMNP-TV-Z]{26}$`). |
| `Then the event has <field> including <value>` | JSONL | Array contains. |
| `Then the JSONL log "<event_type>" event records "<dotted.field>" less than <N>` | JSONL | Nested field lookup + numeric comparison. |
| `Then the JSONL log "<event_type>" event has <field> < <N>` | JSONL | As above. |
| `Then the JSONL log's first event has event == "<type>"` | JSONL | Asserts position 0. |
| `Then the JSONL log's last event has event == "<type>"` | JSONL | Asserts last line. |
| `Then the JSONL log's last event is not "<type>"` | JSONL | Negation. |
| `Then the JSONL log "<event_type>" event contains no model names` | JSONL | Asserts no field value matches any model name from the fixture. |
| `Then contains no file paths` | JSONL | Asserts no field value contains `/` outside known-safe contexts (tool names). |
| `Then contains no SHA256 hashes` | JSONL | Asserts no field value matches `^[0-9a-f]{64}$`. |
| `Then contains only counts and tool names` | JSONL | Whitelist field validation. |
| `Then the JSONL log "<event_type>" event has tools_registered listing all <N> plugin names` | JSONL | Array length + content check. |
| `Then the JSONL log contains zero "<event_type>" events for this session` | JSONL | Count == 0 filtered by session_id. |
| `Then the timing JSON printed to stdout has <field> < <N>` | BIN | Parses stdout (single JSON object); checks field. |

---

## M. Time / Performance Assertions

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then within 500 milliseconds the summary bar shows "<text>"` | SNAPSHOT + timing | Polls captured frames at 50 ms intervals; first matching frame must arrive within 500 ms of action complete. |
| `Then within 1 second the TUI renders the two-pane layout` | JSONL | `process_start_to_first_paint_ms < 1000` from launch.timing event. |

---

## N. Last-Action / Right-Pane Feedback

| Step phrase | Seam | Assertion |
|---|---|---|
| `Given Devon has just zapped llama-cli reclaiming <S> GB and retaining <T> GB against fixture "<name>"` | BIN-HEADLESS | Convenience step composing the full zap flow. |
| `Given Devon has just unified <model> with <N> hardlinks created against fixture "<name>"` | BIN-HEADLESS | Convenience step. |
| `Given Devon has just dry-run unify on <model>` | BIN-HEADLESS | Convenience step. |
| `Given Devon ran unify against fixture "<name>" and 2 of 3 targets succeeded` | BIN-HEADLESS | Combined with cross-fs fixture. |
| `Given Devon sees a "<text>" message in the right pane` | SNAPSHOT precondition | Asserts current frame contains text before proceeding. |
| `Given Devon's pre-zap total was <S> GB shown in the summary bar against fixture "<name>"` | SNAPSHOT precondition | Combined launch + assertion. |
| `Given Devon's pre-unify summary bar shows "<S>" and "<N> models" against fixture "<name>"` | SNAPSHOT precondition | Same. |
| `Given Devon has just completed a zap action against fixture "<name>"` | BIN-HEADLESS | Convenience. |
| `Given the post-action discovery rebuild fails because the tool directory was removed` | FS | Removes the tool dir between action complete and refresh trigger. |
| `When the zap action completes` | BIN-HEADLESS | Wait for `Last action:` substring in frame. |
| `When the unify action completes` | BIN-HEADLESS | Same. |
| `When the action completes` | BIN-HEADLESS | Same (generic). |
| `When the summary bar tries to refresh` | BIN-HEADLESS | Wait for the next render after action-complete event. |
| `Then the body reads "<text>"` | SNAPSHOT | Substring within right-pane body region. |
| `Then it shows the previous values with "(refresh failed)" indicator` | SNAPSHOT | Substring. |
| `Then "[r] retry" is offered in the bottom bar` | SNAPSHOT | Substring. |
| `Then the failed target's path and reason are shown below` | SNAPSHOT | Pane includes "${SOME_PATH}" and a non-empty reason. |
| `Then the "Last action" line is no longer displayed` | SNAPSHOT | Substring NOT in frame. |

---

## O. Detail Screen Steps

| Step phrase | Seam | Assertion |
|---|---|---|
| `When Devon selects the Mistral row and presses Enter` | BIN-HEADLESS | Combined navigation + key. |
| `When Devon presses Enter on a model row to open the detail screen` | BIN-HEADLESS | Same. |
| `Given Devon has opened any detail screen against fixture "<name>"` | BIN-HEADLESS | Combined. |
| `Then the detail screen lists all 3 paths` | SNAPSHOT | Asserts 3 path strings present in detail body. |
| `Then the status reads "<text>"` | SNAPSHOT | Detail status row substring. |
| `Then the reclaim estimate reads "<text>"` | SNAPSHOT | Detail reclaim row substring. |
| `Then the screen shows 1 path` | SNAPSHOT | Path row count == 1. |
| `Then the screen shows "Reclaimed: <S> GB"` | SNAPSHOT | Substring. |
| `Then the main two-pane view is shown` | SNAPSHOT | Frame matches main-view header style. |
| `Then the previously-selected row remains highlighted` | SNAPSHOT | Highlight position preserved. |

---

## P. Help Overlay (US-08)

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then a help overlay opens listing all shortcuts grouped by context` | SNAPSHOT | Frame contains overlay box with multiple context headers ("Main", "Detail", "Dialogs"). |
| `Then the help overlay closes` | SNAPSHOT | Frame returns to underlying view. |
| `Then "[u] unify" is shown but dimmed` | SNAPSHOT | Bottom-bar cell for "[u]" has dim style attribute. |
| `Then "[z] zap tool" is shown brightly` | SNAPSHOT | Bottom-bar cell for "[z]" has normal style. |
| `Then the bottom bar shows "<text>"` | SNAPSHOT | Last row substring. |
| `Then the main shortcuts are not displayed` | SNAPSHOT | Bottom bar substring NOT in frame. |

---

## Q. Format/Capability and Color (US-04, US-16)

| Step phrase | Seam | Assertion |
|---|---|---|
| `Then no ANSI color codes appear in the captured frame` | SNAPSHOT | Captured frame text contains no `\x1b[` sequences. |
| `Then the captured frame's cell at the AWQ row indicator position has foreground color "Red"` | SNAPSHOT | Reads `Buffer::cell(x, y).style.fg`; asserts == `Color::Red`. (Per acceptance-test-plan §10 OQ-2 — structured snapshot comparison.) |
| `Given a registered plugin "<name>" returns an empty slice from accepted_formats()` | FS | Builds a test plugin with `fn accepted_formats(&self) -> &'static [Format] { &[] }`. |
| `Then every row begins with "?"` | SNAPSHOT | All rows in the broken plugin's pane start with `?`. |
| `Then the diagnostics log contains a warning "<text>"` | JSONL | Substring match in diagnostics.log. |

---

## R. Architecture-Lint Step (US-18)

| Step phrase | Seam | Assertion |
|---|---|---|
| `Given the workspace has been built` | BIN | Runs `cargo build --workspace --offline`; asserts success. |
| `When the architecture-lint test runs` | BIN | Runs `cargo test --package modeltap-core --test architecture --locked`; captures result. |

---

## S. Step-Definition File Layout (DELIVER recommendation)

Per `nw-bdd-methodology` "Step Organization by Domain":

```
crates/modeltap-app/tests/acceptance/
├── world.rs                    # World struct + cucumber main
├── steps/
│   ├── launch.rs               # A — TUI launch / quit / size guards
│   ├── discovery.rs            # B — per-plugin discovery
│   ├── navigation.rs           # C — two-pane navigation
│   ├── indicator.rs            # D — o/*/!/?  computation
│   ├── zap_all.rs              # E — US-05
│   ├── zap_one.rs              # F — US-05b
│   ├── unify.rs                # G — US-10
│   ├── cross_fs.rs             # H — US-19
│   ├── running_tool.rs         # I — US-17
│   ├── plugin_trait.rs         # J — US-18
│   ├── platform.rs             # K — US-20
│   ├── kpi.rs                  # L — JSONL assertions
│   ├── timing.rs               # M — performance assertions
│   ├── last_action.rs          # N — right-pane feedback
│   ├── detail_screen.rs        # O — US-13
│   ├── help_overlay.rs         # P — US-08
│   ├── color.rs                # Q — US-04 / US-16 color assertions
│   └── architecture.rs         # R — workspace-metadata lint
└── fixtures/
    ├── build.sh                # the fixture builder
    ├── README.md               # named-fixture inventory
    ├── devon-multi-tool/
    ├── devon-empty/
    ├── devon-only-ollama/
    ├── devon-permission-denied/
    ├── devon-cross-fs/
    ├── k3-bench/
    ├── riley-fifth-plugin/
    └── fakes/
        ├── lsof-running-ollama.sh
        ├── lsof-empty.sh
        └── lsof-missing.sh
```

Each step file is a self-contained module of `#[given(...)] / #[when(...)] / #[then(...)]` functions over the shared `World` type. cucumber-rs auto-discovers them.
