# Requirements: cross-tool-model-unify

Brownfield extension on shipped modeltap v1. Fixes three v1 defects (hardcoded summary bar, on-demand-only hashing, missing main-view `u` handler) and adds one new surface (`[All Unified]` pseudo-tool slot).

---

## Business Context

### Why this feature exists

modeltap v1 shipped with a "unify" promise: "maintain a single, centralized copy of a model to use across your favorite local AI apps, instead of having each one manage a separate copy" (UMR-style). In v1 the underlying engine works (per-plugin `Tool::link()`, canonical selector, plan builder, dedup classifier all exist and are tested), but three separate v1-era wiring gaps make the promise unreachable from the UI:

1. Summary bar hardcodes `Dedup-able: 0 B` (`crates/modeltap-tui/src/render/summary_bar.rs:36`).
2. SHA256 hashing is lazy (per ADR-002), so even a wired bar would have no data at first paint.
3. `u` from the main row list does nothing — the unify dialog only fires from Detail.

The user can therefore never discover that the feature exists, never get the dedup-able total, and never trigger the action from the most natural place. Brownfield fix: wire the existing engine end-to-end and add a `[All Unified]` audit surface.

### Stakeholders

| Stakeholder | Interest |
|---|---|
| Devon (primary user) | Reclaim disk; trust the tool |
| Riley (secondary user, observability/release) | Verify the action via `launch.log`; audit cumulative savings |
| Maintainers (modeltap dev) | Fix v1 bugs in a way that doesn't break shipped contracts (ADR-001 trait frozen) |

### Out of scope

- Central modeltap-owned model store (Q1 closed: NO)
- Persistent index across launches (Q7 closed: NO)
- New `Tool` trait methods (ADR-001 frozen)
- Changing the cross-fs fallback dialog `[s/c/x]` (ADR-008 stays)
- Changing the lsof gate (Q5 stays)

---

## Functional Requirements

### FR-1: Background SHA256 hashing on launch (US-U1)

After first paint, modeltap computes SHA256 for every discovered model file in the background. Progress is visible in the status line. UI remains responsive throughout.

### FR-2: Summary bar reads dedup-able from classifier (US-U2)

The summary bar `Dedup-able` value derives from `core::logic::dedup` — the same source the rows read from. It shows `computing...` while hashing is in progress; it shows the truthful value (including 0 if no duplicates) post-hash.

### FR-3: Per-row dedup glyph (US-U3)

Each model row in the right pane shows a single-character glyph reflecting its dedup classification: `?` `~` `-` `=` `#`.

### FR-4: `u` from main view (US-U4)

Pressing `u` while a row is highlighted in the main view opens the unify dialog with that row's mates pre-populated, OR shows an informative status hint when the action is not applicable (unique model, hash pending).

### FR-5: Unify dialog with concrete preview (US-U5)

The unify dialog shows canonical, per-target rows with checkboxes and savings, and a live total reclaim that updates as targets are toggled. Enter applies via the existing `actions::unify::run()`. Esc cancels.

### FR-6: Live post-unify update (US-U6)

After a successful (full or partial) unify, the affected rows re-classify and the summary bar updates within 200 ms, no restart required.

### FR-7: `[All Unified]` pseudo-tool slot (US-U7)

A pseudo-tool entry in the left pane lists all `#`-glyph models with size, tool count, and savings.

### FR-8: Unified-view empty state (US-U8, P2)

When the unified count is 0, the right pane shows onboarding guidance.

### FR-9: Detail screen shows shared inode (US-U9, P2)

For `#`-glyph models, Detail shows the inode number and groups paths by inode.

### FR-10: Partial-success per-target toast (US-U10, P2)

When a unify partially succeeds, the toast shows per-target outcomes and offers `[r]` retry-failed-only.

---

## Non-Functional Requirements

### NFR-1: First-paint latency (K3 preserved)

p95 first-paint <= 1 s on typical install (~20 GGUF files) on warm SSD. Background hashing must NOT block first paint.

### NFR-2: Hashing budget

Typical install: p95 hash-completion <= 60 s on warm SSD. Cold/external HDD: <= 5 min p95 (acceptable as long as progress indicator is visible throughout).

### NFR-3: UI responsiveness during hashing

Key handlers (j/k/Enter/u/d/q/Esc/space) respond within 100 ms even while hashing is in progress.

### NFR-4: No persistent state across launches

Per Q7, the hash queue lives in process memory only. No lockfile or index file written to disk.

### NFR-5: Single source of truth for dedup data

All UI surfaces displaying dedup-related values (summary bar, row glyphs, `[All Unified]` count, footer totals) read from `core::logic::dedup`. Hardcoded values are forbidden.

### NFR-6: Color is not the only channel

Glyphs (`-`, `=`, `#`, `?`, `~`) carry meaning by character alone. Color is decorative. Respects `NO_COLOR`.

### NFR-7: Crash rate must not regress

The new background-hash worker must not increase the crash rate vs v1 baseline. If the worker panics, it must be isolated (the TUI keeps running with affected rows showing `?`).

### NFR-8: False-`#` rate is zero

A row showing `#` (already-unified) must reflect filesystem ground truth at the moment of paint. No stale-cache lies. (Re-classification after action covers this.)

### NFR-9: Privacy-respecting telemetry

KPI-4 retention measurement requires anonymized install_id. Must be opt-in with a clearly-documented config flag. Default: opt-in not assumed; KPI-4 baseline is established only from consenting installs.

---

## Business Rules

### BR-1: Glyph derivation rule

Glyph computation:

```text
if hash_pending: "?"
elif hash_in_progress: "~"
elif hash_failed: "-" with "!" decorator
elif num_paths_sharing_one_inode >= 2 AND that_inode_has_no_separate_copies_elsewhere: "#"
elif num_separate_inodes_with_same_sha256 >= 2: "="
else: "-"
```

### BR-2: Reclaim arithmetic

For an `=` model with N separate-inode copies of size S, the unify reclaim equals `(N - 1) * S` (assuming no cross-fs fallback chooses copy mode).

### BR-3: Conservative-when-uncertain

Per ADR-002, on hash failure, the model is classified as `-` (unique). Never classified as `=` or `#` without successful hash equality.

### BR-4: Pseudo-tool slot is render-only

`[All Unified]` is not a `Tool` impl (ADR-001 forbids new methods). It's a render filter over the existing model set, classified by the existing dedup classifier.

### BR-5: Unify is non-destructive to canonical

The canonical (kept) tool's file is never moved or modified. Only target tools' files are replaced by hardlinks to the canonical inode.

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| **Hashing throughput collapses on slow disks** (Maya scenario) | M | H | NFR-2 sets 5-min p95 ceiling; status-line progress mitigates the "looks broken" failure mode; DESIGN to validate concurrency/IO strategy |
| **Stale-cache lies (false `#`)** when filesystem state changes between launches | L | H | Q7 stateless rediscovery removes most of this risk; NFR-8 sets the gate |
| **Summary bar still drifts from rows** if DESIGN re-introduces a separate aggregator | L | H | NFR-5 single-source rule + integration-test C1 in shared-artifacts-registry catches it |
| **Partial-success path under-reported** | M | M | US-U10 polishes the toast; existing JSONL log is unchanged |
| **`u` from main view dispatches incorrectly** for `#` or `?` rows | M | M | US-U4 ACs cover all five glyph cases explicitly |
| **Performance regression on `[All Unified]` view** with hundreds of models | L | M | DESIGN to consider; not gated by DISCUSS |
| **ADR-001 trait pressure** to add `link_inode` or similar | L | M | BR-4 + ADR-001 frozen; DESIGN must work within existing 6-method trait |

---

## Domain Glossary

| Term | Definition |
|---|---|
| **canonical** | The tool whose copy of a model is kept; other tools' copies are replaced by hardlinks pointing to this inode |
| **dedup-able** | A set of model files with identical SHA256 living on separate inodes — they could share one inode but currently don't |
| **dedup-mate** | One of the duplicate-content copies of a given model |
| **dedup glyph** | The single-character indicator in the row's dedup column: `?`/`~`/`-`/`=`/`#` |
| **hardlink** | A second filesystem name pointing to the same inode; bytes-on-disk are shared |
| **inode** | The filesystem object holding the file's actual data; multiple paths can share one inode |
| **reclaim** | Bytes returned to free disk space by replacing redundant inodes with hardlinks |
| **unified** | A model whose copies across `>=2` tools share one inode (glyph `#`) |
| **unify** | The action: take a `=` model and turn it into a `#` model by hardlinking targets to canonical |

---

## Wave Handoff

### To DESIGN (solution-architect)

Deliverables in this directory:

- `journey-unify-flow-visual.md` — primary journey, ASCII mockups, error paths
- `journey-unify-flow.yaml` — structured schema
- `journey-unify-flow.feature` — Gherkin scenarios
- `journey-unified-view-visual.md` — secondary journey
- `journey-unified-view.yaml` — structured schema
- `journey-unified-view.feature` — Gherkin scenarios
- `shared-artifacts-registry.md` — every `${variable}` with source and consumers
- `story-map.md` — backbone + walking skeleton
- `prioritization.md` — release ordering
- `user-stories.md` — 10 stories with full BDD scenarios
- `requirements.md` — this file
- `acceptance-criteria.md` — consolidated AC traceability
- `dor-checklist.md` — DoR pass/fail per story
- `outcome-kpis.md` — measurable KPIs and instrumentation requirements

DESIGN must answer (without re-litigating closed constraints):

1. Where does the in-process hash queue live? (`modeltap-core` vs `modeltap-app`)
2. What's the concurrency strategy for parallel hashing without IO contention?
3. How does post-unify re-classification trigger from the JSONL event? (full re-classify vs scoped)
4. How does the `[All Unified]` pseudo-tool slot get rendered without polluting the `Tool` trait?

### To DEVOPS (platform-architect)

Consume `outcome-kpis.md` for:

- New `summary_paint` event in launch.log
- Verify/add `unify_dialog_opened`, `unify_completed_full/partial/aborted` events
- Anonymized install_id (opt-in)
- Guardrail thresholds (first-paint p95, false-`#` rate, crash rate)
