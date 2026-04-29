# Prioritization: modeltap-tui

## Release Priority

| Priority | Release | Target Outcome | KPI Targeted | Rationale |
|---|---|---|---|---|
| 1 | Walking Skeleton (R0) | End-to-end flow works on a real tool | K3 (time-to-first-model-list) + de-risks K1 | Validates discovery + render + destructive-action loop with the smallest viable surface |
| 2 | R1 — Make duplication visible | Devon sees what could be reclaimed | K2 (% deduplicable) | Riskiest assumption: "users care once they SEE the duplication." Validate before building unify. |
| 3 | R2 — Reclaim disk space safely | Devon successfully unifies models | K1 (GB reclaimed) | Highest single-feature value, but riskiest implementation (hardlinks, cross-fs, running tools); needs R1 to have proven the model. |
| 4 | R3 — Built to grow | A 5th tool can be added without core changes | K4 (supported tool count) | Architectural durability; not user-visible value, but a contractual obligation from intake. |

## Backlog (Story-Level)

| Story | Title | Release | Priority | Outcome KPI Link | Dependencies |
|---|---|---|---|---|---|
| US-01 | TUI opens with stub data and quits cleanly | WS | P1 | K3 | None |
| US-02 | Discover Ollama models | WS | P1 | K1, K2 | US-01 |
| US-03 | Two-pane layout (tools left, models right) | WS | P1 | K3 | US-01, US-02 |
| US-05 | Zap a tool's models with typed confirmation | WS | P1 | K1, K5 (zap safety) | US-02, US-03 |
| US-06 | Show last action and reclaimed bytes | WS | P1 | K1 | US-05 |
| US-07 | Discover llama-cli models | R1 | P2 | K1, K2 | US-02 (plugin shape proven) |
| US-12 | Discover Hugging Face cache models | R1 | P2 | K1, K2 | US-02 |
| US-15 | Discover LM Studio models | R1 | P2 | K1, K2 | US-02 |
| US-04 | Show model size and registered tools | R1 | P2 | K2 | US-03 |
| US-09 | Compatibility indicator (o/*/!) on each row | R1 | P2 | K2 | US-04, plugin capability metadata |
| US-13 | Model detail screen | R1 | P2 | K2 | US-04 |
| US-16 | Format-locked indicator (red `!`) | R1 | P2 | K2 | US-09, plugin capability metadata |
| US-08 | Bottom bar with shortcuts always visible | R2 | P3 | none directly (UX polish) | US-03 |
| US-10 | Unify a model across tools using hardlinks | R2 | P3 | K1 | All discovery stories, US-13, dedup-key (Q6) |
| US-14 | Dry-run preview before unify | R2 | P3 | K5 (unify safety) | US-10 |
| US-17 | Detect running tools and warn before unify/zap | R2 | P3 | K5 | US-05, US-10 |
| US-19 | Hardlink fallback when cross-filesystem | R2 | P3 | K1 | US-10 |
| US-11 | Updated totals after action | R2 | P3 | K1 | US-06 (extends it) |
| US-18 | Plugin trait — adding a 5th tool requires no core changes | R3 | P4 | K4 | All discovery stories (concretizes the abstraction) |
| US-20 | Cross-platform path discovery (macOS + Linux) | R3 | P4 | K3 | All discovery stories |

## Value/Urgency/Effort Scoring (Walking Skeleton + R1)

Scale 1-5. Score = `Value * Urgency / Effort` (rounded).

| Story | Value | Urgency | Effort | Score | Notes |
|---|---|---|---|---|---|
| US-01 | 2 | 5 | 1 | 10 | Tiny. Not valuable alone but a hard prerequisite. |
| US-02 | 4 | 5 | 3 | 7 | First real plugin. Drives the trait shape. |
| US-03 | 4 | 5 | 2 | 10 | Two-pane is the entire UX premise. |
| US-05 | 5 | 4 | 3 | 7 | First mutating action. Forces confirmation UX. |
| US-06 | 3 | 4 | 1 | 12 | Trivial; high payoff for visibility-of-system-status. |
| US-07 | 3 | 3 | 2 | 5 | Second plugin. Confirms plugin trait by repetition. |
| US-04 | 3 | 3 | 1 | 9 | Adds detail to right-pane rows. |
| US-09 | 5 | 4 | 3 | 7 | The duplication-visibility story. Riskiest assumption to validate. |
| US-13 | 4 | 3 | 2 | 6 | Detail screen is where the "aha — 8.8 GB" moment happens. |
| US-12 | 3 | 3 | 3 | 3 | HF cache layout is the most variable; budget extra. |
| US-15 | 2 | 2 | 3 | 1 | LM Studio is a small audience and an undocumented format. |
| US-16 | 3 | 2 | 2 | 3 | Capability-metadata-driven; has compounding effect on US-09. |

## Riskiest Assumption Order

| Order | Assumption | Where validated |
|---|---|---|
| 1 | We can discover and enumerate one tool's models from disk reliably | US-02 (Ollama) |
| 2 | A two-pane TUI with `*`/`o`/`!` indicators legibly conveys the duplication problem | US-09 + US-04 |
| 3 | Hardlinks across tool-specific directories actually work, are seen by all tools, and survive tool restarts | US-10 + US-19 |
| 4 | Plugin trait stays small enough that a contributor can add a new tool without core changes | US-18 |

## Notes on Order Inversions

Two intentional deviations from "highest-score first":

1. **US-05 before US-09** (Zap before duplication-visibility), even though US-09 has higher value-per-effort. Reason: US-05 forces the destructive-action confirmation UX into the walking skeleton, where it gets the most scrutiny. Adding it later means retrofitting safety into a UI that didn't have it.
2. **US-08 (bottom bar) deferred to R2.** The intake brief mandates "all keyboard shortcuts always visible." A bare-bones static bar is in the WS via US-03's mockup; the dedicated US-08 is the polished, context-aware version (greys out unavailable shortcuts, highlights the active one). Not WS-critical.

These deviations are documented so DESIGN can revisit if assumptions change.
