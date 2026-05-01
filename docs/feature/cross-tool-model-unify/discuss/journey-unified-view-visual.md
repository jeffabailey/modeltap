# Journey: See What Is Already Unified (Visual)

**Persona**: Devon
**Goal**: Confirm at a glance which models are already shared across tools (one inode, multiple consumers), how many tools each is shared with, and how many bytes that's saving.
**Trigger**: Devon wants reassurance after a unify, OR is auditing months later, OR is debugging "did that thing actually work?"
**Success criteria**:
- One click/keystroke surface that lists all unified models.
- Each row shows: model name, size, list of tools sharing the inode, savings.
- The count in the surface matches the count in the summary bar.

---

## UX Decision: Pseudo-Tool Slot in Left Pane

**Chosen mechanism**: A pseudo-tool entry in the left pane labeled `[All Unified]` with a count badge.

**Why this over the alternatives**:

| Option | Verdict | Reason |
|---|---|---|
| Separate screen behind hotkey | NO | New mental location; new keybinding to teach; user wonders "where am I now?" |
| Filter toggle on main view | NO | Modal state; user can forget the toggle is on; right-pane content meaning becomes ambiguous |
| New pseudo-tool slot `[All Unified]` | YES | Reuses the existing left-pane navigation Devon already has muscle memory for; zero new keybindings; the right pane's meaning is still "models in the selected tool/group" — just with a different selector |
| Right-pane re-group by dedup-key | NO | Changes the meaning of the right pane based on selection; introduces a third "kind of thing" the right pane displays |

The pseudo-tool slot is the **least surprising** answer: navigating with `j/k` lands on it like any other tool; pressing `Enter` or just having it selected populates the right pane with unified models.

---

## Emotional Arc

```
START                 MIDDLE                              END
-----                 ------                              ---
Curious/Auditing      Engaged                             Confident
("did unify           ("yes, here's the list,             ("everything that
 actually do          5 models, total 23.7 GB             should be unified
 anything?")          saved")                             is unified.")
```

Pattern: **Discovery Joy** (Curious -> Exploring -> Delighted).

---

## Flow Diagram

```
[Trigger] Devon wants to audit unified models
    |
    | feels: curious / mild auditor energy
    |
    v
[Step 1] Devon navigates left pane to [All Unified]
    |
    | sees: left-pane focus moves to [All Unified] (3)
    |       right pane re-populates with only unified models
    | feels: engaged
    |
    v
[Step 2] Devon scans the unified list
    |
    | sees: each row: name, size, "shared with: <tool list>", "saves: X GB"
    |       footer: "Total saved by unification: 23.7 GB across 5 models"
    | feels: confident (concrete, factual, scannable)
    |
    v
[Step 3] Devon presses Enter on a row (optional)
    |
    | sees: detail screen for that model showing inode + all paths sharing it
    | feels: validated (proof — the inode IS the same)
```

---

## TUI Mockups

### Step 1 — Left pane navigation lands on [All Unified]

```
+- modeltap ---------------------------------------------- v0.2.0 -+
| TOOLS              | UNIFIED MODELS                               |
|   ollama       (5) | llama-3.1-8b-Q4_K_M  4.7 GB  3 tools  9.4 GB|
|   lm-studio    (4) | phi-3-mini           2.3 GB  2 tools  2.3 GB|
|   hf-cache     (8) | nomic-embed-v1.5     274 MB  4 tools  822 MB|
|   atomic-chat  (2) | qwen2-7b-instruct    4.4 GB  2 tools  4.4 GB|
| > [All Unified] (5)| mistral-7b-v0.3      4.1 GB  3 tools  8.2 GB|
|                    |                                              |
+--------------------+---------------------------------------------+
| Unified: 5 models | Total reclaimed by unification: 25.1 GB      |
+------------------------------------------------------------------+
| [j/k]nav [Enter]details [d]delete [q]quit                       |
+------------------------------------------------------------------+
```

Column meanings:
- `name` — model display name
- `size` — actual file size (one-inode size, since they share)
- `N tools` — how many tools currently share this inode
- `saves` — `(N-1) * size` — how much disk this unification has saved vs. having N copies

### Step 3 — Optional detail (Enter on row)

```
+- modeltap ---------------------------------------------- v0.2.0 -+
| Detail: llama-3.1-8b-Q4_K_M                                      |
| sha256: e5c1...9af2                                              |
| size:   4.7 GB                                                   |
| inode:  4521977 (shared)                                         |
|                                                                  |
| Paths sharing this inode:                                        |
|   * ollama       ~/.ollama/models/blobs/sha256-e5c19af2          |
|   * lm-studio    ~/.cache/lm-studio/.../Q4_K_M.gguf              |
|   * hf-cache     ~/.cache/huggingface/hub/.../Q4_K_M.gguf        |
|                                                                  |
| Saves vs. separate copies: 9.4 GB                                |
|                                                                  |
| [Esc] Back  [d] Delete from one tool  [u] Add another tool       |
+------------------------------------------------------------------+
```

(The `[d]` and `[u]` actions on the detail screen reuse existing handlers — `delete_one` per ADR-009 for `[d]`, and the unify handler with this row's mates pre-loaded for `[u]`.)

---

## Empty State

If no models are unified:

```
+- modeltap ---------------------------------------------- v0.2.0 -+
| TOOLS              | UNIFIED MODELS                               |
|   ollama       (5) |                                              |
|   lm-studio    (4) |   No models are unified yet.                 |
|   hf-cache     (8) |                                              |
|   atomic-chat  (2) |   Navigate to a tool, find a row marked      |
| > [All Unified] (0)|   "=", and press [u] to unify it.            |
|                    |                                              |
|                    |   Models marked "=" can save you disk.       |
+--------------------+---------------------------------------------+
| Dedup-able: 8.4 GB | Unified: 0 models | Total: 47.3 GB           |
+------------------------------------------------------------------+
```

Empty state explains what will appear and how to populate it (per emotional-design skill empty-state checklist).

---

## Shared Artifacts

- `${unified_count}` — left-pane badge AND summary bar AND right-pane footer; source: `core::logic::dedup` classifier
- `${total_saved_by_unification}` — sum of `(N-1) * size` over all `#` rows; source: `core::logic::dedup` derived value; consumed by footer
- `${tool_share_list}` — for each unified model, the list of tools currently sharing the inode; source: per-tool discovery + inode-equality check; consumed by row "N tools" column AND detail screen path list

---

## Integration Checkpoints

| Checkpoint | Validates |
|---|---|
| C6: `[All Unified]` count = number of right-pane rows when `[All Unified]` is selected | Left/right pane counts agree |
| C7: Row's "N tools" count = length of `${tool_share_list}` for that row | Row data is internally consistent |
| C8: Footer "Total reclaimed: X GB" = sum of per-row "saves" column | Aggregation is correct |
| C9: Same model appears with the same `# tools` count whether viewed under `[All Unified]` or under any of the individual tool slots that share it | Single source of truth across views |
