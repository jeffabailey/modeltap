# ADR-012: Telemetry Approach — Opt-In, Local-by-Default

## Status

Accepted (2026-04-28).

## Context

modeltap defines outcome KPIs K1..K5 (`docs/feature/modeltap-tui/discuss/outcome-kpis.md`). Some are measurable without any data-collection infrastructure (K4 by GitHub PR count; K5 by GitHub issue label). Others (K1 bytes reclaimed, K2 dedupable %, K3 first-paint latency) reflect runtime user experience and are not visible to the maintainer without some form of data flow.

The project has a hard privacy constraint:

> **C5 — Privacy by default.** No telemetry leaves the machine without explicit opt-in. Local-AI users are privacy-sensitive by selection.

We need a coherent telemetry approach that respects C5 while still enabling K1/K2/K3 measurement.

## Decision

**Three layers:**

1. **Local logs (always, no opt-in).** A JSONL stream at `~/.modeltap/launch.log` records KPI events. The schema is defined in `kpi-instrumentation.md` and explicitly excludes paths, model names, hashes, and any PII. The user accesses their own data via `modeltap stats`. Nothing leaves the machine.

2. **Optional `modeltap stats` subcommand (v1).** Aggregates the local log into a human-readable summary. Local-only. No network. Ships in v1.

3. **Opt-in aggregate upload (deferred to v1.x).** Designed in `telemetry-design.md` for forward-compatibility but **not implemented in v1**. When implemented, it requires explicit `modeltap telemetry enable`, uploads bucketed weekly aggregates only, contains no identifiers, and is independent (unlinkable) per upload.

## Alternatives considered

### Alternative A — No instrumentation at all

Don't write any local log. Measure K1/K2/K3 via post-hoc surveys only.

**Pros:** maximally privacy-preserving; simplest implementation.

**Cons:** users can't see their own usage stats (`modeltap stats` becomes impossible); CI can't gate K3 against fixture runs (the same instrumentation drives both); maintainer has no signal on real-world K3 latency, leading to blind regressions. **Rejected — gives up too much value for a privacy gain that's already achieved by "local-only by default."**

### Alternative B — Always-on telemetry (opt-out)

Industry-typical. Upload anonymized usage on every launch; user can disable in config.

**Pros:** maximum data; common pattern.

**Cons:** **violates C5** — local-AI users are privacy-sensitive by selection; opt-out is a betrayal of trust for this user base. Also legally fraught (GDPR / CCPA "consent" debates around opt-out). **Rejected on hard constraint.**

### Alternative C — Local logs only, no upload ever

Always-local. Never any upload mechanism. Surveys + GitHub for everything else.

**Pros:** strongest privacy stance; zero infra to ever run.

**Cons:** maintainer has no longitudinal trend data without explicit user effort (responding to surveys). For a small project this may be fine. **Rejected for v1.x but viable as default forever** if surveys prove sufficient.

### Alternative D (CHOSEN) — Local-by-default + opt-in aggregate, v1 ships layer 1+2 only

Layers 1 (local log) and 2 (`modeltap stats`) ship in v1. Layer 3 (opt-in aggregate upload) is designed but not shipped. v1.x decides whether to ship layer 3 based on whether layers 1+2 + surveys + CI benchmarks prove sufficient.

## Consequences

### Positive

- C5 is honored absolutely in v1: no upload code is even compiled in (gated behind a `telemetry-upload` Cargo feature flag, off by default).
- Users get immediate value from `modeltap stats` — a feature, not just instrumentation.
- CI K3 benchmark uses the same JSONL schema as the local log — single instrumentation surface, two consumers.
- The local schema is upload-ready (no PII, bucketable) — adding layer 3 later is a pure additive change.
- Trust: by shipping privacy-preserving by default, modeltap differentiates from typical OSS dev-tools.

### Negative

- Maintainer has weaker K1/K2/K3 visibility in v1 than competing tools that ship opt-out telemetry. Mitigation: first-100-users survey at launch; CI benchmark covers K3 trend.
- If layer 3 is added later, opt-in adoption will be low (industry norm: 1-5% opt-in rates). Trends from opt-in data are biased toward early adopters and enthusiasts. Acceptable — directional signal is enough for a tool of this size.
- Designing for forward-compat (the `modeltap.launch.v1` schema) means making schema decisions now that bind future telemetry — more careful design upfront, less freedom later. Mitigation: schema versioning (`v1`, `v2`) is straightforward.

### Neutral

- `~/.modeltap/launch.log` exists as a file the user can `rm` at any time. modeltap recreates it on next launch with no functional impact. The user is in full control.
- Path redaction in `~/.modeltap/diagnostics.log` (also covered in `kpi-instrumentation.md`) is a related but separate concern; same privacy ethic.

## Enforcement

- v1 ships **without** the `telemetry-upload` feature flag enabled. The upload code path does not compile in default builds. Verified by `cargo build --release` producing a binary that has no `telemetry::upload` symbols.
- `kpi-instrumentation.md` §2.2 enumerates the never-logged-fields list; treated as a hard contract. Any change requires ADR amendment.
- `telemetry-design.md` is the design-only spec for layer 3; does not gate v1 release.
- README.md prominently documents the privacy posture: "modeltap does not send any data to any server, ever. The local file ~/.modeltap/launch.log is for your own use."

## References

- `docs/feature/modeltap-tui/discuss/requirements.md` § Privacy / Telemetry NFRs and § C5.
- `docs/feature/modeltap-tui/devops/kpi-instrumentation.md` — JSONL schema and privacy-by-design field exclusion.
- `docs/feature/modeltap-tui/devops/telemetry-design.md` — deferred layer 3 design for forward-compat.
