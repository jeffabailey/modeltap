# Journey: Unify a Model Across All Tools (Visual)

**Persona**: Devon (small-team dev with Ollama + HF + LM Studio + Atomic Chat)
**Goal**: Take a model that exists as 3 separate copies across 3 tools and make all 3 share one inode, freeing 2/3 of the disk those copies occupied — and SEE the disk reclaimed in the summary bar.
**Trigger**: Devon notices `du -sh ~/.ollama/models` is huge and remembers modeltap claims to "unify."
**Success criteria**:
- Summary bar shows accurate `Dedup-able: X GB` BEFORE action (not the hardcoded 0).
- After pressing `u` on a row, the unify dialog opens with that model's mates pre-populated.
- After confirming, summary bar shows the disk has been reclaimed; the row's indicator flips to `#` (already-unified).

---

## Emotional Arc

```
START                    MIDDLE                                        END
-----                    ------                                        ---
Confused/                Curious                  Confident             Relieved/
Skeptical    --hash-->   "oh, it's working"  --action-->   "this is real"  --done-->   Satisfied
("dedup=0,                ("hashing 3/12...")     ("3 copies -> 1, 14GB)               ("reclaimed
 nothing                                          will be reclaimed")                   14.2 GB.
 works")                                                                                Row now shows #.")
```

Pattern: **Problem Relief** (Frustrated -> Hopeful -> Relieved). The hashing-progress indicator is the critical hope-injection point — without it, Devon never gets out of "this app is broken."

---

## Flow Diagram

```
[Trigger] Devon launches modeltap
    |
    | sees: rows render <100ms with dedup column = "?"
    |       summary bar: "Hashing 0/12... | Dedup-able: computing..."
    | feels: cautiously hopeful (something is happening)
    |
    v
[Step 1] Wait for hashes to settle (background, ~3-15s typical)
    |
    | sees: rows progressively flip "?" -> "=" / "#" / "-" as hashes complete
    |       progress: "Hashing 7/12..."
    |       summary updates live: "Dedup-able: 8.4 GB | Unified: 2 models"
    | feels: discovery joy ("there it is")
    |
    v
[Step 2] Devon navigates to a "=" row (e.g., llama-3.1-8b-Q4_K_M)
    |
    | sees: row highlighted; status line: "llama-3.1-8b-Q4_K_M | 4.7GB | in: ollama, lm-studio, hf"
    | feels: focused
    |
    v
[Step 3] Devon presses [u]
    |
    | sees: modal dialog opens, mates pre-populated:
    |       +--- Unify llama-3.1-8b-Q4_K_M ---+
    |       | Canonical: ollama (4.7 GB)      |
    |       | Will hardlink into:             |
    |       |   [x] lm-studio  (-4.7 GB)      |
    |       |   [x] hf         (-4.7 GB)      |
    |       | Reclaim: 9.4 GB                 |
    |       | [Enter=apply] [Esc=cancel]      |
    |       +---------------------------------+
    | feels: confident (preview is concrete, dollar-amount obvious)
    |
    v
[Step 4] Devon presses [Enter]
    |
    | sees: progress: "Linking 1/2... lm-studio OK"
    |       progress: "Linking 2/2... hf OK"
    |       success toast: "Unified. Reclaimed 9.4 GB."
    | feels: relieved (it worked, real bytes are back)
    |
    v
[Step 5] Devon sees the result persist
    |
    | sees: row now shows "#" (already-unified) glyph
    |       summary bar: "Dedup-able: 0 B (down from 8.4 GB) | Unified: 3 models"
    |       left pane: "[All Unified] (3)" count incremented
    | feels: satisfied; trusts the tool; will use again
```

---

## Step-by-Step TUI Mockups

### Step 0 — First paint (<100ms after launch)

```
+- modeltap ---------------------------------------------- v0.2.0 -+
| TOOLS              | MODELS                                       |
| > ollama       (5) | llama-3.1-8b-Q4_K_M    4.7 GB   ?           |
|   lm-studio    (4) | mistral-7b-instruct    4.1 GB   ?           |
|   hf-cache     (8) | phi-3-mini             2.3 GB   ?           |
|   atomic-chat  (2) | nomic-embed            274 MB   ?           |
|   [All Unified] (?)| qwen2-7b              4.4 GB   ?           |
|                    | ... (12 more)                                |
+--------------------+---------------------------------------------+
| Hashing 0/19... | Dedup-able: computing... | Total: 47.3 GB     |
+------------------------------------------------------------------+
| [j/k]nav [Enter]details [u]unify [d]delete [q]quit              |
+------------------------------------------------------------------+
```

Note: `?` glyph means "not yet hashed." Status line transparently shows hashing progress.

### Step 1 — Hashes resolving (3-15 seconds in)

```
+- modeltap ---------------------------------------------- v0.2.0 -+
| TOOLS              | MODELS                                       |
| > ollama       (5) | llama-3.1-8b-Q4_K_M    4.7 GB   =  <- mates  |
|   lm-studio    (4) | mistral-7b-instruct    4.1 GB   ~           |
|   hf-cache     (8) | phi-3-mini             2.3 GB   #  <- linked |
|   atomic-chat  (2) | nomic-embed            274 MB   -           |
|   [All Unified] (2)| qwen2-7b               4.4 GB   ?           |
|                    | ...                                          |
+--------------------+---------------------------------------------+
| Hashing 14/19... | Dedup-able: 8.4 GB | Unified: 2 models       |
+------------------------------------------------------------------+
| [j/k]nav [Enter]details [u]unify [d]delete [q]quit              |
+------------------------------------------------------------------+
```

Glyph legend (status line second row, suppressed for brevity in mockup):
- `?` = hash pending
- `~` = hashing now
- `-` = unique (no dedup mates found)
- `=` = dedup-able (>=2 separate inodes match)
- `#` = already-unified (>=2 tools share one inode)

### Step 3 — Unify dialog

```
+- modeltap ---------------------------------------------- v0.2.0 -+
| TOOLS              | MODELS                                       |
| > ollama       (5) | +-- Unify: llama-3.1-8b-Q4_K_M -----------+ |
|   lm-studio    (4) | | sha256: e5c1...9af2                      | |
|   hf-cache     (8) | |                                          | |
|   atomic-chat  (2) | | Canonical (kept):                        | |
|   [All Unified] (2)| |   * ollama  : ~/.ollama/models/...       | |
|                    | |     4.7 GB, mtime 2026-04-12             | |
|                    | |                                          | |
|                    | | Will replace with hardlink:              | |
|                    | |   [x] lm-studio                          | |
|                    | |       ~/.cache/lm-studio/models/...      | |
|                    | |       4.7 GB -> 0 B (saves 4.7 GB)       | |
|                    | |   [x] hf-cache                           | |
|                    | |       ~/.cache/huggingface/hub/...       | |
|                    | |       4.7 GB -> 0 B (saves 4.7 GB)       | |
|                    | |                                          | |
|                    | | Total reclaim: 9.4 GB                    | |
|                    | |                                          | |
|                    | | [Enter] Apply  [space] Toggle  [Esc] No  | |
|                    | +------------------------------------------+ |
+--------------------+---------------------------------------------+
| Dedup-able: 8.4 GB | Unified: 2 models | Total: 47.3 GB         |
+------------------------------------------------------------------+
```

### Step 4 — Apply progress

```
+- modeltap ---------------------------------------------- v0.2.0 -+
| ...                |                                              |
|                    |   Unifying llama-3.1-8b-Q4_K_M...            |
|                    |   [######------] lm-studio    OK             |
|                    |   [############] hf-cache     OK             |
|                    |                                              |
|                    |   Reclaimed 9.4 GB.                          |
|                    |                                              |
|                    |   [Enter] Continue                           |
+--------------------+---------------------------------------------+
| Hashing complete | Applying... | Total: 47.3 GB                  |
+------------------------------------------------------------------+
```

### Step 5 — Result, persistent

```
+- modeltap ---------------------------------------------- v0.2.0 -+
| TOOLS              | MODELS                                       |
| > ollama       (5) | llama-3.1-8b-Q4_K_M    4.7 GB   #  <- now   |
|   lm-studio    (4) | mistral-7b-instruct    4.1 GB   =           |
|   hf-cache     (8) | phi-3-mini             2.3 GB   #           |
|   atomic-chat  (2) | nomic-embed            274 MB   -           |
|   [All Unified] (3)|   <- count went 2 -> 3                       |
|                    | qwen2-7b               4.4 GB   =           |
+--------------------+---------------------------------------------+
| Dedup-able: 3.7 GB (was 8.4 GB) | Unified: 3 models | 37.9 GB    |
+------------------------------------------------------------------+
| [j/k]nav [Enter]details [u]unify [d]delete [q]quit              |
+------------------------------------------------------------------+
```

The summary delta (`was 8.4 GB`) is shown briefly (5s) after a unify, then collapses to just the current value.

---

## Error Paths

### E1 — Cross-filesystem (existing ADR-008 dialog)

User presses `u`, dialog opens, user confirms, but lm-studio's models live on a different filesystem from ollama. The existing `[s/c/x]` dialog appears:

```
+-- Cross-filesystem detected -------------------------+
| lm-studio is on /mnt/external (different fs)         |
| Hardlink not possible across filesystems.            |
|                                                      |
| [s] Skip lm-studio (still link hf-cache)             |
| [c] Copy instead of link (saves 0 B for that tool)   |
| [x] Cancel entire unify                              |
+------------------------------------------------------+
```

### E2 — Running tool (existing lsof gate, Q5)

If `lsof` shows any of the target tools currently has the file open:

```
+-- Tool in use --------------------------------------+
| ollama is currently running and has the file open.  |
| Stop ollama and press [r] to retry,                 |
| or [s] to skip ollama for now,                      |
| or [x] to cancel.                                   |
+-----------------------------------------------------+
```

### E3 — Mid-hash interruption (NEW)

User presses `u` on a row whose hash is still computing (`?` or `~`):

```
Status line: "Cannot unify llama-3.1-8b — hash still computing (40%). Try again in a moment."
```

The dialog does NOT open. `u` is a no-op with informative status. Devon can press `u` again any time; once the hash settles to `=`, it works.

### E4 — Partial success (NEW)

After applying, one of N targets fails (cross-fs falls through, permission denied, etc.). The remaining N-1 still complete. The success toast becomes:

```
+-- Partial success ----------------------------------+
| Unified llama-3.1-8b-Q4_K_M into 1 of 2 tools.      |
|   * lm-studio  OK   (saved 4.7 GB)                  |
|   * hf-cache   FAIL: Permission denied              |
|                                                     |
| Reclaimed 4.7 GB. See ~/.modeltap/launch.log        |
| for full details.                                   |
|                                                     |
| [Enter] Continue                                    |
+-----------------------------------------------------+
```

The row's glyph becomes `=` still (because it's no longer fully unified — at least one tool still has its own copy) — Devon can retry later.

### E5 — No dedup mates on the row when `u` pressed

```
Status line: "llama-3.1-8b-Q4_K_M is unique — no copies in other tools to unify with."
```

`u` is a no-op. Devon learns this row isn't a unify candidate.

---

## Shared Artifacts (referenced)

- `${dedup_key_progress}` — "14/19" hashing progress; source: in-process hash queue; consumed by status line
- `${dedup_able_bytes}` — "8.4 GB"; source: `core::logic::dedup` classifier output; consumed by summary bar
- `${unified_count}` — "3 models"; source: `core::logic::dedup` (count of `#` rows); consumed by summary bar AND `[All Unified]` left-pane count
- `${unify_plan}` — `UnifyPlan` struct; source: `core::logic::plan::build_plan()`; consumed by dialog rendering AND `actions::unify::run()`
- `${reclaimed_bytes}` — "9.4 GB"; source: `actions::unify::run()` JSONL event; consumed by success toast AND summary bar delta

Full registry: see `shared-artifacts-registry.md`.

---

## Integration Checkpoints

| Checkpoint | Validates |
|---|---|
| C1: dedup classifier reads from in-process hash cache, NOT from a row's own state | Single source for "is this dedup-able?" |
| C2: summary bar reads `${dedup_able_bytes}` from the same source as the row glyphs | Bar and rows agree |
| C3: `[All Unified]` left-pane count = number of rows with `#` glyph in the right pane | Counts agree across panes |
| C4: After `actions::unify::run()` succeeds, the affected models' rows re-classify on next paint | UI reflects state without restart |
| C5: Hash progress survives a re-discover (state lives in core, not TUI) | Hashes don't reset if user toggles tools |
