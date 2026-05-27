This directory defines the high-level concepts, business logic, and architecture of this project using markdown. It is managed by [lat.md](https://www.npmjs.com/package/lat.md) — a tool that anchors source code to these definitions. Install the `lat` command with `npm i -g lat.md` and run `lat --help`.

## Sections

Index of architecture and design notes covering subsystems whose rationale is non-obvious from the source.

- [[tui-icons]] — per-tool PNG icons in the left pane via `ratatui-image`
- [[inspect-types]] — `Tool::inspect_tool` / `inspect_model` extension and `InspectError` (per ADR-016)
- [[modeltap-store]] — SQLite-backed inventory cache (per ADR-015) for warm-start paint; concurrent-process WAL + `busy_timeout`; pre-mutate revalidator (K5); architecture lints R7 + R8 + R9
- [[test-plugin-seam]] — `MODELTAP_TEST_PLUGINS` env-var seam + in-process `TestTool` (cfg-gated)
- [[warm-start]] — launch-time cache-paint via spawn_blocking; MODELTAP_CACHE_PATH adapter
- [[walking-skeleton-acceptance]] — M1 two-process end-to-end pattern + CACHE introspection seam
- [[tool-detail-tui]] — per-tool detail screen render + Msg/Screen/keymap routing (US-21 TUI half)
- [[plugin-inspect-overrides]] — Ollama HTTP / env-var short-circuit + HF cache-dir detection + user-config search paths
- [[model-detail-tui]] — per-model detail screen Metadata section + open_model_detail orchestration (US-22 in-progress)

## tool-model-info-sqlite-cache feature closure (US-21..US-26)

US-21..US-26 ship a SQLite-backed inventory cache plus K5 pre-mutate
revalidation. US-27 (persistent SHA256) is deferred to Release 3 per ADR-018.

The feature crates: [[modeltap-store]] (cache + revalidator),
[[inspect-types]] (Tool trait extension), [[plugin-inspect-overrides]]
(per-plugin `inspect_*`), [[tool-detail-tui]], [[model-detail-tui]].

The three architecture lints in
`crates/modeltap-app/tests/architecture.rs` are the static enforcement:

- **R7** — only `modeltap-app` may path-depend on `modeltap-store`. The TUI
  must not know SQLite exists; plugin crates must not depend on a sibling
  layer crate.
- **R8** — `modeltap-store` itself does NOT depend on `tokio`, `ratatui`,
  or `crossterm` (neither runtime nor dev-dep). The async hop happens at
  the `modeltap-app` boundary via `tokio::task::spawn_blocking`.
- **R9** — every method-call expression under `modeltap-app::actions::*` or
  `modeltap-app::orchestration::*` that targets one of the four destructive
  `Tool` trait methods (`link`, `delete_one`, `delete_all`, `delete_folder`)
  MUST be preceded — in the same fn body — by an invocation of
  `revalidate::pre_mutate(...)`. A future contributor adding a 5th
  destructive call site without a guard fails CI immediately.

## Cache safety invariant (K5)

ADR-015 §3 forbids the cache from enabling a stale-data destructive action.
The invariant lives in two halves:

1. `Cache::verify_against_fs(model_id)` re-`stat()`s every
   `cache_model_files` row for the given model and compares the live
   `(mtime, size, inode, dev)` quad to the cached row. Outcome:
   `Match` (proceed) / `Drift { fresh }` (re-introspect + writeback) /
   `Gone` (auto-refresh per-tool inventory).
2. `revalidate::pre_mutate` is the orchestrator-side helper every
   destructive entry point in `crates/modeltap-app/src/actions/*.rs` calls
   before invoking the plugin. R9 (above) is the static guarantee.

The four currently-guarded destructive sites are `actions::unify::run`,
`actions::zap::run`, `actions::delete_one::run`, and
`actions::folder_delete::run`. The zap path additionally enumerates per-tool
inventory via `discover()` and revalidates each model before invoking
`delete_all`. The folder-delete path revalidates every model in the targeted
`<author>/<repo>` group.

## Phase 06 finalize regression gate

`tests/acceptance/regression_gate.rs` pins five cross-feature invariants:

1. Parent `modeltap-tui/distill/features/master-acceptance.feature` non-`@skip`
   scenario count `>= 90` (US-01..US-20 + US-05b coverage baseline).
2. Sibling `folder-group-bulk-delete.feature` carries `>= 1` scenario tagged
   `@milestone-N` for every `N` in `1..=6` (M1..M6 coverage invariant).
3. The 3 scenarios in `sha256-persistence.feature` retain BOTH `@release-3`
   AND `@skip` tags (US-27 deferral per ADR-018).
4. INT-INFO-9 vocabulary sample — `modeltap_tui::screens::help_overlay::
   render_help_lines()` output contains "refresh tool", "refresh all",
   "recovery banner", "tool detail", "model detail" verbatim.
5. AC-22-7 sentinel pin — `tests/acceptance/model_detail.rs` contains the
   live value of `modeltap_app::orchestration::open_tool_detail::
   INSPECT_PANIC_SENTINEL` as a substring. Catches the drift class that
   broke the un-introspectable scenario when step 03-02 added Ollama's
   `inspect_model` override (FileReadable → INSPECT_PANIC_SENTINEL,
   not Unsupported → METADATA_UNSUPPORTED_SENTINEL).

Mutation-testing kill rate gate (CLAUDE.md §"Mutation Testing Strategy"):
`modeltap-core::{domain::inspect, logic::inventory_diff}` and
`modeltap-store::{types, repo, revalidate}` both clear the per-feature 80%
threshold on `cargo-mutants 27.0.0`.
