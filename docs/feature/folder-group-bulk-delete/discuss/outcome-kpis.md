# Outcome KPIs — folder-group-bulk-delete

## Feature: folder-group-bulk-delete

### Objective

Devon Park, the local-AI power user who audits many quant variants in HF repos, can delete a whole `<author>/<repo>/` folder in one keystroke + one typed confirmation — reclaiming disk, sweeping sidecars, and preserving cross-tool hardlinks — within a single TUI session.

### Outcome KPIs

| # | Who | Does What | By How Much | Baseline | Measured By | Type |
|---|---|---|---|---|---|---|
| K-FGD-1 | Devon (HF-audit user) | Completes a folder-delete from `Shift+F` press to post-action summary | p50 ≤ 15 s wall-clock; p90 ≤ 30 s for a 21-file repo (typing time included) | Today: 20+ × `[d]` + typed confirm + Enter per file (~60-180 s for 20 files, plus orphan sidecars still on disk) — measured against current US-05b loop | Local timing log: `Instant::now()` delta from `Shift+F` keypress to post-action summary render (per-action log line) | Leading (outcome) |
| K-FGD-2 | Devon (HF-audit user) | Reclaims a whole HF repo with O(1) keystrokes regardless of file count | ≤ 1 hotkey (Shift+F) + length-of-folder-path typed (~30 chars for typical `bartowski/Llama-3.2-1B-Instruct-GGUF`) + 1 Enter = ~35 keystrokes total — independent of file count | Today: ~22 keystrokes × N_files under US-05b loop (≈440 keystrokes for a 20-file repo); plus the user has no way to sweep sidecars | Keystroke count instrumented in folder-delete dialog code path | Leading (outcome) |
| K-FGD-3 | Devon (HF-audit user) | Confirms with byte-exact path match — never deletes the wrong repo | Mis-target rate < 1% of dialog opens (mismatch + abort); accidental wrong-repo deletes = 0 in the first 90 days post-release | Today: N/A (feature doesn't exist) | Local log: count typed-confirmation mismatches per dialog open; tag GitHub issues `accidental-folder-delete` | Leading (guardrail) |

### Measurement Definition for K-FGD-2 (RF-6)

"Keystroke" = total key events received by the folder-delete dialog input handler from the moment the dialog opens to the moment Enter is pressed (or Esc cancels). This includes:

- Modifier-only events that complete a shortcut (the `Shift+F` itself counts as 1 event that opens the dialog; it is NOT counted within the dialog's own keystroke total)
- All character input into the typed-confirmation field
- Corrections (Backspace, Ctrl+W word-delete)
- The final Enter

It does NOT include:

- Terminal-side preprocessing (escape codes that don't reach the application)
- Cursor-movement events (Left/Right within the input field) — these are observable but do not advance toward confirmation

Baseline comparison: under the current US-05b loop, deleting 20 files requires ~22 keystrokes per file (`[d]` + typed-id ~17 chars + Enter + Esc-back ≈ 20) × 20 files ≈ 400 keystrokes. The folder-delete target is ~35 keystrokes (Shift+F counted at the parent level, then path ~30 chars + Enter) — an order-of-magnitude reduction.

### Metric Hierarchy

- **North Star**: K-FGD-2 — keystrokes per repo delete. The whole feature exists to reduce this. The story is meaningful only because the ratio drops by an order of magnitude.
- **Leading Indicators**: K-FGD-1 (wall-clock latency confirms the keystroke reduction translates to perceived speed)
- **Guardrail Metrics**:
  - K-FGD-3 — typed-confirmation safety (must NOT degrade; 0 accidental wrong-folder deletes)
  - Parent K5 (no accidental data loss) — extended: folder-delete must not break cross-tool hardlinks for any shared file
  - Parent K3 — dialog must open within 200ms of `Shift+F` (the inventory grouping pass must not slow first paint)

### Connection to Parent Feature KPIs

This feature drives the parent's existing KPIs:

| Parent KPI | This feature's contribution |
|---|---|
| K1 (bytes reclaimed per session) | Folder-delete in one operation can reclaim 5-50 GB — the largest reclaim event in a typical session |
| K2 (% deduplicable models visible) | Folder grouping makes the cross-tool indicator (`*`) more legible within a repo context (Devon sees "1 of 21 is shared with Ollama") |
| K5 (no accidental loss) | Typed confirmation + per-file shared/unique classification + cross-tool hardlink preservation |

### Measurement Plan

| KPI | Data Source | Collection Method | Frequency | Owner |
|---|---|---|---|---|
| K-FGD-1 | `~/.modeltap/sessions.log` (opt-in, reuses parent's logging) | Each folder-delete writes a line: `{timestamp, action: folder-delete, folder_path, file_count, wall_clock_ms, typing_ms}` | Per action | platform-architect (DEVOPS) wires instrumentation; product-owner reviews aggregates |
| K-FGD-2 | Same log | Same line includes `keystroke_count` (instrumented in the dialog input handler) | Per action | platform-architect |
| K-FGD-3 | Same log + GitHub issues | Log: count of `dialog_open_count` and `mismatch_cancel_count`; issue tag `accidental-folder-delete` for survey | Quarterly aggregate; per-incident review | maintainer |

> Telemetry remains opt-in (parent feature's constraint). Default writes only to local log; uploading aggregates requires `modeltap telemetry enable`.

### Hypothesis

We believe that **a collapsible folder-header row in the right pane + a Shift+F hotkey + a typed-confirmation dialog that itemises per-file shared/unique split** for **Devon (HF-audit users)** will achieve **a p50 folder-delete time of ≤ 15 seconds and ~35 keystrokes per repo regardless of file count**.

We will know this is true when **Devon, with a 20+ file HF repo to discard, opens the folder-delete dialog and completes the operation in p50 ≤ 15 seconds with one Shift+F press, types the path, presses Enter, and sees the reclaim message — with no orphan sidecars and any cross-tool hardlinks preserved**.

### Smell Test (per KPI)

| KPI | Measurable today? | Rate not total? | Outcome not output? | Has baseline? | Team can influence? | Has guardrails? |
|---|---|---|---|---|---|---|
| K-FGD-1 | Yes (after instrumentation) | Time-bounded p50/p90 per action — acts as a rate | Yes — wall-clock is observable user experience | Yes — measured via current US-05b loop | Yes — dialog and execution speed are fully owned | Yes — K-FGD-3 ensures K-FGD-1 is not gamed by skipping confirmation |
| K-FGD-2 | Yes (keystroke instrumented in input handler) | Yes — keystrokes per action, independent of file count | Yes — keystroke count is observable behaviour | Yes — measured via US-05b loop (~22 × N_files) | Yes — entirely controlled by UX design | Yes — K-FGD-3 ensures K-FGD-2 reduction doesn't come at safety cost |
| K-FGD-3 | Yes (log + issue tracker) | Yes — % of dialog opens with mismatch; absolute count of accidental deletes | Yes — typed-confirmation correctness is observable | Yes — 0 today (feature doesn't exist) | Yes — controlled by dialog text clarity | Itself a guardrail |

All three KPIs pass the smell test. K-FGD-2 baseline is measured today (parent US-05b path is implemented), so the comparison is concrete.

### Handoff Notes for DEVOPS (platform-architect)

1. **Instrumentation**: reuse parent feature's `~/.modeltap/sessions.log` (opt-in). Add new log line schema for `folder-delete` action including `folder_path`, `file_count`, `wall_clock_ms`, `typing_ms`, `keystroke_count`, `mismatch_cancel_count_for_session`.
2. **Baseline collection**: K-FGD-2 baseline (the ~22 keystrokes × N_files figure) should be measured against the current US-05b loop in a deliberate first-week-post-release survey of 5-10 users. Not blocker; KPI target is independent of the exact baseline.
3. **Alert thresholds**:
   - K-FGD-1: dialog-open latency > 500 ms = regression alert (build CI; integrated into parent's K3 latency check)
   - K-FGD-3: any `accidental-folder-delete` issue = manual review trigger
4. **No real-time dashboards needed** — this is a local CLI tool. Quarterly aggregate of opt-in telemetry is sufficient.
