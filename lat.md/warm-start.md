# Warm Start

The warm-start path is the launch-time routine that paints inventory from the SQLite cache before any cold-scan runs. It is the user-visible payoff of [[modeltap-store]] (the `< 100 ms` p90 K-INFO-1 budget is met here, not in the store crate).

## Rationale

ADR-015 splits launch into two phases: a fast warm paint from cache, then a background cold reconcile that owns mutating decisions. Warm-start is the first half; it never mutates state and never blocks on a cold scan.

`tokio::task::spawn_blocking` is the pattern the architecture mandates (architecture-design.md §7.1) because the `modeltap-store` crate is deliberately synchronous `rusqlite` — running it on the async runtime's worker pool would stall reactor progress. Each cache call therefore hops to the blocking pool and back.

Cache-path resolution lives in [[crates/modeltap-app/src/adapters/cache_path.rs|modeltap-app::adapters::cache_path]], not in the store. Keeping `dirs::data_dir()` out of `modeltap-store` lets the store crate stay platform-agnostic and lets acceptance tests pin the path via env-var without touching the store API.

## Cache-path resolver

[[crates/modeltap-app/src/adapters/cache_path.rs|`resolve(cli_override, env_override)`]] returns the path of the SQLite cache file with a three-tier override chain.

CLI override wins first (the future `--cache-path <path>` flag, currently unused). Then `MODELTAP_CACHE_PATH` — the production caller passes `std::env::var_os("MODELTAP_CACHE_PATH").as_deref()`. Then `dirs::data_dir().join("modeltap").join("cache.sqlite")` — the documented default per AC-23-1.

The resolver returns `CachePathError::NoDataDir` only when `dirs::data_dir()` itself cannot resolve. This is rare on supported platforms (macOS, Linux, WSL) but possible if `$HOME` is unset under test. Callers MUST propagate the error and fall through to cold-start (C-INFO-2).

## Warm-start orchestration

[[crates/modeltap-app/src/orchestration/warm_start.rs|`warm_start::run`]] is the async entry point. Inputs: `WarmStartConfig { cache_enabled, log_dir }` and the resolved `cache_path`. Output: `WarmStartResult { inventory, source }` plus a side-effect JSONL line.

`cache_enabled = false` (driven by `--no-cache` and `[cache] enabled = false`) short-circuits to `WarmStartSource::Disabled` with an empty inventory — cold-start then owns the launch entirely.

## Opt-out

Step 04-02 wires two user-facing opt-out levers to a single composition-root boolean: the [[crates/modeltap-app/src/main.rs|`--no-cache` flag]] and the file-based [[crates/modeltap-app/src/config.rs|`AppConfig.cache.enabled`]]. The flag wins when both are set.

The TOML lives at `~/.modeltap/config.toml` (or `MODELTAP_CONFIG_PATH` for tests) under `[cache] enabled = false`. A user with `cache.enabled = true` can still bypass for one launch via `--no-cache`. When the combined boolean is false, `Cache::open` is NEVER called downstream: the warm-start orchestrator, the tool-detail cache path, and the reconcile-writeback all short-circuit on the same gate.

The byte-precise invariant is asserted by [[tests/src/fixtures/dir_manifest.rs|`DirManifest`]] — a recursive `(relative_path, size, mtime)` snapshot over `xdg-data/modeltap/`. The cache-opt-out acceptance suite ([[tests/acceptance/cache_opt_out.rs|`cache_opt_out.rs`]]) snapshots the directory before each launch, runs the modeltap binary, re-snapshots, and asserts `before.assert_equal(&after)`. AC-23-8 + AC-23-9 are proven this way: zero new bytes means no `cache.sqlite`, no `-wal`, no `-shm`.

INT-INFO-6 (`modeltap --version` exits 0 with a corrupt cache) is satisfied by clap's auto-version handler — `#[command(version)]` on the `Cli` struct exits before `main()`'s body runs, so the cache resolution path is never reached. The fourth opt-out scenario seeds a 16 KB non-SQLite blob at `MODELTAP_CACHE_PATH` to prove this directly: if the version path ever regressed to opening the cache, the test would fail.

`Cache::open(_)` is wrapped in `spawn_blocking`. Branching on `CacheOpenResult` distinguishes `OpenedFresh` (empty schema → cold-start will populate), `OpenedExisting` (paint), and `OpenedAfterMigration { from, to }` (paint, but the composition root may surface a banner).

When the path proceeds to paint, a single `spawn_blocking` reads `cache.tools()` + `cache.models_for_tool(tool_id)` per tool inside one blocking hop — single round-trip per architecture-design.md §8.1.

The `launch.warm_paint_ms` JSONL event is emitted only on the `OpenedExisting` / `OpenedAfterMigration` paths (the warm path). Fresh and disabled paths emit no warm-paint event; cold-start logs its own `launch.first_paint_ms` later. Writes are best-effort — an unwritable log dir never blocks the launch (C-INFO-2 again).
