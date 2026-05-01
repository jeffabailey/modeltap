# Outcome KPIs: cross-tool-model-unify

## Feature Objective

Make the v1 "unify" promise true: a small-team developer with multiple local AI tools can launch modeltap, see real dedup-able bytes, press one key on a row, confirm a concrete reclaim total, and see the disk actually returned — within a single session.

---

## Outcome KPIs

| # | Who | Does What | By How Much | Baseline | Measured By | Type |
|---|---|---|---|---|---|---|
| KPI-1 (Activation) | Devon-class users with >=2 tools installed and >=1 cross-tool duplicate present | Sees a non-zero `Dedup-able` value in the summary bar within first paint+hash window | 95% of qualifying sessions, **within 60 s of launch** on a typical install (~20 GGUF files, warm disk) | v1 baseline: 0% (summary bar hardcoded to 0 B) | `launch.log` JSONL: `event=summary_paint, dedup_able_bytes=N` where N>0; sessions tagged "qualifying" by `tools_with_models >= 2 AND has_cross_tool_duplicate == true` | Leading (primary) |
| KPI-2 (Adoption) | Devon-class users in qualifying sessions | Invokes `u` (from main view OR detail screen) at least once | 60% of qualifying sessions per week | v1 baseline: unknown but believed near 0% (key is broken from main view) | `launch.log`: `event=unify_dialog_opened` per session | Leading (primary) |
| KPI-3 (Success) | Users who invoke `u` and confirm | Completes a unify without cross-fs friction OR running-tool friction blocking the flow | 80% of `u`-confirm invocations result in `event=unify_completed_full` (vs `unify_completed_partial` or `unify_aborted`) | v1 baseline: N/A (path didn't exist) | `launch.log`: ratio of `unify_completed_full` / `unify_dialog_confirmed` | Leading (secondary) |
| KPI-4 (Retention proxy) | Devon-class users who completed >=1 unify in week N | Launches modeltap >=1 time in week N+1 | 50% week-over-week return rate | v1 baseline: unknown (no retention instrumentation) | `launch.log` analysis: distinct-day launch counts per anonymized install_id | Lagging (secondary) |

### Smell Test Results

| Check | KPI-1 | KPI-2 | KPI-3 | KPI-4 |
|---|---|---|---|---|
| Measurable today? | Needs `summary_paint` event in launch.log (NEW) | Needs `unify_dialog_opened` event (likely already exists; verify) | Needs `unify_completed_full/partial/aborted` events (likely exists; verify wording) | Needs anonymized install_id (NEW — privacy-respecting opt-in) |
| Rate not total? | Yes (% of qualifying sessions) | Yes (% per week) | Yes (ratio) | Yes (return rate) |
| Outcome not output? | Yes — measures user behavior (sees the value) not feature-shipped | Yes — measures key-press behavior | Yes — measures completion ratio | Yes — measures return |
| Has baseline? | Yes (0%) | Approximate (~0%) | N/A new | Unknown — establish via instrumentation |
| Team can influence? | Yes — direct via wiring + hash speed | Yes — direct via discoverability | Yes — direct via fallback UX quality | Indirect — through aggregate experience |
| Has guardrails? | See below | See below | See below | See below |

### Guardrail Metrics (must NOT degrade)

| Guardrail | Target | Rationale |
|---|---|---|
| First-paint latency (K3) | <= 1 s p95 | Hashing is background; rows must still render fast |
| Crash rate | 0% increase vs v1 | Unify is destructive-adjacent; failures here are scary |
| Discovery time per launch | <= 5 s p95 | Stateless rediscovery (Q7) must not get slower |
| False-`#` rate | 0 occurrences | A row showing `#` (already unified) must be filesystem-truth, not a stale cache lie |

---

## Metric Hierarchy

- **North Star**: KPI-1 (Activation) — `% of qualifying sessions where Dedup-able > 0 within 60 s`. This is THE metric. If it isn't moving from ~0% (v1) to >=95%, the feature failed regardless of any other measure. The v1 bug is the activation step; fixing it IS the feature.
- **Leading Indicators**: KPI-2 (Adoption — does anyone press `u`?), KPI-3 (Success — when they do, does it work?)
- **Guardrail Metrics**: First-paint latency, crash rate, discovery time, false-`#` rate.

---

## Outcome Mapping Chain

```
Business KPI (Lagging/Impact)
    Trust in modeltap as a real disk-management tool
        |
        v
    Customer Behavior (Leading/Outcome — KPI-2, KPI-4)
        +-- Users press u and complete a unify (+from ~0% to >=60%)
        +-- Users return week-over-week (>=50%)
        |
        v
    Secondary Behavior (Leading/Secondary — KPI-1, KPI-3)
        +-- Users see Dedup-able > 0 within 60 s (>=95%)
        +-- Confirmed unifies complete fully (>=80%)
```

---

## Measurement Plan

| KPI | Data Source | Collection Method | Frequency | Owner |
|---|---|---|---|---|
| KPI-1 | `~/.modeltap/launch.log` JSONL | Add `summary_paint` event with `dedup_able_bytes` field (new instrumentation in scope for DEVOPS wave) | Per-session, aggregated weekly | platform-architect |
| KPI-2 | `launch.log` | `unify_dialog_opened` event (verify exists; add if missing) | Per-session, aggregated weekly | platform-architect |
| KPI-3 | `launch.log` | `unify_completed_full` / `_partial` / `_aborted` events | Per-action, aggregated weekly | platform-architect |
| KPI-4 | `launch.log` | Anonymized install_id (opt-in); count distinct-day launches per id | Weekly cohort | platform-architect |

DEVOPS wave (platform-architect) consumes this file to design the launch.log schema additions and any opt-in retention-tracking infrastructure.

---

## Hypotheses

**H1 (Activation)**: We believe that wiring the dedup classifier to the summary bar (US-U2) and adding background hashing with progress (US-U1) for Devon-class users with >=2 tools will achieve KPI-1. We will know this is true when **>=95% of qualifying sessions show Dedup-able > 0 in the summary bar within 60 s of launch**.

**H2 (Adoption)**: We believe that letting `u` work from the main view with mates pre-populated (US-U4) will achieve KPI-2. We will know this is true when **>=60% of qualifying sessions per week include at least one `unify_dialog_opened` event**.

**H3 (Success)**: We believe that the existing cross-fs `[s/c/x]` and lsof gates plus the new partial-success toast (US-U10) provide enough recovery affordance that **>=80% of confirmed unifies complete fully** rather than aborting.

**H4 (Retention)**: We believe that delivering on the "unify works" promise (KPI-1 + KPI-3) is sufficient to bring users back, achieving **>=50% week-over-week return**. (Lagging — confirms the outcome chain rather than driving it.)

---

## Handoff to DEVOPS (platform-architect)

The DEVOPS wave needs to plan instrumentation for:

1. **New launch.log events**: `summary_paint` (with `dedup_able_bytes`, `unified_count`, `tools_with_models`, `has_cross_tool_duplicate`). Verify or add: `unify_dialog_opened`, `unify_completed_full`, `unify_completed_partial`, `unify_aborted`.
2. **Anonymized install_id (opt-in)**: needed for KPI-4 retention. Privacy-respecting; user can opt out via config.
3. **No real-time dashboards required**: weekly aggregation of launch.log is sufficient at this stage.
4. **Guardrail alerts**: first-paint p95 > 1 s OR false-`#` rate > 0 should trigger investigation. Crash rate increase > baseline should trigger rollback consideration.
5. **Baseline measurement**: Activation baseline is 0% (provable from v1 source: hardcoded `0 B`). Adoption baseline approximate but treated as ~0%. Retention baseline must be established in the first 4 weeks post-release.
