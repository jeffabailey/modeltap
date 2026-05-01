# Shared Artifacts Registry: cross-tool-model-unify

Every `${variable}` in the journey artifacts has a single source of truth and a list of consumers. Untracked artifacts are the primary cause of the v1 bug being fixed (summary bar hardcoded `0` while the dedup classifier had its own truth).

---

## Artifact: `dedup_key_progress`

| Field | Value |
|---|---|
| Source of truth | In-process hash queue in `modeltap-core` (or wherever DESIGN places it). Conceptually: "current count and total of SHA256 computations." |
| Consumers | Status line: `Hashing N/M...` |
| Owner | DISCUSS hands off; DESIGN decides crate placement |
| Integration risk | MEDIUM — only one consumer today, but future progress UI may add more |
| Validation | Status line `N/M` matches the actual count of completed/total hashes at the moment of paint |

---

## Artifact: `dedup_able_bytes`

| Field | Value |
|---|---|
| Source of truth | `modeltap-core::logic::dedup` classifier output, summed over rows where glyph would be `=` (multiple separate inodes match) |
| Consumers | Summary bar `Dedup-able: X GB` value AND every row glyph rendered as `=` |
| Owner | `modeltap-core` |
| Integration risk | **HIGH** — this is the v1 bug. The summary bar hardcoded `"Dedup-able: 0 B"` instead of reading from the classifier. DESIGN must wire the bar to the same classifier the rows use. |
| Validation | `summary_bar.dedup_able_bytes == sum(row.size for row in rows if row.glyph == '=')` |

---

## Artifact: `unified_count`

| Field | Value |
|---|---|
| Source of truth | `modeltap-core::logic::dedup` classifier output, count of distinct dedup-keys where `>=2` paths share one inode |
| Consumers | Summary bar `Unified: N models` AND `[All Unified]` left-pane badge AND right-pane footer when `[All Unified]` is selected |
| Owner | `modeltap-core` |
| Integration risk | **HIGH** — three consumers; if any reads from a different source, they will silently disagree |
| Validation | All three displays show the same N |

---

## Artifact: `unify_plan`

| Field | Value |
|---|---|
| Source of truth | `modeltap-core::logic::plan::build_plan(canonical, mates)` — already exists |
| Consumers | Unify dialog rendering AND `actions::unify::run()` orchestrator |
| Owner | `modeltap-core` |
| Integration risk | LOW — one builder, one consumer chain |
| Validation | Plan shown in dialog matches plan applied; reclaim total in dialog == sum of bytes that will be replaced by hardlinks |

---

## Artifact: `reclaimed_bytes`

| Field | Value |
|---|---|
| Source of truth | Final JSONL event from `actions::unify::run()` — sum of bytes for successfully linked targets |
| Consumers | Success toast AND summary bar delta (`was X GB`) AND `launch.log` |
| Owner | `actions::unify::run()` |
| Integration risk | MEDIUM — partial-success path makes this a non-trivial sum; must NOT include failed targets |
| Validation | Toast value == sum of `link_ok` events; partial-success scenario covered in feature file |

---

## Artifact: `tool_share_list`

| Field | Value |
|---|---|
| Source of truth | Per-tool discovery + filesystem `stat()` inode equality check |
| Consumers | `[All Unified]` row "N tools" column AND detail screen path list AND row glyph computation (`#` requires `>=2` paths sharing one inode) |
| Owner | DESIGN decides — likely `modeltap-core` derived from per-plugin discovery output |
| Integration risk | **HIGH** — Q7 stateless rediscovery means this is recomputed each launch; must be consistent within a session |
| Validation | `len(tool_share_list[m]) == row_for(m).tool_count == len(detail_screen_paths_for(m))` for every unified model `m` |

---

## Artifact: `total_saved_by_unification`

| Field | Value |
|---|---|
| Source of truth | Derived: `sum((len(tool_share_list[m]) - 1) * size(m) for m in unified_models)` |
| Consumers | `[All Unified]` right-pane footer |
| Owner | `modeltap-core` derived value |
| Integration risk | LOW — single consumer |
| Validation | Footer value equals sum of per-row "saves" column in the same view |

---

## Artifact: row glyph (`?` `~` `-` `=` `#`)

| Field | Value |
|---|---|
| Source of truth | `modeltap-core::logic::dedup` classifier per-row output |
| Consumers | Row rendering in every left-pane selection (individual tools and `[All Unified]`) |
| Owner | `modeltap-core::logic::dedup` |
| Integration risk | **HIGH** — glyph drives the meaning of every other artifact; must be derived from the same classifier the summary bar reads from |
| Validation | Same model `m` shows the same glyph regardless of which left-pane slot is selected (excluding `[All Unified]` which only ever shows `#` rows) |

---

## Cross-Artifact Consistency Tests (handed to DISTILL)

These should appear in the master acceptance suite, NOT just unit tests:

1. **Single-source dedup**: assert `summary_bar.dedup_able_bytes == sum(row.size for row in rows if row.glyph == '=')` after hashing completes.
2. **Unified-count parity**: assert `summary_bar.unified_count == [All Unified].badge_count == count(row for row in rows if row.glyph == '#')`.
3. **Pane-switch invariance**: select tool A, note the glyph for model M; select tool B (where M also exists); glyph for M is the same.
4. **Reclaim arithmetic**: after a unify, assert `summary_bar.dedup_able_bytes_delta == toast.reclaimed_bytes` (signed).
5. **Hashing progress monotonic**: `dedup_key_progress.completed` only increases during a session (never goes backward).
