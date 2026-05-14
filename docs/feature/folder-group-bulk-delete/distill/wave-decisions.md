# Wave Decisions — folder-group-bulk-delete (DISTILL)

**Wave:** DISTILL (5 of 6)
**Author:** Quinn (nw-acceptance-designer)
**Date:** 2026-05-11

Decisions made during DISTILL that affect the executable specification beyond what DISCUSS / DESIGN / ADR-010 already locked. These are NOT architectural decisions (those belong in ADRs); they are acceptance-test-shape decisions inside the locked-upstream constraints.

---

## D1 — Walking Skeleton Strategy

**Decision: Strategy B (real I/O against fixture-populated temp dirs).** Inherits from parent `modeltap-tui` acceptance-test-plan §7.

**Rationale:** modeltap is a local desktop CLI; "real I/O" means real filesystem operations against synthetic-but-realistic temp trees. The walking skeleton must answer "can Devon delete a folder and reclaim disk?", which requires real `unlink(2)` against a real tempdir. No costly external dependency to mock — no cloud, no network, no third-party service. The same strategy the parent uses; no reason to diverge.

**Consequence on critique-dimension 9 (Walking Skeleton Boundary Proof):**
- 9a: Strategy declared (here). PASS.
- 9b: WS implementation matches strategy (M1 uses `@real-io` + `@adapter-integration`, no `@in-memory`). PASS.
- 9c: Every NEW driven adapter has a real I/O test. The new adapter is `HfPlugin::delete_folder` (+ its `enumerate_sidecars` + `delete_one_at` reuse). M1 walking skeleton covers it via `@adapter-integration`. PASS.
- 9d: Litmus test "if we delete the real adapter, would the WS still pass?" — NO, the WS asserts on `path.exists() == false` against the real tempdir. PASS.
- 9e: No `@in-memory` tag appears on M1 or any walking skeleton scenario. PASS.

---

## D2 — Typed-path trailing slash handling

**Decision: trailing slash is treated as a MISMATCH (cancel with no destructive action).**

DISCUSS / acceptance-criteria locks "byte-exact, case-sensitive" comparison (AC-8). A trailing slash is a different string of bytes — therefore mismatch — therefore cancel.

**Why not normalize?** Three reasons:
1. **Safety-first principle (D-FGD-2):** ambiguity in the confirmation comparator is the precise failure mode the typed-confirm pattern exists to prevent. "Normalize on the user's behalf" introduces a class of "I added a slash; the system deleted what I expected, but if I'd added something else it would have done what I didn't expect" cognitive load.
2. **The dialog tells the user the exact string.** The dialog prompt is "Type `bartowski/Llama-3.2-1B-Instruct-GGUF` to confirm". The user-friendly path is to type what's shown — no normalization needed.
3. **The K-FGD-3 guardrail (mis-target rate < 1%)** is easier to achieve with a strict comparator than a forgiving one — a forgiving comparator widens the surface where "I typed something close but wrong" reaches a destructive path.

**Scenario covered:** `features/folder-group-delete.feature` M2 "Typed path with trailing slash is treated as mismatch" (`@ac-8`).

---

## D3 — Key-count upper bound for M6

**Decision: assert `keystroke_count <= 40` for the 20-file folder scenario.**

**Computation (per outcome-kpis.md K-FGD-2 measurement definition):**
- `Shift+F` itself is the dialog opener and is counted at the parent (it transitions the app from main view to dialog state). **Not counted** in the dialog's keystroke total per the K-FGD-2 definition.
- Typed path `bartowski/Llama-3.2-1B-Instruct-GGUF` is 33 characters. The repo-path-length-30-chars figure in outcome-kpis.md was an estimate; the actual M1 walking-skeleton fixture uses a 33-character path. Use 33 as the typical-typed-length.
- Enter: 1 key.
- Typical typed total: 33 + 1 = **34 keys**.
- Allowance for a few corrections (Backspace / Ctrl+W): +6 keys.
- Recommended assertion bound: **40 keys**.

The 40-key bound is reachable without corrections; corrections can push it above 40 in practice. **DELIVER may tighten to 36 after first measurement** of typical user corrections; the spec is conservative.

**Note on file-count independence:** the same scenario for a 5-file folder also asserts `keystroke_count <= 40`. The 20-file vs 5-file equality is the property — keystroke count does NOT scale with file count. This is the entire point of the feature.

---

## D4 — EBUSY simulation seam

**Decision: use a `MODELTAP_TEST_EBUSY_PATHS` env var seam in the HF plugin's `delete_one_at` wrapper.**

The portable, low-ceremony approach. Alternatives considered:

1. **Real `flock(LOCK_EX)` from a sibling helper process** — works on Linux; macOS unlink-while-open semantics differ; cross-platform CI brittleness.
2. **A `FakeFsOps` adapter behind a new port** — most pure-architecturally; adds infrastructure overhead disproportionate to the test surface (one scenario family).
3. **`MODELTAP_TEST_EBUSY_PATHS` env var** — single line of test code in the plugin (`if env::var("MODELTAP_TEST_EBUSY_PATHS").contains(this_path) { return Err(EBUSY) }`), zero impact on production code path. **Chosen.**

The seam is gated behind `#[cfg(any(test, feature = "test-harness"))]` to ensure release builds do not include it. DELIVER may choose `cfg(test)` if the test infrastructure can access it from integration tests; if not, the `test-harness` feature is the fallback.

**Rejected for the contract test 3.11.S.4:** the contract test uses chmod-based permission-denied (mode 0555 directory) instead of EBUSY. Permission denied is portable (Linux + macOS), requires no test seam, and exercises a distinct error class. Both failure modes are tested: EBUSY at Layer A (M4 first scenario), permission-denied at Layer A (M4 third scenario) AND Layer B contract.

---

## D5 — Plugin contract test for M5 — Layer A vs Layer B split

**Decision: M5 (capability boundary) appears at BOTH layers.**

- **Layer A** (`features/folder-group-delete.feature` M5 Scenario Outline): asserts user-observable behavior — "if the orchestrator dispatches a folder-delete against Ollama, the right pane shows `Ollama does not support folder-delete`". This is the journey-completeness assertion (Mandate 3).
- **Layer B** (`plugin-contract-spec.md` §3.11.U.1): asserts trait-level behavior — `Ollama.delete_folder(&plan).await == Err(DeleteError::Unsupported)`. This is the contract assertion (CM-A: through the public port).

Both required. The E2E proves the orchestrator surfaces the error; the contract test proves the plugin returns the right thing in the first place.

**Edge case acknowledged:** AC-5 says "Shift+F on a non-HF tool is a no-op" — meaning the keymap dispatch should NEVER reach a non-HF plugin's `delete_folder` in the live UI. The M5 Layer A scenario is therefore mostly defensive — it asserts what happens IF the dispatch somehow reached a non-HF plugin (e.g., via a future bug or a 5th-plugin scenario). DELIVER may downgrade the assertion to "the dispatch never reaches the non-HF plugin's `delete_folder` in the first place" — that is, M5 Layer A asserts via JSONL log "no folder-delete dispatch occurred for non-HF tool" instead of "the dialog says Unsupported". Either is acceptable; spec records the more-restrictive form (user-observable text) as the primary, the JSONL form as the fallback.

---

## D6 — `@property` tags and DELIVER's proptest discretion

**Decision: scenarios tagged `@property` may be implemented as proptest invariants OR as concrete-example E2E scenarios; DELIVER chooses.**

The `@property` tag signals "this is a universal invariant; consider proptest". The four `@property`-tagged scenarios in this feature:

1. M3 "For any folder, per-file classification matches compute_indicator on every child" (AC-13).
2. M6 "Every aborted typed-confirmation results in zero filesystem mutations" (AC-8).
3. integration-checkpoints "For any folder, file_count equals models plus sidecars" (INT-FGD-2).
4. integration-checkpoints "For any folder, total_bytes equals reclaim plus retain" (INT-FGD-3).
5. integration-checkpoints "The typed-confirmation comparator reads folder_group.path, not a hardcoded literal" (INT-FGD-7).

For #1, #3, #4: DELIVER **should** implement as proptests in `modeltap-core/tests/folder_group_proptest.rs` — they are pure-function invariants over generated `Inventory` / `FolderGroup` values.

For #2: keep as E2E (the cardinality of "any input" is large, but the test can exercise a handful of mutation classes — wrong prefix, wrong case, extra char, missing char — which is what the cucumber-rs scenario captures).

For #5: a hybrid — a single E2E scenario asserts the comparator works correctly; a lint test (DELIVER-owned) in `crates/modeltap-tui/tests/lint.rs` asserts no string literal matching `<author>/<repo>` appears inline in the dispatch code.

---

## D7 — Scenario count vs critique-dimension Dim 1 (error-path ratio)

**Decision: 15 scenarios in `folder-group-delete.feature`, of which 9 are error/edge paths (60%).** Plus 10 invariant scenarios in `integration-checkpoints.feature`.

**Error-path count:**
- M2 (4 scenarios): 4 error paths (wrong path, Esc, trailing slash, no-op on non-HF).
- M3 (4 scenarios): 1 error/edge (the @property scenario asserting classification correctness across edge cases).
- M4 (3 scenarios): 3 error paths (EBUSY, retry-after-partial, permission-denied).
- M5 (1 scenario): 1 error path (Unsupported — though it is also the happy path for non-HF plugins).
- Total error/edge: 9 of 15 = 60%.

**Above the 40% minimum** by a comfortable margin. The feature is intrinsically error-path-heavy because the typed-confirm pattern's whole purpose is to make destructive ops hard to invoke accidentally.

---

## D8 — Skipped DEVOPS context

**Decision: no environments.yaml consulted; Dim 8 Check B treated as "not applicable" for this feature.**

DEVOPS artifacts do not exist for `folder-group-bulk-delete/` (per wave-config note: skip DEVOPS context). The parent's `modeltap-tui` features inherit Strategy B and the same fixture mechanism; this feature adds 6 new named fixtures within the same env contract. No new environment targets, no new coexistence requirements, no platform-specific code paths beyond what the parent already handles (macOS / Linux / WSL).

**Consequence on critique-dimension 8 Check B:** flagged "not applicable" rather than "blocker" or "high". The reviewer (Sentinel) is asked to confirm this is acceptable for a brownfield additive feature.

---

## D9 — Three follow-ups deferred to DELIVER

These are listed in `acceptance-test-plan.md` § 8 as DELIVER decisions; recapped here for explicit handoff visibility:

1. The exact `enumerate_sidecars` suffix list in the HF plugin (AC-14 / B-FGD-2).
2. Whether the EBUSY seam is gated by `cfg(test)` or by a `test-harness` cargo feature.
3. Whether the M5 Layer A assertion is "right pane shows Unsupported text" or "JSONL log records no dispatch occurred".

None are blockers for DELIVER; the spec accommodates both choices in each case.

---

## D10 — Reviewer escalations (anticipated)

Two findings that may surface during peer review which the acceptance-designer-reviewer (Sentinel) should be aware are SCOPED OUT:

1. **KPI measurability (escalate to PO-reviewer at DELIVER post-merge gate):** the K-FGD-1 (latency p50 ≤ 15s) target is asserted in `outcome-kpis.md` but is NOT a Layer A scenario in this DISTILL because user wall-clock latency depends on per-machine perf — it is a quarterly aggregate target. The M6 keystroke-count scenario covers the K-FGD-2 measurable surface. If a reviewer wants K-FGD-1 in a scenario, that is `@escalate:po-reviewer` per critique-dimensions § Reviewer Scope Boundaries.

2. **Infrastructure readiness (escalate to PA-reviewer at DEVOPS-to-DISTILL handoff):** the `MODELTAP_TEST_EBUSY_PATHS` env-var seam is a test infrastructure choice. If a reviewer wants it modeled as a port instead, that is `@escalate:pa-reviewer`.
