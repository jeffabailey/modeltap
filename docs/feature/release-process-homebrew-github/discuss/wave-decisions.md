# Wave Decisions Summary — release-process-homebrew-github

DISCUSS wave (wave 2 of 6) decisions, assumptions, and handoff state.

## Wave Configuration

| Setting | Value | Rationale |
|---|---|---|
| Format | all (visual + yaml + gherkin) | Per `/nw:new` wizard configuration |
| Research depth | lightweight | Per wizard; brownfield project, single-maintainer scope |
| Elicitation depth | comprehensive | Per wizard; infrastructure feature with multiple integration points |
| Feature type | infrastructure | Per wizard; release pipeline + cross-repo automation |
| Walking skeleton | no (treat WS as a release slice within this feature) | Per wizard; modeltap-tui already shipped its skeleton |
| JTBD analysis | skip | Per wizard; jobs are well-understood (cut release, install, upgrade) |
| Auto mode | active | Per wizard; minimize interruptions, surface only contested decisions |

## Personas (assumed, not negotiated)

- **Jeff Bailey** (primary) — single maintainer of `jeffabailey/modeltap`, owns the tap repo, runs macOS Sonoma + Linux WSL.
- **Devon Park** (secondary) — multi-tool local-AI power user reused from `modeltap-tui`. macOS or Linux. Installs via Homebrew.
- **Riley Chen** (tertiary) — open-source contributor reused from `modeltap-tui`. Reads workflow + runbook.

These personas were chosen without surfacing to the user because:
1. The maintainer is the user (single-maintainer project; auto-mode discourages asking the user about themselves).
2. Devon and Riley are reused verbatim from `modeltap-tui` discuss artifacts (already approved by reviewer).

## Decisions Resolved

| ID | Decision | Choice | Reasoning | Status |
|---|---|---|---|---|
| D1 | Tap repo location | `jeffabailey/homebrew-modeltap` (personal namespace) | Simplest for v1; future org migration is a one-way decision deferred until contributor count justifies it | **CONFIRMED by user 2026-05-03** |
| D2 | Release-cut trigger | Manual `git tag` push | Simplest; least magic; smallest failure surface for v1 | **CONFIRMED by user 2026-05-03** |
| D3 | macOS code signing/notarization | Skip for v1; document `xattr -dr com.apple.quarantine` workaround | Notarization requires Apple Developer account + signing secrets in CI; out of scope for the first release-process feature | **CONFIRMED by user 2026-05-03** |
| D4 | Binaries shipped | Just `modeltap` (the `modeltap-app` binary) | `modeltap-cli` listed as future in `CLAUDE.md`; not yet built; ship through this same pipeline when it exists | Default stands — `modeltap-cli` does not exist yet |
| D5 | Changelog generation | `git-cliff` driven by conventional commits | Recent commit history (`fix(ci):`, `chore(docs):`, `refactor(gpt4all):`) already follows convention; hand-curated CHANGELOG drifts from intent | Default stands — convention already in use |
| D6 | SLSA build provenance | Required (`actions/attest-build-provenance@v2`) | ~30s overhead per build; no maintainer toil; free supply-chain hygiene for an OSS tool | Default stands — pure win, no trade-off |
| D7 | Tap-bump credential mechanism | Fine-grained PAT (`GH_TAP_TOKEN`) | Simplest for v1; migrate to GitHub App if multiple maintainers join | Default stands — DESIGN may revisit |
| D8 | Submission to homebrew-core | NOT in v1 | Custom tap is the v1 distribution channel; revisit after 6 months / 100+ stars | Default stands — reversible later |

All three contested decisions (D1, D2, D3) were surfaced via a single batched `AskUserQuestion` from the `/nw:new` parent and confirmed at the recommended defaults. D4-D8 are reversible implementation details and stand at their proposed defaults; DESIGN may revisit if architectural analysis reveals a better path.

## Phase Execution Summary

| Phase | Activity | Status |
|---|---|---|
| Phase 1 | JTBD analysis | SKIPPED (per wizard) |
| Phase 2 | Journey design (visual + YAML + Gherkin + shared-artifacts) | COMPLETE |
| Phase 2.5 | User story mapping (backbone, walking skeleton, release slices, prioritization) | COMPLETE |
| Phase 2.7 | Scope assessment (Elephant Carpaccio gate) | PASSED — right-sized as a single feature with three slices |
| Phase 3 | Coherence validation (CLI vocabulary, emotional arc, shared artifacts integrity) | COMPLETE |
| Phase 4 | Requirements crafting (LeanUX stories, AC, KPIs, DoR) | COMPLETE |
| Phase 5 | Validate and handoff (peer review hard gate) | **APPROVED by `nw-product-owner-reviewer` 2026-05-03** |

## Artifacts Produced

All under `docs/feature/release-process-homebrew-github/discuss/`:

| Artifact | Lines | Purpose |
|---|---|---|
| `journey-tag-to-brew-install-visual.md` | ~245 | Visual journey, ASCII flow, emotional arc, TUI mockups, vocabulary table, open-decision flag |
| `journey-tag-to-brew-install.yaml` | ~285 | Structured journey schema (steps, shared_artifacts, integration_checkpoints, gherkin per step) |
| `journey-tag-to-brew-install.feature` | ~175 | Top-level Gherkin scenarios (single source for DISTILL acceptance tests) |
| `shared-artifacts-registry.md` | ~95 | Single source of truth registry for every `${variable}` |
| `story-map.md` | ~125 | Backbone activities, walking-skeleton slice, release slices, scope assessment |
| `prioritization.md` | ~95 | Outcome-driven priorities, MoSCoW, V*U/E scores, dependency order |
| `outcome-kpis.md` | ~90 | K-T2T, K-PIPE, K-COVER, K-TOIL, K-PROV, K-CONTRIB with measurement plans |
| `requirements.md` | ~155 | Domain glossary, FRs (story-map summary), NFRs, architectural constraints, risks, handoff package |
| `user-stories.md` | ~840 | 15 LeanUX stories with problem/persona/examples/UAT/AC/KPIs/tech-notes/dependencies |
| `acceptance-criteria.md` | ~155 | Consolidated AC index per story + 6 cross-story integration ACs |
| `dor-checklist.md` | ~80 | 9-item DoR per story, anti-pattern scan, walking skeleton coverage |
| `wave-decisions.md` | (this file) | Wave-level decisions, assumptions, handoff state |

## Story Inventory

15 stories total, distributed across 3 release slices:

- **Walking Skeleton (7 stories)**: US-01 (release-prep), US-02 (validate-tag), US-03 (CI parity gates), US-04 (single-target build), US-05 (publish-github-release), US-06 (bump-tap-formula opens PR), US-15 (end-user installs and verifies).
- **Release 1 — Multi-arch real release (4 stories)**: US-07 (4-target matrix), US-08 (atomic-publish guard), US-09 (SLSA attestation), US-10 (formula renders 4 platform blocks).
- **Release 2 — Hands-off automation (4 stories)**: US-11 (auto-merge), US-12 (idempotent retry), US-13 (RELEASING.md runbook), US-14 (workflow file ≤250 lines).

Estimated total effort: **~10 days** across all 15 stories. Walking skeleton alone: ~5.5 days.

## DoR Status

**ALL 15 STORIES PASS** the 9-item DoR checklist. Feature is ready for DESIGN handoff. See `dor-checklist.md` for per-story evidence.

Two flagged-for-reviewer items:
1. US-09 has 2 explicit Gherkin scenarios (one below the 3-7 guideline) — accepted because the story is genuinely small (~0.5 day) and the alternative paths are exhaustive.
2. US-15 has 8 ACs (one above the 7 guideline) — accepted because each platform is a distinct AC; AC count != UAT count.

## Handoff Readiness

### To DESIGN (solution-architect)
Ready. Walking skeleton scope (US-01..US-06 + US-15) is the first design target. All hard constraints (C1-C8) documented in `requirements.md`.

### To DEVOPS (platform-architect)
Ready. KPI instrumentation requirements documented in `outcome-kpis.md` "Handoff Notes for DEVOPS". DEVOPS-side work items:
1. Wire `workflow_run: completed` follow-up workflow for K-PIPE alerting.
2. Document `gh attestation verify` command in README.
3. Add `GH_TAP_TOKEN` expiry monitoring (out of scope; tracked separately).

### To DISTILL (acceptance-designer / Quinn)
Ready (after DESIGN). Source materials:
- Gherkin scenarios in `journey-tag-to-brew-install.feature`
- Per-story UAT scenarios in `user-stories.md`
- Cross-story integration ACs (INT.AC-1..6) in `acceptance-criteria.md`

DISTILL must design test infrastructure that can stand up an ephemeral tap repo or use a mock tap for fast iteration (cross-repo testing is the unique challenge of this feature).

## Risks Surfaced (managed in downstream waves)

| Risk | Probability | Impact | Owner |
|---|---|---|---|
| Cross-repo PAT expires silently | Medium | High | DEVOPS (monitoring); maintainer (rotation) |
| GitHub Actions outage on release day | Low | Medium | Maintainer (defer release) |
| aarch64-linux cross-compile breaks due to dependency C-FFI | Medium | Medium | DESIGN (test cross-compile in CI) |
| Homebrew DSL changes break formula template | Low | Medium | DESIGN (pin `brew test-bot` action version) |
| Maintainer pushes mismatched tag | Medium | Low (caught fast) | C1 + US-02 mitigates |
| Half-published release confuses users | Low | High | C2 + US-08 mitigates |
| macOS Gatekeeper blocks unsigned binary | High (every install) | Low (workaround documented) | DEFERRED (D3); future feature |

Full risk register in `requirements.md` "Risks" section.

## Peer Review (Inline Self-Review Using nw-po-review-dimensions)

A formal `nw-product-owner-reviewer` Task invocation is not available in this execution context (subagent). An inline structured self-review against the 5 review dimensions follows; the parent agent should still invoke the formal reviewer before the DISCUSS wave is closed.

```yaml
review_id: "req_rev_20260503_inline"
reviewer: "product-owner (self-review mode, nw-po-review-dimensions)"
artifact: "docs/feature/release-process-homebrew-github/discuss/*"
iteration: 1

strengths:
  - "Single source of version truth (Cargo.toml workspace.package.version) explicitly tracked across all 5 journey steps and verified by validate-tag job (US-02) and modeltap --version assertion (US-15)"
  - "Atomic-publish guarantee (C2 + US-08) prevents half-published releases via GitHub Actions needs: DAG — pure workflow-graph property, no imperative complexity"
  - "Walking skeleton is genuinely thin: one target, one platform, no auto-merge, no SLSA — proves the riskiest assumption (cross-repo automation) before scaling"
  - "Every shared artifact in the registry has a single source of truth and explicit consumers; integration checkpoints are testable invariants"
  - "Outcome KPIs are GitHub-native (no external telemetry); consistent with modeltap-tui's privacy-by-default constraint (C5)"
  - "Open decisions D1-D8 surfaced explicitly with proposed defaults and reversibility notes"

issues_identified:
  confirmation_bias:
    technology_bias:
      - assessment: "GitHub Actions, Homebrew, git-cliff, actions/attest-build-provenance@v2 — all named explicitly. Justified by problem domain (the repo is on GitHub, the install channel is Homebrew, the convention is conventional commits). Not a bias issue."
      severity: "none"
    happy_path_bias:
      - assessment: "Error scenarios covered: mismatched tag (US-02), missing CHANGELOG section (US-05), token expiry (US-06, US-12), single-target build failure halts release (US-07, US-08), brew test-bot failure withholds auto-merge (US-11), install during tap-update window (US-15), workflow file lint failure (US-14). 7+ explicit error paths across 15 stories."
      severity: "none"
    availability_bias:
      - assessment: "No 'same as previous project' framing. Personas reused from modeltap-tui are explicit reuses, not unexamined defaults."
      severity: "none"

  completeness_gaps:
    missing_stakeholders:
      - assessment: "Maintainer (Jeff), end user (Devon), contributor (Riley) all represented. GitHub Actions runners called out as operational dependency. Homebrew project as out-of-band. No missing stakeholder."
      severity: "none"
    missing_error_scenarios:
      - assessment: "All major error paths covered (see happy_path_bias above). One latent concern: rate-limiting on GitHub API (e.g., during a high-frequency release burst) is not explicitly modeled. Acceptable for v1 — single-maintainer projects do not burst-release."
      severity: "low"
      recommendation: "Note rate-limiting as a future operational concern in DEVOPS handoff (already implicitly covered by K-PIPE)"
    missing_nfrs:
      - assessment: "Performance, reliability, cross-platform, security, observability, maintainability, privacy all covered. No critical NFR gap."
      severity: "none"

  clarity_issues:
    vague_performance:
      - assessment: "All performance NFRs have numeric thresholds: K-T2T median ≤15 min / p90 ≤25 min, build duration ≤5 min/target, K-PIPE ≥95% success. No vague 'fast' or 'reliable'."
      severity: "none"
    ambiguous_requirements:
      - assessment: "All requirements are unambiguous; two architects would design similar pipelines. Choice of cross-compile mechanism (cross vs rustup target) is intentionally left to DESIGN — flagged in US-07 tech notes, not ambiguous."
      severity: "none"

  testability_concerns:
    non_testable_ac:
      - assessment: "All ACs are observable: 'brew install succeeds', 'modeltap --version prints exactly X', 'gh attestation verify returns success', 'rendered formula has 4 platform blocks'. K-CONTRIB is the weakest (qualitative proxy via issue label) — flagged in outcome-kpis.md smell test."
      severity: "low"
      recommendation: "K-CONTRIB measurement methodology should be revisited if issue-label proxy proves weak after 3-5 releases"

  priority_validation:
    q1_largest_bottleneck: "YES — cross-repo automation IS the riskiest assumption (Maurya); walking skeleton validates it first"
    q2_simple_alternatives: "ADEQUATE — D2 explicitly considers release-please / cargo-release and rejects them for v1 simplicity; D6 considers signing alternatives and defers; D7 considers GitHub App and defers"
    q3_constraint_prioritization: "CORRECT — atomicity (C2) and version-truth-singularity (C1) are correctly prioritized as the highest-impact integrity constraints"
    q4_data_justified: "JUSTIFIED — KPI targets (15 min T2T, 95% pipeline success) are realistic per GitHub Actions performance norms; no greenfield baseline exists yet but 3-5 release baseline window is documented in outcome-kpis.md"
    verdict: "PASS"

approval_status: "approved"
critical_issues_count: 0
high_issues_count: 0
medium_issues_count: 0
low_issues_count: 2
```

**Self-review verdict: APPROVED with two LOW-severity advisories** (rate-limiting note for DEVOPS, K-CONTRIB measurement revisit after baseline). Neither blocks DESIGN handoff.

## Peer Review (Independent — `nw-product-owner-reviewer` "Eclipse")

**Verdict: APPROVED → hand off to DESIGN.** Confirmed 2026-05-03.

Eclipse independently validated all four hard gates and the producer's self-assessment:

- **Journey coherence**: PASS (5 steps connected, 12 shared artifacts tracked, realistic data, no orphans, emotional arc coherent)
- **Definition of Ready**: PASS (15/15 stories pass all 9 items)
- **Antipattern detection**: PASS (0 violations; "Implement-X" advisory dismissed as contextually appropriate for an infrastructure feature where problem domain forces technology choice)
- **Requirements quality**: PASS across all 5 dimensions (no confirmation bias, complete, clear/measurable, testable, correctly prioritized)

Issue counts confirmed: 0 critical, 0 high, 0 medium, 2 low (the same two advisories the producer self-flagged: GitHub API rate-limiting note for DEVOPS, K-CONTRIB proxy revisit after 3–5 releases). Neither low blocks DESIGN.

Walking skeleton scope (US-01..US-06 + US-15) is well-bounded for the first design pass.

## Cross-Feature Coupling Notes

This feature is the second feature in the modeltap repository. It is loosely coupled to `modeltap-tui`:

- **Hard dependencies**: this feature ships the binary built by `modeltap-app` (which depends on `modeltap-tui`). The `modeltap --version` and `modeltap` (TUI launches) verifications in US-15 delegate to `modeltap-tui` US-01.
- **Reused personas**: Devon Park, Riley Chen (verbatim from `modeltap-tui`).
- **Reused vocabulary**: "tool", "model" terms are NOT relevant to this feature; this feature introduces new vocabulary ("tag", "release", "tap", "tap-bump", "atomic publish", "CI parity gates", "SLSA attestation"). No vocabulary conflict.
- **No conflicting constraints**: `modeltap-tui` C1-C7 are all about the TUI and plugins. This feature's C1-C8 are all about the release pipeline. No overlap, no conflict.
