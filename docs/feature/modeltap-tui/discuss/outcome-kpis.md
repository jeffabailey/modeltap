# Outcome KPIs — modeltap-tui

## Feature: modeltap-tui

### Objective

Local-AI power users can see, deduplicate, and clean up their locally-downloaded models across multiple tools in one place — reclaiming disk space without copying bytes and without breaking any of the supported tools.

### Outcome KPIs

| # | Who | Does What | By How Much | Baseline | Measured By | Type |
|---|---|---|---|---|---|---|
| K1 | Devon (multi-tool local-AI user) | Reclaims disk space in a single zap or unify session | At least 5 GB reclaimed in the median session, at least 20 GB at p90 | 0 GB (no tool exists today) | Self-reported via opt-in `modeltap stats` (or telemetry if user opts in) — bytes-reclaimed counter aggregated per session | Leading (outcome) |
| K2 | Devon (multi-tool local-AI user) | Identifies what fraction of his registered models are deduplicable cross-tool | At least 30% of registered models marked `*` (already-shared) plus `o` (could-be-shared) at first launch for users with 2+ tools | Unknown today (no tool measures this) | Computed locally on launch; logged with consent | Leading (secondary) |
| K3 | Devon (any user) | Sees the inventory after launching `modeltap` | First paint within 1 second on a typical workstation; full inventory rendered within 3 seconds for ≤500 models | N/A | Built-in startup timing (logged to file) | Leading (secondary, performance) |
| K4 | Contributor (open-source) | Adds support for a new tool by implementing one trait without modifying core code | Minimum 1 community-contributed plugin merged within 6 months of v1.0 release; "core code unchanged" verified by diff scope | 0 (UMR has no plugin model; modeltap is a fresh start) | GitHub PR review checklist enforces "no changes outside `plugins/<new>/`" | Leading (secondary, ecosystem health) |
| K5 | Devon (multi-tool local-AI user) | Completes a destructive action (zap or unify) with zero accidental data loss | 0 reports of accidental destruction in the first 90 days post-v1; 100% of zap actions preceded by typed confirmation; 100% of unify actions preceded by visible plan | N/A | Crash/issue log review at 30/60/90 days; user survey on first-100-users cohort | Leading (guardrail) |

### Metric Hierarchy

- **North Star**: K1 — bytes reclaimed per session. The whole tool exists to reclaim disk; this is the OMTM.
- **Leading Indicators**: K2 (visibility-of-the-problem precedes action) and K3 (sub-second startup precedes engagement).
- **Guardrail Metrics**:
  - K5 — destructive-action safety (must NOT degrade; zero accidental losses).
  - First-paint latency must NOT exceed 1 second (degrading K3 hurts adoption).
  - Plugin-load failure must NOT crash the TUI (one bad plugin does not take down the others).
- **Ecosystem Indicator**: K4 — supported tool count growing post-launch.

### Measurement Plan

| KPI | Data Source | Collection Method | Frequency | Owner |
|---|---|---|---|---|
| K1 | `~/.modeltap/sessions.log` (opt-in) | Each completed zap/unify writes a line: `{timestamp, action, bytes_reclaimed}` | Per session | platform-architect to wire telemetry; product-owner reports |
| K2 | Inventory build at launch | At start, log: `{timestamp, total_models, marked_star, marked_o, marked_bang}` | Per launch | platform-architect |
| K3 | Built-in timing | `Instant::now()` deltas at: process start → first paint → full inventory | Per launch | platform-architect |
| K4 | GitHub `plugins/` directory | Count of subdirectories matching plugin trait | Quarterly | maintainer review |
| K5 | GitHub issues + user survey | Tag `accidental-loss` on issues; quarterly survey to first-100 cohort | Quarterly + on-incident | maintainer review |

> Telemetry is **opt-in**. Default behavior writes only to a local log file; uploading aggregate stats requires the user to run `modeltap telemetry enable`. This is a hard constraint — local-AI users are privacy-sensitive by selection.

### Hypothesis

We believe that **a two-pane TUI with explicit duplication indicators and a typed-confirmation zap** for **multi-tool local-AI power users** will achieve **a median 5 GB reclaimed per session and 30% of registered models marked as deduplicable**.

We will know this is true when **a typical user with 2+ tools launches `modeltap`, sees `*` indicators on at least a third of their models, runs at least one zap or unify within the first session, and reclaims at least 5 GB**.

### Smell Test (per KPI)

| KPI | Measurable today? | Rate not total? | Outcome not output? | Has baseline? | Team can influence? | Has guardrails? |
|---|---|---|---|---|---|---|
| K1 | Yes (after instrumentation) | Total — but per-session, so acts as a rate over usage | Yes — bytes reclaimed is observable behavior | Yes — 0 GB (tool doesn't exist) | Yes — depends on how compelling the unify/zap UX is | Yes — K5 ensures K1 isn't gamed at the cost of safety |
| K2 | Yes | Yes — % of registered models | Yes — describes user's perception of duplication | No — unknown today, will establish on first deployment | Partial — depends on user's library; team influences detection accuracy | Yes — K3 latency must not degrade |
| K3 | Yes (built-in timing) | Time-bounded threshold, not gross count | Yes — sub-second feedback IS the behavior change | None — greenfield | Yes — startup path is fully owned | Implicit — degrading hurts K1/K2 |
| K4 | Yes (PR count) | Total over time | Yes — new plugins == ecosystem health behavior | 0 | Partial — depends on community; we control trait quality | Yes — diff-scope enforcement ensures plugins really are isolated |
| K5 | Yes (issue tracker) | Yes — accidents per release | Yes — destructive-action safety is observable | N/A | Yes — confirmation UX is fully owned | Itself a guardrail for K1 |

All KPIs pass. K2 needs baseline collection during the first month post-v1 release to set realistic targets — flag for platform-architect.

### Handoff Notes for DEVOPS (platform-architect)

1. **Instrumentation**: K1, K2, K3 require local timing/logging hooks. K1/K2 require user opt-in.
2. **Privacy**: telemetry must be local-only by default. Any upload requires explicit consent.
3. **Baseline collection**: K2 has no baseline; first 30 days post-release is a baseline-gathering period before targets are enforced.
4. **No real-time dashboards needed** — this is a local CLI tool. Quarterly review of opt-in aggregates is sufficient.
5. **Alert thresholds**: K3 first-paint > 2 seconds = regression alert (build CI). K5 any `accidental-loss`-tagged issue = manual review trigger.
