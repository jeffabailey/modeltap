# Intake Brief — tool-model-info-sqlite-cache

**Date**: 2026-05-16
**Wave entry point**: `/nw:new` → recommended next: `/nw:discuss`
**Status**: Intake captured; awaiting DISCUSS wave.

## User request (verbatim)

> Add an ability to get information about each tool and each model. Let's also refactor this code so it stores information about all the data collected from the tools and models in a local sqlite database.

## What this feature covers (parsed)

1. **New capability — per-tool and per-model information views.** Detailed inspection of a single tool (status, version, install path, capabilities, model count, disk usage, etc.) and a single model (size, quantization, SHA256, source repo, dedup peers across tools, file path, parameters, etc.). UX surface unknown — could be a third pane, a modal, a new screen, or an `info` CLI subcommand. To be defined in DISCUSS.

2. **Architectural refactor — local SQLite persistence layer.** All data currently collected during stateless rediscovery (per-tool scans, file lists, SHA256s, dedup groupings, capabilities) becomes persisted in a local SQLite database, instead of (or in addition to) being rebuilt in-memory on every launch.

## Reconciliation required with prior design

The persistence ask **directly inverts** a closed DESIGN constraint documented in `CLAUDE.md`:

> "**Stateless rediscovery on every launch.** Per Q7. No persistent index file. Each launch walks every tool's directory."

This means DISCUSS must produce, and DESIGN must record (via a new ADR superseding Q7), answers to at least:

- **Refresh policy** — when is SQLite considered authoritative vs. stale? Per-tool TTL? mtime-watched directories? Manual refresh? Background refresh on launch?
- **Cold-start UX** — first launch (empty DB) vs. warm launch (cached). Does K3 (< 1 s first paint) now mean "first paint from cache" or "complete scan"?
- **SHA256 caching** — Q6 already says SHA256 is computed lazily and cached in process memory only. With SQLite, is SHA256 persisted? Invalidated by mtime/size? This affects dedup correctness across launches.
- **Migration & schema versioning** — first product with a schema. Pick a migration strategy now to avoid pain later.
- **Concurrency** — what if two modeltap processes run? Q5 says detect-and-prompt-then-retry for write actions; what about SQLite write contention?
- **Privacy/data-locality** — where lives the DB (`$XDG_DATA_HOME/modeltap/cache.sqlite`?), backup story, what's safe to log.

## Tool trait impact (provisional)

Likely additions to the `Tool` trait:

- `inspect_tool(&self) -> Result<ToolDetail>` — capabilities, version, root path, health.
- `inspect_model(&self, id: &ModelId) -> Result<ModelDetail>` — per-model deep info.

Both should be cacheable into the SQLite layer. `modeltap-core` keeps pure transforms.

## New component (provisional)

A new crate or module — `modeltap-store` (sqlite-backed) — sits between plugins and the app composition root. Pure-functional facade over `rusqlite` (or `sqlx` if async-required by the I/O edge model already used).

## Affected user stories (provisional)

From `docs/feature/modeltap-tui/distill/features/master-acceptance.feature` — re-examine US-01 (initial scan), US-02 (model listing), US-05 (dedup), US-18 (plugin contract). Likely net-new: US-21 (tool details), US-22 (model details), US-23 (persistence semantics).

## Open questions for DISCUSS

1. Is this **one** feature or two (info-views + persistence)? They're related but separable: info-views work without SQLite (slower); SQLite-only-without-info-views is an architectural change with no user-visible win. Recommend ship as one feature with info-views as the user-visible motivator and persistence as the enabler.
2. Does this gate, or pause, the in-flight `folder-group-bulk-delete` DELIVER? Roadmap there is approved (6 phases, 16 steps, 62h) — see `docs/feature/folder-group-bulk-delete/deliver/roadmap.json`.
3. Backwards compatibility with users who have a `cache.sqlite` from an earlier dev build? (None should exist yet, but worth deciding now.)

## Next step

Run `/nw:discuss tool-model-info-sqlite-cache` to formalize requirements, define user stories, and produce acceptance criteria. DISCUSS output should explicitly call out the Q7-stateless-rediscovery reversal so DESIGN can write the ADR.
