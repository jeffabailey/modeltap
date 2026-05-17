# Outcome KPIs — tool-model-info-sqlite-cache

## Feature: tool-model-info-sqlite-cache

### Objective

Devon Park, the multi-tool local-AI power user, can drill into per-tool and per-model details inside the TUI to confirm metadata before acting, AND open modeltap dozens of times per day with sub-100ms warm-start paint instead of paying the full discovery cost every launch — without ever regressing the parent K5 (zero accidental data loss) safety guardrail.

### Outcome KPIs

| # | Who | Does What | By How Much | Baseline | Measured By | Type |
|---|---|---|---|---|---|---|
| K-INFO-1 | Devon (any user, warm-start launch) | Sees the inventory paint after launching modeltap when a valid cache exists | p50 ≤ 80 ms, p90 ≤ 150 ms from process start to first paint | Parent K3: ~150 ms skeleton + ~1.15 s full inventory (every launch) | Built-in startup timing log (`Instant::now()` delta from process start to first `terminal.draw()`); written to `~/.modeltap/launch.log` | Leading (outcome) |
| K-INFO-2 | Devon (mid-session) | Refreshes the selected tool's inventory after an out-of-band change | p50 ≤ 500 ms, p90 ≤ 1 s wall-clock from `[r]` press to "as of just now" provenance | Today: quit + relaunch ~1.15 s plus context-switch tax | Per-action log line `refresh tool=<id> wall_clock_ms=<n>` | Leading (outcome) |
| K-INFO-3 | Devon (warm-start launch) | Has cache entries that are still in-TTL and painted from cache | ≥ 80% of tool entries within TTL for active users (≥ daily usage) | Today: 0% (no cache exists) | `cache.tools` row last_scan_at vs current time at warm-paint dispatch; per-launch metric | Leading (secondary) |
| K-INFO-4 | Devon (any user) | Recovers cleanly from a corrupted cache without "modeltap won't start" | 100% of detected corruption events result in successful cold-start fallback + recovery banner | Today: N/A (no cache → no failure mode) | Issue tracker tag `cache_recovery_failed`; target 0 reports | Leading (guardrail — MANDATORY) |
| K-INFO-5 | Devon (any user investigating "(error)") | Resolves a tool's discovery error without leaving the TUI | ≥ 80% of "(error)" investigations end with the user fixing the cause and running `[r]` from the tool detail screen | Today: 0% (tool detail screen does not exist; user goes to `diagnostics.log` in a separate terminal) | Self-reported via opt-in `~/.modeltap/sessions.log` action sequences: `(error)_seen` → `tool_detail_opened` → `[r]_pressed` → `error_cleared` | Leading (outcome) |
| K-INFO-6 | Devon (any user with detail screen open) | Acts decisively (unify, delete, or dismiss-confidently) from the detail screen without leaving the TUI to look up metadata | ≥ 90% of detail-screen opens result in a downstream action OR a confident `[Esc]` (no follow-up `gguf-dump` / `huggingface-cli scan-cache` in a separate terminal within 1 minute) | Today: detail screen lacks tool-native metadata; quantifiable fraction of detail-screen opens require external lookup (Devon's anecdotal: "most of them") | Hard to instrument across processes; proxy = self-reported survey at end of first month post-release | Leading (outcome) |
| K-INFO-7 | Devon (any user, warm-start launch) | Pays cache-layer overhead at launch | ≤ 50 ms additional startup time vs. cold-start-only for cache reads + open + WAL setup | Baseline = pure ADR-003 cold-start time | `Instant::now()` delta around `Cache::open()` + initial read; logged to `~/.modeltap/launch.log` | Leading (guardrail — cache must be an optimisation, not a tax) |
| K-INFO-8 | Devon (any user with large library, unify session) | Opens the unify dialog without waiting for a SHA256 compute (Release 3, post-US-27) | ≥ 70% of unify dialogs open with all dedup keys present (no "computing..." placeholders) when `[cache] persist_sha256 = true` | Today: 0% on first unify of session; varies after background hash pool warms up | Per-action log line `unify_dialog_open hashes_ready=<n>/<total>` | Leading (outcome, Release 3) |

### KPI deltas from parent `modeltap-tui` KPIs

| Parent KPI | Status under this feature |
|---|---|
| K1 (bytes reclaimed per session, ≥ 5 GB median) | **Unchanged target.** Indirectly improved by US-22 (confidence to unify) and US-27 (SHA256 ready means unify dialog opens faster). |
| K2 (% deduplicable models marked ≥ 30%) | **Unchanged target.** No direct interaction. |
| **K3 (first paint < 1 s)** | **REDEFINED — split into K3a + K3b.** K3a (warm-start) = K-INFO-1 (≤ 100 ms p90); K3b (cold-start) = unchanged parent target (≤ 150 ms skeleton + ≤ 1.15 s full inventory). Both must pass CI. |
| K4 (community plugin count, ≥ 1 in 6 months) | **Unchanged target.** Trait extension (Q-INFO-1) MUST default-impl `NotSupported` to avoid contributor friction. |
| **K5 (zero accidental data loss in 90 days)** | **Unchanged target — NEW guardrail extends.** Pre-mutate revalidation (US-26) MUST never let stale cache data drive a destructive action. Verified by integration tests. |

### Metric Hierarchy

- **North Star**: K-INFO-1 (warm-start first paint ≤ 100 ms p90). This is the single most observable change for the user — modeltap goes from "always loading" to "instantly there."
- **Leading Indicators**:
  - K-INFO-3 (cache hit ratio ≥ 80%) — supports K-INFO-1.
  - K-INFO-2 (manual refresh ≤ 500 ms p50) — supports user trust in the cache.
  - K-INFO-5 + K-INFO-6 (in-TUI resolution rate) — supports J1 + J2 user-visible value.
- **Guardrail Metrics**:
  - K-INFO-4 (cache corruption recovery 100%) — MANDATORY; cache must never be load-bearing for correctness.
  - K-INFO-7 (cache layer overhead ≤ 50 ms) — MANDATORY; cache must be optimisation, not tax.
  - K5 (parent — zero accidental data loss) — extends to cache-driven scenarios.
  - K3b (cold-start performance) — must not regress.
- **Release 3 / future indicator**: K-INFO-8 (unify dialog hash readiness ≥ 70%).

### Measurement Plan

| KPI | Data Source | Collection Method | Frequency | Owner |
|---|---|---|---|---|
| K-INFO-1 | `~/.modeltap/launch.log` (opt-in, reuses parent's logging) | `Instant::now()` deltas at: process start → cache_open_complete → first_paint | Per launch | platform-architect (DEVOPS) wires; product-owner reviews |
| K-INFO-2 | `~/.modeltap/sessions.log` (opt-in) | Per-action log line `refresh tool=<id> scope=tool\|all wall_clock_ms=<n>` | Per refresh | platform-architect |
| K-INFO-3 | `~/.modeltap/launch.log` | At warm-paint dispatch, log `warm_paint tools_in_ttl=<n> tools_total=<m>` | Per launch | platform-architect |
| K-INFO-4 | Issue tracker (`cache_recovery_failed` tag); `~/.modeltap/diagnostics.log` | Manual issue review + log scrape; target 0 reports | Quarterly; per-incident | maintainer |
| K-INFO-5 | `~/.modeltap/sessions.log` (opt-in) | Action sequence tracking: `(error)_seen` → `tool_detail_opened` → `[r]_pressed` → `error_cleared` | Per investigation | maintainer review |
| K-INFO-6 | Self-reported survey (no good in-process proxy; not instrumented v1) | First-month post-release email/issue survey to first-100 cohort | One-time + ad-hoc | maintainer |
| K-INFO-7 | `~/.modeltap/launch.log` | `Instant::now()` delta around `Cache::open()` and initial read | Per launch | platform-architect |
| K-INFO-8 | `~/.modeltap/sessions.log` (opt-in, Release 3) | Per-action log line `unify_dialog_open hashes_ready=<n> hashes_total=<m>` | Per dialog | platform-architect |

> Telemetry is **opt-in** (parent C5 carries forward). Default writes only to local logs; uploading aggregates requires `modeltap telemetry enable`.

### Hypothesis

We believe that **adding tool/model detail screens (US-21, US-22) and a SQLite-backed cache with warm-start paint + pre-mutate revalidation (US-23..US-26)** for **Devon (multi-tool power user who opens modeltap many times per day)** will achieve **a warm-start p90 first-paint of ≤ 150 ms, an in-TUI investigation/action rate of ≥ 80% (K-INFO-5/6), and zero accidental data loss attributable to cache staleness over the first 90 days post-release**.

We will know this is true when **Devon's launch.log shows warm-start times in the 50-150 ms band for ≥ 80% of launches in a typical week, his sessions.log shows ≥ 80% of "(error)" investigations resolved in-TUI, and the issue tracker shows zero `cache_recovery_failed` or `accidental-loss` tagged issues after 90 days**.

### Smell Test (per KPI)

| KPI | Measurable today? | Rate not total? | Outcome not output? | Has baseline? | Team can influence? | Has guardrails? |
|---|---|---|---|---|---|---|
| K-INFO-1 | Yes (after instrumentation) | Time-bounded threshold, acts as a rate | Yes — sub-100ms paint IS the behaviour change | Yes — parent K3 ~150 ms skeleton + ~1.15 s full | Yes — startup path fully owned | Yes — K-INFO-7 ensures overhead doesn't accrue |
| K-INFO-2 | Yes (after instrumentation) | Time-bounded threshold | Yes — refresh wall-clock is observable | Yes — quit-and-relaunch ~1.15 s | Yes — refresh path fully owned | Yes — K-INFO-4 (cache must not break) |
| K-INFO-3 | Yes | Yes — ratio of tools in-TTL | Yes — describes user's reality | No — first launch establishes baseline | Partial — depends on usage frequency vs TTL | Yes — TTL configurable |
| K-INFO-4 | Yes (issue tracker) | Rate of recovery events | Yes — recovery correctness is observable | N/A (feature creates the gap) | Yes — recovery path fully owned | Itself a guardrail |
| K-INFO-5 | Yes (after instrumentation) | Yes — ratio of investigations resolved in-TUI | Yes — workflow change is observable | Today: anecdotally near 0% | Yes — detail screen content fully owned | Implicit — K-INFO-7 (overhead) |
| K-INFO-6 | Hard (cross-process detection); proxied by survey | Yes — % of detail opens | Yes — workflow change | Today: anecdotal | Yes — metadata richness fully owned | Implicit |
| K-INFO-7 | Yes (timing) | Time-bounded | Yes — overhead is observable | Baseline = ADR-003 cold-start | Yes | Itself a guardrail |
| K-INFO-8 | Yes (after Release 3) | Ratio | Yes | Today: 0% on first unify of session | Yes | Implicit |

All 8 KPIs pass the smell test. K-INFO-3 has no baseline today; first month post-release is a baseline-gathering period before targets are enforced. K-INFO-6 is proxied by survey rather than instrumented because cross-process detection (modeltap detecting `gguf-dump` in another terminal) is intrusive.

### Connection to JTBD opportunity scores

| KPI | Drives outcome | Score |
|---|---|---|
| K-INFO-1, K-INFO-3 | O2 (warm-start latency) | 13.5 — Underserved |
| K-INFO-2 | O5 (reflect out-of-band changes) | 12.5 — Underserved |
| K-INFO-5 | O4 (diagnose tool error) | 12.0 — Underserved |
| K-INFO-6 | O1 (confirm model metadata) + O8 (in-TUI metadata) | 15.5 + 14.0 — Extremely Underserved + Underserved |
| K-INFO-4 | O9 (cache corruption guardrail) | mandatory |
| K-INFO-7 | O7 (modeltap won't start guardrail) | overserved-today; KPI keeps it overserved |
| K-INFO-8 | O6 (avoid re-hash) | 11.5 — borderline; Release 3 |

### Handoff Notes for DEVOPS (platform-architect)

1. **Instrumentation:** All KPIs except K-INFO-6 are local-log-based. Reuse parent feature's `~/.modeltap/launch.log` and `~/.modeltap/sessions.log` schemas; add new log line tags as listed in `shared-artifacts-registry.md` "Updated parent artifacts" row for `diagnostics.log`.
2. **Baseline collection:** K-INFO-3 has no baseline today (cache doesn't exist); first 30 days post-Release 2 is a baseline-gathering period.
3. **Alert thresholds:**
   - K-INFO-1: warm-start p90 > 200 ms = CI regression alert.
   - K-INFO-7: cache layer overhead > 100 ms = CI regression alert.
   - K-INFO-4: any `cache_recovery_failed` issue = manual review trigger.
   - K3b (parent cold-start): unchanged from parent's alerting.
4. **No real-time dashboards needed** (parent constraint carries forward). Quarterly aggregate review of opt-in telemetry is sufficient.
5. **Telemetry stays opt-in** (parent C5 — non-negotiable).
6. **K-INFO-6 instrumentation deferred** to a post-Release 2 survey approach; do not invest engineering time in cross-process detection.

### Handoff Notes for DISTILL (acceptance-designer)

Outcome KPIs ARE testable from BDD where they're time-bounded:

- K-INFO-1 → `@us_25 Scenario: Warm start paints cached inventory within 100 ms`
- K-INFO-2 → `@us_24 Scenario: [r] refreshes the selected tool` (within 1 second AC)
- K-INFO-4 → `@us_23 Scenario: Cache corruption is detected on open and recovered automatically`
- K-INFO-7 → New `@property` scenario in DISTILL for cache-overhead bound.

K-INFO-3, K-INFO-5, K-INFO-6, K-INFO-8 are usage-pattern metrics measured over time, not in single-scenario tests; they're for DEVOPS dashboards (such as they are for a local CLI tool) and quarterly review.
