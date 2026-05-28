# Walking-Skeleton Acceptance

The M1 scenario "Devon's second launch shows yesterday's inventory instantly from cache" is the Phase 01 exit gate.

It proves the cache-path → store → warm-start → JSONL-log vertical works end-to-end by invoking two real `modeltap` binaries that share `MODELTAP_CACHE_PATH`.

## Rationale

Two real processes (not one in-process call) is the only way to prove the cache is actually persisted across launches. An in-process test could trivially "pass" by reusing live in-memory state.

Hermeticity is non-negotiable: every fixture is per-scenario tempdir, every env-var is per-scenario, and no test reads or writes a user's real `~/.local/share/modeltap/`. The `MODELTAP_CACHE_PATH` env-var seam (resolved by [[warm-start]]'s cache-path adapter) is what makes that hermeticity possible.

The scenario uses the [[test-plugin-seam]] (`MODELTAP_TEST_PLUGINS=test-tool`) so it doesn't depend on a real Ollama or HF installation. The TestTool returns one synthetic model file the fixture pre-creates.

## CACHE seam helper

[[tests/src/fixtures/cache_fixtures.rs|`CacheVerifier`]] is the `@cache-introspection`-tagged read-only view over the cache file. It opens via `rusqlite::Connection::open_with_flags(_, SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI)` so it never contends with the `modeltap` process for the write lock.

The helper exposes the minimum surface the scenario needs: `pragma_user_version()` (proves the v0→v1 migrator landed — AC-23-3), `count_rows(table, where)` (proves the reconcile wrote rows), and `model_count_for(tool_id)` (returns `Option<i64>` so callers can distinguish "no row" from "row with zero").

This is the only place the acceptance crate reads SQLite directly — every other assertion goes through the modeltap binary's observable surface (stdout frame capture, JSONL log events). Per acceptance-test-plan.md §1 CM-A.

## Fixture pattern

[[tests/src/fixtures/cache_fixtures.rs|`DevonCacheEmptyFixture`]] builds a fresh per-scenario tempdir tree on every call.

Layout: `<temp>/xdg-data/modeltap/` (the `MODELTAP_CACHE_PATH` parent dir — `cache.sqlite` is deliberately absent), `<temp>/test-tool/models/test-model-7b.gguf` (the file the TestTool's `discover()` reports), `<temp>/logs/` (`MODELTAP_LOG_DIR`), and `<temp>/modeltap-home/` (~/.modeltap diagnostics).

The synthetic GGUF bytes are not real GGUF — the walking skeleton only needs `size_bytes > 0` to record a non-zero row. Real GGUF parsing is exercised by integration tests in phase 04+.

Dropping the fixture removes the tree recursively, so each scenario is fully isolated.

## Two-process invariant

Process A: `assert_cmd::cargo_bin("modeltap")` invoked headless with `MODELTAP_TEST_PLUGINS=test-tool`, `MODELTAP_TEST_TOOL_ROOT`, `MODELTAP_CACHE_PATH`, and `MODELTAP_LOG_DIR` all pointed at the fixture tempdir.

Warm-start sees no cache file, falls through to cold-start, the reconcile path writes `cache.sqlite` with the TestTool's one row, and the process exits.

Process B: same binary, same env vars, fresh invocation. Warm-start now sees `cache.sqlite`, opens it via `CacheOpenResult::OpenedExisting`, paints the inventory from `cache_tools` + `cache_models`, and emits `launch.warm_paint_ms` to `<log_dir>/launch.log`.

The scenario asserts: `cache.sqlite` exists after process A; `CacheVerifier` reads `user_version = 1` and `cache_models WHERE tool_id = 'test-tool' = 1`; the second process's JSONL contains a `launch.warm_paint_ms` event with `duration_ms ≤ 150`. The 150 ms bound is K-INFO-1's p90 target loosened for CI noise.

The two-process cache-persistence invariant is now driven by [[tests/acceptance/ui_navigate_shortcuts.rs]] (the AC-27-1 leg: two real launches sharing one `MODELTAP_CACHE_PATH`, asserting the second skips the re-hash) plus [[tests/acceptance/ui_loads.rs]] (first-paint inventory). The original per-feature `cache_walking_skeleton` + `steps/cache_lifecycle` binaries were removed in the lean-UI-suite consolidation (load / navigate+shortcuts / close); the detailed cache lifecycle is covered by crate-level `modeltap-store` / `modeltap-app` tests.
