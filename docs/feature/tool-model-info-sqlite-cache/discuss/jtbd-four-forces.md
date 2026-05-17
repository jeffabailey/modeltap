# Four Forces Analysis — tool-model-info-sqlite-cache

Each job from `jtbd-job-stories.md` analysed for Push (current frustration) / Pull (new attraction) / Anxiety (fear of switching) / Habit (inertia of staying). Switch happens when Push + Pull > Anxiety + Habit. Where the balance is tight, the design implication is called out.

The persona is Devon Park throughout (parent feature's primary user). No interviews were conducted for this analysis — forces are inferred from intake brief, parent feature artifacts, and the user's verbatim request. Confidence: **team-estimate**, refinable after first-week-post-release user-cohort survey.

## J1 — Verify a model is what I think it is

| Force | Strength | Description |
|---|---|---|
| **Push** | HIGH | US-13 model detail screen exists but is shallow (paths + dedup key + reclaim estimate). Devon currently context-switches to `gguf-dump`, `huggingface-cli scan-cache`, `file` to answer "what quant is this exactly?" — every detail-screen open is a friction point. |
| **Pull** | HIGH | A detail view that surfaces tool-native introspection (Ollama manifest fields, GGUF KV pairs, HF `manifest.json`/`config.json` excerpts, computed SHA256) means Devon never leaves the TUI for routine inspection. |
| **Anxiety** | MEDIUM | Detail view that takes >2 seconds to render (because every open triggers fresh introspection + SHA256) breaks the flow worse than leaving the TUI. ALSO: detail view that shows stale metadata (cache says Q4_K_M, disk says Q5_K_M after a re-download) is worse than no metadata. |
| **Habit** | LOW | Devon's current habit of running `gguf-dump` is friction, not comfort. He'll switch the first time the in-TUI detail view loads instantly with the same info. |

**Balance:** Push + Pull strongly exceeds Anxiety + Habit. Highest-priority job.

**Design implications:**
- Detail view MUST load from cache when available (sub-100ms).
- Cache MUST show provenance ("introspected at <timestamp>") so Devon knows freshness.
- Detail view MUST offer a manual refresh shortcut for "re-introspect this model now" (covered by J4 globally).

---

## J2 — Audit a tool's health and inventory cost at a glance

| Force | Strength | Description |
|---|---|---|
| **Push** | MEDIUM | "(error)" annotation per US-02 routes Devon to `~/.modeltap/diagnostics.log` in a different terminal pane. Per-tool detail collapses 3-4 separate commands into one keystroke. |
| **Pull** | MEDIUM-HIGH | One-keystroke per-tool dashboard with install path, version, model count, disk usage, last-scan-time, last-error, configured search paths. |
| **Anxiety** | MEDIUM | Version detection is fragile (Ollama HTTP, llama-cli binary metadata, LM Studio Electron about-box). False or missing version data feels less competent than no version data. Show only what's reliably detectable. |
| **Habit** | MEDIUM | Devon's `ls`/`du`/`ollama --version` muscle memory is real. Tool detail view replaces these but they're cheap habits to break. |

**Balance:** Push + Pull > Anxiety + Habit. Strong second-priority job.

**Design implications:**
- Version field is optional per tool (omit gracefully when undetectable).
- Last-scan timestamp is mandatory — it's the freshness contract.
- Detail view MUST show the configured search paths so Devon can sanity-check "is modeltap scanning the right place?"

---

## J3 — Trust modeltap's first-paint inventory across launches

| Force | Strength | Description |
|---|---|---|
| **Push** | MEDIUM | Stateless rediscovery's skeleton-first paint (~150 ms to placeholders, ~1.15 s to full inventory) is honest. But for a tool opened 10+ times/day, cumulative loading-state-time is real. Devon's hands wait on keystrokes. |
| **Pull** | HIGH | Sub-100 ms paint of *real* inventory from persisted cache. Background reconcile updates the view if anything changed since last launch. |
| **Anxiety** | HIGH | Stale cache acted-on (unify dialog built from cache; file gone) breaks K5 (no accidental data loss). This is the dominant fear. ADR-003 explicitly rejected persistent indexes partly on these grounds. |
| **Habit** | HIGH | Devon believes modeltap is stateless (ADR-003 is documented, he reads docs). Reversing this is a contract change. He needs to *see* that the cache is provenance-stamped and that destructive actions still revalidate before acting. |

**Balance:** Push + Pull approximately equals Anxiety + Habit. **This is the most contested job.** The reversal of ADR-003 succeeds or fails on how convincingly the design addresses Anxiety + Habit.

**Design implications (the hard ones):**
- Cache must be **paint-only**; destructive actions (unify, zap, delete-one, folder-delete) MUST re-stat the target file before mutating. Cache is for visibility; truth is the filesystem.
- Provenance display: "as of 14 minutes ago, refreshing..." line under the summary bar. Always-visible.
- Background reconcile on every launch (not on-demand): cache is *seed*, not *source of truth*.
- Per-tool TTL: cache entries older than 24 hours are skipped on paint (re-discovered before display). Configurable in `~/.modeltap/config.toml`.
- Hard escape hatch: `modeltap --no-cache` flag bypasses the cache for a launch (returns to ADR-003 behaviour) and is the recovery action if cache misbehaves.
- New ADR superseding Q7 must explicitly enumerate when cache is authoritative vs. consulted vs. ignored.

---

## J4 — Manual refresh for one tool or globally

| Force | Strength | Description |
|---|---|---|
| **Push** | MEDIUM | Quit + relaunch is the current refresh mechanism. ~3 s penalty per refresh. Cumulative across a download session (Devon may `ollama pull` 3 models and re-check 3 times). |
| **Pull** | MEDIUM | One-keystroke `[r]` refresh of the selected tool (Shift+R for all tools). Sub-second for selected tool. |
| **Anxiety** | LOW | Refresh during in-flight action would be confusing — disable `[r]` when a dialog is open. Otherwise safe (just re-runs discovery). |
| **Habit** | LOW | Quit+relaunch is friction, not comfort. Devon will adopt `[r]` immediately if it's in the bottom bar. |

**Balance:** Push + Pull > Anxiety + Habit. Low-effort high-multiplier on J3.

**Design implications:**
- `[r]` is the per-tool refresh; `Shift+R` is global refresh.
- Both update the provenance display ("as of just now").
- Both are no-ops when a dialog is open.

---

## J5 — Persist SHA256 hashes across launches

| Force | Strength | Description |
|---|---|---|
| **Push** | MEDIUM | First unify of the day is slow because the background hash pool (ADR-013) has to re-hash everything. For users with 50+ GB libraries, this is a real wait. |
| **Pull** | MEDIUM-HIGH | Persisted `(path, mtime, size) → SHA256` cache means hashes survive launches. Second-launch unify dialog is instant. K1 (bytes reclaimed) benefits indirectly because slow dedup grouping discourages exploration. |
| **Anxiety** | HIGH | mtime-preserving file replacement (`cp --preserve=timestamps` of a swapped file) lets stale hashes through. For unify (which decides what gets hardlinked), this could destroy data. ADR-003 alternative B was rejected specifically on this concern. |
| **Habit** | LOW | Users don't think about SHA256 caching; they feel "first unify is slow." Transparent benefit. |

**Balance:** Push + Pull ≈ Anxiety + Habit. Adoption depends on how anxiety is addressed.

**Design implications:**
- Cache validity check is `(path, mtime, size, inode_dev)` — not just mtime+size. inode_dev change defeats most accidental cases.
- Pre-unify, MUST re-stat the file. If mtime+size+inode_dev still match the cache entry, the cached hash is used. If any drift, rehash before acting.
- Add `modeltap cache verify` developer command that rehashes everything and compares against cache (drift detection).
- Default: SHA256 cache is **on**. Opt-out via `modeltap --no-hash-cache` or `config.toml`.
- Document in NEW ADR that the safety floor is "cache is consulted, filesystem is authoritative on mutate."

---

## J6 — Recover from a corrupted or unreadable cache

| Force | Strength | Description |
|---|---|---|
| **Push** | LOW (today) but HIGH (after introducing the cache) | Currently no cache means no failure mode. After J3/J5 ship, a corruption failure mode is created. Without explicit recovery, it becomes "modeltap won't launch." |
| **Pull** | HIGH | Auto-detect corruption, rename to `.corrupt-<timestamp>`, log, proceed with empty cache. Cache becomes pure-upside. |
| **Anxiety** | MEDIUM | Silent recovery hides real problems (disk failure). Recovery MUST log clearly and surface to the user on the next launch ("dropped corrupted cache; see log"). |
| **Habit** | N/A | New failure mode, no habit to displace. |

**Balance:** Push + Pull >> Anxiety + Habit. **Mandatory** guardrail story — this job MUST be solved before the cache ships, not after.

**Design implications:**
- SQLite open uses `PRAGMA integrity_check` on launch (in dev mode; in production rely on the open-time error).
- On `SQLITE_CORRUPT` or schema-version-with-no-migration, rename file and proceed with empty.
- Log line schema includes the failure reason.
- Next-launch warning banner: "Previous cache file was corrupted and reset; see ~/.modeltap/diagnostics.log."

---

## J7 — Concurrent modeltap processes share the cache safely

| Force | Strength | Description |
|---|---|---|
| **Push** | LOW (until it bites) | A naive SQLite open (rollback journal) would let process A's commit hose process B's reader. Currently no shared state so no problem. |
| **Pull** | HIGH | WAL mode + busy timeout is the standard answer; both processes work. |
| **Anxiety** | LOW | Each process sees the cache state at the time of its own background-refresh. Acceptable. |
| **Habit** | HIGH | Multi-pane terminal usage is universal. Users will hit this on day one. |

**Balance:** Push + Pull > Anxiety + Habit, with Habit forcing the issue (users will run two instances day-one).

**Design implications:**
- SQLite opens with `PRAGMA journal_mode=WAL` and `PRAGMA busy_timeout=5000`.
- Each process's background refresh is independent; "stale" reads are acceptable because filesystem revalidation gates destructive actions (J3 design implication).
- No file locking, no PID detection — matches parent intake Q5 ("detect-and-prompt-then-retry" for tool conflicts).

---

## Aggregate force balance

| Job | Push | Pull | Anxiety | Habit | Net (P+Pl vs A+H) |
|---|---|---|---|---|---|
| J1 — Verify a model | HIGH | HIGH | MEDIUM | LOW | **Strongly positive** |
| J2 — Audit a tool | MEDIUM | MEDIUM-HIGH | MEDIUM | MEDIUM | **Positive** |
| J3 — Trust first-paint | MEDIUM | HIGH | HIGH | HIGH | **Contested** (design-critical) |
| J4 — Manual refresh | MEDIUM | MEDIUM | LOW | LOW | **Positive** |
| J5 — Persist SHA256 | MEDIUM | MEDIUM-HIGH | HIGH | LOW | **Contested** (design-critical) |
| J6 — Cache corruption recovery | HIGH (post-cache) | HIGH | MEDIUM | N/A | **Strongly positive** (mandatory) |
| J7 — Concurrent processes | LOW | HIGH | LOW | HIGH | **Positive** (table-stakes) |

## Cross-job design rules surfaced

These rules emerged from multiple jobs simultaneously — they must be honoured by the eventual DESIGN ADR:

1. **Cache is paint-only; filesystem is authoritative on mutate.** (J3, J5) Destructive actions MUST re-stat / re-introspect / re-hash before acting.
2. **Provenance is always visible.** (J3, J4) The "as of <timestamp>" line lives in the summary bar; users always know how fresh their view is.
3. **Refresh is one keystroke, never a relaunch.** (J3, J4) `[r]` per-tool, `Shift+R` global.
4. **Cache failure ≠ launch failure.** (J6) Corruption recovery is a v1 requirement, not v1.x.
5. **No file locking, no PID detection.** (J7) Match parent intake Q5; lean on SQLite WAL + busy timeout.
6. **Per-tool TTL is configurable.** (J3) Default 24 h; users with high-churn libraries can shorten it.
7. **Opt-out flag mandatory.** (J3, J5) `--no-cache` returns to ADR-003 behaviour. The cache is an optimisation, never a dependency.

These rules drive the requirements in `requirements.md` and the acceptance criteria in `acceptance-criteria.md`.
