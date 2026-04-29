# Journey: Cleanup and Unify Local AI Models — Visual

**Feature:** modeltap-tui
**Persona:** Devon Park — Local-AI power user on macOS, runs Ollama + llama.cpp + LM Studio + an `hf` cache, has ~340 GB of model files spread across home directory, suspects significant duplication. Comfortable in a terminal. Keyboard-first.
**Goal:** See every locally-downloaded model in one place, deduplicate models across tools without copying bytes, and reclaim disk space by zapping a tool's models.

## Emotional Arc

This is a developer-power-user tool. Emotional language is proportionate — this is not a consumer onboarding flow.

| Phase | State | What drives it |
|---|---|---|
| Start | Mildly frustrated, suspicious ("how much disk am I wasting?") | Disk pressure, scattered tools, no single view |
| Mid | Focused, in-control | Clear two-pane layout, bottom bar reminds shortcuts, status updates within 100ms |
| End | Satisfied, in-control | Disk space reclaimed shown numerically; `u` confirmed cross-tool availability |

Failure-mode emotional rule: every destructive action must end with the user feeling **in control**, not surprised. `z` always confirms; `u` shows what changed.

## Journey Flow (ASCII)

```
[Trigger]                                   [End state]
"Disk is full" / "Which tool       Disk space reclaimed.
 has model X?" / "I want to use    Single canonical model
 model X with tool Y."             usable across N tools.
       |                                       ^
       v                                       |
+------+------+   +------+------+   +------+---+--+   +-------+-------+   +------+------+
| Step 1      |   | Step 2      |   | Step 3       |   | Step 4        |   | Step 5      |
| Launch TUI  |-->| Browse tools|-->| Inspect      |-->| Decide:       |-->| Execute     |
|             |   | + models    |   | duplicates / |   | unify (u) OR  |   | + verify    |
|             |   |             |   | red icons    |   | zap (z)       |   | outcome     |
+-------------+   +-------------+   +--------------+   +---------------+   +-------------+
 Feels:            Feels:            Feels:              Feels:              Feels:
 "let's see"       "in control"      "I get it"          "deliberate"        "in control"
 <100ms paint      4 tools left      red = locked        confirmation        bytes saved
                   models right      green dots = dup    visible             shown
```

## Step-by-Step Detail

### Step 1: Launch TUI

**Command:** `modeltap`

**TUI mockup (initial paint, must appear in <1s):**

```
+- modeltap ---------------------------------------------------------------+
| Tools                | Models in Ollama (12, 47.3 GB)                    |
| > Ollama        12   |                                                   |
|   llama-cli     6    |   * llama3:8b-instruct-q4_K_M       4.7 GB        |
|   Hugging Face  31   |   * mistral:7b-instruct-q4_K_M      4.4 GB        |
|   LM Studio     9    |   * qwen2.5:14b-q4_K_M              8.9 GB        |
|                      |   ...                                             |
|                      |                                                   |
|  Total: 58 models    |   o = unique to this tool                         |
|  Disk: 138.4 GB      |   * = also registered in another tool             |
|  Dedup-able: 22 GB   |   ! = format-locked (only this tool can use)      |
+--------------------------------------------------------------------------+
| [<-/->] tools  [up/down] models  [u] unify  [z] zap tool  [?] help [q]   |
+--------------------------------------------------------------------------+
```

**Shared artifacts referenced:** `${tool_count}=4`, `${tool.model_count}`, `${tool.disk_usage}`, `${total.disk_usage}`, `${total.dedupable}` — all sourced from the in-memory inventory built by the discovery phase (single source of truth: per-tool plugin discovery results aggregated into one `Inventory` value).

**Emotional state:** entry "let's see what I have" → exit "OK, I have a map." Confidence-builder: counts and bytes appear immediately, not after typing a query.

**Integration checkpoint:** Disk totals shown in the left pane MUST equal the sum of individual model sizes shown when each tool is selected. Mismatch = silent inventory bug.

---

### Step 2: Browse tools and models

**TUI mockup (Hugging Face selected, showing red-icon model):**

```
+- modeltap ---------------------------------------------------------------+
| Tools                | Models in Hugging Face (31, 78.2 GB)              |
|   Ollama        12   |                                                   |
|   llama-cli     6    |   o meta-llama/Llama-3-8B-Instruct  16.0 GB       |
| > Hugging Face  31   |     [GGUF: q4_K_M]                                |
|   LM Studio     9    |   * mistralai/Mistral-7B-v0.3        4.4 GB       |
|                      |     [GGUF: q4_K_M] — also in: Ollama, LM Studio   |
|                      |   ! TheBloke/something-AWQ           8.1 GB       |
|                      |     [AWQ — only Hugging Face accepts this]        |
|                      |   ...                                             |
+--------------------------------------------------------------------------+
| [<-/->] tools  [up/down] models  [u] unify  [z] zap tool  [?] help [q]   |
+--------------------------------------------------------------------------+
```

**Why the red icon (`!`) here:** the AWQ-quantized model can only be consumed by Hugging Face's tooling — none of llama-cli, Ollama, or LM Studio in v1 declare AWQ as an accepted format. The icon signals "do not bother trying to unify; this is locked to one tool by format."

**Shared artifacts:** `${model.id}`, `${model.size}`, `${model.format}`, `${model.compatible_tools}` — all sourced from the per-tool plugin's listing output, joined by the dedup key (see shared-artifacts-registry).

**Emotional state:** entry "scan the list" → exit "I understand which are duplicates and which are locked." Hick's Law applies: three icons, no more.

**Integration checkpoint:** A model marked `*` (multi-tool) MUST resolve to at least 2 entries pointing at the same physical content (per the dedup key strategy). A model marked `!` MUST have `compatible_tools.len() == 1`.

---

### Step 3: Inspect duplicates and locked models

User presses Enter on a `*` model to see details:

```
+- Model detail: mistralai/Mistral-7B-v0.3 (q4_K_M, GGUF) -----------------+
|                                                                          |
| Size on disk:         4.4 GB                                             |
| Dedup key:            sha256:8f3e...c102 (file content hash)             |
|                                                                          |
| Registered with:                                                         |
|   - Ollama        /Users/devon/.ollama/models/blobs/sha256-8f3e...       |
|   - llama-cli     /Users/devon/llms/mistral-7b-q4.gguf                   |
|   - Hugging Face  ~/.cache/huggingface/hub/.../mistral-7b-q4.gguf        |
|                                                                          |
| Status: NOT UNIFIED — 3 separate copies exist on disk (13.2 GB total).   |
| If unified: would reclaim 8.8 GB.                                        |
|                                                                          |
| Press [u] to unify, [Esc] to back, [d] to delete from one tool.          |
+--------------------------------------------------------------------------+
| [Esc] back   [u] unify   [d] delete-from-one   [?] help                  |
+--------------------------------------------------------------------------+
```

**Shared artifacts:** `${model.dedup_key}` (the canonical identity — see open question Q6 in intake brief; this journey assumes content-hash but flags it for DESIGN), `${model.disk_paths[]}`, `${model.reclaimable_bytes}`.

**Emotional state:** entry "is this actually a dupe?" → exit "yes, 3 copies, 8.8 GB to reclaim." User now has data to make a deliberate decision.

**Integration checkpoint:** `reclaimable_bytes = (count_of_copies - 1) * file_size`. If the dedup key matches but the file sizes differ, that's a dedup-key bug — flag and refuse to offer unify.

---

### Step 4a: Unify (`u`)

User presses `u` on the model from Step 3.

```
+- Unify: mistralai/Mistral-7B-v0.3 ----------------------------------------+
|                                                                           |
| Strategy: keep one canonical copy, hardlink the others to it.             |
|                                                                           |
| Canonical:    /Users/devon/.ollama/models/blobs/sha256-8f3e...c102        |
|               (chosen from existing copies — Ollama blob is already       |
|                content-addressed, so we keep it as the canonical inode)   |
|                                                                           |
| Hardlinks to be created (replacing existing copies):                      |
|   llama-cli    /Users/devon/llms/mistral-7b-q4.gguf                       |
|   Hugging Face ~/.cache/huggingface/hub/.../mistral-7b-q4.gguf            |
|                                                                           |
| modeltap does NOT create a new copy or central store — per intake Q1.     |
|                                                                           |
| Disk reclaim: 8.8 GB     This action is non-destructive but rewrites      |
|                          file inodes. Tools currently using these files   |
|                          should be stopped first.                         |
|                                                                           |
| Running tools detected: ollama (PID 4421)                                 |
|                                                                           |
| [Enter] proceed   [n] dry-run only   [Esc] cancel                         |
+---------------------------------------------------------------------------+
```

**Shared artifacts:** `${canonical_path}` (single source of truth: the chosen existing tool-owned copy — typically the largest or the content-addressed one; modeltap does not own a central store, per intake Q1), `${hardlink_targets[]}` (per-tool, sourced from each plugin's `link_path_for(model)` method).

**Emotional state:** entry "I want to deduplicate" → exit "I see exactly what will change." Norman's principle of feedback: the user sees the plan before executing.

**Integration checkpoint:** Detection of running processes that have the file open (lsof or platform equivalent) surfaces a "close the tool and retry" prompt (per intake Q5 — detect-and-prompt-then-retry, not silent override). After unify, every link target MUST resolve via `stat`/`fstat` to the same inode as the canonical.

---

### Step 4b: Zap (`z`)

User presses `z` while a tool (not a model) is selected in the left pane.

```
+- Zap all models for: llama-cli ------------------------------------------+
|                                                                          |
| THIS WILL DELETE 6 MODELS (21.4 GB) FROM llama-cli.                      |
|                                                                          |
| Of these, 4 are ALSO registered with another tool — those tools will     |
| keep their copies. 2 are unique to llama-cli and will be permanently     |
| removed.                                                                 |
|                                                                          |
| Type the tool name to confirm:  [           ]                            |
|                                                                          |
| [Esc] cancel                                                             |
+--------------------------------------------------------------------------+
```

User must type `llama-cli` exactly. Anything else cancels.

**Shared artifacts:** `${tool.name}`, `${tool.models[]}`, `${tool.unique_models[]}`, `${tool.disk_usage}`.

**Emotional state:** entry "this needs to go" → exit "confirmed and gone." Norman's constraint principle: typed confirmation is the strongest cheap guard against fat-finger destruction.

**Integration checkpoint:** After zap, re-running discovery MUST yield zero models for that tool. The tool itself remains supported (plugin still loads); only its model files are deleted.

---

### Step 5: Verify outcome

```
+- modeltap ---------------------------------------------------------------+
| Tools                | Last action: zap llama-cli (success)              |
|   Ollama        12   |                                                   |
| > llama-cli     0    |   No models registered with llama-cli.            |
|   Hugging Face  31   |                                                   |
|   LM Studio     9    |   Reclaimed: 14.6 GB                              |
|                      |   (6.8 GB retained — also linked from other tools)|
|  Total: 52 models    |                                                   |
|  Disk: 117.0 GB      |                                                   |
|  Dedup-able: 22 GB   |                                                   |
+--------------------------------------------------------------------------+
| [<-/->] tools  [up/down] models  [u] unify  [z] zap tool  [?] help [q]   |
+--------------------------------------------------------------------------+
```

**Shared artifacts:** `${last_action.bytes_reclaimed}`, `${last_action.bytes_retained}`, `${total.disk_usage}` (recomputed).

**Emotional state:** entry "did it work?" → exit "yes, X GB reclaimed." Visibility-of-system-status (Nielsen #1).

**Integration checkpoint:** The new total disk usage MUST equal old total minus reclaimed bytes (within rounding).

## Error Paths (acknowledged)

| Failure | UX response |
|---|---|
| A plugin crashes during discovery | TUI launches anyway; that tool shows `(error: see logs)` and other tools still work |
| Hardlink creation fails (cross-filesystem) | Fall back to copy + warn, OR refuse and explain ("can't hardlink across filesystems") — DESIGN to choose |
| Tool is running and holds file open during unify | Soft warning shown in Step 4a; user can override |
| Zap confirmation typed incorrectly | Cancel and return to main view; no destructive partial state |
| File appears in inventory but is missing on disk | Show with strike-through; offer "remove from index" action |

## CLI vocabulary (consistency check)

| Concept | Term used | Never call it |
|---|---|---|
| The four supported integrations | "tool" | "client", "backend", "engine" |
| A downloaded model file/blob | "model" | "file", "weights", "checkpoint" |
| Making one file usable across tools | "unify" | "link", "share", "merge" (the keystroke is `u` for unify) |
| Deleting all models for a tool | "zap" | "purge", "clear", "wipe" (the keystroke is `z` for zap) |
| The canonical store | "store" | "registry", "vault", "library" |
| The unique-to-one-tool indicator | "format-locked" or red `!` | "incompatible", "broken" |
