# Peer Review — DEVOPS wave — modeltap-tui

**Reviewer:** nw-platform-architect-reviewer (independent, Haiku)
**Date:** 2026-04-28
**Verdict:** **APPROVED** for handoff to DISTILL and DELIVER.

---

## Summary

The DEVOPS wave (wave 4 of 6) for `modeltap-tui` is APPROVED with no blocking issues. Platform design is sound, privacy-compliant (C5), appropriately scoped for a v1 single-binary OSS Rust CLI, and all KPIs (K1..K5) are correctly wired. All 10 critical review criteria pass.

---

## Critical issues (block DISTILL/DELIVER)

**None identified.**

External validity satisfied:
- ✅ Deployment path complete: commit → CI → tag → release workflow → GitHub Releases + Homebrew + crates.io
- ✅ Observability enabled: local JSONL by default; opt-in upload designed for v1.x (not shipped)
- ✅ Rollback path documented: users reinstall prior version; GitHub Releases permanent; Homebrew formula history
- ✅ Security gates integrated: cargo-deny (licenses + advisories), clippy SAST, architecture rule R1 enforced in CI

## High issues (should fix before DELIVER)

**None identified.**

## Medium issues (defer to DELIVER acceptable)

| # | Issue | Location | Severity | Recommendation |
|---|---|---|---|---|
| MD-1 | K3 benchmark Linux-only — rationale not documented in workflow | `ci-pipeline.md` §5.3 | Informational | Add a workflow comment ("macOS slower; full test suite covers macOS; K3 gate Linux-only by design") |
| MD-2 | cargo-dist version-update procedure not documented | `release-strategy.md` §2 | Procedural | Add to `RELEASE.md` (a DELIVER-owned doc): how to upgrade cargo-dist when major versions land |
| MD-3 | Manual E2E checklist doesn't address team split | `production-readiness-checklist.md` §8 | Procedural | Add note: solo maintainer runs both macOS + Linux sequentially; multi-person teams can split across members |

None block DISTILL or DELIVER.

---

## Spot-check results

### Privacy / C5 compliance — PASS

- v1 ships zero upload code (gated behind disabled feature flag)
- Local JSONL schema explicitly forbids PII (no model names, paths, SHA256s, usernames, hostnames)
- Opt-in is explicit (`modeltap telemetry enable` required)
- Session IDs are per-launch ULIDs, never persisted
- Path redaction in `diagnostics.log` (paths to stderr only, never to file)

Evidence: `kpi-instrumentation.md` §2.2 + §7; `telemetry-design.md` §2-3; ADR-012 enforcement section.

### KPI coverage — PASS

| KPI | Wired? | Mechanism |
|---|---|---|
| K1 disk reclaimed (GB / session) | ✅ | `action.zap_*` and `action.unify` events; formula documented |
| K2 dedupable % | ✅ | `launch.inventory` event; formula `(marked_star + marked_open) / total_models` |
| K3 first-paint latency | ✅ | `launch.timing` event; CI hard-fail > 2 s, soft-warn > 1 s |
| K4 community plugins (count) | ✅ | GitHub PR count, manual quarterly review |
| K5 accidental-loss (90 days) | ✅ | GitHub issues tagged `accidental-loss` |

Evidence: `kpi-instrumentation.md` §4; `ci-pipeline.md` §5 thresholds match `outcome-kpis.md`.

### Scope discipline — PASS

Correctly out-of-scope (and documented as non-decisions):
- ✅ No Kubernetes (single-binary, not a service)
- ✅ No Terraform (no cloud resources)
- ✅ No Prometheus / Grafana / Loki (local CLI)
- ✅ No SLOs / SLIs (OSS desktop tool, no SRE)
- ✅ No canary / blue-green (user decides when to upgrade)

Correctly in-scope: CI, release tooling, K3 benchmark, architecture rule, local instrumentation, production-readiness checklist. Evidence: `platform-design.md` §1 "Explicitly NOT in scope".

### CI design — PASS

- macOS + Linux matrix (both must pass)
- Stages: fmt, clippy `-D warnings`, test, integration, plugin contract, cargo-deny, architecture rule R1, K3 benchmark
- R1 test uses `cargo metadata` JSON to enforce: core has no plugin deps; plugins independent; TUI has no concrete plugin imports
- K3 bench: Linux runner, headless mode, fixture-based (~50 models, 4 plugins). Thresholds: < 2 s hard, < 1 s target, < 5 s full inventory (CI variability margin)

Evidence: `ci-pipeline.md` §§2-5; the `architecture.rs` test code in §4 is elegant and correct.

### Release strategy — PASS

- cargo-dist (Rust-native, Cargo-integrated, first-party support)
- 4 targets: x86_64-darwin, aarch64-darwin, x86_64-linux-gnu, aarch64-linux-gnu (WSL = x86_64-linux-gnu per intake)
- Channels: GitHub Releases (canonical), Homebrew tap (Devon persona), crates.io (Rust users)
- SemVer contract: `Tool` trait in `modeltap-core` is the public API; trait change = MAJOR
- Code signing deferred to v1.x (defensible for technical OSS audience; Gatekeeper workaround in `INSTALL.md`)
- Reproducibility: `Cargo.lock` committed, cargo-dist version pinned, GHA versions pinned

Evidence: `release-strategy.md` §§1-8; ADR-011 with rejected alternatives (goreleaser, hand-rolled, cross+scripts).

### Plugin extensibility (C1 / US-18) DEVOPS support — PASS

- Architecture rule R1 mechanically enforces: core has zero plugin deps, plugins don't depend on each other
- New plugin path = add crate under `plugins/<name>/`, add to workspace members, implement `Tool` trait
- CI test prevents violations (`PLUGIN_CRATES` list updated for new plugins)
- `CONTRIBUTING.md` (DELIVER deliverable) will spell out the steps for Riley

Evidence: `ci-pipeline.md` §4 (architecture.rs); `platform-design.md` §7 (plugin trait signature).

### Production-readiness checklist quality — PASS (4 spot-checked)

| Item | Evidence requirement | Quality |
|---|---|---|
| CI green on macOS 7 days | GitHub Actions run history | Specific |
| All US-01..US-20 + US-05b AC met | mapping table + CI logs | Specific |
| `cargo dist plan` shows 4 targets + checksums + formula | command output | Specific |
| Ollama E2E on macOS | asciinema recording | Specific |

All 16 items have specific, observable evidence requirements — not vague aspirations.

### ADR quality (010 / 011 / 012) — PASS

| ADR | Context | Decision | Alternatives | Consequences |
|---|---|---|---|---|
| ADR-010 (GitHub Actions) | ✅ | ✅ | GitLab, CircleCI, Buildkite (rejected) | ✅ |
| ADR-011 (cargo-dist) | ✅ | ✅ | goreleaser, hand-rolled, cross+scripts (rejected); cargo-release / native-cargo (not considered) | ✅ |
| ADR-012 (opt-in telemetry, local-by-default) | ✅ | ✅ | no instrumentation, always-on, local-only-forever (all considered) | ✅ explicitly addresses C5 |

ADR-012 is the highest-stakes one and explicitly justifies why opt-in (not "never") was chosen — `modeltap stats` reads local logs, CI K3 gate needs the instrumentation, no PII ever leaves the machine.

### Anti-patterns — NONE

- No re-invented tooling (uses cargo-dist, GHA, Homebrew tap — standard stack)
- No bespoke schemas (JSONL is open and tool-friendly)
- No premature optimization (deferred persistent SHA256 cache to v1.x per OQ-5)
- No security/privacy theater (privacy is structural, not performative)
- No vague NFRs (every threshold is numeric with units)

### WSL / cross-platform reality — PASS

WSL = Linux x86_64 from the kernel's point of view. CI on `ubuntu-latest` covers WSL implicitly. No special WSL-only code path. "Native Windows refuses with clear message" is the correct guard for v1. Honest answer: no separate WSL CI runner needed (would be wasted minutes).

---

## DORA metrics

| Metric | Enabled? | Notes |
|---|---|---|
| Deployment frequency | ✅ | Tag-driven, as-needed cadence; multiple per week possible |
| Lead time | ✅ | Commit → PR → CI (5-10 min) → merge → tag → release (5-15 min) ≈ 20 min total |
| Change failure rate | ✅ | Trackable via GitHub Releases + issue labels; K5 (`accidental-loss`) is the guardrail |
| Time to restore | ✅ | User reinstalls prior version; hotfix released within 24h for data-loss bugs |

All four enabled. Design does not bottleneck speed.

---

## Recommendation

**APPROVED for DISTILL + DELIVER handoff.**

Three medium items (workflow comment, version-update doc, team-split note) are deferred to DELIVER and do not block.

DELIVER post-DISTILL next steps:
1. Address MD-1 / MD-2 / MD-3.
2. Run HF + LM Studio linking spikes (DESIGN M-1, < 1 day each).
3. Manual E2E on macOS + Linux per `production-readiness-checklist.md` §8.
4. Author `CONTRIBUTING.md`, `SECURITY.md`, `INSTALL.md`, `RELEASE.md`.

---

## Reviewer notes

This file was authored from the reviewer's full inline analysis after her Task session ended without writing the file directly. Content is verbatim where she produced explicit text; reorganized faithfully where she produced bullet findings. Any future re-review should re-run the reviewer agent end-to-end.
