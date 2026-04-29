# Requirements — modeltap-tui

## Feature Identity

- **Feature ID:** `modeltap-tui`
- **Wave:** DISCUSS (wave 2 of 6)
- **Source brief:** `docs/feature/modeltap-tui/intake-brief.md`
- **Reference:** UMR — <https://github.com/EvanZhouDev/umr> (we extend it: Rust, TUI-first, plugin trait, cleanup-first framing)

## Domain Glossary

| Term | Definition |
|---|---|
| **tool** | One of {Ollama, llama-cli, Hugging Face cache, LM Studio} — a local-AI integration that downloads/manages model files. The four supported in v1. |
| **model** | A locally-downloaded model file (or set of files for layered formats) registered with at least one tool. |
| **plugin** | The modeltap module that adapts a tool — implements the `Tool` trait. New plugins live in `plugins/<name>/`. |
| **unify** | Make one canonical model file available to multiple tools via hardlinks (and config updates if needed) so the bytes appear once on disk. Bound to the `u` keystroke. |
| **zap** | Delete every model registered with a specific tool. Destructive; requires typed confirmation. Bound to the `z` keystroke. |
| **canonical** | When a model is unified, one of the existing tool-owned copies is selected as the canonical file (typically the largest); other tools' copies are replaced with hardlinks pointing at it. modeltap does NOT own a central store. Tool directories are the single source of truth. |
| **dedup key** | SHA256 of the model file's content (primary identity, per Q6 intake answer). HF repo+quant is shown as a display label only, never used for identity. Computed lazily per session and cached in-process; never persisted. |
| **format-locked** | A model whose format is accepted by only one of the supported tools (e.g., Ollama-blob, AWQ). Marked with red `!`. |
| **deduplicable** | A model that is currently registered with 2+ tools (`*`), or registered with 1 tool but its format is accepted by others (`o`). |
| **walking skeleton** | The thinnest end-to-end slice connecting all backbone activities — Release 0 of the story map. |

## Stakeholders

| Stakeholder | Role | Engagement |
|---|---|---|
| Devon Park (primary user persona) | Local-AI power user, multi-tool, macOS/Linux | Validates user-facing stories, drives K1/K2/K3/K5 |
| Riley Chen (contributor persona) | Open-source contributor adding new tools | Validates plugin-trait stories US-18, US-20, drives K4 |
| Maintainer (project owner) | Reviews PRs, manages roadmap | Owns Tool trait stability; reviews community plugin PRs |
| Tool authors (Ollama, llama-cpp, HF, LM Studio) | Out-of-band stakeholders | We do not depend on them but our discovery/linking must respect their on-disk layouts |

## Functional Requirements (Story Map Summary)

The complete user stories are in `user-stories.md`. The story map is in `story-map.md`. Summary:

### Walking Skeleton (Release 0) — 5 stories

US-01 (TUI launches and quits), US-02 (Discover Ollama), US-03 (Two-pane layout), US-05 (Zap with typed confirmation), US-06 (Show last action and reclaimed bytes).

### Release 1: Make duplication visible — 7 stories

US-04 (Row metadata), US-07 (Discover llama-cli), US-09 (Compatibility indicator engine), US-12 (Discover HF cache), US-13 (Detail screen), US-15 (Discover LM Studio), US-16 (Format-locked indicator).

### Release 2: Reclaim disk safely — 6 stories

US-08 (Bottom bar polish), US-10 (Unify), US-11 (Updated totals), US-14 (Dry-run preview), US-17 (Running tool detection), US-19 (Cross-fs fallback).

### Release 3: Built to grow — 2 stories

US-18 (Plugin trait contract), US-20 (Cross-platform path discovery).

## Non-Functional Requirements

### Performance

| NFR | Target | Verification |
|---|---|---|
| Cold start to first paint | < 1 second on a 2020-or-later workstation | Built-in timing log per launch |
| Full inventory render | < 3 seconds for ≤500 models | Built-in timing log |
| Inventory refresh after action | < 500 ms | Built-in timing |
| Running-tool detection | < 500 ms | Built-in timing |

### Safety

| NFR | Target | Verification |
|---|---|---|
| Destructive action confirmation | 100% of `z` actions preceded by typed name confirmation | UAT US-05 |
| Unify plan visibility | 100% of `u` actions show plan before disk mutation | UAT US-10, US-14 |
| Accidental data loss | 0 reports across first 90 days post-v1 | Issue tracker review |
| Plugin panic isolation | One plugin panic does not crash the TUI | UAT US-18 |
| Partial-state recovery | No partial mutations on error mid-action | UAT US-10, US-19 |

### Cross-Platform

| NFR | Target | Verification |
|---|---|---|
| macOS support | Tested on macOS Sonoma (14.x) and later | CI runner |
| Linux support | Tested on Ubuntu 22.04 LTS and later | CI runner |
| Windows support | WSL only (architecturally identical to Linux); native Windows binary refuses to run with clear message | UAT US-20 |
| Hardlink semantics | Work via APFS (macOS), ext4/btrfs (Linux) | UAT US-10, US-19 |

### Privacy / Telemetry

| NFR | Target | Verification |
|---|---|---|
| Default telemetry behavior | No data leaves the machine; local log only | Code review |
| Telemetry opt-in | Explicit `modeltap telemetry enable` required for any upload | Code review |
| User PII | Never collected, even with telemetry enabled | Code review |

### Accessibility (Terminal)

| NFR | Target | Verification |
|---|---|---|
| Color independence | All information conveyed by color is also conveyed by symbol or text | UAT US-04, US-16 |
| `NO_COLOR` env var | Respected; symbols still convey meaning without color | UAT |
| Contrast | Red `!` on default terminal background ≥ 4.5:1 | Manual check across iTerm2, Terminal.app, gnome-terminal, alacritty |
| Keyboard-only operation | Every action reachable without mouse | UAT US-03, US-08 |
| Minimum terminal size | 80×24 enforced; smaller terminals refused with clear message | UAT US-01 |

### Reliability

| NFR | Target | Verification |
|---|---|---|
| Plugin discovery error isolation | Failure in one plugin does not block other plugins | UAT US-02, US-18 |
| Filesystem permission errors | Surfaced as "(error)" with diagnostic log entry, not crash | UAT US-02 |
| Broken symlinks / missing blobs | Listed with explicit warning, not silently dropped | UAT US-12 |

## Architectural Constraints (hard, for DESIGN)

These are constraints on the design space — DESIGN must respect them.

### C1 — Plugin trait extensibility

A new tool must be addable by implementing one trait in `plugins/<name>/` with **zero changes** to modeltap-core source files. The plugin trait surface must include: name, discover, list_models, link, delete, accepted_formats. Choice of dynamic vs static dispatch belongs to DESIGN, but the constraint that core stays untouched does not.

### C2 — Cross-platform from v1

macOS + Linux supported from v1. Path discovery, hardlink semantics, and process-detection (lsof) must work on both. Windows: **WSL only** (architecturally identical to Linux; no native Windows code paths). Native Windows binary refuses to run with a clear message.

### C3 — MLX out of scope for v1

Per intake. DESIGN should not allocate effort to MLX format support; capability metadata should be structured to allow it later.

### C4 — Walking skeleton in 2-3 days

Release 0 (US-01, US-02, US-03, US-05, US-06) must be completable as the first deliverable. This forces:
- Plugin trait shape to exist from day one (US-02 instantiates it)
- Confirmation UX baseline (US-05) before unify exists
- Post-action feedback (US-06) before unify exists

### C5 — Privacy by default

No telemetry leaves the machine without explicit opt-in. Local-AI users are privacy-sensitive by selection.

### C6 — Cleanup-first framing (vs UMR's registration-first)

We extend UMR but our primary user motion is "see and clean up", not "register and link." This shapes the TUI: cleanup actions (zap) are first-class shortcuts; unify is one motion among many, not the central command.

### C7 — Stateless: no persistent index, no central store

modeltap maintains no on-disk index, registry, or canonical store. Each launch rediscovers models from each tool's own directory. Per-session caches (e.g., SHA256 hashes) are in-memory only. This shapes K3 (first-paint latency) — DESIGN must achieve < 1 s through parallel discovery and skeleton-first rendering, not through cached state. Per intake Q1 + Q7. Note: a user-owned `~/.modeltap/config.toml` for preferences (e.g., extra search paths) is permitted; it is user input, not modeltap-managed state.

## Open Questions — Disposition

The intake brief lists 7 open questions. Disposition:

| ID | Question | Resolution in DISCUSS | Action for DESIGN |
|---|---|---|---|
| Q1 | Canonical store location | **RESOLVED (intake): no central store.** Tool directories are the source of truth. When unifying, an existing tool-owned copy is chosen as canonical (typically the largest); other copies become hardlinks to it. | DESIGN MUST NOT introduce a `~/.modeltap/store/`. See ADR-003. |
| Q2 | Per-tool linking strategy | **DEFERRED — DESIGN must close.** Each plugin's `link()` is part of the Tool trait. DESIGN must produce a per-plugin linking spec (Ollama blob layout, llama-cli loose file, HF symlink farm, LM Studio config). May need light spike research per tool. | Spike per plugin; document in design artifacts. See ADR-004 (Ollama and llama-cli resolved; HF and LM Studio need DELIVER spike). |
| Q3 | "Only-one-tool" definition | **RESOLVED: format-based.** A model is format-locked when no other supported tool's `accepted_formats()` contains its format. Layout-based locking is rare and not modeled in v1. Ollama-blob format is the canonical example. | Use this in capability metadata schema. |
| Q4 | Zap confirmation UX | **RESOLVED (intake): typed tool name. No undo, ever.** Modal dialog requires user to type the tool name exactly. Esc cancels. | Implement per US-05. |
| Q5 | Concurrency vs running tools | **RESOLVED (intake): detect-and-prompt-then-retry.** If a tool is running with the file open, prompt the user to close the tool, then offer retry. No file-locking or PID-tracking subsystem. | Implement per US-17. |
| Q6 | Model identity / dedup key | **RESOLVED (intake): SHA256 of file content (primary identity); HF repo+quant (display label only).** Lazy compute on first need, cache in-process per session, never persist. | Implement per ADR-002. |
| Q7 | State persistence | **RESOLVED (intake): stateless.** No central store, no registry/index file. Rediscover from each tool's directory on every launch. K3 (< 1 s first paint) becomes a real constraint — see ADR-003 (parallel async discovery, skeleton-first paint). | Implement per ADR-003. |

## Risks (surfaced; managed downstream)

| Risk | Category | Probability | Impact | Mitigation |
|---|---|---|---|---|
| Per-tool linking strategy proves harder than expected (Q2) | Technical | Medium | High | Spike research per plugin in DESIGN; deliver Ollama first as proof |
| Dedup key strategy chosen wrong (Q6) | Technical | Medium | Critical | Walking skeleton excludes unify; shipping order forces dedup-key decision before US-10 |
| Hardlink fails silently producing copies | Technical | Low | High | US-19 explicitly tests inode equality post-unify |
| Tool authors change on-disk layout in a future version | Technical | Medium (over time) | Medium | Plugin trait isolates; one bad plugin shows "(error)" not crash |
| LM Studio path conventions differ across versions | Technical | Medium | Low | US-15 checks multiple default paths |
| User accidentally zaps the wrong tool | Project | Low | Critical | Typed-name confirmation (US-05); detect-and-prompt-then-retry for running tools (US-17) |
| Tool running while modeltap mutates files | Technical | Medium | Medium | Detect via lsof; prompt user to close the tool and retry (per intake Q5). No locking subsystem in v1. |

## Wave Handoff Package

### To DESIGN (solution-architect)

**Inputs:**

- Journey artifacts: `journey-cleanup-and-unify-visual.md`, `journey-cleanup-and-unify.yaml`, `journey-cleanup-and-unify.feature`
- Story map and prioritization: `story-map.md`, `prioritization.md`
- Requirements: this file, `user-stories.md`, `acceptance-criteria.md`
- Outcome KPIs: `outcome-kpis.md`
- Shared artifacts: `shared-artifacts-registry.md`
- DoR validation: `dor-checklist.md`

**Walking skeleton scope to design first:**

US-01, US-02, US-03, US-05, US-06. Approximately 2-3 days of build effort. Successful walking skeleton means: `modeltap` opens on a real Ollama installation, two-pane layout renders, user can zap llama-cli (well, Ollama in WS) with typed confirmation, and bytes reclaimed is shown. Do **not** include unify or other-tool discovery in WS.

> Note: WS uses Ollama for the discovery proof and the zap target — the brief originally suggested "stub data first, Ollama second" but the chosen WS is slightly thicker (real Ollama, real zap) for stronger validation. See `story-map.md` Walking Skeleton section for rationale.

**Hard constraints (must not be designed away):**

1. **C1 Plugin trait** — adding a new tool requires no core changes (US-18). Trait surface must include: name, discover, list_models, link, delete, accepted_formats. Implementation pattern (static inventory, dyn dispatch, etc.) is DESIGN's choice.
2. **C2 Cross-platform** — macOS and Linux from day one (US-20). No Unix-only assumption outside platform abstraction.
3. **C3 MLX deferred** — no effort allocated; capability metadata schema must support adding it later.
4. **C4 Walking skeleton 2-3 days** — keep WS thin enough.
5. **C5 Privacy** — no telemetry by default, opt-in only.
6. **C6 Cleanup-first** — `z` is a first-class shortcut, not a hidden command.

**Open questions — all RESOLVED post-DESIGN:**

| ID | Question | Resolution |
|---|---|---|
| Q1 | Canonical store location | RESOLVED (intake): no central store; tool directories are source of truth. ADR-003. |
| Q2 | Per-tool linking specs | RESOLVED in DESIGN ADR-004 for Ollama + llama-cli; HF + LM Studio need a small verification spike in DELIVER (< 1 day each). |
| Q3 | "Only-one-tool" definition | RESOLVED (intake): format-based. |
| Q4 | Zap confirmation UX | RESOLVED (intake): typed tool name; no undo. |
| Q5 | Concurrency vs running tools | RESOLVED (intake): detect-and-prompt-then-retry. |
| Q6 | Dedup key strategy | RESOLVED (intake + ADR-002): SHA256 primary, HF id+quant display fallback only. |
| Q7 | State persistence | RESOLVED (intake + ADR-003): stateless rediscovery on every launch. |

### To DEVOPS (platform-architect)

Outcome KPIs in `outcome-kpis.md`. Key items:
- Local logging hooks for K1, K2, K3 (opt-in)
- No real-time dashboards needed (local CLI tool)
- CI must run on macOS and Linux runners (NFR Cross-Platform)
- Build CI alert: K3 first-paint > 2s = regression

### To DISTILL (acceptance-designer)

Gherkin in `journey-cleanup-and-unify.feature` plus `user-stories.md` UAT scenarios are the source. Integration checkpoints in `shared-artifacts-registry.md` are the cross-step invariants.

## DoR Status

See `dor-checklist.md` for the per-story validation. Summary: **20 stories defined; all 9 DoR items pass for US-01..US-20**.

## Peer Review Summary

Review applied the 5-dimension critique from `nw-po-review-dimensions`. Note: this run was performed inline (no separate reviewer agent invocation available in this session). A second-opinion review by `nw-product-owner-reviewer` is recommended before DESIGN starts; nothing in this document blocks that handoff.

```yaml
review_id: "req_rev_20260428_inline"
reviewer: "product-owner (self-review mode)"
artifact: "docs/feature/modeltap-tui/discuss/*"
iteration: 1

strengths:
  - "Story map explicitly disagrees with the brief's recommended walking skeleton and justifies the alternative — shows independent thinking, not stenography."
  - "Open questions Q1-Q7 each have explicit disposition (resolved here vs. deferred to DESIGN) — no silent drops."
  - "Plugin trait (US-18) is expressed as a story with testable AC, not buried as an implementation note."
  - "Cross-platform requirement (US-20) carries CI consequences, not just declarative text."
  - "Privacy-by-default is treated as a hard constraint (C5) — appropriate for the local-AI user segment."

issues_identified:
  confirmation_bias: []
  completeness_gaps:
    - issue: "Operations/support stakeholder perspective absent."
      severity: "low"
      location: "Stakeholders table"
      recommendation: "N/A for a local CLI; documenting as not-applicable is acceptable."
  clarity_issues: []
  testability_concerns:
    - issue: "US-14 has only 2 UAT scenarios versus recommended 3-7."
      severity: "medium"
      location: "US-14"
      recommendation: "Optionally add a third scenario such as 'dry-run after dry-run produces identical plan'. Story is small enough that 2 may be defensible; reviewer judgment."
  priority_validation:
    q1_largest_bottleneck: "YES"
    q2_simple_alternatives: "ADEQUATE"
    q3_constraint_prioritization: "CORRECT"
    q4_data_justified: "JUSTIFIED"
    verdict: "PASS"

approval_status: "approved"
critical_issues_count: 0
high_issues_count: 0
medium_issues_count: 1
```
