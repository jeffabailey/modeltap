# Acceptance Review (self-review): cross-tool-model-unify

Author: Quinn (nw-acceptance-designer), DISTILL wave (5 of 6).
Reviewer: self, applying the 9-dimension critique from
`~/.claude/skills/nw-ad-critique-dimensions/SKILL.md`.

Per the parent agent's contract, scenario count > 3 so the full review
applies (no fast-path).

---

## Dim 1: Happy Path Bias

**Status: PASS.**

Counts (43 total scenarios in master-acceptance.feature):

- Happy / observable-success scenarios: ~19 (e.g., "Successful full unify
  flips glyph", "Dialog body shows canonical, targets, savings, and total
  reclaim", `[All Unified]` row format, walking skeleton, etc.).
- Error / boundary / negative scenarios: ~24 (e.g., "u on - row shows
  status hint, no dialog", "u on ? row", "Hash failure row shows '!'",
  "Esc cancels with no fs change", "Honest 0 when no duplicates",
  "Partial unify leaves glyph as =", "Total-failure toast shows zero
  reclaim", "Quitting during hashing exits cleanly", "Hashing-progress
  empty state", "Detail handles missing inode", "Cross-fs fallback fires").

Error/boundary ratio: ~56% (target: >=40%). Comfortably above.

---

## Dim 2: GWT Format Compliance

**Status: PASS.**

Every scenario is a single Given/When/Then triple (with multi-step Givens
and Thens, but a single When per scenario). The walking skeleton has
multiple When/Then beats (Devon launches, then hashes complete, then
Devon highlights and presses u, then presses Enter), which is acceptable
per BDD methodology for a vertical-slice scenario; the linear narrative is
preserved and each beat names a single action.

Negative scenarios use parallel structure ("When Devon presses 'u'... Then
no dialog opens"); each "Then" asserts an observable outcome, not the
absence of a method call.

---

## Dim 3: Business Language Purity

**Status: PASS (with one caveat).**

Grep audit of the master-acceptance.feature for technical terms:

- `database`, `API`, `HTTP`, `REST`, `JSON`, `controller`, `Lambda`,
  `Redis`, `Kafka`: ZERO occurrences.
- `inode`: 4 occurrences. **Caveat**: "inode" is technical, but it is the
  Devon-domain concept by which Devon understands "shared vs. separate
  copies" — see `journey-unified-view.feature` and the v1 master-acceptance
  for prior precedent. The DISCUSS artifacts use it consistently as a
  domain term, not an implementation term. Acceptable.
- `SHA256`: 2 occurrences. Same justification — this is the dedup-key
  concept Devon refers to in interviews. The Gherkin uses it only in the
  WS dialog-body assertion ("model name and SHA256 prefix") and in US-U1
  ("SHA256 hashing"). Acceptable per DISCUSS vocabulary.
- `hardlink`: 1 occurrence in the WS scenario ("a real disk-saving
  hardlink unify"). Devon's term, used throughout DISCUSS. Acceptable.
- Status codes / HTTP verbs: ZERO.

The Rust integration test files contain technical terms (Rust types like
`PathBuf`, `Command`, `tempfile::TempDir`) — but those are the Layer 2
"step methods" that delegate to the production binary; per Mandate 2's
three-layer model, Gherkin (Layer 1) is the surface that must stay pure.
Verified pure.

---

## Dim 4: Coverage Completeness

**Status: PASS.**

`docs/feature/cross-tool-model-unify/discuss/user-stories.md` enumerates
US-U1..US-U10. Every story has at least one `@us-uN` tagged scenario:

- US-U1: 5 scenarios + WS coverage.
- US-U2: 3 scenarios.
- US-U3: 6 scenarios.
- US-U4: 5 scenarios.
- US-U5: 5 scenarios.
- US-U6: 3 scenarios.
- US-U7: 5 scenarios.
- US-U8: 2 scenarios.
- US-U9: 3 scenarios.
- US-U10: 3 scenarios.

Cross-cutting AC-NFR-* and AC-CONS-* are covered or `@property`-tagged for
DELIVER's proptest implementation.

---

## Dim 5: Walking Skeleton User-Centricity

**Status: PASS.**

Litmus test for `@walking-skeleton` scenario "Devon reclaims disk by
unifying a duplicated model from the main view":

1. Title describes user goal: **YES** ("reclaims disk by unifying", not
   "end-to-end flow through all layers").
2. Given/When/Then describe user actions: **YES** ("Devon launches",
   "Devon highlights and presses u", "Devon presses Enter").
3. Then steps describe user observations: **YES** ("Devon sees both rows",
   "the summary bar shows...", "the model row glyph flips...", "launch.log
   records an 'action.unify' event"). The launch.log assertion is a
   stretch — it's an observable for Riley (the auditor persona), not Devon
   directly. Justifiable because the JSONL events are part of the
   acceptance contract per kpi-instrumentation.md.
4. Non-technical stakeholder can confirm: **YES** — see
   `walking-skeleton.md` "Demo-ability" section. Six-step demo, no Rust
   types, observable on screen.

---

## Dim 6: Priority Validation

**Status: PASS.**

Luna's prioritization (`discuss/prioritization.md`) explicitly designates
US-U1..U7 as P1 (walking-skeleton release) and US-U8..U10 as P2 (polish).
This DISTILL artifact's `@walking-skeleton` tag covers the smallest slice
of US-U1..U6 that exercises the v1-bug fix end-to-end. US-U7
(`[All Unified]` slot) is in P1 but not on the WS critical path — Luna's
prioritization explicitly orders it as a parallel branch (US-U7 ships
alongside US-U4 in step 3b). Treating it as a focused-scenario set
(`@us-u7 @skip`) rather than part of WS matches that prioritization.

---

## Dim 7: Observable Behavior Assertions

**Status: PASS.**

Audit of every Then step in every scenario:

- `inode` checks (post-unify): observable via `stat()`, the user-visible
  proof of unification. Pass.
- `summary bar shows ...`: observable in the rendered frame. Pass.
- `row glyph is "..."`: observable in the rendered frame. Pass.
- `dialog opens`, `dialog body shows ...`: observable. Pass.
- `launch.log records ...`: observable in the JSONL log file (Riley's
  auditor surface). Pass.
- `Devon has not restarted modeltap`: observable temporal property (one
  process invocation). Pass.

Zero "method called", "internal field set", or "private function invoked"
assertions in Gherkin or Rust scaffolds. Sample Rust assertions:
- `assert_eq!(post_a, post_b, ...)` — comparing two `stat().ino()` values
  (filesystem-observable).
- `events.iter().any(|e| e.get("event") == Some("action.unify"))` — JSONL
  log content (auditor-observable).
- `frame.contains("[All Unified]")` — rendered TUI frame text (Devon-
  observable).

No internal-state assertions.

---

## Dim 8: Traceability Coverage

### Check A — Story-to-Scenario mapping

**Status: PASS.**

Every story ID US-U1..US-U10 has at least one `@us-uN` tagged scenario in
`master-acceptance.feature`. See `test-scenarios.md` for the full mapping.

### Check B — Environment-to-Scenario mapping

**Status: PASS WITH NOTE.**

`docs/feature/cross-tool-model-unify/devops/environments.yaml` does not
exist. Per the parent agent's instructions, this is a brownfield extension
on shipped v1; CI infra is reused. The default environment set (`clean`,
`with-pre-commit`, `with-stale-config`) is the contract per the
critique-dimensions skill, but those defaults are about pre-commit hooks
and stale configs — not relevant to a TUI feature whose "environment" is
the layout of per-tool model directories.

The Rust integration tests use real, distinct environment fixtures (the
two-blob duplicate, the pre-unified two-tool, the single-tool unique),
each of which exercises a different filesystem-state precondition. This
matches the SPIRIT of the environment-to-scenario mapping (per-environment
preconditions named in Givens) without inheriting the literal defaults
that don't apply.

NOTE: this is a brownfield-feature exception, not a generalizable pattern.

---

## Dim 9: Walking Skeleton Boundary Proof

### 9a — WS Strategy Declaration

**Status: PASS.**

Per the parent agent's intake: "Real services (real plugin discovery, real
filesystem fixtures in tempdirs, real `tokio` runtime)". This is Strategy C
(real adapters, real I/O at the WS boundary). The WS Rust scaffold is
tagged `@real-io @adapter-integration`.

### 9b — Strategy-Implementation Match

**Status: PASS.**

The walking-skeleton Rust test:
- Builds a real on-disk fixture in `tempfile::tempdir()`.
- Spawns the real modeltap binary via `assert_cmd::Command::cargo_bin`.
- Uses the real plugin set (4 production plugins + atomic-chat-fixture if
  enabled, but for the WS we pin atomic-chat to /nonexistent so only
  ollama and hf load).
- Asserts on real `stat().ino()` values and real JSONL log lines.

No `@in-memory` tag on the WS scenario. No mock plugin in the WS file.

### 9c — Adapter Integration Coverage

**Status: PASS for the new feature scope.**

The new feature does NOT introduce new driven adapters — it reuses every
v1 adapter (Ollama plugin, HF plugin, LM Studio plugin, Atomic Chat
plugin, lsof adapter, filesystem). Each of those already has a v1
acceptance test exercising it against real I/O. The new code is:
- `hash_pool` (in `modeltap-app`): uses the existing `Sha2Hasher` and
  `Sha256Cache`. Both already have unit tests.
- `core::logic::dedup` extensions: pure functions; testable as units in
  modeltap-core's existing test suite.
- `render::all_unified`, `render::row` updates, `update::handle_msg`
  extensions: pure update / render layer; testable via TestBackend
  snapshot tests during DELIVER.

The walking skeleton exercises ALL of these against real I/O.

### 9d — Walking Skeleton Fixture Tier

Litmus: "If I deleted the real adapter, would this WS still pass?"
Answer: NO. The WS asserts `stat().ino()` on the real filesystem; if the
hardlink-creating adapter were a fake, the inodes wouldn't merge and the
test would fail. Pass.

### 9e — Strategy Drift Detection

`grep @in-memory docs/feature/cross-tool-model-unify/distill/features/`:
zero occurrences. The WS scenario uses real adapters consistently.

---

## Issues identified

None at blocker / high severity.

Notes (informational, not findings):

1. **Harness extension (NOT a new env-var seam)**: several `@skip`
   scenarios will require a "wait-for-hashing-complete" sentinel token in
   `headless.rs::tokenize_script`. This is a one-line addition to the
   tokenizer, NOT a new env-var seam. Per the parent agent's contract,
   tokenizer additions are inside the existing harness (allowed); new env
   vars are forbidden. The crafter should add the sentinel during DELIVER.

2. **"Force hash failure" mechanism**: AC-U3.5 ("Hash failure shows '-' +
   '!' decorator") and AC-U6.3 ("Partial-success") need deterministic
   ways to induce failures. Acceptable mechanisms (no new env-var seam):
   chmod 000 on a fixture file, or a fixture path that does not exist.
   Crafter chooses during DELIVER.

3. **Two helper smoke tests in `us_u1_walking_skeleton.rs`** are NOT
   `#[ignore]`d (they pass today as fixture-builder tripwires). They are
   not Gherkin scenarios; they are sanity probes. Total ignored RED test
   count remains 26, equaling the count of new ignored scenarios.

---

## Approval status

**APPROVED for handoff to DELIVER (software-crafter).**

The acceptance suite establishes the outer-loop contract for the
cross-tool-model-unify feature. The walking-skeleton scenario is
demo-able to a non-technical stakeholder. All seven P1 stories have Rust
RED scaffolds; all ten stories have Gherkin coverage. Mandate compliance:

- **CM-A** (driving ports only): every Rust scaffold drives the production
  binary via `assert_cmd::Command::cargo_bin("modeltap")`. No internal
  imports of `modeltap_core::*` or `modeltap_tui::*` types in the new
  acceptance files. (The WS file imports are `assert_cmd`, `serde_json`,
  `tempfile`, `std::fs`, `std::os::unix::fs::MetadataExt`, `std::path`,
  `std::time` only.)
- **CM-B** (business language): Gherkin grep clean of HTTP/JSON/DB/etc
  terms; technical terms limited to Devon-domain vocabulary (inode,
  SHA256, hardlink) per DISCUSS conventions.
- **CM-C** (user journeys): the WS scenario delivers observable user value
  (disk reclaimed); focused scenarios are independently demo-able units of
  user behavior. Walking-skeleton count: 1; focused/error scenarios: 32.
- **CM-D** (pure-function extraction): not directly applicable to this
  acceptance layer (Mandate 4 is a unit-test layer concern). The DESIGN
  artifact already extracts `compute_dedup_glyph`, `collect_unified_rows`,
  `dedup_summary` as pure functions in `core::logic::dedup` (per
  data-models.md), which the crafter will test directly during DELIVER's
  inner loop.
