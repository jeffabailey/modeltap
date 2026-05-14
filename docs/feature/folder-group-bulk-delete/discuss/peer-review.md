# Peer Review — folder-group-bulk-delete (DISCUSS wave)

Reviewer persona shift: from requirements analyst (Luna) to independent requirements reviewer applying the 5 critique dimensions per `nw-po-review-dimensions/SKILL.md`.

Mindset: fresh perspective, assume nothing, challenge assumptions, verify stakeholder needs. Brownfield extension scrutinised for additional risk: drift from parent feature, regression on parent invariants, and stranded artefacts.

## Review Iteration 1

```yaml
review_id: "req_rev_20260511_folder_group_bulk_delete_iter1"
reviewer: "nw-product-owner (review mode)"
artifact: "docs/feature/folder-group-bulk-delete/discuss/*"
iteration: 1

strengths:
  - "Brownfield discipline is excellent. Every new artifact explicitly cross-references the parent feature's equivalent (parent journey, parent shared-artifacts-registry, parent ADR-009). The shared-artifacts-registry has separate 'New artifacts' and 'Updated parent artifacts' sections, making the diff scope auditable."
  - "Single-story scope is correctly defended in story-map.md with a 'Why This Is One Story, Not Many' subsection enumerating the bad splits (layer-based, sidecar-deferred, partial-failure-deferred) — head off oversight protest before reviewer can raise it."
  - "Domain examples use real HF repo (bartowski/Llama-3.2-1B-Instruct-GGUF), real file sizes (657 MB ... 2.5 GB), real PIDs (4421), real partial-failure numbers (19/21). No generic data anti-pattern."
  - "Safety rubric is explicit and matches ADR-009: typed-confirmation for bulk-irreversible (mirrors US-05 not US-05b). Rationale documented in requirements NF-FGD-2."
  - "Per-file shared/unique classification is enforced to reuse US-09's compute_compatibility() machinery — single-engine invariant captured both in registry and in AC-13. Prevents drift between row indicator and dialog itemisation."
  - "Open questions for DESIGN (Q-FGD-1, Q-FGD-2, Q-FGD-3) are scoped narrowly: trait shape, concurrency model, dedup-key analogue. None bleed back into DISCUSS territory."
  - "Outcome KPIs include a measurable baseline (K-FGD-2 against current US-05b loop) — not a greenfield 'unknown' which is the usual escape hatch."
  - "Three-tier deletion model (US-05 zap / US-05b delete-one / US-05c folder-delete) is now coherent: each granularity matches a user mental model (whole tool / one file / one repo) and a confirmation strength (typed / [y/n] for shared / typed)."

issues_identified:
  confirmation_bias:
    - issue: "Happy path bias risk on partial-failure UX. The journey describes the partial-failure flow but the post-action mockup shows it as a textual itemisation that may be hard to scan. With 21 files and 8 failures, the right pane scrolls."
      severity: "low"
      location: "journey-folder-group-delete-visual.md Step 4 partial-failure mockup"
      recommendation: "Acceptable for v1 — failures are rare (lsof-detected before; uncommon during). DELIVER may choose to truncate the failed-file list above some threshold. Flag as a known UX trade-off in handoff package; not a DoR failure."

    - issue: "Availability bias — feature mirrors US-05 (zap) typed-confirmation strength even for folders where ALL files are shared. In that pathological case, no inode is ever freed (only HF registrations removed), and US-05b would use [y/n] for the same operation on a single file."
      severity: "low"
      location: "requirements F-FGD-4, NF-FGD-2"
      recommendation: "Trade-off acknowledged: the bulk operation crosses many files and even if each is individually shared, the registration removals together are still irreversible (no undo). Typed-confirmation across the board keeps the rubric simple. Document this trade-off explicitly in wave-decisions.md."

  completeness_gaps:
    - issue: "What happens when a folder header is visible but the underlying directory was deleted out-of-band (user manually rm'd it between launch and Shift+F)? AC-15 covers read-only cache but not 'directory gone'."
      severity: "medium"
      location: "acceptance-criteria.md, requirements F-FGD-8"
      recommendation: "Add an AC: 'If the folder's absolute_path no longer exists at execution time, the dialog refuses with 'folder no longer exists — inventory will refresh' and prompts a re-discovery.' This is a real edge case in stateless-rediscovery model (intake Q7). Captured below as Required Fix #1."

    - issue: "No AC for the case where the user is currently INSIDE the folder-delete dialog and a different action shouldn't be possible (e.g., Tab still works to focus the input field, but pressing 'z' for zap should be inert while modal is open)."
      severity: "low"
      location: "acceptance-criteria.md"
      recommendation: "Implicit per ratatui modal-dialog conventions and the parent's existing dialog handling. Mention in handoff package but do not require a new AC — it is enforced by event-loop design, not requirements."

    - issue: "No NFR for what 'sweep sidecars' means when an HF repo has hundreds of sidecar files (e.g., a model with many quantisation-specific .imatrix sidecars or many README translations). Performance assumption is ≤500 folder groups but not 'files per folder'."
      severity: "low"
      location: "requirements NF-FGD-1"
      recommendation: "Add a note: 'Per-folder file count assumption: typical HF repo has 1-30 files. Folders with >100 files use a progress bar but no special handling.' Captured below as Required Fix #2."

    - issue: "No story for the user observing folder grouping WITHOUT intending to delete (i.e., the grouping is also valuable as a comprehension tool). Is the grouping always on, or toggleable?"
      severity: "medium"
      location: "requirements F-FGD-1"
      recommendation: "Specify: grouping is ALWAYS on for the HF plugin (it is a property of the HF cache layout, not an opt-in display mode). Folders with 1 file collapse to look identical to the existing row format. Captured below as Required Fix #3."

    - issue: "No regression-test guidance for parent feature's existing scenarios. Cross-feature integration AC INT-FGD-8 says they 'continue to pass' but doesn't say which existing scenarios are at risk."
      severity: "low"
      location: "acceptance-criteria.md INT-FGD-8"
      recommendation: "Parent's US-04 (row format), US-12 (HF discovery), US-13 (detail screen entry from row) are the highest-risk regression surfaces. Flag in handoff package; DISTILL wave will pick up the regression scenarios."

  clarity_issues:
    - issue: "The term 'shared' is now overloaded. In US-05b it means 'this single model file is registered with another tool'. In US-05c it means the same thing per file BUT the folder-delete dialog also says 'unique' meaning 'unique to HF'. A reader may confuse 'unique to HF cache' with 'unique among many files in the folder' (i.e., a unique filename)."
      severity: "medium"
      location: "requirements F-FGD-3, journey TUI mockups"
      recommendation: "In the dialog body, prefer 'only registered with Hugging Face' over 'unique' on first read; the breakdown line can then use 'unique' as a shorthand. Captured below as Required Fix #4."

    - issue: "The hotkey is documented as 'Shift+F' (rendering as `[F]` in the bottom bar). Parent uses lowercase letters (`[d]`, `[z]`, `[u]`). The uppercase convention is new — clarify so DESIGN doesn't substitute another key."
      severity: "low"
      location: "wave-decisions.md (not yet written)"
      recommendation: "Document the hotkey decision explicitly in wave-decisions.md with rationale: 'Shift+F chosen to (a) avoid collision with [d] single-file and [z] whole-tool, (b) signal bulk-but-not-whole-tool granularity by being capital, (c) read as 'folder' mnemonically.' Captured below as Required Fix #5."

  testability_concerns:
    - issue: "AC-13 ('Per-file unique-vs-shared classification uses compute_compatibility()') is observably enforced by code-review/grep but not by a runtime test."
      severity: "low"
      location: "acceptance-criteria.md US-05c.AC-13"
      recommendation: "Acceptable as an architectural-style AC (parallel to parent US-18.AC-3 'no changes to modeltap-core source files'). DISTILL wave should add a contract test: 'fuzz: for random folder + inventory, folder_group.classify_unique_vs_shared() agrees with compute_compatibility() per file.' Flag for DISTILL handoff."

    - issue: "K-FGD-2 (keystrokes per repo delete) target reads '~35 keystrokes total for a typical <author>/<repo> (~30 chars)' but does not specify what counts as a 'keystroke' for measurement (modifier keys? backspace corrections?)."
      severity: "low"
      location: "outcome-kpis.md K-FGD-2"
      recommendation: "Specify in outcome-kpis.md: 'Keystroke count = total key events received by the dialog input handler from open to Enter, including modifier-only events (Shift), excluding shell-side preprocessing (terminal escape codes). Corrections (backspace) count.' Captured below as Required Fix #6."

  priority_validation:
    q1_largest_bottleneck: "YES"
    q2_simple_alternatives: "ADEQUATE"
    q3_constraint_prioritization: "CORRECT"
    q4_data_justified: "JUSTIFIED"
    verdict: "PASS"

    notes: |
      Q1: HF-repo cleanup is documented as Devon's most frequent cleanup task (5-10 new repos/month). 21+ keystroke ceremony today is the bottleneck. Confirmed in requirements business context.

      Q2: prioritization.md "Out of Scope" enumerates rejected alternatives (folder-delete for other plugins; dry-run shortcut; CLI flag; trash bin) with reasons. Adequate.

      Q3: HF-only-in-v1 constraint is correctly prioritised. The user-mentioned "we don't generalise to Ollama" (intake scope constraint #1) is honoured. No minority constraint dominating.

      Q4: KPI baselines are measured (K-FGD-2 against current US-05b loop) or measurable (K-FGD-1 timing). K-FGD-3 baseline = 0 because feature doesn't exist. Data-justified.

approval_status: "conditionally_approved"
critical_issues_count: 0
high_issues_count: 0
medium_issues_count: 3
low_issues_count: 8

required_fixes_before_approval:
  - id: "RF-1"
    severity: "medium"
    description: "Add AC for the 'folder directory gone out-of-band' case. The stateless-rediscovery model (intake Q7) makes this a real edge case."
    location: "acceptance-criteria.md, requirements F-FGD-8"

  - id: "RF-2"
    severity: "low"
    description: "Add NFR note about files-per-folder assumptions (typical 1-30; >100 still works but uses progress bar)."
    location: "requirements NF-FGD-1"

  - id: "RF-3"
    severity: "medium"
    description: "Clarify that folder grouping is always on for HF (not toggleable); single-file folders collapse to a one-row form."
    location: "requirements F-FGD-1"

  - id: "RF-4"
    severity: "medium"
    description: "Use 'only registered with Hugging Face' on first read in dialog body; reserve 'unique' as a shorthand in the breakdown line. Avoids overloading 'unique' against 'unique filename'."
    location: "journey-folder-group-delete-visual.md (Step 2 mockup), requirements F-FGD-3"

  - id: "RF-5"
    severity: "low"
    description: "Document the Shift+F hotkey choice + rationale in wave-decisions.md so DESIGN does not substitute another key."
    location: "wave-decisions.md (to be written)"

  - id: "RF-6"
    severity: "low"
    description: "Specify what 'keystroke' means for K-FGD-2 measurement (count of key events received by dialog input handler from open to Enter)."
    location: "outcome-kpis.md K-FGD-2"

recommendation: |
  Conditionally approved pending RF-1 through RF-6. All fixes are surgical edits to existing
  files — no new artifacts required. Total estimated rework: < 30 minutes.

  After fixes, status moves to "approved" and DESIGN handoff is unblocked.
```

---

## Review Iteration 2 (post-fix)

After applying the six required fixes, the reviewer re-checks each one. See the "Resolution log" below.

```yaml
review_id: "req_rev_20260511_folder_group_bulk_delete_iter2"
reviewer: "nw-product-owner (review mode)"
artifact: "docs/feature/folder-group-bulk-delete/discuss/*"
iteration: 2

resolution_log:
  - fix_id: "RF-1"
    status: "RESOLVED"
    evidence: "acceptance-criteria.md now includes US-05c.AC-20 covering folder-no-longer-exists. requirements F-FGD-8 extended with the second pre-flight check."

  - fix_id: "RF-2"
    status: "RESOLVED"
    evidence: "requirements NF-FGD-1 now states typical 1-30 files per folder; >100 uses progress bar without special handling."

  - fix_id: "RF-3"
    status: "RESOLVED"
    evidence: "requirements F-FGD-1 now explicitly states 'grouping is always on for the HF plugin'; one-file folders render in a one-row collapsed form."

  - fix_id: "RF-4"
    status: "RESOLVED"
    evidence: "journey-folder-group-delete-visual.md Step 2 dialog mockup now reads 'only registered with Hugging Face' on first line; subsequent lines use the 'unique' shorthand. requirements F-FGD-3 updated to match."

  - fix_id: "RF-5"
    status: "RESOLVED"
    evidence: "wave-decisions.md documents the Shift+F decision with three-reason rationale and the rejected alternatives."

  - fix_id: "RF-6"
    status: "RESOLVED"
    evidence: "outcome-kpis.md K-FGD-2 row now includes a Measurement Definition footnote specifying keystroke counting rules."

approval_status: "approved"
critical_issues_count: 0
high_issues_count: 0
medium_issues_count: 0
low_issues_count: 0

final_recommendation: |
  All six required fixes resolved. DoR re-validates (9/9 PASS for US-05c — see dor-checklist.md).
  Brownfield discipline is strong. Open questions Q-FGD-1, Q-FGD-2, Q-FGD-3 are correctly
  scoped to DESIGN.

  APPROVED for DESIGN handoff (solution-architect).
```

## Reviewer's Final Statement

The DISCUSS wave for folder-group-bulk-delete is **APPROVED** after one iteration of revisions. The brownfield extension is correctly disciplined: it inherits the parent's vocabulary, safety rubric, and emotional-arc rules without re-inventing them, and it identifies the three places where new artifacts and trait extensions are needed without prescribing the technical answers (those go to DESIGN as Q-FGD-1, Q-FGD-2, Q-FGD-3).

The single-story scope is correctly defended. The KPIs are measurable today against the existing US-05b loop. The safety rubric matches ADR-009. Cross-tool hardlink preservation is the riskiest assumption and is correctly elevated to an integration checkpoint.

Hand off to solution-architect.
