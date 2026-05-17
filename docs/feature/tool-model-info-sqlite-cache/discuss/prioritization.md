# Prioritization: tool-model-info-sqlite-cache

Release sequencing recommendation, including the critical question of how to sequence this feature against the in-flight `folder-group-bulk-delete` DELIVER roadmap.

## Release Priority

| Priority | Release | Stories | Target Outcome | KPI | Rationale |
|---|---|---|---|---|---|
| 1 | **Release 1 — Inspection** | US-21, US-22 | Devon drills into any tool or model and gets the full picture without leaving the TUI | O1 (15.5), O8 (14.0) | User-led ("Add an ability to get information... first"); highest opportunity scores; ships without architectural risk (no cache) |
| 2 | **Release 2 — Cache** | US-23, US-24, US-25, US-26 | Warm-start instant paint; provenance always visible; manual refresh in one keystroke; cache corruption never blocks launch | O2 (13.5), O5 (12.5), O3 (11.0 guardrail), O9/O10 (mandatory) | Architectural refactor with explicit safety guardrails; corruption-recovery and WAL concurrency required from day one |
| 3 | **Release 3 — SHA256 persistence** | US-27 | Re-launch does not re-hash unchanged files | O6 (11.5) | Incremental win; defer until Release 2 dogfooded; opt-in flag first |

## Backlog suggestions

| Story | Release | Priority | Outcome Link | Dependencies |
|---|---|---|---|---|
| US-21 Tool detail screen | R1 | P1 | O8 (14.0), O4 (12.0) | parent US-03, US-08, US-18 (Tool trait extension Q-INFO-1) |
| US-22 Model detail with metadata | R1 | P1 | O1 (15.5), O8 (14.0) | parent US-13 (extends), US-18 (Tool trait extension Q-INFO-1) |
| US-23 Cache schema + recovery + WAL | R2 | P2 | O9, O10 (mandatory); enables O2 | new `modeltap-store` crate |
| US-24 Manual refresh hotkeys + provenance | R2 | P2 | O5 (12.5) | US-23 (cache exists), US-08 (bottom bar) |
| US-25 Warm-start cache read | R2 | P2 | O2 (13.5) | US-23 |
| US-26 Background reconcile + revalidation rule | R2 | P2 | O3 (11.0 guardrail); supports O2 | US-23, US-25 |
| US-27 SHA256 persistence | R3 | P3 | O6 (11.5) | US-23, ADR-013 (background hash pool) |

---

## Sequencing question: does this feature pause `folder-group-bulk-delete` (in-flight DELIVER)?

The folder-group-bulk-delete DELIVER roadmap is approved (62 hours, 6 phases) and currently in progress. This feature is in DISCUSS. Several plausible options.

### Option A — Pause folder-group-bulk-delete, ship this feature first

**Pros:** This feature is architecturally larger (reverses ADR-003) and benefits from being designed before more parent-feature stories accrete on top of the stateless model.

**Cons:**
- 62h of work is mid-flight; pausing creates context loss for the developer (solo) and risks merge conflicts when work resumes.
- `folder-group-bulk-delete` is right-sized as 1 story and delivers immediate K-FGD-1/2/3 value (Devon's most painful current friction — the 21-keystroke HF folder cleanup).
- Halting an approved roadmap mid-execution is exactly the kind of context-switch tax this team's DELIVER discipline is built to avoid.

### Option B — Run in parallel (folder-group continues; this feature DISCUSS/DESIGN happens alongside)

**Pros:**
- Solo developer can context-switch between DELIVER work (folder-group, mechanical implementation per the roadmap) and DISCUSS/DESIGN thinking (this feature, deeper) — different cognitive modes.
- This feature's DISCUSS + DESIGN waves take days/weeks and do not require code changes; the parallel work is documentation + ADR.
- Folder-group's DELIVER artifacts (folder-grouping logic in HF plugin, `[F]` hotkey) are orthogonal to this feature's surface area — no merge conflict risk.

**Cons:**
- Solo developer at risk of context fragmentation if they actually try to write code for both simultaneously.
- This feature's DESIGN ADR (cache architecture, ADR-003 supersession) is a substantial deliverable that needs focused attention.

### Option C (RECOMMENDED) — Queue this feature behind folder-group; complete DISCUSS now while folder-group DELIVER continues

**The recommended sequence:**

1. **Complete this DISCUSS wave** (current sprint). Outputs: requirements, stories, journey, KPIs, ADR-pending. Locks in the design space and constraint reversal so DESIGN can start the moment folder-group DELIVER finishes.
2. **Continue folder-group-bulk-delete DELIVER to completion** (~remaining of the 62h roadmap; mechanical implementation work). Don't pause it.
3. **Start this feature's DESIGN wave after folder-group merges** — context is fresh from DELIVER experience (validates that the stateless model is in fact painful for warm-start scenarios), and the ADR that supersedes ADR-003 can reference the now-shipped state.
4. **DEVOPS / DISTILL / DELIVER for this feature** then run as a normal sequence.

**Pros:**
- DISCUSS work (this current activity) does not require code changes — zero conflict with in-flight DELIVER.
- Folder-group DELIVER continues on schedule.
- This feature's DESIGN starts with fresh context informed by the just-shipped folder-group work.
- No half-committed state on either feature.

**Cons:**
- This feature ships ~62h later than Option B's hypothetical parallel-DESIGN path.
- The user must accept that the inspection feature (which they asked for) won't be in their hands for ~3-4 weeks (folder-group DELIVER completion + this feature's design/devops/distill/deliver).

**Rationale for recommendation:**

This is a solo-developer codebase. The cognitive cost of holding two unfinished features open is real. The parent feature's DELIVER discipline (per CLAUDE.md "rust-idiomatic, multi-paradigm" guidance + the prior wave artifacts) shows the team values "finish one thing before starting the next."

Furthermore, the constraint reversal of ADR-003 is a serious decision that benefits from being designed against the *as-shipped* state of the parent feature, not the *as-currently-being-built* state. Folder-group's DELIVER will land changes to the HF plugin (folder-grouping, sidecar enumeration) that this feature's `inspect_tool()` for HF will want to introspect. Designing this feature's HF inspection before folder-group ships risks under-specifying the surface.

**Action items if Option C is accepted:**

- This DISCUSS wave runs to completion now (in progress).
- Folder-group DELIVER continues per its approved 62h roadmap.
- This feature's DESIGN wave is queued for "after folder-group v0.3.0 release" — estimated 2-3 weeks out.
- The user is notified of the sequencing and given the chance to override (e.g., if they prefer Option B parallel mode).

**Action items if Option B (parallel) is accepted:**

- This DISCUSS wave runs to completion now.
- This feature's DESIGN wave starts immediately upon DISCUSS completion.
- Folder-group DELIVER continues; developer time-slices between DELIVER mechanical work and DESIGN thinking.
- Schedule a checkpoint after 2 weeks to verify neither stream is stalling the other.

**Action items if Option A (pause) is accepted:**

- Folder-group's last completed phase is committed and the next-phase work is documented as "paused at phase N."
- This feature runs DISCUSS → DESIGN → DEVOPS → DISTILL → DELIVER (estimated 5-6 weeks for a feature of this size).
- Folder-group resumes after this feature ships.

---

## My recommendation: **Option C (queue this feature behind folder-group)**

The user can override. The recommendation is informed by:

1. Solo developer + cognitive cost of fragmentation.
2. ADR-003 supersession benefits from designing against shipped state.
3. Folder-group's HF plugin changes are inputs to this feature's HF inspection scope.
4. The user's intake brief explicitly flagged this as Open Question #2 ("Does this gate / pause `folder-group-bulk-delete`?") and asked for a recommendation — Option C is the lowest-risk recommendation that honours the approved roadmap.

---

## Schema versioning strategy (from intake Open Question #3)

DESIGN owns the final choice, but this DISCUSS wave makes the recommendation:

**Recommendation: `rusqlite_migration` crate** (minimal dep, idiomatic, well-maintained).

| Strategy | Pros | Cons | Verdict |
|---|---|---|---|
| `rusqlite_migration` | Minimal dep (~50 lines of code wrapping rusqlite); embeds SQL in the binary; forward-only migrations match our needs; no async runtime change | One more dep to audit | **RECOMMENDED** |
| Hand-rolled SQL + custom migrator | Zero dep; full control; can match project's "lean dependencies" preference | We write what `rusqlite_migration` already does; reinvented wheel; risk of subtle bugs in version-comparison and migration-order logic | Acceptable fallback if DESIGN rejects the dep on principle |
| `sqlx::migrate!` | Industry-standard migration framework | Requires switching from `rusqlite` to `sqlx`, which has a different sync/async story; sqlx's compile-time SQL checking pulls a DATABASE_URL into the build — friction for CI; bigger dep footprint | Rejected — disproportionate to needs |

**Migration file layout (recommended):**

```
crates/modeltap-store/migrations/
  0001_initial.sql                  # creates cache.meta, cache.tools, cache.models tables
  0002_add_sha256_persistence.sql   # adds cache.sha256 table for US-27
  0003_add_metadata_kv.sql          # adds cache.models.metadata_kv JSON column for US-22 follow-up
```

**Versioning:** `PRAGMA user_version = N` where N is the highest applied migration number. Binary's `EXPECTED_SCHEMA_VERSION` constant = highest migration shipped in this binary.

**Forward-only:** No down migrations. Schema changes are additive (new tables, new nullable columns); destructive changes are deferred or accompanied by a corruption-recovery-style rebuild.

**Backwards-compat with hypothetical pre-v1 cache files:** None should exist; if a user has a `cache.sqlite` from a dev build (intake Q3), the migration framework treats it as version-mismatch → recovery → cold-start. Documented in the corruption-recovery banner.

---

## Refresh policy summary (closed in `requirements.md`)

- **On launch:** warm-start paint from cache; background reconcile per-tool in parallel (existing ADR-003 pipeline reused).
- **TTL per-tool:** default 24 h; entries older than TTL are NOT painted from cache (cold-start for that tool).
- **Manual:** `[r]` refreshes selected tool; `[Shift+R]` refreshes all.
- **Mid-session:** no automatic re-reconcile during a session; user-driven via manual refresh.
- **Pre-mutate:** every destructive action re-stats target files against cache; drift → re-introspect; gone → abort + refresh.
- **Cache disabled:** `--no-cache` CLI or `cache.enabled = false` config; behaves exactly as ADR-003.

---

## KPI deltas from existing modeltap-tui

| Parent KPI | Status under this feature |
|---|---|
| K1 (bytes reclaimed per session) | Unchanged target; this feature may incrementally improve K1 indirectly because inspection + dedup confidence drives more unify usage |
| K2 (% deduplicable models marked) | Unchanged target |
| **K3 (first paint < 1 s)** | **REDEFINED** — see new KPI `K-INFO-3` in `outcome-kpis.md`. Splits into K3a (warm-start ≤100 ms) and K3b (cold-start ≤150 ms skeleton + ≤1.15 s full inventory, unchanged from ADR-003) |
| K4 (community plugin count) | Unchanged target; trait extension Q-INFO-1 may add friction for contributors — must be designed to minimise impact |
| K5 (zero accidental data loss) | Unchanged target; **new guardrail invariant** — cache must never cause K5 regression via stale-data action (US-26 pre-mutate revalidation rule) |
