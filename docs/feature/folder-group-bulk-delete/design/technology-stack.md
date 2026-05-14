# Technology Stack — folder-group-bulk-delete

**Wave:** DESIGN (3 of 6) — brownfield extension
**Parent stack:** `docs/feature/modeltap-tui/design/technology-stack.md`

## Summary

**Zero new dependencies.** This feature is implemented entirely against the technology choices the parent feature already locked in. All license / OSS-preference / `cargo-deny` decisions inherit from the parent.

## Dependencies used (all already in workspace)

| Crate | Existing use | New use in this feature |
|---|---|---|
| `tokio` | async runtime, parallel discovery, `spawn_blocking` for I/O | Same envelope for `HfPlugin::delete_folder` — `spawn_blocking` wraps the sync unlink loop |
| `async-trait` | `Tool` trait surface | Default-body method addition (ADR-010) uses `async-trait` semantics; no version bump |
| `walkdir` | HF `cache_walk::list_snapshot_files` | Sidecar enumeration walks `models--<author>--<repo>/` and emits non-snapshot files (`README.md`, `.imatrix`, `.gguf.urls`, plus refs/, blobs/ entries exclusive to this repo). Reuses `walkdir::WalkDir`. |
| `std::fs` | `remove_file`, `metadata`, `canonicalize` | Same primitives — empty-tree cleanup uses `remove_dir` after the unlink pass |
| `thiserror` | domain error enums | `FolderDeleteError` variants (in `modeltap-core::types`), plus a new `DeleteError::Unsupported` variant |
| `serde` | type derives | New types (`FolderGroup`, `FolderDeletePlan`, `FolderDeleteOutcome`) derive `Serialize` for diagnostic JSONL |
| `tracing` | structured logs | Per-folder span `modeltap.hf.folder_delete` for diagnostics |
| `tempfile` (test) | plugin fixtures | New contract test fixtures: empty repo, single-file repo, mixed unique+shared, sidecar-heavy repo, EBUSY-simulated, read-only cache |
| `insta` (test) | TUI snapshot tests | New snapshots: folder header collapsed/expanded, folder-delete dialog body, post-action partial-failure summary |
| `proptest` (test) | property-based tests on pure fns | New property tests for `group_by_hf_repo` (group invariants: every model in exactly one group; folder.file_count == count(models) + count(sidecars)) and `classify_unique_vs_shared` (conservative-when-uncertain: tentative dedup keys never classify as Shared) |
| `ratatui` + `crossterm` | TUI rendering | New row type (folder header) and dialog view follow the existing patterns in `view/confirm_dialog.rs` |

## What's intentionally NOT here

- **No new async primitive.** `delete_folder` reuses the `spawn_blocking` envelope from `delete_one`.
- **No file-locking crate.** Per Q-FGD-2 decision (Option A, inherited from ADR-009 / intake Q5), there is no folder-level lock.
- **No journaling crate.** Per D-FGD-6 (no rollback), there is no per-unlink journal.
- **No new dependency for sidecar pattern matching.** Sidecar suffixes (`.md`, `.imatrix`, `.gguf.urls`) are matched by `Path::extension` + `Path::file_name` against a static list owned by the HF plugin. A regex crate is not justified.

## License and architectural-lint compliance

All dependencies above are MIT / Apache-2.0 / dual-licensed and already pass the parent's `cargo-deny` allow-list. The architecture lint test (parent §8.2 — `tests/architecture.rs`) covers the new module placements without modification:

- `modeltap-core::logic::folder_group` is a sub-module of `logic/` — covered by R6 (core has no async runtime / TUI / network deps).
- `modeltap-core::types` extensions stay within the core crate — covered by R1 (core has no plugin deps).
- `plugins/hf::folder_delete` is a sub-module of the HF plugin — covered by R2 (plugins don't depend on each other) and R5 (only app depends on plugins).
- `modeltap-tui::view::folder_confirm_dialog` is a new view sibling of `confirm_dialog` — covered by R3 (TUI has no plugin deps) and R4 (TUI has no I/O deps).
- `modeltap-app::orchestration::execute_folder_delete` is a new orchestration sibling of `execute_zap` — covered by R5 (app is the only assembler).

## Total dependency footprint change

**+0 direct deps. +0 transitive deps.** This is a strict additive code change against the parent's stack.
