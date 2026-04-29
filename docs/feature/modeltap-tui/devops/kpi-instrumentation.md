# KPI Instrumentation — modeltap-tui

**Default state (per C5 + NFR Privacy):** all instrumentation is **local-only**. No data leaves the machine. The opt-in upload path is designed in `telemetry-design.md` but not implemented in v1.

**Mechanism:** structured JSONL events appended to `~/.modeltap/launch.log` via the `tracing` + `tracing-appender` crates (Rust ecosystem standard, already a transitive dep of many commonly-used crates).

## 1. Files Written

| Path | Format | Purpose | Rotation |
|---|---|---|---|
| `~/.modeltap/launch.log` | JSONL (one JSON object per line) | KPI events: launch, action, paint timing, inventory composition | Size-based, 10 MB cap, keep 3 rotations |
| `~/.modeltap/diagnostics.log` | JSONL | Errors, panics, plugin failures, warnings | Size-based, 10 MB cap, keep 3 rotations |
| `~/.modeltap/config.toml` | TOML | Optional user-owned config (extra search paths, future preferences) | Never written by modeltap (user-managed) |

Log rotation is implemented via `tracing-appender::rolling::Builder`:

```rust
use tracing_appender::rolling::Rotation;
use tracing_appender::rolling::RollingFileAppender;

let launch_log = RollingFileAppender::builder()
    .rotation(Rotation::NEVER)  // we use size-based, see custom rotation below
    .filename_prefix("launch")
    .filename_suffix("log")
    .max_log_files(3)
    .build("~/.modeltap")
    .expect("init launch log");
```

Note: `tracing-appender::rolling` natively supports time-based rotation; size-based rotation is a small wrapper. If acceptable to DELIVER, daily rotation is fine for the diagnostics log; for the launch log, prefer size-based since sessions are bursty (a long-running interactive session could write many events). DELIVER picks the implementation; the contract here is "10 MB cap, keep 3."

## 2. JSONL Schema for `launch.log`

All events share a common envelope:

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:23:01.123Z",
  "session_id": "01HXY...",          // ULID, generated at launch, not persisted
  "event": "<event_type>",
  "modeltap_version": "1.0.0",
  "platform": "macos-aarch64",       // or linux-x86_64, linux-aarch64
  "...": "...event-specific fields..."
}
```

`session_id` is a per-launch ULID — it correlates events within one session but is regenerated on every launch and never persisted across runs. This preserves stateless-ness (ADR-003) and prevents cross-session tracking.

`platform` is `${target_os}-${target_arch}` from the binary's compile-time targets — useful when reviewing local logs on user-reported issues; trivially derivable from the binary.

### 2.1 Event types

#### `launch.started`

Emitted as the first event in every session.

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:23:01.123Z",
  "session_id": "01HXY...",
  "event": "launch.started",
  "modeltap_version": "1.0.0",
  "platform": "macos-aarch64"
}
```

#### `launch.timing` (K3)

Emitted once per session after full inventory completes.

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:23:02.245Z",
  "session_id": "01HXY...",
  "event": "launch.timing",
  "modeltap_version": "1.0.0",
  "platform": "macos-aarch64",
  "process_start_to_first_paint_ms": 142,
  "first_paint_to_all_discovered_ms": 781,
  "all_discovered_to_indicators_ms": 187,
  "full_inventory_ms": 1110,
  "model_count": 47,
  "plugin_timings_ms": {
    "ollama": 412,
    "llama-cli": 89,
    "hf": 781,
    "lm-studio": 156
  }
}
```

**KPI mapping:** K3 (`first_paint_ms`, `full_inventory_ms`).

#### `launch.inventory` (K2)

Emitted once per session after compatibility indicators computed.

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:23:02.300Z",
  "session_id": "01HXY...",
  "event": "launch.inventory",
  "modeltap_version": "1.0.0",
  "platform": "linux-x86_64",
  "total_models": 47,
  "marked_star_count": 12,        // already shared (registered with 2+ tools)
  "marked_open_count": 8,         // could be shared (1 tool now, format compatible with others)
  "marked_bang_count": 3,         // format-locked (no other tool accepts)
  "marked_question_count": 1,     // unknown (e.g., hash failed)
  "tools_registered": ["ollama", "llama-cli", "hf", "lm-studio"]
}
```

`tools_registered` lists the plugins compiled into the binary (always the same 4 in v1; useful when contributors add a 5th).

**KPI mapping:** K2 (`(marked_star_count + marked_open_count) / total_models`). The numerator is "deduplicable" per the requirements glossary.

**Privacy:** no model names, no paths, no SHA256s — only counts.

#### `action.zap_all`

Emitted after the user completes a `z` action on a tool (delete all models for that tool).

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:25:14.000Z",
  "session_id": "01HXY...",
  "event": "action.zap_all",
  "modeltap_version": "1.0.0",
  "platform": "macos-aarch64",
  "tool": "ollama",
  "models_removed": 6,
  "bytes_reclaimed": 21474836480,    // 20 GB
  "duration_ms": 3120,
  "outcome": "success"               // success | partial | error
}
```

#### `action.zap_one`

Emitted after the user deletes a single model (`d` from detail screen).

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:26:00.000Z",
  "session_id": "01HXY...",
  "event": "action.zap_one",
  "modeltap_version": "1.0.0",
  "platform": "macos-aarch64",
  "tool": "hf",
  "bytes_reclaimed": 4831838208,
  "was_shared": false,               // true if model was hardlinked across tools (no bytes reclaimed even though deleted)
  "duration_ms": 412,
  "outcome": "success"
}
```

#### `action.unify`

Emitted after the user completes a `u` action.

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:30:21.000Z",
  "session_id": "01HXY...",
  "event": "action.unify",
  "modeltap_version": "1.0.0",
  "platform": "macos-aarch64",
  "tools_unified": ["ollama", "llama-cli"],
  "bytes_reclaimed": 8589934592,     // 8 GB freed by replacing 2nd copy with hardlink
  "duration_ms": 84,
  "outcome": "success"
}
```

#### `action.unify_dry_run`

Emitted when the user previews unify without applying.

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:30:00.000Z",
  "session_id": "01HXY...",
  "event": "action.unify_dry_run",
  "modeltap_version": "1.0.0",
  "platform": "macos-aarch64",
  "tools_in_plan": ["ollama", "llama-cli"],
  "bytes_would_reclaim": 8589934592,
  "duration_ms": 12
}
```

#### `launch.ended`

Emitted at clean exit (Esc/q from main screen). Not emitted on crash or signal kill.

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:35:00.000Z",
  "session_id": "01HXY...",
  "event": "launch.ended",
  "modeltap_version": "1.0.0",
  "platform": "macos-aarch64",
  "session_duration_ms": 720000,
  "actions_count": 3
}
```

### 2.2 What is NOT in the schema (privacy)

The following are **never** logged, even locally:

- Model names, model file paths, model SHA256s
- Hugging Face repo IDs or quantization tags
- User home directory or username
- Hostname or machine identifier (other than coarse `platform` triplet)
- IP address (no network in v1; no incidental capture either)
- Timezone (timestamps are UTC; `ts` field has `Z` suffix only)

This preserves the ability to share `~/.modeltap/launch.log` in a bug report without leaking sensitive information.

## 3. JSONL Schema for `diagnostics.log`

Diagnostics use the standard `tracing` JSON output. Schema is event-shaped:

```json
{
  "ts": "2026-04-28T14:23:01.456Z",
  "level": "ERROR",
  "target": "modeltap_plugin_ollama::discover",
  "message": "permission denied reading manifest",
  "session_id": "01HXY...",
  "fields": {
    "path_redacted": true,
    "errno": 13
  }
}
```

Path values are redacted (`path_redacted: true`) — actual paths are sent only to stderr in interactive runs (so the user can see them and act), never to the log file. This is a stronger privacy default than typical "log everything" — opt-in: a future `MODELTAP_VERBOSE_LOGS=1` env var could include paths for self-debugging.

## 4. KPI Measurement Plan

| KPI | Source | Computation | Reporting cadence |
|---|---|---|---|
| K1 (bytes reclaimed) | `launch.log` events `action.zap_all`, `action.zap_one` (not unify — unify replaces with hardlink, no bytes reclaimed by zap), `action.unify` (hardlink replacement = bytes reclaimed) | Sum `bytes_reclaimed` per session; report median and p90 across sessions | First 100 users surveyed at v1 launch; quarterly review of opt-in aggregates (when v1.x telemetry ships) |
| K2 (dedupable %) | `launch.log` event `launch.inventory` | `(marked_star_count + marked_open_count) / total_models`; baseline-collect first 30 days post-v1 | Quarterly aggregate after baseline established |
| K3 (first paint latency) | `launch.log` event `launch.timing` (`first_paint_ms`); CI benchmark also gates per-PR | Median, p90 across sessions; CI gates per-PR | Per-PR (CI); quarterly trend (telemetry, when shipped) |
| K4 (community plugins) | GitHub PR count under `plugins/<name>/` | Count merged PRs that add a new plugin directory | Quarterly maintainer review |
| K5 (accidental loss) | GitHub issues tagged `accidental-loss` | Count over rolling 90-day window | On-incident + quarterly |

### 4.1 Local user access — `modeltap stats` subcommand

A `modeltap stats` subcommand reads `~/.modeltap/launch.log` and prints a summary:

```
$ modeltap stats
modeltap session statistics (last 90 days, local only)

Sessions:           42
Models discovered:  median 47, max 128
Bytes reclaimed:    median 4.8 GB / session, total 218 GB
Actions:            zap_all 3, zap_one 18, unify 22, dry_run 14

First-paint latency (K3):
  median:  142 ms
  p90:     287 ms
  worst:   1.2 s   (1 session — was the disk asleep?)

Inventory composition (K2, last launch):
  total:        47 models
  shared (*):   12 (25.5%)
  could share (o): 8 (17.0%)
  format-locked (!): 3 (6.4%)
  unknown (?):   1 (2.1%)
```

**Decision: include `modeltap stats` in v1.** Implementation cost is small (read JSONL, aggregate, print) and it gives users immediate value from the local log. If DELIVER finds it bloats walking skeleton timeline, the fallback is to defer to v1.x — but the JSONL schema is already correct so no schema change is needed at that point.

### 4.2 Maintainer access (no telemetry)

Until opt-in telemetry ships in v1.x, the maintainer measures K1/K2/K3 trends via:

1. **First-100-users survey** at v1.0.0 launch — emailed to anyone who opens an issue, tweets, or stars the repo. Single Google Form: "How much disk did you reclaim? What % of your models were marked deduplicable? Did first paint feel fast?"
2. **CI benchmark trend** for K3 — built-in trend visible in GitHub Actions artifact history.
3. **GitHub issues** for K4 (PR count) and K5 (`accidental-loss` label).

This is sufficient for the first 90 days. Telemetry upload is the next-step optimization, not a requirement.

## 5. Privacy Implementation Notes (for DELIVER)

- The launch log writer must redact paths in any `tracing` event before serialization. A `RedactPathsLayer` middleware in the tracing pipeline is the cleanest implementation.
- `session_id` is generated via `ulid::Ulid::new()` (or equivalent) — entropy-derived, not seeded by hostname/PID/time-only, to prevent cross-session correlation.
- The opt-in telemetry upload (v1.x) MUST read the local log and re-aggregate to coarser counts before upload — the local log itself MUST NOT be uploaded verbatim. See `telemetry-design.md`.
- `~/.modeltap/` directory is created with `mode 0700` (user-only). Logs are written `0600`.

## 6. Schema Versioning

The `schema` field (`modeltap.launch.v1`) is the contract for log consumers (`modeltap stats` and the future telemetry uploader). Breaking changes:

- Adding a new event type → no schema bump
- Adding a new field to an existing event → no schema bump (consumers tolerate extra fields)
- Removing or renaming a field → bump to `v2`; both versions emitted in parallel for one minor release; `v1` removed in the release after that
- Changing field semantics → bump to `v2`

The schema field is per-event, not per-file, so a log file may contain mixed `v1` and `v2` events during a transition period. The `modeltap stats` subcommand handles both.

## 7. Local Log as Debugging Aid

The schema is structured so that a user reporting a bug can attach `~/.modeltap/diagnostics.log` and `~/.modeltap/launch.log` and the maintainer can:

- See the version, platform, action sequence
- See timing breakpoints (was K3 met?)
- See plugin failures (which plugin failed; what error class)
- NOT see paths, names, or anything personally identifying

This is a deliberate design tradeoff: more aggressive logging would help debugging but breach C5. Path-redaction-by-default + verbose-opt-in is the chosen balance.

## 8. Disk Footprint Bound

Worst case: 4 log files (launch + 2 rotations + current) + 4 diagnostics = 80 MB cap on `~/.modeltap/`. Acceptable for a tool whose purpose is to free hundreds of GB.

If users complain, add `modeltap logs prune` subcommand in v1.x. Not v1 priority.
