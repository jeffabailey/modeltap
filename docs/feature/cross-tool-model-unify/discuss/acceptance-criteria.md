# Acceptance Criteria Traceability: cross-tool-model-unify

Consolidated AC, traced from each story to its UAT scenarios. The DISTILL wave (acceptance-designer) consumes this for the master acceptance suite.

| AC ID | Story | Acceptance Criterion | Source UAT Scenario |
|---|---|---|---|
| AC-U1.1 | US-U1 | First paint <=1 s with rows visible and `?` glyphs | "Hashing starts after first paint" |
| AC-U1.2 | US-U1 | Status line shows live `Hashing N/M` count | "Status line advances as hashes complete" |
| AC-U1.3 | US-U1 | UI key handlers respond <100 ms during hashing | "UI remains responsive during hashing" |
| AC-U1.4 | US-U1 | p95 hash-completion <=60 s on typical warm install | "Hashing completes within budget on typical install" |
| AC-U1.5 | US-U1 | Quit during hashing exits <500 ms with no persistent state | "Quit during hashing shuts down cleanly" |
| AC-U2.1 | US-U2 | No hardcoded `Dedup-able: 0 B` literal in summary_bar.rs | "Summary bar shows computing during hash phase" |
| AC-U2.2 | US-U2 | Bar reads from `core::logic::dedup` | "Bar reads from same source as row glyphs" |
| AC-U2.3 | US-U2 | Bar shows `computing...` during hashing | "Summary bar shows computing during hash phase" |
| AC-U2.4 | US-U2 | Sum of `=`-row sizes equals bar value post-hash | "Bar reads from same source as row glyphs" |
| AC-U2.5 | US-U2 | Honest 0 when no duplicates | "Honest zero when no duplicates" |
| AC-U3.1 | US-U3 | Each row has glyph in fixed column | "Glyph reflects classifier output" |
| AC-U3.2 | US-U3 | Glyphs match legend exactly: `?`/`~`/`-`/`=`/`#` | "Glyph reflects classifier output" + 4 others |
| AC-U3.3 | US-U3 | Glyph derived from `core::logic::dedup` | "Glyph reflects classifier output" |
| AC-U3.4 | US-U3 | Glyph updates reactively, no manual refresh | "Hashing-in-progress glyph is ~" |
| AC-U3.5 | US-U3 | Hash failure shows `-` + `!` decorator + status text | "Hash failure marked but not blocking" |
| AC-U3.6 | US-U3 | Help screen documents legend | (testable via help-screen scenario in DISTILL) |
| AC-U4.1 | US-U4 | `u` handled in main-view row-list handler | "u on = row opens dialog with mates" |
| AC-U4.2 | US-U4 | `=` row: dialog opens with mates pre-populated | "u on = row opens dialog with mates" |
| AC-U4.3 | US-U4 | `#` row: informational dialog | "u on # row opens informational dialog" |
| AC-U4.4 | US-U4 | `-` row: status hint, no dialog | "u on - row shows status hint, no dialog" |
| AC-U4.5 | US-U4 | `?`/`~` row: status hint, no dialog | "u on ? row shows status hint, no dialog" |
| AC-U4.6 | US-U4 | Existing `u`-from-Detail behavior preserved | "u still works from Detail screen (no regression)" |
| AC-U5.1 | US-U5 | Dialog body shows model+sha+canonical+per-target+total | "Dialog shows canonical, targets, and total reclaim" |
| AC-U5.2 | US-U5 | Total reclaim recomputes live as targets toggled | "Toggling a target updates the total" |
| AC-U5.3 | US-U5 | `[space]` toggles target checkbox | "Toggling a target updates the total" |
| AC-U5.4 | US-U5 | `[Enter]` invokes `actions::unify::run()` | "Enter applies the plan" |
| AC-U5.5 | US-U5 | `[Esc]` closes with no filesystem effect | "Esc cancels with no filesystem change" |
| AC-U5.6 | US-U5 | Cross-fs ADR-008 fallback fires when applicable | "Existing cross-fs fallback still fires (ADR-008)" |
| AC-U5.7 | US-U5 | Lsof Q5 detect-and-prompt-then-retry continues to fire | (covered in journey-unify-flow.feature::Tool-in-use) |
| AC-U6.1 | US-U6 | Affected rows re-classify within 200 ms of event | "Successful full unify flips glyph and updates summary" |
| AC-U6.2 | US-U6 | Glyph flips `=` -> `#` on full success | "Successful full unify flips glyph and updates summary" |
| AC-U6.3 | US-U6 | Glyph stays `=` on partial success | "Partial unify leaves glyph as =" |
| AC-U6.4 | US-U6 | Summary `Dedup-able` decreases by reclaimed bytes | "Successful full unify flips glyph and updates summary" |
| AC-U6.5 | US-U6 | `(was X GB)` delta shown for ~5 s then collapses | "Summary bar shows transient delta then collapses" |
| AC-U6.6 | US-U6 | `Unified: N` increments only on full success | "Partial unify leaves glyph as =" |
| AC-U6.7 | US-U6 | No restart required for any update | "No-restart requirement" |
| AC-U7.1 | US-U7 | Slot present in left pane below real tools | "Slot is present in left pane" |
| AC-U7.2 | US-U7 | Badge shows live unified count | "Counts agree across surfaces" |
| AC-U7.3 | US-U7 | Selecting slot filters right pane to `#` rows | "Selecting the slot filters the right pane" |
| AC-U7.4 | US-U7 | Each row shows name, size, tool count, savings | "Row format includes tool count and savings" |
| AC-U7.5 | US-U7 | Footer shows `Unified: N | Total reclaimed: X GB` | "Footer aggregates totals" |
| AC-U7.6 | US-U7 | Badge / summary / row count always equal | "Counts agree across surfaces" |
| AC-U8.1 | US-U8 | Empty state shown when count=0 and hashing complete | "Empty state shown when count is zero" |
| AC-U8.2 | US-U8 | Different message when hashing in progress | "Empty state distinguishes still hashing from truly empty" |
| AC-U8.3 | US-U8 | Guidance includes concrete next step | "Empty state shown when count is zero" |
| AC-U9.1 | US-U9 | `#` model detail shows inode + grouped paths | "Detail shows shared inode for # model" |
| AC-U9.2 | US-U9 | `=` model detail groups paths by inode | "Detail for = model shows multiple inodes" |
| AC-U9.3 | US-U9 | Footer offers `[d]` and `[u]` | (testable via key-handler scenario in DISTILL) |
| AC-U9.4 | US-U9 | Missing-inode case handled gracefully | "Detail handles missing inode info gracefully" |
| AC-U10.1 | US-U10 | Toast lists each target's outcome (OK/FAIL+reason+bytes) | "Toast lists each target's outcome" |
| AC-U10.2 | US-U10 | Toast shows total reclaim from OK targets | "Toast lists each target's outcome" |
| AC-U10.3 | US-U10 | `[r]` retry-failed-only when failures present | "Retry-failed-only re-runs only the failures" |
| AC-U10.4 | US-U10 | Retry only re-runs failed targets | "Retry-failed-only re-runs only the failures" |
| AC-U10.5 | US-U10 | Toast points to launch.log for full detail | "Toast lists each target's outcome" |

---

## Cross-cutting Acceptance (NFRs)

| AC ID | Source | Criterion |
|---|---|---|
| AC-NFR-1 | NFR-1 | First-paint p95 <=1 s preserved (K3 budget) |
| AC-NFR-2 | NFR-2 | Hashing p95 <=60 s typical / <=5 min cold |
| AC-NFR-3 | NFR-3 | UI key handlers respond <100 ms during hashing |
| AC-NFR-4 | NFR-4 | No lockfile or index written to disk |
| AC-NFR-5 | NFR-5 | All dedup-related UI values read from `core::logic::dedup` |
| AC-NFR-6 | NFR-6 | Glyphs distinguishable without color; respects NO_COLOR |
| AC-NFR-7 | NFR-7 | Background-hash worker panic isolated; TUI keeps running |
| AC-NFR-8 | NFR-8 | False-`#` rate is 0 (filesystem-truth at paint time) |
| AC-NFR-9 | NFR-9 | install_id is opt-in via documented config flag |

---

## Cross-artifact Consistency Acceptance

From `shared-artifacts-registry.md` "Cross-Artifact Consistency Tests":

| AC ID | Criterion |
|---|---|
| AC-CONS-1 | `summary_bar.dedup_able_bytes == sum(row.size for row in rows if row.glyph == "=")` |
| AC-CONS-2 | `summary_bar.unified_count == [All Unified].badge_count == count(row.glyph == "#")` |
| AC-CONS-3 | Pane-switch invariance: same model shows same glyph regardless of left-pane selection |
| AC-CONS-4 | `summary_bar.dedup_able_bytes_delta == toast.reclaimed_bytes` after a unify |
| AC-CONS-5 | `dedup_key_progress.completed` is monotonic during a session |
