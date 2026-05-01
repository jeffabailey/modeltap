<!-- markdownlint-disable MD024 -->

# User Stories: cross-tool-model-unify

10 stories total. 7 in the walking skeleton (US-U1..U7), 3 in the polish release (US-U8..U10).

---

## US-U1: Background SHA256 hashing with progress

### Problem

Devon is a small-team developer who launches modeltap and wants to know which models are duplicated across his Ollama, LM Studio, HF cache, and Atomic Chat installations. With v1, the summary bar shows `Dedup-able: 0 B` because SHA256 hashes are computed only on-demand (lazily) when a Detail screen is opened. The summary bar therefore lies to him on every launch — he never learns there are dedup candidates without manually opening every model's detail.

### Who

- **Devon** | small-team developer with 2+ local AI tools installed | wants to reclaim disk without losing models

### Solution

After first paint completes (rows visible <1 s, K3 budget preserved), modeltap kicks off background SHA256 hashing for every discovered model file. A progress indicator in the status line shows `Hashing N/M...`. As each hash completes, dependent UI (row glyphs, summary bar) updates reactively. Hashes are cached in process memory only (per ADR-002, no persistent index per Q7).

### Domain Examples

#### 1: Happy Path — typical install

Devon has 19 GGUF files across 4 tools, total 47.3 GB, on a warm SSD. He launches modeltap. Within 1 s all 19 rows are visible with `?` glyphs. Status line shows `Hashing 0/19...`. Over the next ~12 seconds, the indicator advances to `Hashing 19/19 (complete)` and rows progressively flip from `?` to `=`, `#`, or `-`.

#### 2: Edge Case — large model, slow disk

Maya (Devon-class, but on an external HDD) has 6 large 70B models totalling 240 GB. Hashing takes ~3 minutes. The status line continues showing `Hashing 4/19...` throughout, so Maya sees the work is in progress and doesn't conclude the tool is broken. She can navigate, view details, and use other commands during this time — hashing does not block the UI.

#### 3: Error Boundary — hash interrupted by quit

Devon launches, sees `Hashing 7/19...`, then presses `q` to quit before hashing completes. The hash worker shuts down cleanly within 200 ms. No stale lockfile or partial state survives (per Q7, no persistent index). On next launch, hashing starts fresh.

### UAT Scenarios (BDD)

#### Scenario: Hashing starts after first paint

```gherkin
Given Devon has 19 model files across 4 tools
When Devon launches modeltap
Then within 1 second all 19 rows are visible with glyph "?"
And the status line shows "Hashing 0/19..."
```

#### Scenario: Status line advances as hashes complete

```gherkin
Given hashing is in progress
When the background worker completes 7 hashes
Then the status line shows "Hashing 7/19..."
And 7 rows have flipped from "?" to one of "-", "=", or "#"
```

#### Scenario: UI remains responsive during hashing

```gherkin
Given hashing is in progress (status: "Hashing 4/19...")
When Devon presses "j" to navigate down
Then the highlighted row changes within 100 ms
And hashing progress is unaffected
```

#### Scenario: Hashing completes within budget on typical install

```gherkin
Given a typical install (~20 GGUF files, ~50 GB total, warm SSD)
When Devon launches modeltap
Then hashing of all files completes within 60 seconds (p95)
```

#### Scenario: Quit during hashing shuts down cleanly

```gherkin
Given hashing is in progress
When Devon presses "q"
Then the application exits within 500 ms
And no partial-state file is written to disk
```

### Acceptance Criteria

- [ ] First paint completes within 1 s with rows visible and `?` glyphs (K3 preserved)
- [ ] Background hash worker starts after first paint, not before
- [ ] Status line shows live `Hashing N/M...` count, updating at least every 250 ms or per-completion
- [ ] UI remains responsive (key handlers respond within 100 ms) during hashing
- [ ] Typical install (~20 files, ~50 GB, warm SSD) completes p95 within 60 s
- [ ] On quit during hashing, exit completes within 500 ms with no persistent state left behind

### Outcome KPIs

- **Who**: Devon-class users with >=2 tools and >=1 cross-tool duplicate
- **Does what**: Sees `Dedup-able > 0` in summary bar (depends on hashes completing for at least one duplicated model)
- **By how much**: 95% of qualifying sessions, within 60 s of launch (KPI-1)
- **Measured by**: `launch.log` `summary_paint` event with `dedup_able_bytes > 0`
- **Baseline**: 0% (v1 hardcoded)

### Technical Notes

- Hash queue lives in `modeltap-core` (DESIGN to confirm exact placement)
- Per ADR-002, hashes cached in process memory only; no persistent index (Q7)
- DESIGN must establish hashing concurrency strategy (parallelism, IO contention) — DISCUSS does not lock this
- Composition root in `modeltap-app` spawns the worker
- Dependencies: `Tool::list_models()` per ADR-001 (existing, frozen)

---

## US-U2: Wire dedup-able bytes from classifier to summary bar

### Problem

Devon launches modeltap and the summary bar shows `Dedup-able: 0 B` even when his disk has 9.4 GB of cross-tool duplicates. The reason: in v1, `crates/modeltap-tui/src/render/summary_bar.rs:36` hardcodes `"Dedup-able: 0 B"`. The dedup classifier in `modeltap-core::logic::dedup` already produces a correct value but it was never wired up. Devon has no way to know there's anything to dedup.

### Who

- **Devon** | small-team developer | wants honest UI — when something is dedup-able, the summary bar should say so

### Solution

The summary bar reads `dedup_able_bytes` from the same `core::logic::dedup` classifier the rows read from. As hashes complete (US-U1), the classifier output changes; the summary bar reflects the new value on next paint. While hashing is in progress, the summary bar shows `Dedup-able: computing...` rather than a misleading number.

### Domain Examples

#### 1: Happy Path — value appears as hashes settle

Devon's install has 3 copies of llama-3.1-8b-Q4_K_M (4.7 GB each, 9.4 GB redundant) plus 2 copies of phi-3-mini (2.3 GB, 2.3 GB redundant — but already hardlinked). On launch, summary bar shows `Dedup-able: computing...`. After all hashes settle, `Dedup-able: 9.4 GB` (only the still-separate copies count; phi-3-mini is `#`, not `=`).

#### 2: Edge Case — no duplicates

Riley has 8 distinct models with no cross-tool overlap. After hashing, summary bar shows `Dedup-able: 0 B` — but truthfully so, because the classifier confirmed it. Status line additionally clarifies `Hashing complete | Dedup-able: 0 B | Total: 22.1 GB`.

#### 3: Error Boundary — hashing partially complete

Devon's hashing is at 12/19 when he glances at the bar. It shows `Dedup-able: 4.7 GB (still hashing 7 files...)` — accurate at-the-moment value, plus indication it may grow.

### UAT Scenarios

#### Scenario: Summary bar shows computing during hash phase

```gherkin
Given hashing is in progress and 0 hashes have completed
When the summary bar paints
Then it shows "Dedup-able: computing..."
And it does NOT show "Dedup-able: 0 B"
```

#### Scenario: Summary bar updates as classifier output changes

```gherkin
Given hashing has completed for both copies of llama-3.1-8b-Q4_K_M (4.7 GB) but not other models
When the dedup classifier classifies llama-3.1-8b-Q4_K_M as "="
Then the summary bar shows "Dedup-able: 4.7 GB"
And later, after another duplicate model's hashes complete (3.2 GB), the summary bar shows "Dedup-able: 7.9 GB"
```

#### Scenario: Bar reads from same source as row glyphs

```gherkin
Given hashing is complete
When the summary bar shows "Dedup-able: 9.4 GB"
Then the sum of sizes of rows displaying glyph "=" is exactly 9.4 GB
```

#### Scenario: Honest zero when no duplicates

```gherkin
Given Riley's install has no cross-tool duplicates
When hashing completes for all models
Then the summary bar shows "Dedup-able: 0 B"
And status line shows "Hashing complete"
```

### Acceptance Criteria

- [ ] `summary_bar.rs` no longer contains a hardcoded `"Dedup-able: 0 B"` literal
- [ ] Summary bar reads `dedup_able_bytes` from `core::logic::dedup` (single source)
- [ ] During hashing phase (any hash incomplete), bar shows `computing...`
- [ ] Sum of sizes of `=`-glyph rows equals summary-bar `Dedup-able` value at all times after hashing completes
- [ ] When no duplicates exist post-hash, bar honestly shows `Dedup-able: 0 B`

### Outcome KPIs

- **Who**: Devon-class users with >=1 cross-tool duplicate
- **Does what**: Sees the truthful `Dedup-able` value
- **By how much**: 95% of qualifying sessions show non-zero dedup-able post-hash (KPI-1)
- **Measured by**: `launch.log` `summary_paint` event
- **Baseline**: 0% (hardcoded in v1)

### Technical Notes

- Bug location: `crates/modeltap-tui/src/render/summary_bar.rs:36`
- Single-source-of-truth principle (per `shared-artifacts-registry.md` artifact `dedup_able_bytes`)
- Dependencies: US-U1 (background hashing must produce data for classifier)

---

## US-U3: Row glyph reflects dedup state

### Problem

Devon needs to scan the model list and instantly know which rows are unify candidates, which are already unified, and which are unique. v1 displays no per-row dedup indicator at all in the main list; the only way to learn is to open Detail on every row.

### Who

- **Devon** | scanning a list of 19+ models | needs at-a-glance signal

### Solution

Each row in the right pane shows a single-character dedup glyph in a fixed column:

| Glyph | Meaning |
|---|---|
| `?` | Hash pending — not yet computed |
| `~` | Hashing in progress for this file right now |
| `-` | Unique — no copies of this exact content in any other tool |
| `=` | Dedup-able — multiple copies exist on separate inodes |
| `#` | Already unified — multiple tools share one inode |

Glyphs are computed by `core::logic::dedup` (same source as summary bar — see US-U2). The legend is shown in the help screen (`?` key) and is also discoverable via tooltip-on-row at the status line.

### Domain Examples

#### 1: Happy Path — mixed states visible

Devon's main view shows: `llama-3.1-8b =`, `phi-3-mini #`, `nomic-embed -`, `qwen2-7b ?`. He understands at a glance: llama is dedup-able, phi is already unified, nomic is unique, qwen still being hashed.

#### 2: Edge Case — all unique

Riley's install: every row shows `-`. Summary bar agrees with `Dedup-able: 0 B`. Visual coherence: nothing dedup-able means no `=` rows.

#### 3: Error Boundary — hash failure on one file

A corrupted GGUF file fails to hash. That row shows `-` (treated as unique-by-default per ADR-002 conservative-when-uncertain) and a small `!` decorator next to the glyph. Hovering or status-line shows `hash failed: read error — treated as unique`.

### UAT Scenarios

#### Scenario: Glyph reflects classifier output

```gherkin
Given the dedup classifier classifies llama-3.1-8b-Q4_K_M as "="
When the row paints
Then the row shows the glyph "=" in the dedup column
```

#### Scenario: Glyph for already-hardlinked is "#" not "="

```gherkin
Given phi-3-mini is hardlinked between ollama and hf-cache (one inode, 2 paths)
When hashing completes and classification runs
Then the phi-3-mini row glyph is "#"
And it is NOT "="
```

#### Scenario: Hashing-in-progress glyph is "~"

```gherkin
Given the hash worker is currently computing SHA256 for qwen2-7b
When the row paints
Then the qwen2-7b row glyph is "~"
```

#### Scenario: Unique row shows "-"

```gherkin
Given nomic-embed exists only in ollama with no copies elsewhere
When hashing completes
Then the nomic-embed row glyph is "-"
```

#### Scenario: Pre-hash glyph is "?"

```gherkin
Given hashing has not yet started for mistral-7b
When the row paints
Then the mistral-7b row glyph is "?"
```

#### Scenario: Hash failure marked but not blocking

```gherkin
Given the hash worker fails to read a corrupted GGUF file
When the row paints
Then the row glyph is "-"
And a "!" decorator is shown next to the glyph
And the status line on row-select shows "hash failed: <reason> — treated as unique"
```

### Acceptance Criteria

- [ ] Every row in the right pane has a dedup glyph in a fixed column
- [ ] Glyphs match the legend exactly: `?`, `~`, `-`, `=`, `#`
- [ ] Glyph derives from `core::logic::dedup` classifier (single source)
- [ ] Glyph updates reactively as hashing progresses — no manual refresh needed
- [ ] Hash failure shows `-` plus a `!` decorator and informational status text
- [ ] Help screen documents the legend

### Outcome KPIs

- Contributes to KPI-1 (Activation): glyphs are the user-facing signal that dedup-able exists.
- Contributes to KPI-2 (Adoption): a visible `=` is the affordance that invites pressing `u`.

### Technical Notes

- Single source: `core::logic::dedup` classifier
- Color: `=` and `#` benefit from accent color (e.g., cyan/green) but MUST also be distinguishable by glyph alone (per UX accessibility — color not the only channel)
- Dependencies: US-U1 (hash data), US-U2 (single-source pattern established)

---

## US-U4: `u` from main view opens unify dialog with mates pre-populated

### Problem

In v1, `u` only fires from the Detail screen after Enter on a row. Devon presses `u` from the main row list and nothing happens. The hotkey is a lie.

### Who

- **Devon** | scanning rows, sees a `=` glyph, expects to act on it directly | doesn't want to drill into Detail just to invoke unify

### Solution

Pressing `u` while a row is highlighted in the main view:

- If the row's glyph is `=`: opens the unify dialog with the highlighted model's dedup-mates pre-populated (canonical selected via existing `select_canonical()`, all mates checked).
- If the row's glyph is `#`: opens an informational state showing the existing share, with affordance to add more tools (if there are tools that don't have the model and could).
- If the row's glyph is `-`: status-line message `<model> is unique — no copies in other tools to unify with.` Dialog does NOT open.
- If the row's glyph is `?` or `~`: status-line message `Cannot unify <model> — hash still computing. Try again in a moment.` Dialog does NOT open.

### Domain Examples

#### 1: Happy Path — `=` row

Devon highlights `llama-3.1-8b-Q4_K_M =`, presses `u`. Dialog opens showing canonical=ollama, mates=[lm-studio, hf-cache], reclaim=9.4 GB.

#### 2: Edge Case — `#` row

Devon highlights `phi-3-mini #` (already shared by ollama and hf-cache), presses `u`. Dialog opens in "informational" mode: shows current share, plus checkbox `[ ] add lm-studio (would save 0 B; lm-studio doesn't have phi-3-mini yet — would copy 2.3 GB)`. Mostly a read-only confirmation that this row is already done.

#### 3: Error Boundary — `?` row

Devon highlights `qwen2-7b ?`, presses `u`. No dialog. Status line: `Cannot unify qwen2-7b — hash still computing. Try again in a moment.` Devon waits 5 seconds, glyph flips to `=`, presses `u` again — works.

### UAT Scenarios

#### Scenario: u on = row opens dialog with mates

```gherkin
Given the llama-3.1-8b-Q4_K_M row is highlighted with glyph "="
And it has dedup-mates in lm-studio and hf-cache
When Devon presses "u"
Then the unify dialog opens
And the dialog shows canonical = ollama (selected by core::logic::canonical_selector)
And the dialog lists lm-studio and hf-cache as targets, checked
```

#### Scenario: u on # row opens informational dialog

```gherkin
Given the phi-3-mini row is highlighted with glyph "#"
And it is already shared between ollama and hf-cache
When Devon presses "u"
Then the unify dialog opens in informational mode
And the dialog states "phi-3-mini is already unified across 2 tools"
```

#### Scenario: u on - row shows status hint, no dialog

```gherkin
Given the nomic-embed row is highlighted with glyph "-"
When Devon presses "u"
Then no dialog opens
And the status line shows "nomic-embed is unique — no copies in other tools to unify with."
```

#### Scenario: u on ? row shows status hint, no dialog

```gherkin
Given the qwen2-7b row is highlighted with glyph "?"
When Devon presses "u"
Then no dialog opens
And the status line shows "Cannot unify qwen2-7b — hash still computing. Try again in a moment."
```

#### Scenario: u still works from Detail screen (no regression)

```gherkin
Given Devon opened Detail for llama-3.1-8b-Q4_K_M
When Devon presses "u" from the Detail screen
Then the unify dialog opens (existing v1 behavior preserved)
```

### Acceptance Criteria

- [ ] `u` keypress is handled in the main view's row-list handler
- [ ] Dialog opens with mates pre-populated for `=` rows
- [ ] Informational dialog shown for `#` rows
- [ ] Status-line hint shown (no dialog) for `-`, `?`, `~` rows
- [ ] Existing `u`-from-Detail behavior is preserved (no regression)

### Outcome KPIs

- **Who**: Devon-class users in qualifying sessions
- **Does what**: Invokes `u` from the main view at least once
- **By how much**: 60% of qualifying sessions per week (KPI-2)
- **Measured by**: `launch.log` `unify_dialog_opened` events
- **Baseline**: ~0% (v1 broken)

### Technical Notes

- Dialog reuses existing `Msg::OpenUnifyDialog(plan)` — DESIGN to add a main-view `u`-keypress producer
- `core::logic::plan::build_plan()` already accepts canonical+mates; this story wires the inputs from the highlighted row
- Dependencies: US-U3 (glyph determines dispatch behavior)

---

## US-U5: Unify dialog shows concrete reclaim preview and applies plan

### Problem

When v1 ever does open a unify dialog (from Detail), Devon needs to be confident about what will happen before pressing Enter. Vague "are you sure?" copy is destructive-feeling for an action Devon doesn't fully trust yet.

### Who

- **Devon** | about to confirm a multi-file filesystem operation | needs concrete preview to feel confident

### Solution

The unify dialog shows:

- The model name and SHA256 prefix (8 chars).
- The canonical (kept) tool and its full path.
- Each replacement target as a checkbox row, each showing `tool` + `full path` + `4.7 GB -> 0 B (saves 4.7 GB)`.
- A bold total: `Total reclaim: X.Y GB`.
- Action footer: `[Enter] Apply  [space] Toggle  [Esc] Cancel`.

On Enter, the existing `actions::unify::run()` executes the plan. Progress lines appear per target. Existing cross-fs `[s/c/x]` and lsof gates fire as needed (no new error UX in this story — see US-U10 for partial-success polish).

### Domain Examples

#### 1: Happy Path — three-tool unify

Devon's dialog for llama-3.1-8b-Q4_K_M shows canonical=ollama, two checked targets, `Total reclaim: 9.4 GB`. He presses Enter. Two progress lines tick to OK. Toast: `Unified. Reclaimed 9.4 GB.`

#### 2: Edge Case — user toggles off one target

Devon doesn't want to unify into hf-cache yet (he's about to clean it). In the dialog, he navigates to the hf-cache row and presses space — checkbox unchecks. Total updates to `Total reclaim: 4.7 GB`. He presses Enter. Only lm-studio is hardlinked.

#### 3: Error Boundary — user cancels

Devon opens the dialog, reads it, decides to do this later. Presses Esc. Dialog closes; no filesystem action; row glyph stays `=`.

### UAT Scenarios

#### Scenario: Dialog shows canonical, targets, and total reclaim

```gherkin
Given Devon presses u on llama-3.1-8b-Q4_K_M with mates in lm-studio and hf-cache
When the dialog opens
Then it shows canonical "ollama: ~/.ollama/models/.../sha256-e5c19af2"
And it shows target row "lm-studio: ~/.cache/lm-studio/.../Q4_K_M.gguf  4.7 GB -> 0 B (saves 4.7 GB)" with checkbox checked
And it shows target row "hf-cache: ~/.cache/huggingface/.../Q4_K_M.gguf  4.7 GB -> 0 B (saves 4.7 GB)" with checkbox checked
And it shows "Total reclaim: 9.4 GB"
And it shows "[Enter] Apply  [space] Toggle  [Esc] Cancel"
```

#### Scenario: Toggling a target updates the total

```gherkin
Given the dialog is open with two targets checked and "Total reclaim: 9.4 GB"
When Devon navigates to the hf-cache row and presses space
Then the hf-cache checkbox is unchecked
And the total updates to "Total reclaim: 4.7 GB"
```

#### Scenario: Enter applies the plan

```gherkin
Given the dialog has both targets checked
When Devon presses Enter
Then a progress UI appears showing per-target status
And actions::unify::run() is invoked with the plan
And on success, the toast shows "Unified. Reclaimed 9.4 GB."
```

#### Scenario: Esc cancels with no filesystem change

```gherkin
Given the dialog is open
When Devon presses Esc
Then the dialog closes
And no filesystem operation occurs
And the row glyph remains unchanged
```

#### Scenario: Existing cross-fs fallback still fires (ADR-008)

```gherkin
Given lm-studio's models live on a different filesystem
And the dialog is open with lm-studio checked
When Devon presses Enter
Then the cross-fs [s/c/x] dialog appears for lm-studio
```

### Acceptance Criteria

- [ ] Dialog body shows: model name, SHA256 prefix, canonical path, per-target rows with checkboxes and savings, total reclaim
- [ ] Total reclaim recomputes live as targets are toggled
- [ ] `[space]` toggles a target's checkbox
- [ ] `[Enter]` applies the plan via existing `actions::unify::run()`
- [ ] `[Esc]` closes dialog with no filesystem effect
- [ ] Cross-fs ADR-008 fallback continues to fire when applicable
- [ ] Lsof Q5 detect-and-prompt-then-retry continues to fire when applicable

### Outcome KPIs

- Contributes to KPI-2 (concrete preview drives confidence to actually press Enter) and KPI-3 (success ratio).

### Technical Notes

- Reuses existing `actions::unify::run()` — orchestration is already in place
- Reuses existing cross-fs and lsof gates
- Dependencies: US-U4 (dialog open path); existing `Tool::link()` per ADR-001

---

## US-U6: Post-unify row glyph and summary bar update without restart

### Problem

After a successful unify, Devon needs to see proof the row is now unified. A v1-style "you have to restart to see the change" would gut the emotional payoff and gut trust.

### Who

- **Devon** | just confirmed a unify | needs immediate, visible confirmation

### Solution

After `actions::unify::run()` emits its success event:

- Affected models' rows are re-classified by `core::logic::dedup`.
- Row glyphs flip from `=` to `#` (when fully unified).
- Summary bar `Dedup-able` decreases by the reclaimed amount; for ~5 seconds it shows a parenthetical `(was X GB)` delta then collapses.
- Summary bar `Unified: N models` increments.
- `[All Unified]` (US-U7) badge increments.
- All without a restart, all reading from the same single classifier source.

### Domain Examples

#### 1: Happy Path — full success

Pre-unify: `Dedup-able: 8.4 GB | Unified: 2 models`. Devon unifies llama-3.1-8b. Post-unify: `Dedup-able: 3.7 GB (was 8.4 GB) | Unified: 3 models`. Llama row flips to `#`. After 5 s, summary collapses to `Dedup-able: 3.7 GB | Unified: 3 models`.

#### 2: Edge Case — only some targets succeed (cross-fs skip)

Devon unifies; user picked `[s]` skip on cross-fs dialog for lm-studio. Only hf-cache linked. Post-action: row glyph stays `=` (because at least one tool still has its own copy). Summary bar Dedup-able decreases by 4.7 GB only (the linked target). `Unified: N` does not increment (model is not fully unified).

#### 3: Error Boundary — unify failed entirely

User confirmed but every target failed (e.g., permission denied across the board). No state changes: glyphs stay as before, summary unchanged. Toast reports failures (US-U10 polishes this).

### UAT Scenarios

#### Scenario: Successful full unify flips glyph and updates summary

```gherkin
Given Devon successfully unifies llama-3.1-8b-Q4_K_M into 2 tools
When actions::unify::run() emits "unify_completed_full"
Then within 200 ms the llama-3.1-8b-Q4_K_M row glyph is "#"
And the summary bar "Dedup-able" value has decreased by 9.4 GB
And the summary bar "Unified" count has incremented by 1
```

#### Scenario: Summary bar shows transient delta then collapses

```gherkin
Given a unify just completed reclaiming 9.4 GB
When the summary bar paints immediately after
Then it shows "Dedup-able: 3.7 GB (was 8.4 GB)"
When 5 seconds pass
Then the summary bar shows "Dedup-able: 3.7 GB" without the delta annotation
```

#### Scenario: Partial unify leaves glyph as =

```gherkin
Given Devon unifies and one of two targets fails
When actions::unify::run() emits "unify_completed_partial"
Then the affected row glyph remains "="
And the "Unified" count does NOT increment
And the "Dedup-able" value decreases by only the successful target's bytes
```

#### Scenario: No-restart requirement

```gherkin
Given Devon completes a unify
When Devon does NOT restart modeltap
Then within the same session, all UI surfaces (row glyph, summary bar, [All Unified] badge) reflect the post-unify state
```

### Acceptance Criteria

- [ ] On `unify_completed_full`, affected rows re-classify within 200 ms of the event
- [ ] Glyph flips `=` -> `#` for fully-unified models
- [ ] Glyph remains `=` for partial-success models
- [ ] Summary bar `Dedup-able` decreases by reclaimed bytes
- [ ] Summary bar shows `(was X GB)` delta for ~5 s then collapses
- [ ] Summary bar `Unified: N models` increments only on full success
- [ ] No restart required for any of the above

### Outcome KPIs

- KPI-3 (Success): post-unify state reflects ground truth — required for the action to feel real.

### Technical Notes

- Re-classification trigger consumes the JSONL event from `actions::unify::run()`
- DESIGN to decide whether re-classification is full-pass or scoped to affected dedup-keys (perf consideration)
- Dependencies: US-U2, US-U3, US-U5

---

## US-U7: `[All Unified]` pseudo-tool slot in left pane

### Problem

Devon wants a single place to audit "what is currently unified" without having to scan every row in every tool's view looking for `#` glyphs. He also wants the count visible in left-pane navigation muscle memory, not a hidden hotkey.

### Who

- **Devon** | post-unify auditor / cumulative-savings tracker | wants a single navigation step

### Solution

A pseudo-tool entry appears in the left pane, positioned below the four real tool slots (or wherever the existing left-pane order ends), labeled `[All Unified]` with a count badge: `[All Unified] (5)`. Selecting it (j/k navigation, no new keybinding) populates the right pane with only the rows whose glyph is `#`. Each row shows: model name, size, `N tools`, `saves X GB`. Footer: `Unified: N models | Total reclaimed by unification: X.Y GB`.

### Domain Examples

#### 1: Happy Path — 5 unified models

Devon navigates to `[All Unified] (5)`. Right pane shows 5 rows, footer shows `Unified: 5 models | Total reclaimed by unification: 25.1 GB`.

#### 2: Edge Case — count matches across surfaces

The badge `(5)`, the right-pane row count `5`, and the summary bar `Unified: 5 models` all show the same number. Same model viewed under `[ollama]` shows glyph `#`; viewed under `[All Unified]` shows the row in the list.

#### 3: Error Boundary — count is zero

Devon hasn't unified anything. Slot shows `[All Unified] (0)`. Selecting it shows the empty-state guidance (US-U8 P2).

### UAT Scenarios

#### Scenario: Slot is present in left pane

```gherkin
Given Devon launches modeltap with 4 tools configured
When the left pane paints
Then it shows the 4 tool slots
And below them, a slot labeled "[All Unified]" with a count badge
```

#### Scenario: Selecting the slot filters the right pane

```gherkin
Given 5 models are classified as "#"
When Devon navigates to [All Unified]
Then the right pane shows exactly 5 rows
And every row corresponds to a model with glyph "#"
```

#### Scenario: Row format includes tool count and savings

```gherkin
Given the [All Unified] view shows llama-3.1-8b-Q4_K_M (4.7 GB, 3 tools sharing)
When the row paints
Then it shows "llama-3.1-8b-Q4_K_M  4.7 GB  3 tools  saves 9.4 GB"
And "saves" equals (3 - 1) * 4.7 GB
```

#### Scenario: Footer aggregates totals

```gherkin
Given the [All Unified] view shows 5 unified models
When the footer paints
Then it shows "Unified: 5 models | Total reclaimed by unification: <SUM> GB"
And SUM equals the sum of "saves" across the 5 rows
```

#### Scenario: Counts agree across surfaces

```gherkin
Given the [All Unified] badge shows "(5)"
Then the summary bar "Unified" count shows 5
And the right pane row count, when [All Unified] is selected, is 5
```

### Acceptance Criteria

- [ ] `[All Unified]` slot appears in the left pane below the real tool slots
- [ ] Badge shows the unified-model count (live, reactive)
- [ ] Selecting the slot populates the right pane with only `#`-glyph models
- [ ] Each row shows: name, size, tool count, savings
- [ ] Footer shows `Unified: N models | Total reclaimed by unification: X.Y GB`
- [ ] Badge count, summary-bar count, and right-pane row count are always equal

### Outcome KPIs

- KPI-4 (Retention proxy): a visible cumulative-savings number is the most likely thing to bring Devon back to check progress.

### Technical Notes

- Pseudo-tool concept: NOT a new `Tool` impl (ADR-001 forbids that); it's a left-pane render concept that filters the existing model set. DESIGN to confirm placement.
- Dependencies: US-U3 (`#` glyph computation), US-U2 (single-source pattern)

---

## US-U8: `[All Unified]` empty state with onboarding guidance

### Problem

A user opening `[All Unified]` for the first time, before any unification has happened, sees an empty list with no guidance. This is a dead end and looks broken (per emotional-design empty-state checklist).

### Who

- **Devon (or new user)** | no models unified yet | curious what this slot does

### Solution

When the unified count is 0, the right pane shows guidance:

```text
No models are unified yet.

Navigate to a tool, find a row marked "=", and press [u] to unify it.

Models marked "=" can save you disk.
```

### Domain Examples

#### 1: Happy Path — fresh install

Devon first launch, no unification done. Selects `[All Unified] (0)`. Sees guidance text. Reads it. Navigates back, finds an `=` row, presses `u`.

#### 2: Edge Case — install becomes empty after deletes

Devon previously had 3 unified, then deleted/zapped them. `[All Unified] (0)` now shows the same guidance.

#### 3: Error Boundary — hashing not yet complete

`[All Unified] (?)` count is unknown because hashing is in progress. Empty state shows: `Hashing in progress. Unified models will appear here as soon as hashing completes.`

### UAT Scenarios

#### Scenario: Empty state shown when count is zero

```gherkin
Given Devon's install has zero unified models
When Devon navigates to [All Unified]
Then the right pane shows the guidance text
And it includes the "press [u] on an = row" instruction
```

#### Scenario: Empty state distinguishes "still hashing" from "truly empty"

```gherkin
Given hashing is in progress and unified count is unknown
When Devon navigates to [All Unified]
Then the right pane shows "Hashing in progress. Unified models will appear here..."
```

#### Scenario: Empty state disappears once a model is unified

```gherkin
Given the empty state is shown
When Devon completes a unify in another view
Then on next paint, the [All Unified] view shows the unified row instead of guidance
```

### Acceptance Criteria

- [ ] When unified count is 0 and hashing is complete, guidance text is shown
- [ ] When unified count is unknown (hashing in progress), a different "still hashing" message is shown
- [ ] Guidance includes a concrete next step (press `u` on an `=` row)

### Outcome KPIs

- KPI-4 (Retention proxy): improves first-run impression; reduces "it looks broken" abandonment.

### Technical Notes

- Pure rendering story; no core logic changes
- Dependencies: US-U7

---

## US-U9: Detail screen for unified model shows shared inode and paths

### Problem

A skeptical Devon (or auditor Riley) wants filesystem-level proof that the inode is actually shared after a unify. v1 detail screen exists but doesn't surface inode info.

### Who

- **Riley (auditor)** | needs proof | wants to verify with `ls -li` mentally

### Solution

Detail screen for a `#` model shows: SHA256, size, inode number, list of all paths sharing that inode. Action footer: `[Esc] Back  [d] Delete from one tool (delete_one)  [u] Add another tool`.

### Domain Examples

#### 1: Happy Path — verify inode

Riley opens detail on llama-3.1-8b-Q4_K_M. Sees: `inode: 4521977 (shared)`. Below: 3 path lines all under that inode. Closes detail satisfied.

#### 2: Edge Case — partially-unified (`=` glyph)

Riley opens detail on a `=` model. Detail shows multiple inodes (one per separate copy). Layout adapts: groups paths by inode.

#### 3: Error Boundary — inode info unavailable

On a filesystem where `stat()` doesn't return useful inode info (e.g., some network mounts), detail shows `inode: <not available on this filesystem>` with explanation.

### UAT Scenarios

#### Scenario: Detail shows shared inode for # model

```gherkin
Given llama-3.1-8b-Q4_K_M is unified across 3 tools (one inode)
When Devon opens the Detail screen for it
Then the detail shows the inode number
And it lists all 3 paths grouped under that inode
And it shows "Saves vs. separate copies: 9.4 GB"
```

#### Scenario: Detail for = model shows multiple inodes

```gherkin
Given a model has 3 separate copies on 3 different inodes
When Devon opens Detail
Then the detail groups paths by inode
And shows N inode groups
```

#### Scenario: Detail handles missing inode info gracefully

```gherkin
Given the filesystem does not expose useful inode numbers
When Devon opens Detail
Then the detail shows "inode: <not available on this filesystem>"
And no crash occurs
```

### Acceptance Criteria

- [ ] Detail screen for `#` models shows inode number and grouped paths
- [ ] Detail screen for `=` models groups paths by inode (one group per copy)
- [ ] Footer offers `[d]` (existing delete_one per ADR-009) and `[u]` (existing unify dialog with this model)
- [ ] Missing-inode case handled with informational text

### Outcome KPIs

- KPI-4 (Retention proxy): builds trust via verifiability.

### Technical Notes

- `stat()` for inode is filesystem-dependent; DESIGN to confirm Tool trait can expose this without ADR-001 violation (likely via path metadata accessible to core)
- Dependencies: US-U7 (entry point from `[All Unified]`)

---

## US-U10: Partial-success reporting (per-target outcome in toast)

### Problem

When a unify partially succeeds (e.g., one target fails after others linked), v1 logs to JSONL but the user-facing toast is non-specific. Devon doesn't know which tool failed or why without grepping launch.log.

### Who

- **Devon** | just hit a partial-success path | wants to know which tool failed and why, without leaving the TUI

### Solution

The success toast shows per-target outcomes inline:

```text
+-- Partial success ------------------------+
| Unified llama-3.1-8b-Q4_K_M into 1 of 2.  |
|   * lm-studio  OK    (saved 4.7 GB)       |
|   * hf-cache   FAIL: Permission denied    |
|                                           |
| Reclaimed 4.7 GB. Full details in:        |
|   ~/.modeltap/launch.log                  |
|                                           |
| [Enter] Continue  [r] Retry failed only   |
+-------------------------------------------+
```

### Domain Examples

#### 1: Happy Path — single failure

Devon's hf-cache directory is read-only. Toast shows what's above. He fixes permissions, presses `[r]`, hf-cache succeeds, second toast shows `Unified into all tools. Reclaimed 4.7 GB.`

#### 2: Edge Case — total failure

All targets failed. Toast shows: `Unified into 0 of 2. Reclaimed 0 GB.` with per-target failure reasons. `[r] Retry all` offered.

#### 3: Error Boundary — failures without categories

A target fails with a low-level OS error code; toast shows raw error message verbatim plus a hint: `OS error 13 (EACCES). Often means permission denied — check directory writability.`

### UAT Scenarios

#### Scenario: Toast lists each target's outcome

```gherkin
Given a unify completes with lm-studio OK and hf-cache failed with "Permission denied"
When the toast appears
Then it shows "Unified into 1 of 2"
And it shows "lm-studio  OK    (saved 4.7 GB)"
And it shows "hf-cache   FAIL: Permission denied"
And it shows "Reclaimed 4.7 GB"
```

#### Scenario: Retry-failed-only re-runs only the failures

```gherkin
Given the partial-success toast is shown with hf-cache as the only failure
When Devon presses "r"
Then a new unify is attempted ONLY for hf-cache (not lm-studio)
And the existing lm-studio hardlink is not touched
```

#### Scenario: Total failure shows zero reclaim

```gherkin
Given all targets fail
When the toast appears
Then it shows "Unified into 0 of N"
And it shows "Reclaimed 0 GB"
And the row glyph remains "="
```

### Acceptance Criteria

- [ ] Toast lists each target's outcome (OK / FAIL: <reason>) and per-target bytes
- [ ] Toast shows total reclaim (sum of OK targets)
- [ ] Toast offers `[r] Retry failed only` when at least one failure occurred
- [ ] Retry-failed-only re-runs only failed targets, leaving successes untouched
- [ ] Toast points to `~/.modeltap/launch.log` for full JSONL detail

### Outcome KPIs

- KPI-3 (Success): improves "completed without friction" by giving a one-key recovery path on partial failures.

### Technical Notes

- Reuses existing JSONL events from `actions::unify::run()`
- Toast is a TUI rendering concern; per-target events already exist
- Dependencies: US-U5
