<!-- markdownlint-disable MD024 -->

# User Stories — modeltap-tui

All stories share the persona **Devon Park** unless noted. Devon is a local-AI power user on macOS or Linux who runs at least two of {Ollama, llama-cli, Hugging Face cache, LM Studio} and has noticed disk pressure from duplicate model files. Stories US-18 and US-20 use a second persona: **Riley Chen**, an open-source contributor who wants to add support for a fifth tool (Atomic Chat).

Story IDs are stable for cross-document referencing. Acceptance Criteria use Given/When/Then derived from `journey-cleanup-and-unify.feature`.

---

## US-01: TUI launches and quits cleanly

### Problem

Devon Park runs three local-AI tools and is unsure his terminal will even render a complex TUI without flicker or escape-sequence bleed. He needs to know the foundation is solid before he trusts it with destructive actions.

### Who

- Devon Park, multi-tool local-AI power user, macOS Terminal.app and Linux `xterm-256color`, comfortable with vim-style keys.

### Solution

A `modeltap` binary that, when run, draws the two-pane layout with placeholder data on first execution and exits cleanly on `q` or Ctrl+C, restoring the terminal state.

### Domain Examples

#### 1: Happy path — Devon launches in iTerm2

Devon, on macOS in iTerm2 (256-color), runs `modeltap` from his shell. Within 1 second the two-pane layout paints. He presses `q` and his shell prompt returns with no escape characters left on screen.

#### 2: Edge — Devon launches inside `tmux`

Devon, inside a `tmux` 3.4 session over SSH to a Linux box, runs `modeltap`. The TUI renders correctly (no broken box-drawing characters). He presses Ctrl+C; the terminal returns to normal.

#### 3: Error — Devon's terminal is too narrow

Devon's terminal window is 60 columns wide (TUI requires 80 minimum). He runs `modeltap`; instead of crashing, modeltap prints "Terminal too narrow: need at least 80 columns, found 60. Resize and retry." and exits with code 2.

### UAT Scenarios (BDD)

#### Scenario: Devon launches modeltap on macOS

Given Devon is in iTerm2 on macOS with a 100-column-wide terminal
When Devon runs `modeltap`
Then within 1 second the TUI renders the two-pane layout
And the bottom bar shows the keyboard shortcuts

#### Scenario: Devon quits with q

Given Devon is in modeltap with the TUI rendered
When Devon presses `q`
Then modeltap exits with code 0
And the terminal is restored to normal cursor and color state

#### Scenario: Devon quits with Ctrl+C

Given Devon is in modeltap with the TUI rendered
When Devon presses Ctrl+C
Then modeltap exits with code 130
And the terminal is restored

#### Scenario: Terminal too narrow refuses to start

Given Devon's terminal is 60 columns wide
When Devon runs `modeltap`
Then modeltap prints "Terminal too narrow: need at least 80 columns, found 60"
And exits with code 2 without rendering a partial TUI

### Acceptance Criteria

- [ ] Cold start to first paint completes in under 1 second on a 2020-or-later workstation
- [ ] Pressing `q` exits with code 0 and restores terminal state
- [ ] Pressing Ctrl+C exits with code 130 and restores terminal state
- [ ] Terminal narrower than 80 columns is detected and refused with a usage error (exit 2)
- [ ] No escape-sequence garbage left on screen after exit, in any of {Terminal.app, iTerm2, tmux, alacritty, xterm}

### Outcome KPIs

- **Who**: Devon (and any first-time user)
- **Does what**: Sees the inventory after launching modeltap (K3)
- **By how much**: First paint within 1 second
- **Measured by**: Built-in startup timing logged to `~/.modeltap/launch.log`
- **Baseline**: N/A (greenfield)

### Technical Notes

- Ratatui-based TUI with explicit terminal capability detection.
- Must implement panic hooks that restore the terminal before printing the panic.
- Walking-skeleton scope: the right pane may show stub data; real Ollama discovery comes in US-02.

### Dependencies

None. This is the first story.

---

## US-02: Discover Ollama models

### Problem

Devon has a populated `~/.ollama/models/` directory with manifests and blobs. He needs modeltap to enumerate every Ollama-registered model with its size and on-disk path so the right pane shows real, current data — not stub data.

### Who

- Devon Park, multi-tool local-AI power user, has Ollama 0.1.x or later installed.

### Solution

The Ollama plugin reads `~/.ollama/models/manifests/registry.ollama.ai/library/<name>/<tag>` and resolves blob references in `~/.ollama/models/blobs/sha256-*` to produce a list of `Model { id, size, format, on_disk_path }` entries.

### Domain Examples

#### 1: Happy path — Devon's library

Devon has 12 Ollama models including `llama3:8b-instruct-q4_K_M`, `mistral:7b-instruct-q4_K_M`, and `qwen2.5:14b-q4_K_M`. modeltap launches; the right pane (Ollama selected) lists all 12 with sizes summing to 47.3 GB.

#### 2: Edge — Devon has only Ollama installed

Devon has Ollama but no llama-cli, no HF cache, no LM Studio. modeltap shows Ollama in the left pane with 12 models; other tools show 0 models with "(not installed)" beside them.

#### 3: Error — Ollama directory is unreadable

Devon's `~/.ollama/models/` has wrong permissions (he `sudo`'d once and left it owned by root). modeltap shows "Ollama (error: permission denied — see ~/.modeltap/diagnostics.log)" in the left pane and continues to render the other tools.

### UAT Scenarios (BDD)

#### Scenario: Ollama models are discovered and listed

Given Devon has 12 Ollama models in `~/.ollama/models/` totaling 47.3 GB
When Devon runs `modeltap`
And selects Ollama in the left pane
Then the right pane lists all 12 models with their tags
And each row shows the model size in GB
And the right-pane header shows "Models in Ollama (12, 47.3 GB)"

#### Scenario: Missing Ollama installation is handled

Given Devon does not have Ollama installed (no `~/.ollama/` directory)
When Devon runs `modeltap`
Then Ollama appears in the left pane with "0" model count and "(not installed)" annotation
And modeltap continues to function for other tools

#### Scenario: Unreadable Ollama directory does not crash modeltap

Given Devon's `~/.ollama/models/` exists but is unreadable due to permissions
When Devon runs `modeltap`
Then Ollama appears in the left pane with "(error)" annotation
And the diagnostic message is written to `~/.modeltap/diagnostics.log`
And other tools render normally

### Acceptance Criteria

- [ ] All Ollama models in standard locations are discovered, with correct id, size, and on-disk path
- [ ] Total disk usage reported equals sum of unique blob sizes (manifests sharing a blob count it once)
- [ ] Missing Ollama directory is handled as "not installed", not as an error
- [ ] Unreadable Ollama directory is handled as "error", not a crash
- [ ] Discovery completes within 2 seconds for ≤200 models

### Outcome KPIs

- **Who**: Devon
- **Does what**: Sees real Ollama models in the inventory
- **By how much**: 100% of installed Ollama models are listed (validated against `ollama list`)
- **Measured by**: Manual cross-check during testing; `modeltap diff ollama` developer command in early releases
- **Baseline**: 0 (no tool exists)

### Technical Notes

- Ollama's on-disk layout: manifests at `~/.ollama/models/manifests/<registry>/<repo>/<tag>`, blobs at `~/.ollama/models/blobs/sha256-<hash>`. Blobs are content-addressed and shared between manifest entries.
- Multiple manifests can reference the same blob; for size accounting, deduplicate by blob hash within Ollama itself.
- Cross-platform: same paths on macOS and Linux when Ollama is installed via the official installer.

### Dependencies

- US-01 (TUI must launch first)

---

## US-03: Two-pane layout (tools left, models right)

### Problem

Devon needs to navigate between tools and within a tool's model list using only the keyboard. The layout must make obvious which tool is selected, which model is highlighted, and which actions are available.

### Who

- Devon Park, keyboard-first user.

### Solution

Ratatui layout: left pane (≤30 cols) lists tools with model counts; right pane (remaining width) lists models for the selected tool. Left/right arrow keys switch tools; up/down arrows move within the current pane. Bottom bar always shows shortcuts.

### Domain Examples

#### 1: Happy path — Devon switches tools

Devon launches modeltap. Ollama is highlighted by default. He presses Right Arrow; the highlight moves to llama-cli and the right pane refreshes to show llama-cli's 6 models.

#### 2: Edge — Devon scrolls a long list

Devon selects Hugging Face (31 models). The right pane shows the first 28 fitting on screen with `[scroll: 28/31]` in the bottom-right. Down Arrow past row 28 scrolls.

#### 3: Error — Devon presses an unbound key

Devon presses `x` (no binding). Nothing happens; the bottom bar briefly highlights to remind him of valid shortcuts.

### UAT Scenarios (BDD)

#### Scenario: Default selection is the first tool

Given modeltap has just launched with 4 tools available
When the TUI first renders
Then the first tool (alphabetically first installed) is highlighted in the left pane
And its models are shown in the right pane

#### Scenario: Arrow keys switch between tools

Given Devon is on the Ollama tool in the left pane
When Devon presses Right Arrow
Then the highlight moves to llama-cli
And the right pane refreshes to show llama-cli's models
And the right-pane header reads "Models in llama-cli (6, 21.4 GB)"

#### Scenario: Arrow keys scroll within a long model list

Given Devon has selected Hugging Face with 31 models
And the visible window holds 28 rows
When Devon presses Down Arrow past the last visible row
Then the list scrolls
And the bottom-right indicator shows the position e.g. "29/31"

#### Scenario: Unbound key is silently ignored

Given Devon is in modeltap
When Devon presses `x` (which has no binding)
Then no action is taken
And the bottom bar briefly highlights as a visual reminder

### Acceptance Criteria

- [ ] Left pane lists tools with model count and disk usage; selected tool is visibly highlighted
- [ ] Right pane shows models for the currently-selected tool with id and size
- [ ] Left/Right arrows switch tools; Up/Down arrows scroll within the right pane
- [ ] Tab key cycles between left-pane focus and right-pane focus (mouse alternative)
- [ ] Bottom bar shows current shortcuts at all times, occupying exactly one row
- [ ] Unbound keys produce no destructive action and no error message

### Outcome KPIs

- **Who**: Devon
- **Does what**: Navigates the inventory with no mouse, no scroll-back search
- **By how much**: All four tools and their model lists reachable within 5 keypresses from launch
- **Measured by**: Keyboard-path counting during UAT
- **Baseline**: N/A

### Technical Notes

- Ratatui layout: `Layout::default().direction(Direction::Vertical).constraints([Length(N), Length(1)])`, then horizontal split for top.
- Selection state lives in the central `App` state struct; arrow keys dispatch through update messages (Elm-style).

### Dependencies

- US-01, US-02 (or any other discovery story to populate panes)

---

## US-04: Show model size and registered tools on each row

### Problem

Devon's model list rows currently show only id and size; he can't tell from the row whether the model is registered with one tool or many. Without that, the deduplication value proposition is invisible.

### Who

- Devon Park.

### Solution

Each model row in the right pane shows: indicator (o/*/!), model id, size, and (when applicable) "also in: tool1, tool2".

### Domain Examples

#### 1: Happy path — Devon sees a multi-tool model

Devon's row for `mistral:7b-instruct-q4_K_M` (selected tool: Ollama) reads `* mistral:7b-instruct-q4_K_M  4.4 GB  also in: llama-cli, Hugging Face`.

#### 2: Edge — Single-tool model, compatible elsewhere

Devon's row for `meta-llama/Llama-3-8B` (in HF cache only, GGUF format which other tools accept) reads `o meta-llama/Llama-3-8B  16.0 GB`.

#### 3: Error — Format unknown to modeltap

Devon's row for a model in some unrecognized format reads `? unknown-format-model  3.2 GB  [format: ?]` with a tooltip on detail screen explaining "format not recognized; cannot determine compatibility."

### UAT Scenarios (BDD)

#### Scenario: Multi-tool model shows the * indicator and other tools

Given Mistral-7B-v0.3 q4_K_M is registered in Ollama, llama-cli, and Hugging Face
And Devon is viewing the Ollama right pane
When the row for Mistral renders
Then it begins with the `*` indicator
And it shows "also in: llama-cli, Hugging Face"

#### Scenario: Single-tool but compatible model shows o

Given Llama-3-8B GGUF is in Hugging Face only
And Ollama and llama-cli accept GGUF
When the row renders
Then it begins with `o`
And it shows no "also in" annotation

#### Scenario: Unknown format shows ? indicator

Given a model file has a format modeltap does not recognize
When the row renders
Then it begins with `?`
And the format field shows "[format: ?]"

### Acceptance Criteria

- [ ] Every right-pane row displays one of {`o`, `*`, `!`, `?`} as its first character
- [ ] `*`-marked rows display "also in: ..." with the other tool names
- [ ] Indicator color: `o` neutral, `*` neutral, `!` red, `?` yellow (paired with the symbol — never color-only)
- [ ] Indicator is computed from `model.compatible_tools` per the dedup-key strategy

### Outcome KPIs

- **Who**: Devon
- **Does what**: Visually distinguishes deduplicable models from format-locked ones
- **By how much**: 100% of rows show a correct indicator on first render
- **Measured by**: UAT against a synthetic library with known compatibility expectations
- **Baseline**: N/A

### Technical Notes

- Indicator computation depends on Q3 (only-one-tool definition) and Q6 (dedup key). DESIGN must close both.
- Color must respect `NO_COLOR` env var per clig.dev.

### Dependencies

- US-03 (right pane), US-09 (compatibility computation engine — but US-04 ships the row format; US-09 ships the engine)

---

## US-05: Zap a tool's models with typed confirmation

### Problem

Devon wants to wipe all 6 models for llama-cli (he's stopped using it) but has been bitten before by `rm -rf` typos. He needs a destructive command with the strongest cheap guard against accident.

### Who

- Devon Park.

### Solution

When a tool is selected in the left pane, pressing `z` opens a confirmation dialog that requires Devon to type the tool's name exactly. The dialog also shows model count, total bytes, and unique-vs-shared breakdown so Devon knows exactly what will be lost vs preserved elsewhere.

### Domain Examples

#### 1: Happy path — Devon zaps llama-cli

Devon has 6 llama-cli models (4 also in Ollama, 2 unique) totaling 21.4 GB. He presses `z`. The dialog reads "THIS WILL DELETE 6 MODELS (21.4 GB) FROM llama-cli. 4 are also registered with another tool ... 2 are unique to llama-cli and will be permanently removed." He types `llama-cli` and presses Enter. All 6 are removed; 14.6 GB is reclaimed (21.4 - 6.8 retained); the 4 shared models remain in Ollama.

#### 2: Edge — Devon types the wrong name

Devon presses `z` for llama-cli, types `llamacli` (no hyphen), presses Enter. The dialog closes with no changes; the right pane shows the original list.

#### 3: Error — Zap on a tool with zero models

Devon zaps Hugging Face which has 0 models registered. The dialog reads "Hugging Face has 0 models. Nothing to zap." with [Esc] to close.

### UAT Scenarios (BDD)

#### Scenario: Devon zaps llama-cli successfully

Given Devon has selected "llama-cli" with 6 models (4 shared, 2 unique) totaling 21.4 GB
When Devon presses `z`
And types "llama-cli"
And presses Enter
Then the 2 unique model files are deleted from disk
And the 4 shared model registrations are removed from llama-cli
And the 4 shared models remain in their other tools
And modeltap reports "Reclaimed 14.6 GB"

#### Scenario: Wrong typed name cancels zap

Given Devon has opened the zap dialog for "llama-cli"
When Devon types "llamacli" and presses Enter
Then no models are deleted
And the dialog closes returning to the main view

#### Scenario: Zap on empty tool shows benign message

Given Devon has selected a tool with 0 models
When Devon presses `z`
Then a dialog reads "<tool name> has 0 models. Nothing to zap."
And only Esc is offered

#### Scenario: Esc cancels zap at any point

Given Devon has opened the zap dialog
When Devon presses Esc
Then the dialog closes with no changes

### Acceptance Criteria

- [ ] Pressing `z` opens the confirmation dialog showing model count, total bytes, unique count, shared count
- [ ] User must type the exact tool name (case-sensitive) — anything else cancels
- [ ] Esc cancels at any point with no destructive action
- [ ] After successful zap, all unique model files are deleted; all shared registrations are removed but other tools' copies remain
- [ ] Post-action message shows bytes reclaimed and bytes retained

### Outcome KPIs

- **Who**: Devon
- **Does what**: Reclaims disk space safely
- **By how much**: Zero accidental data loss across the first 90 days (K5)
- **Measured by**: Issue tracker tag `accidental-loss` count = 0 after 90 days
- **Baseline**: N/A

### Technical Notes

- "Unique to this tool" computation depends on dedup-key strategy (Q6) — must be conservative: if dedup-key is uncertain, treat as unique (preserves data).
- Filesystem deletion is irreversible; no soft-delete trash in v1 (deferred).

### Dependencies

- US-02 (or other discovery), US-03 (left pane selection)

---

## US-05b: Delete a single model from one tool

### Problem

Devon wants to remove a single model from a single tool — for example, an old `llama2:7b` that he no longer uses but doesn't want to wipe his entire Ollama install for. The tool-wide `z` is too coarse for this. Per intake F4 (updated), single-model delete is a first-class operation, not a workaround.

### Who

- Devon Park.

### Solution

From the model detail screen (US-13), pressing `d` opens a single-model delete confirmation. The dialog shows the model id, size, the tool it will be removed from, and whether the same model is also registered with other tools (so Devon knows what is preserved elsewhere). Confirmation rule:

- **Unique to this tool** (not registered with any other supported tool): typed-name confirmation (type the model id exactly), matching the safety bar of US-05.
- **Shared with other tools** (the file content is preserved elsewhere): single-key `[y/n]` confirmation; lower friction is acceptable because no content is lost.

Esc cancels at any point.

### Domain Examples

#### 1: Happy path — Devon deletes Mistral from llama-cli only

Devon opens detail for `mistral:7b-instruct-q4_K_M` (registered in Ollama AND llama-cli). He presses `d` from the detail screen. The dialog reads "Delete `mistral:7b-instruct-q4_K_M` (4.4 GB) from llama-cli? Same model also registered with: Ollama. Press [y] to confirm, [n] or Esc to cancel." Devon presses `y`. The llama-cli copy is deleted. The Ollama copy remains. Reclaim: 4.4 GB.

#### 2: Edge — Unique model requires typed confirmation

Devon opens detail for `oddball-model-only-in-llama-cli`. He presses `d`. The dialog reads "DELETE `oddball-model-only-in-llama-cli` (3.2 GB) from llama-cli. This is the ONLY copy — the file will be permanently removed. Type the model id to confirm." Devon types `oddball-model-only-in-llama-cli` and presses Enter. The file is deleted. Reclaim: 3.2 GB.

#### 3: Error — Deletion fails (running tool)

Devon presses `d` for an Ollama model while `ollama serve` is running with the file open. Per intake Q5, modeltap surfaces "Ollama is running and has this file open. Close Ollama and retry." with `[r] retry` and `[Esc] cancel`. No partial mutation.

### UAT Scenarios (BDD)

#### Scenario: Shared single-model delete uses [y/n] confirmation

Given Mistral-7B is registered with both Ollama and llama-cli
And Devon is on the Mistral detail screen viewing the llama-cli registration
When Devon presses `d`
And presses `y`
Then the llama-cli copy of Mistral is deleted
And the Ollama copy is unaffected
And modeltap reports "Reclaimed 4.4 GB"

#### Scenario: Unique single-model delete requires typed confirmation

Given a model is registered with only one tool (no other supported tool has it)
When Devon presses `d` from the detail screen
Then the dialog requires the user to type the model id exactly
And only the typed-id-then-Enter path performs the deletion

#### Scenario: Esc cancels at any point

Given Devon has opened the single-model delete dialog
When Devon presses Esc
Then no file is deleted
And the detail screen returns

#### Scenario: Running tool surfaces detect-and-retry prompt

Given the tool whose copy would be deleted is currently running with the file open
When Devon presses `d` and confirms
Then modeltap surfaces "Close <tool> and retry" with [r] retry / [Esc] cancel
And no partial mutation occurs

### Acceptance Criteria

- [ ] `[d]` shortcut on detail screen opens the single-model delete dialog
- [ ] Shared model uses [y/n] confirmation; unique model requires typed-id confirmation
- [ ] After deletion, only the targeted tool's registration/file is removed
- [ ] Other tools' copies (if any) remain unaffected and still openable
- [ ] If the targeted tool is running and holds the file open, surface "close tool and retry" per intake Q5; do NOT mutate
- [ ] Reclaim total = size of the deleted file (only one inode removed)
- [ ] Bottom bar shows `[d] delete-from-one` only when applicable (detail screen)

### Outcome KPIs

- Drives K1 (bytes reclaimed per session) and K5 (no accidental loss)
- **Measured by:** per-action log line `delete_one tool=<t> id=<m> bytes=<n>`

### Technical Notes

- Per ADR-009, `Tool::delete_one(model)` is a first-class trait method (distinct from `delete_all`). DELIVER must NOT model this as a special case of zap.
- Conservative-deletion rule (per ADR-002): if the dedup key is uncertain, treat as unique (preserves the data).
- Cross-platform: `std::fs::remove_file` works on macOS and Linux identically.

### Dependencies

- US-13 (detail screen — entry point), US-09 (compatibility computation determines unique vs shared), ADR-009 (`delete_one` trait method)

---

## US-06: Show last action and reclaimed bytes

### Problem

After zap or unify, Devon needs immediate proof that the action worked and how much disk was reclaimed. Without it, he goes to a separate terminal and runs `du -sh` — defeating the tool.

### Who

- Devon Park.

### Solution

Right pane displays a header line "Last action: <action> <target> (<status>)" and a body line "Reclaimed: <N> GB (<M> GB retained — also linked from other tools)" until the next user navigation.

### Domain Examples

#### 1: Happy path — Post-zap message

After zapping llama-cli (reclaiming 14.6 GB, retaining 6.8 GB shared), the right pane shows "Last action: zap llama-cli (success)" and "Reclaimed: 14.6 GB (6.8 GB retained — also linked from other tools)".

#### 2: Edge — Post-unify message

After unifying Mistral-7B (reclaiming 8.8 GB), the right pane shows "Last action: unify mistral:7b (success)" and "Reclaimed: 8.8 GB (1 inode, 3 hardlinks)".

#### 3: Error — Action failed partially

After unify on a model where one target failed (cross-fs), the right pane shows "Last action: unify mistral:7b (partial: 2 of 3 targets linked)" and the failure detail link.

### UAT Scenarios (BDD)

#### Scenario: Successful zap shows reclaimed and retained bytes

Given Devon has just zapped llama-cli reclaiming 14.6 GB and retaining 6.8 GB
When the action completes
Then the right pane shows "Last action: zap llama-cli (success)"
And the right pane shows "Reclaimed: 14.6 GB (6.8 GB retained — also linked from other tools)"
And the summary bar shows updated total disk usage

#### Scenario: Successful unify shows hardlink count

Given Devon has just unified Mistral-7B with 3 hardlinks created
When the action completes
Then the right pane shows "Last action: unify mistral:7b (success)"
And the body reads "Reclaimed: 8.8 GB (1 inode, 3 hardlinks)"

#### Scenario: Partial unify shows partial-success message

Given Devon ran unify and 2 of 3 targets succeeded (1 cross-filesystem failure)
When the action completes
Then the right pane shows "Last action: unify mistral:7b (partial: 2 of 3 targets linked)"
And the failed target's path and reason are shown below

### Acceptance Criteria

- [ ] After every zap or unify, the right pane shows the last action and outcome
- [ ] Successful actions show bytes reclaimed
- [ ] Partial successes show how many targets succeeded and which failed and why
- [ ] Summary bar's total disk usage refreshes within 500ms of action completion

### Outcome KPIs

- **Who**: Devon
- **Does what**: Sees confirmation of disk space reclaimed
- **By how much**: 100% of zap/unify actions show the post-action message with byte counts
- **Measured by**: UAT
- **Baseline**: N/A

### Technical Notes

- Refresh of summary bar requires re-running discovery for the affected tool; should be cached so it's fast.

### Dependencies

- US-05 (zap action) and US-10 (unify action)

---

## US-07: Discover llama-cli models

### Problem

Devon stores GGUF files for llama.cpp in a non-standard directory (`~/llms/`). modeltap needs to enumerate them with their sizes.

### Who

- Devon Park, llama-cli user.

### Solution

The llama-cli plugin scans configured search paths (default: `~/llms/`, `~/models/`, plus a configurable list) for `.gguf` files and lists them as models with id = filename, size = file size, format = parsed from GGUF header.

### Domain Examples

#### 1: Happy path

Devon has `~/llms/mistral-7b-q4.gguf` and `~/llms/llama-3-8b.gguf`. modeltap shows llama-cli with 2 models matching those filenames.

#### 2: Edge — Configurable search path

Devon stores files in `/data/models/`. He adds `/data/models` to `~/.modeltap/config.toml` under `[plugins.llama-cli] search_paths = [...]`. modeltap finds them.

#### 3: Error — Corrupt GGUF file

Devon has `~/llms/oops.gguf` that's truncated. modeltap shows it with `[format: corrupt]` indicator and skips it from compatibility computation.

### UAT Scenarios (BDD)

#### Scenario: Default search paths

Given Devon has `~/llms/mistral-7b-q4.gguf` (4.4 GB) and no config override
When Devon launches modeltap and selects llama-cli
Then the model is listed with size 4.4 GB

#### Scenario: Configured additional search path

Given Devon has set `[plugins.llama-cli] search_paths = ["/data/models"]` in config
And `/data/models/extra.gguf` exists
When Devon launches modeltap
Then the file appears in the llama-cli model list

#### Scenario: Corrupt GGUF flagged but does not crash

Given `~/llms/corrupt.gguf` is truncated
When Devon launches modeltap
Then llama-cli shows the model with "[format: corrupt]"
And the discovery does not crash

### Acceptance Criteria

- [ ] Default search paths `~/llms/`, `~/models/` are scanned recursively
- [ ] Additional paths from `~/.modeltap/config.toml` are honored
- [ ] Each `.gguf` file is parsed for header metadata (architecture, quantization)
- [ ] Corrupt or unreadable files are listed with `[format: corrupt]` rather than skipped silently
- [ ] Cross-platform: works on macOS and Linux with the same default paths

### Outcome KPIs

- Drives K1 (more discoverable models = more reclaim opportunity) and K2 (more inventory coverage)

### Technical Notes

- GGUF header format documented at <https://github.com/ggerganov/ggml/blob/master/docs/gguf.md>
- Parse just enough to identify quantization + architecture; do not load full tensor data.

### Dependencies

- US-02 (plugin shape proven first)

---

## US-08: Bottom bar with shortcuts always visible

### Problem

Devon forgets shortcuts. He needs a single line at the bottom that shows what keys do what — and grays out shortcuts that aren't applicable to the current focus.

### Who

- Devon Park.

### Solution

A single-line bottom bar that lists the active shortcuts for the current screen. Shortcuts unavailable in the current context are dimmed but still shown. Pressing `?` opens a help overlay with all shortcuts.

### Domain Examples

#### 1: Happy path

When the left pane is focused on Ollama, the bar shows `[<-/->] tools  [up/down] models  [u] unify  [z] zap tool  [?] help [q] quit` with `[u]` dimmed (no model selected for unify).

#### 2: Edge — Detail screen has different shortcuts

When the model detail screen is open, the bar shows `[Esc] back  [u] unify  [d] delete-from-one  [?] help` (and the main shortcuts are not shown).

#### 3: Edge — Help overlay

When Devon presses `?`, an overlay appears listing all shortcuts grouped by context. Pressing `?` or Esc closes it.

### UAT Scenarios (BDD)

#### Scenario: Unavailable shortcuts are dimmed

Given Devon is in the main view with no model selected
When the bottom bar renders
Then `[u] unify` is shown but dimmed
And `[z] zap tool` is shown brightly because a tool is always selected

#### Scenario: Detail screen shortcuts replace main shortcuts

Given Devon has opened the model detail screen
When the screen renders
Then the bottom bar shows `[Esc] back   [u] unify   [d] delete-from-one   [?] help`
And does not show the main-view shortcuts

#### Scenario: Help overlay shows all shortcuts

Given Devon is in the main view
When Devon presses `?`
Then an overlay opens showing all shortcuts grouped by context (main, detail, dialogs)
And pressing `?` or Esc closes the overlay

### Acceptance Criteria

- [ ] Bottom bar occupies exactly one row
- [ ] Shortcuts unavailable in the current context are visibly dimmed but not removed
- [ ] Detail screens and dialogs replace the main bar with their own shortcut list
- [ ] `?` opens a comprehensive help overlay; `?` or Esc closes it
- [ ] Shortcuts shown in the bar match the actual key handler dispatch table (single source of truth)

### Outcome KPIs

- Indirect (UX polish; supports K3 — engagement/retention).

### Technical Notes

- Single `SHORTCUT_TABLE` const drives both the bar render and the dispatch — avoids drift.

### Dependencies

- US-03

---

## US-09: Compatibility indicator (o/*/!) on each row

### Problem

Devon needs to know at-a-glance which models he can deduplicate (cross-tool linkable) and which are stuck in one tool by format.

### Who

- Devon Park.

### Solution

A compatibility-computation engine: per model, given its format and the registered plugins' accepted-formats sets, compute `compatible_tools[]` and decide the indicator: `*` if the model is currently in 2+ tools, `o` if only in 1 tool but >=1 other tool accepts the format, `!` if no other tool accepts the format.

### Domain Examples

#### 1: Happy path — Mistral GGUF

Mistral-7B-v0.3 q4_K_M (GGUF) is in Ollama, llama-cli, HF. Indicator: `*`. compatible_tools = {Ollama, llama-cli, HF, LM Studio}.

#### 2: Edge — Llama-3 GGUF in HF only

Llama-3-8B (GGUF) is only in HF. Other tools accept GGUF. Indicator: `o`. compatible_tools = {Ollama, llama-cli, HF, LM Studio}.

#### 3: Error — AWQ in HF only

TheBloke/something-AWQ is in HF only. No other tool accepts AWQ. Indicator: `!`. compatible_tools = {HF}.

### UAT Scenarios (BDD)

#### Scenario: Multi-tool model gets *

Given a model is currently registered with 2+ tools per dedup-key matching
When the indicator is computed
Then it is `*`

#### Scenario: Format-compatible single-tool model gets o

Given a model is in 1 tool only
And at least one other supported tool accepts its format
When the indicator is computed
Then it is `o`

#### Scenario: Format-locked model gets !

Given a model is in 1 tool only
And no other supported tool accepts its format
When the indicator is computed
Then it is `!`

### Acceptance Criteria

- [ ] Indicator computation runs per model during inventory build
- [ ] Computation uses the plugin's declared accepted-formats list (capability metadata)
- [ ] The set of indicators is exactly {`o`, `*`, `!`, `?`} — no others
- [ ] Indicator is recomputed after any zap or unify (state may have changed)

### Outcome KPIs

- **Who**: Devon
- **Does what**: Identifies deduplicable models at a glance
- **By how much**: K2 — at least 30% of models marked `*` or `o` for users with 2+ tools
- **Measured by**: Inventory log on launch
- **Baseline**: Unknown; first 30 days post-launch establish baseline

### Technical Notes

- Format compatibility table is part of plugin capability metadata — see US-18.
- "Only-one-tool" (red icon) is **format-based** — confirmed in journey; resolves intake Q3.

### Dependencies

- US-04 (the row that displays the indicator), discovery stories (US-02, US-07, US-12, US-15)

---

## US-10: Unify a model across tools using hardlinks

### Problem

Devon has Mistral-7B q4_K_M three times on disk (Ollama blob, llama-cli loose file, HF cache) — 13.2 GB for one model. He wants one canonical copy and the three tool paths pointing at it via hardlinks, reclaiming 8.8 GB.

### Who

- Devon Park.

### Solution

Pressing `u` on a `*`-marked model opens the unify dialog showing **which existing tool-owned copy will be chosen as canonical** (typically the largest, or the content-addressed one if available — e.g., the Ollama blob), the hardlink targets per tool, and the disk reclaim. On Enter, modeltap replaces the other copies with hardlinks pointing at the chosen canonical and updates each tool's registration as needed. **modeltap does NOT maintain its own central store** — per intake Q1, tool directories are the source of truth.

### Domain Examples

#### 1: Happy path — Devon unifies Mistral-7B

Devon presses `u` on Mistral-7B (3 copies, all SHA256-equal). The dialog shows the chosen canonical `/Users/devon/.ollama/models/blobs/sha256-8f3e...c102` (the existing Ollama blob, picked because Ollama already content-addresses) and 2 hardlink targets to be created (the llama-cli `.gguf` and the HF cache blob). He presses Enter. modeltap replaces the llama-cli and HF copies with hardlinks pointing at the Ollama blob. Reclaims 8.8 GB. No new files appear under `~/.modeltap/`.

#### 2: Edge — Already-unified model

Devon presses `u` on a model that's already unified (all paths point to the same inode). The dialog reads "Already unified — all 3 registrations point to the same file." with [Esc] to close.

#### 3: Error — Cross-filesystem target

Devon's Ollama dir is on `/`, his llama-cli dir is on `/data` (different filesystems). On unify, hardlinks to `/data` fail. modeltap shows the partial-success message and offers a copy-fallback per US-19.

### UAT Scenarios (BDD)

#### Scenario: Unify creates hardlinks and reclaims disk

Given Mistral-7B-v0.3 has 3 separate copies of 4.4 GB across 3 tools on the same filesystem
When Devon presses `u` and Enter
Then one existing tool-owned copy is chosen as canonical (per the canonical-selection rule — largest copy, or the content-addressed one if available)
And the other 2 tools' paths are replaced with hardlinks to the canonical (verified by inode equality)
And no file is created under `~/.modeltap/`
And modeltap reports "Reclaimed 8.8 GB"

#### Scenario: Already-unified model shows benign message

Given a model's 3 registered paths all stat to the same inode
When Devon presses `u`
Then the dialog reads "Already unified — all 3 registrations point to the same file."
And no action is taken

#### Scenario: Each tool's registration updates correctly

Given Mistral was registered with Ollama as a manifest pointing to a blob
When unify replaces the blob with a hardlink to canonical
Then `ollama list` (the underlying Ollama CLI) still shows the model
And running it via Ollama still works

### Acceptance Criteria

- [ ] Unify dialog shows canonical path, hardlink target list, and disk reclaim before any action
- [ ] After Enter, every target stats to the same inode as canonical
- [ ] Each plugin's `link()` method handles its tool's specific registration (Ollama blob layout, HF symlink, etc.) — see Q2 for DESIGN
- [ ] If any target fails (cross-fs, permissions), the partial-success path runs and per-target error is shown
- [ ] Already-unified models are detected and the dialog is informational only

### Outcome KPIs

- **Who**: Devon
- **Does what**: Reclaims disk via cross-tool unification
- **By how much**: K1 — median 5 GB reclaimed per unify session
- **Measured by**: Per-action log
- **Baseline**: 0

### Technical Notes

- Q1 (no central store; pick existing canonical), Q5 (detect-and-prompt-then-retry), Q6 (SHA256 primary) all RESOLVED via intake. Q2 (per-tool linking) RESOLVED in DESIGN ADR-004 for Ollama + llama-cli; HF + LM Studio need a verification spike in DELIVER (< 1 day each).
- macOS APFS and Linux ext4/btrfs all support hardlinks. AFP/SMB network mounts may not — that's the cross-fs case.

### Dependencies

- US-04, US-09 (compatibility indicator), US-13 (detail screen often is the entry point)

---

## US-11: Updated totals after action

### Problem

After zap or unify, the summary bar's totals must reflect the new state, otherwise Devon can't trust the display.

### Who

- Devon Park.

### Solution

After any mutating action, modeltap rebuilds the affected tool's inventory (or invalidates and recomputes incrementally) and refreshes the summary bar within 500ms.

### Domain Examples

#### 1: Happy path — Zap updates totals

Pre-zap: 138.4 GB total, 58 models. Post-zap of llama-cli (-14.6 GB, -6 models): summary bar shows 123.8 GB, 52 models.

#### 2: Edge — Unify keeps model counts but reduces disk

Pre-unify: 138.4 GB, 58 models. Post-unify of Mistral (-8.8 GB on disk, model count unchanged because still registered everywhere): summary bar shows 129.6 GB, 58 models, dedup-able decreases by 8.8 GB.

#### 3: Error — Refresh fails

If discovery fails after action, summary bar shows stale numbers with a `(refresh failed)` indicator and a `[r] retry` shortcut.

### UAT Scenarios (BDD)

#### Scenario: Totals update after zap

Given Devon's pre-zap total was 138.4 GB
When Devon zaps llama-cli reclaiming 14.6 GB
Then within 500ms the summary bar shows the new total (123.8 GB, within rounding)

#### Scenario: Totals update after unify (disk down, model count steady)

Given Devon's pre-unify total was 138.4 GB and 58 models
When Devon unifies Mistral reclaiming 8.8 GB
Then the summary bar shows total 129.6 GB
And model count unchanged at 58
And dedup-able decreases by 8.8 GB

#### Scenario: Refresh failure shows degraded indicator

Given the post-action discovery rebuild fails
When the summary bar tries to refresh
Then it shows the previous values with "(refresh failed)" indicator
And `[r] retry` is offered

### Acceptance Criteria

- [ ] After any zap/unify, summary bar refreshes within 500ms
- [ ] Refresh failures degrade gracefully with `(refresh failed)` indicator and retry shortcut
- [ ] New total = old total - bytes_reclaimed (within rounding)

### Outcome KPIs

- Indirect — supports K1 (visibility of reclaim drives further use)

### Technical Notes

- Incremental update preferred over full rediscovery for speed.

### Dependencies

- US-06 (post-action message), US-05 and US-10 (the actions themselves)

---

## US-12: Discover Hugging Face cache models

### Problem

Devon's HF cache (`~/.cache/huggingface/hub/`) is a complex symlink farm with `models--<org>--<repo>/snapshots/<rev>/<file>`. modeltap needs to enumerate models there.

### Who

- Devon Park, has used `huggingface-cli` or the `hf` CLI to download models.

### Solution

The HF plugin walks `~/.cache/huggingface/hub/`, identifies model directories, resolves the snapshot symlinks to the actual blob files, and produces Model entries with id = `<org>/<repo>`, format inferred from filename (gguf, safetensors, bin, awq, etc.).

### Domain Examples

#### 1: Happy path — Standard HF cache

Devon has 31 model directories under `~/.cache/huggingface/hub/`. modeltap lists them all with sizes summing to 78.2 GB.

#### 2: Edge — `HF_HOME` override

Devon has set `HF_HOME=/data/hf-cache`. modeltap reads `HF_HOME` and scans there instead of default.

#### 3: Error — Broken symlink

Devon's `~/.cache/huggingface/hub/models--foo--bar/snapshots/abc/file.gguf` is a broken symlink (manual cleanup). modeltap shows the model with `[broken: missing blob]` and excludes its size from totals.

### UAT Scenarios (BDD)

#### Scenario: Default HF cache is discovered

Given Devon has 31 models in `~/.cache/huggingface/hub/`
When Devon launches modeltap and selects Hugging Face
Then 31 models are listed with their org/repo ids and sizes

#### Scenario: HF_HOME override is honored

Given Devon's `HF_HOME=/data/hf-cache`
And `/data/hf-cache/hub/` contains 5 model directories
When Devon launches modeltap
Then 5 models are listed under Hugging Face

#### Scenario: Broken symlinks are flagged

Given a model directory has a broken snapshot symlink
When discovery runs
Then the model is listed with `[broken: missing blob]`
And its size does not contribute to the Hugging Face disk usage

### Acceptance Criteria

- [ ] `HF_HOME` env var (or default `~/.cache/huggingface/`) is read
- [ ] All `models--<org>--<repo>` directories are enumerated; snapshot symlinks resolved to blobs
- [ ] Model id = `<org>/<repo>` (path-style canonical form)
- [ ] Format inferred from filename suffix
- [ ] Broken symlinks are reported, not silent

### Outcome KPIs

- Drives K1, K2

### Technical Notes

- HF cache structure documented at <https://huggingface.co/docs/huggingface_hub/guides/manage-cache>
- Cross-platform: `~/.cache/huggingface/` on Linux, `~/Library/Caches/huggingface/` is NOT used by HF (HF uses XDG-style on macOS too); confirm with HF docs.

### Dependencies

- US-02 (plugin shape)

---

## US-13: Model detail screen

### Problem

Devon needs a per-model deep-dive: all paths, dedup key, exact reclaim estimate, and an entry point to unify or delete-from-one.

### Who

- Devon Park.

### Solution

Pressing Enter on a model row opens a detail screen showing: model id, format, size, dedup key, list of registered tools with paths, status (unified / not unified / partially unified), and reclaim estimate.

### Domain Examples

#### 1: Happy path — Mistral detail

Detail for Mistral-7B q4_K_M shows 3 paths, dedup key sha256:8f3e..., NOT UNIFIED, 8.8 GB reclaimable.

#### 2: Edge — Single-tool model

Detail for an HF-only AWQ model shows 1 path, dedup key, "single tool — unify not applicable", reclaim 0.

#### 3: Error — Stale entry

Detail for a model whose blob is missing on disk shows "blob missing" warning and offers "remove from index".

### UAT Scenarios (BDD)

#### Scenario: Detail screen shows duplicate paths and reclaim estimate

Given Mistral-7B q4_K_M has 3 separate file copies of 4.4 GB across 3 tools
When Devon selects Mistral and presses Enter
Then the detail screen lists all 3 paths
And shows status "NOT UNIFIED — 3 separate copies (13.2 GB total)"
And shows "If unified: would reclaim 8.8 GB"

#### Scenario: Single-tool model detail offers no unify

Given an AWQ model only in HF
When Devon opens its detail
Then the screen shows 1 path
And the [u] shortcut is dimmed with note "single tool — unify not applicable"

#### Scenario: Already-unified model detail

Given a model whose 3 registered paths all stat to the same inode
When Devon opens its detail
Then the status reads "UNIFIED — 1 inode, 3 hardlinks"
And reclaim estimate reads "Reclaimed: 8.8 GB"

### Acceptance Criteria

- [ ] Detail screen shows id, format, size, dedup key, and per-tool paths
- [ ] Status is one of: UNIFIED, NOT UNIFIED, PARTIALLY UNIFIED, SINGLE TOOL
- [ ] Reclaim estimate computed correctly per status
- [ ] Esc returns to main view

### Outcome KPIs

- Drives K2 (visibility of duplication)

### Technical Notes

- This is the critical "aha — 8.8 GB!" moment. Layout must make the number unmissable.

### Dependencies

- US-04, US-09

---

## US-14: Dry-run preview before unify

### Problem

Devon wants to see exactly what unify will do before he authorizes it — what file becomes canonical, what hardlinks get created, what disk gets reclaimed.

### Who

- Devon Park.

### Solution

The unify dialog includes a `[n] dry-run only` shortcut that prints the same plan as Enter would execute, but performs no filesystem mutation. After dry-run, Devon can press Enter to proceed, or Esc to cancel.

### Domain Examples

#### 1: Happy path — Devon dry-runs first

Devon presses `u`, the dialog shows the plan, he presses `n`. modeltap prints "(dry-run) Would create canonical at ... Would create hardlinks at ... Would reclaim 8.8 GB. No filesystem changes made." Devon then presses Enter to proceed.

#### 2: Edge — Dry-run after dry-run

Devon presses `n` twice. The plan is shown twice; nothing changes.

#### 3: Error — Dry-run reveals a problem

Dry-run reveals one target is on a different filesystem. Devon cancels with Esc.

### UAT Scenarios (BDD)

#### Scenario: Dry-run shows the plan without touching disk

Given Devon has opened the unify dialog
When Devon presses `n`
Then the dialog shows "(dry-run) Would create canonical ... Would create hardlinks ... Reclaim: 8.8 GB"
And no filesystem changes occur
And Devon can still proceed or cancel

#### Scenario: Dry-run reveals cross-filesystem issue

Given the canonical store and a tool's path are on different filesystems
When Devon presses `n`
Then the dry-run output reads "WARNING: target X on different filesystem — would fall back to copy"
And Devon can still cancel safely

### Acceptance Criteria

- [ ] `n` produces the same plan as Enter would, with no filesystem mutation
- [ ] Dry-run output is clearly labeled `(dry-run)` to distinguish from real action
- [ ] Cross-filesystem and permission issues are surfaced during dry-run

### Outcome KPIs

- Drives K5 (safety guardrail)

### Dependencies

- US-10

---

## US-15: Discover LM Studio models

### Problem

Devon uses LM Studio occasionally; its models live in `~/.cache/lm-studio/models/<org>/<repo>/<file>` (macOS/Linux). modeltap needs to enumerate them.

### Who

- Devon Park.

### Solution

The LM Studio plugin scans `~/.cache/lm-studio/models/` (or the configured path) and produces Model entries.

### Domain Examples

#### 1: Happy path — Devon's 9 LM Studio models

Devon has 9 models totaling 38.7 GB. modeltap lists them.

#### 2: Edge — LM Studio not installed

`~/.cache/lm-studio/` does not exist. LM Studio shows in left pane with "(not installed)".

#### 3: Edge — Older LM Studio path

Some versions used `~/.lmstudio/models/`. Plugin checks both default paths.

### UAT Scenarios (BDD)

#### Scenario: LM Studio cache is discovered

Given Devon has 9 models in `~/.cache/lm-studio/models/`
When Devon launches modeltap and selects LM Studio
Then 9 models are listed with their ids and sizes

#### Scenario: Older path is honored

Given Devon's models are in `~/.lmstudio/models/` (older convention)
When Devon launches modeltap
Then the models are listed under LM Studio

#### Scenario: Not installed is benign

Given neither LM Studio path exists
When Devon launches modeltap
Then LM Studio shows in the left pane with "(not installed)"

### Acceptance Criteria

- [ ] Default paths `~/.cache/lm-studio/models/` and `~/.lmstudio/models/` are both checked
- [ ] Configured override via `~/.modeltap/config.toml` is honored
- [ ] Each file is parsed for format from filename suffix
- [ ] "Not installed" is distinguished from "error"

### Outcome KPIs

- Drives K1, K2

### Technical Notes

- LM Studio's path conventions are not fully standardized — DESIGN may need a brief spike per intake Q2.

### Dependencies

- US-02

---

## US-16: Format-locked indicator (red `!`) for one-tool-only models

### Problem

Devon should never waste effort trying to unify a model that's stuck in one tool by format. The `!` indicator (and only the `!`) signals "format-locked, do not bother."

### Who

- Devon Park.

### Solution

When `compute_compatibility(model.format, plugins)` returns a single-element compatible_tools list AND that tool is the model's current home, the indicator is `!` rendered in red. Detail screen for such a model dims the [u] shortcut.

### Domain Examples

#### 1: Happy path — AWQ in HF

TheBloke/something-AWQ in HF only. Indicator: `!`. Detail screen says "single tool — unify not applicable."

#### 2: Edge — Ollama-blob format

A model in Ollama's blob format (some models have non-standard manifest entries). Indicator: `!`. Detail screen says "Ollama-blob format — only Ollama can consume."

#### 3: Error — Capability metadata missing

A plugin's `accepted_formats()` returns empty (bug). The compatibility engine treats this as "unknown" and the indicator is `?`, not `!`.

### UAT Scenarios (BDD)

#### Scenario: AWQ model gets red !

Given TheBloke/something-AWQ is in HF only
And Ollama, llama-cli, LM Studio do not list AWQ in their accepted_formats
When the indicator is computed
Then the indicator is `!`
And the row's color is red

#### Scenario: Format-locked model in detail screen

Given a `!`-marked model
When Devon opens its detail screen
Then the [u] shortcut is dimmed
And the screen shows "single tool — unify not applicable"

#### Scenario: Missing capability metadata is `?` not `!`

Given a plugin's accepted_formats() returns empty
When indicators are computed
Then models in that tool get `?` not `!`
And a developer-mode warning is logged

### Acceptance Criteria

- [ ] `!` indicator rendered in red (paired with the symbol — never color-only)
- [ ] [u] shortcut is dimmed/disabled on `!` models in detail screen
- [ ] Empty/missing capability metadata produces `?` not `!`
- [ ] WCAG contrast: red on default terminal background ≥ 4.5:1 for normal text

### Outcome KPIs

- Drives K2 (visibility of which models are not deduplicable)

### Technical Notes

- "Only-one-tool" is **format-based** per intake Q3 resolution.
- `NO_COLOR` env var: `!` still renders as the symbol; only color is dropped.

### Dependencies

- US-09 (the engine), US-04 (the row format)

---

## US-17: Detect running tools and warn before unify/zap

### Problem

If Ollama is running and has a model file open, swapping that file out (unify) or deleting it (zap) could crash Ollama or corrupt its session. Devon needs a warning.

### Who

- Devon Park.

### Solution

Before any mutating action, modeltap runs a `detect_running_tools()` check (lsof on macOS/Linux) and lists any tool process that has files in scope. The warning is **soft**: Devon can still proceed.

### Domain Examples

#### 1: Happy path — No running tools

Devon presses `u`. detect_running_tools returns []. The dialog proceeds normally without a warning section.

#### 2: Edge — Ollama running

Ollama serve is running with PID 4421 and has the file open. Dialog shows "Running tools detected: ollama (PID 4421)". Devon can proceed.

#### 3: Error — lsof unavailable

On a system without lsof, the dialog shows "running-tool detection unavailable on this system". User proceeds at own risk.

### UAT Scenarios (BDD)

#### Scenario: Running tool warning shown

Given the Ollama process is running and has the model file open
When Devon presses `u` on a model registered in Ollama
Then the dialog shows "Running tools detected: ollama (PID ...)"
And Devon can still proceed

#### Scenario: No running tools, no warning

Given no supported tool is currently running
When Devon presses `u`
Then the dialog has no running-tool warning section

#### Scenario: lsof unavailable

Given the system has no `lsof` or equivalent tool
When Devon opens the unify dialog
Then the dialog shows "Running-tool detection unavailable on this system"

### Acceptance Criteria

- [ ] Before unify or zap, detect_running_tools runs and results are shown in dialog
- [ ] Warning is soft: Devon can proceed despite warning
- [ ] If lsof / detection is unavailable, message says so explicitly
- [ ] Detection completes within 500ms (does not delay the dialog noticeably)

### Outcome KPIs

- Supports K5 (safety)

### Technical Notes

- macOS and Linux: lsof typically present. On stripped containers it may not be. Failure to detect is non-fatal.
- Per intake Q5 resolution: soft-warning behavior is the v1 stance; DESIGN may revise.

### Dependencies

- US-05, US-10

---

## US-18: Plugin trait — adding a 5th tool requires no core changes

### Problem

Riley Chen, an open-source contributor, wants to add support for Jan (a fifth local-AI tool) without forking modeltap or changing core code. Without a clean plugin contract, every new tool means a core PR — slowing adoption and centralizing maintenance.

### Who

- Riley Chen, open-source contributor with intermediate Rust experience, wants to add Atomic Chat support and submit a PR.

### Solution

A documented `Tool` trait in modeltap-core exposing: `name()`, `discover()`, `list_models()`, `link()`, `delete()`, `accepted_formats()`. New plugins live in `plugins/<name>/` and self-register via a registration macro or inventory pattern. The trait is stable across minor versions; CI checks no changes outside `plugins/<new>/` for new-tool PRs.

### Domain Examples

#### 1: Happy path — Riley adds Atomic Chat

Riley creates `plugins/atomic-chat/` implementing the Tool trait. He registers it (via the inventory crate per ADR-001 — zero changes outside `plugins/atomic-chat/`). He opens a PR. The PR diff touches only `plugins/atomic-chat/` and the workspace `Cargo.toml`. CI confirms `modeltap-core` is unchanged. modeltap builds; on launch, Atomic Chat appears in the left pane.

#### 2: Edge — A plugin that needs a SQLite read

Some hypothetical tool stores its catalog in SQLite. Riley's plugin uses `rusqlite` as a private dependency declared in `plugins/atomic-chat/Cargo.toml`. The plugin compiles independently; modeltap-core does not depend on rusqlite.

#### 3: Error — A plugin returns malformed data

A plugin's `list_models()` panics. modeltap-core catches the panic, marks the tool as "error" in the left pane, and continues running for other tools.

### UAT Scenarios (BDD)

#### Scenario: A new plugin appears in the left pane on launch

Given a new plugin "atomic-chat" implementing the Tool trait is registered
When modeltap launches
Then "Atomic Chat" appears in the left pane
And modeltap-core source files are unchanged from the previous version

#### Scenario: A plugin panic does not crash modeltap

Given a buggy plugin panics during list_models
When modeltap launches
Then the buggy tool shows "(error)" in the left pane
And other tools render normally
And the panic is logged to ~/.modeltap/diagnostics.log

#### Scenario: Plugin trait is stable across minor versions

Given a plugin compiles against modeltap-core 1.x
When modeltap-core upgrades to 1.(x+1)
Then the plugin compiles unchanged

### Acceptance Criteria

- [ ] Tool trait defined in modeltap-core with: name, discover, list_models, link, delete, accepted_formats
- [ ] At least 4 plugins (Ollama, llama-cli, HF, LM Studio) exist as separate modules — proves the pattern works
- [ ] Adding a 5th plugin requires zero changes to modeltap-core source files
- [ ] Plugin panics are caught at the plugin boundary; one bad plugin does not crash the TUI
- [ ] Trait is documented in `CONTRIBUTING.md` with a worked example

### Outcome KPIs

- **Who**: Open-source contributor (Riley)
- **Does what**: Adds a new tool via a plugin module
- **By how much**: K4 — at least 1 community-contributed plugin merged within 6 months
- **Measured by**: GitHub PR count
- **Baseline**: 0 (greenfield)

### Technical Notes

- Plugin registration mechanism: build-time inventory crate or static slice. Choice belongs to DESIGN.
- The trait must be small enough that "implementing all 6 methods" fits in one PR review.
- Implications for testing: each plugin needs its own test fixtures (sample directories).

### Dependencies

- All discovery stories (US-02, US-07, US-12, US-15) — these prove the trait by repetition. US-18 is the **explicit** version that documents and tests the contract.

---

## US-19: Hardlink fallback when cross-filesystem

### Problem

Devon's `/data/models/` is on a different physical filesystem than `~/.modeltap/store/`. Hardlinks across filesystems are impossible. modeltap must handle this gracefully.

### Who

- Devon Park.

### Solution

When a hardlink target's filesystem differs from canonical, modeltap reports the issue per-target during dry-run AND during real run. Per-target options offered: skip (leave that copy alone), copy (waste disk but unify the others). User chooses per-target or "skip all cross-fs targets."

### Domain Examples

#### 1: Happy path — All same filesystem

All paths are on `/`. Unify proceeds with hardlinks.

#### 2: Edge — One target on different fs

Canonical at `~/.modeltap/store/` on `/`. Two targets on `/`, one on `/data` (different fs). Dialog says: "1 of 3 targets on different filesystem. Options: [s] skip cross-fs targets, [c] copy to cross-fs targets (no disk reclaim for those), [x] cancel."

#### 3: Error — All targets cross-fs

Canonical store and all targets are on different filesystems from each other (rare but possible). Dialog says "all targets on different filesystems — unify cannot proceed."

### UAT Scenarios (BDD)

#### Scenario: All-same-filesystem unify proceeds normally

Given all paths and the canonical store are on the same filesystem
When Devon proceeds with unify
Then hardlinks are created for all targets
And no fallback prompt appears

#### Scenario: Cross-filesystem target offers fallback

Given 2 of 3 targets are on the same filesystem as canonical, 1 is on a different one
When Devon proceeds with unify
Then the dialog reads "1 of 3 targets on different filesystem"
And options are "[s] skip cross-fs / [c] copy / [x] cancel"
And user choice is honored per-target

#### Scenario: All-cross-fs unify is refused

Given canonical and all targets are on mutually different filesystems
When Devon presses `u`
Then the dialog shows "all targets on different filesystems — unify cannot proceed"
And no action is taken

### Acceptance Criteria

- [ ] Filesystem check (using `stat` device IDs) runs per-target before linking
- [ ] Cross-fs targets surface in dry-run and real-run with explicit options
- [ ] Skip option leaves the target untouched
- [ ] Copy option copies bytes (disk not reclaimed for that target) and reports in summary
- [ ] No partial-state corruption on error

### Outcome KPIs

- Supports K1 (still reclaim where possible) and K5 (safety)

### Technical Notes

- `std::fs::hard_link` returns EXDEV on cross-fs in Rust. Catch and route to fallback.

### Dependencies

- US-10

---

## US-20: Cross-platform path discovery (macOS + Linux)

### Problem

Riley wants to verify that every plugin works on both macOS and Linux from day one. Discovery paths and linking semantics differ subtly; Windows is explicitly out of scope.

### Who

- Riley Chen (contributor verifying portability), Devon Park (user on either OS).

### Solution

Each plugin declares its discovery paths per-platform via a small platform abstraction. CI runs the test suite on both macOS and Linux runners. README documents supported platforms. Windows is not supported in v1; plugin code must compile on Windows for development convenience but the binary is not distributed for Windows.

### Domain Examples

#### 1: Happy path — Devon on Linux

Devon on Ubuntu 22.04 runs modeltap. All four plugins discover their respective paths under `$HOME` correctly.

#### 2: Edge — Devon on macOS

Devon on macOS Sonoma runs modeltap. Same four plugins discover correctly. Hardlinks via APFS work normally.

#### 3: Error — Windows attempt

A user on Windows downloads source and tries to run. The build either succeeds (development friendly) or fails with a clear `cfg!(windows)` panic at startup explaining "Windows not supported in v1."

### UAT Scenarios (BDD)

#### Scenario: Linux discovery paths

Given Devon is on Ubuntu 22.04
When Devon runs modeltap
Then all four plugins find their installations using their default Linux paths
And unify uses Linux hardlink semantics correctly

#### Scenario: macOS discovery paths

Given Devon is on macOS Sonoma
When Devon runs modeltap
Then all four plugins find their installations using their default macOS paths
And unify uses APFS hardlink semantics correctly

#### Scenario: CI runs on both platforms

Given the modeltap test suite
When CI runs
Then it runs on at least one macOS runner and one Linux runner
And both must pass before merge

#### Scenario: Windows is explicitly refused

Given a user attempts to run modeltap on Windows
When the binary starts
Then it panics or exits with "Windows not supported in v1 — see roadmap"

### Acceptance Criteria

- [ ] Each plugin has per-OS path defaults (cfg! gated)
- [ ] CI runs on both macOS and Linux runners; both must pass to merge
- [ ] README states supported platforms explicitly
- [ ] Windows: build may compile but binary refuses to run with a clear message
- [ ] No path is hardcoded to "/" or Unix-only assumptions outside platform abstraction

### Outcome KPIs

- Drives K3 (TUI-reachability across user base) and K4 (contributor-friendliness)

### Technical Notes

- Use `dirs` or `directories` crate for cross-platform home/cache paths.
- HF cache: `~/.cache/huggingface/` on Linux; `~/.cache/huggingface/` ALSO on macOS (HF uses XDG) — confirm.

### Dependencies

- All discovery stories
