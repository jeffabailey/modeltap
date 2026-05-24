//! Fixture builders and seam helpers for the `tool-model-info-sqlite-cache`
//! acceptance suite.
//!
//! Per acceptance-test-plan.md §1 CM-A and §3 (fixture strategy), this module
//! owns two cooperating pieces:
//!
//! 1. **`devon_cache_empty` fixture** — a per-scenario tempdir tree with the
//!    same shape as the parent's `devon-multi-tool` fixture (synthetic tool
//!    directories) plus a `xdg-data/modeltap/` directory the scenario points
//!    `MODELTAP_CACHE_PATH` into. NO `cache.sqlite` exists at construction
//!    time — the walking-skeleton's first process writes it. The fixture also
//!    pre-creates the TestTool's model directory and the synthetic
//!    `test-model-7b.gguf` file the TestTool's `discover()` returns.
//!
//! 2. **`CacheVerifier` CACHE seam helper** — an `@cache-introspection`-tagged
//!    test utility that opens the cache file via a READ-ONLY
//!    `rusqlite::Connection` so step-definition assertions can verify
//!    `PRAGMA user_version` and row counts without contending with the real
//!    `modeltap` process for the write lock. Per acceptance-test-plan.md §1
//!    CM-A: this is a test-utility, not a production import, and is gated
//!    to `[dev-dependencies]`.
//!
//! The CACHE seam is the only place the acceptance crate reads the SQLite
//! file directly; all other assertions go through the modeltap binary's
//! observable surface (stdout frame capture, JSONL log events).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

use crate::test_tool::{TEST_MODEL_FILENAME, TEST_TOOL_NAME};

/// Filesystem layout owned by one M1 walking-skeleton scenario. The TempDir
/// is retained so the directory survives until the test asserts; dropping
/// the fixture recursively removes the tree.
///
/// Layout (all paths relative to `temp.path()`):
///
/// ```text
/// <temp>/
///   xdg-data/
///     modeltap/                     <-- MODELTAP_CACHE_PATH parent dir
///   test-tool/
///     models/
///       test-model-7b.gguf          <-- TestTool::discover() returns this
///   logs/                           <-- MODELTAP_LOG_DIR
///   modeltap-home/                  <-- ~/.modeltap (diagnostics.log etc)
/// ```
///
/// Process A writes `<temp>/xdg-data/modeltap/cache.sqlite` via the warm-start
/// reconcile path; process B reads it via the same env var.
pub struct DevonCacheEmptyFixture {
    pub temp: TempDir,
}

impl DevonCacheEmptyFixture {
    /// Build a fresh fixture tree. Pre-writes the TestTool's synthetic model
    /// file so the first `modeltap` process's discovery has a non-zero
    /// `size_bytes` to record in the cache row.
    pub fn build() -> Self {
        let temp = TempDir::new().expect("create devon-cache-empty tempdir");
        // xdg-data/modeltap/ — the cache directory; cache.sqlite is absent.
        let xdg_modeltap = temp.path().join("xdg-data").join("modeltap");
        std::fs::create_dir_all(&xdg_modeltap).expect("create xdg-data/modeltap");
        // test-tool/models/<file>.gguf — the file TestTool::discover reports.
        let model_dir = temp.path().join("test-tool").join("models");
        std::fs::create_dir_all(&model_dir).expect("create test-tool/models");
        let model_path = model_dir.join(TEST_MODEL_FILENAME);
        std::fs::write(&model_path, b"synthetic-walking-skeleton-gguf-bytes")
            .expect("seed synthetic gguf");
        // logs/ — MODELTAP_LOG_DIR receives launch.log.
        let log_dir = temp.path().join("logs");
        std::fs::create_dir_all(&log_dir).expect("create logs/");
        // modeltap-home/ — diagnostics.log (not yet exercised in WS, but
        // matches acceptance-test-plan.md §3 fixture skeleton).
        let modeltap_home = temp.path().join("modeltap-home");
        std::fs::create_dir_all(&modeltap_home).expect("create modeltap-home/");
        Self { temp }
    }

    /// Absolute path to `<temp>/xdg-data/modeltap/cache.sqlite` — the value
    /// the scenario sets `MODELTAP_CACHE_PATH` to.
    pub fn cache_path(&self) -> PathBuf {
        self.temp
            .path()
            .join("xdg-data")
            .join("modeltap")
            .join("cache.sqlite")
    }

    /// Absolute path to `<temp>/test-tool` — the root the TestTool's
    /// `discover()` walks. The scenario sets `MODELTAP_TEST_TOOL_ROOT` to
    /// `<test_tool_root>/models` so the TestTool finds `test-model-7b.gguf`.
    pub fn test_tool_root(&self) -> PathBuf {
        self.temp.path().join("test-tool").join("models")
    }

    /// Absolute path to `<temp>/logs` — the value the scenario sets
    /// `MODELTAP_LOG_DIR` to. Process A and process B share this directory so
    /// the JSONL `launch.warm_paint_ms` event from process B is visible to
    /// the CACHE seam's caller.
    pub fn log_dir(&self) -> PathBuf {
        self.temp.path().join("logs")
    }
}

// ---------------------------------------------------------------------------
// Step 04-01 — broken-cache fixtures for the recovery path (AC-23-7 /
// AC-23-10 / AC-23-11). Each fixture pre-installs a broken cache.sqlite at
// the same xdg-data/modeltap/ location DevonCacheEmptyFixture uses, then
// exposes the same env-var triad (MODELTAP_CACHE_PATH, MODELTAP_DIAGNOSTICS_DIR,
// MODELTAP_LOG_DIR) so the existing M1 walking-skeleton scenarios can swap
// the fixture without touching the acceptance step definitions.
// ---------------------------------------------------------------------------

/// Pre-installed corrupt cache fixture: writes 16 KB of deterministic
/// non-SQLite bytes at `<temp>/xdg-data/modeltap/cache.sqlite`. SQLite
/// returns `SQLITE_NOTADB` on first `Connection::open` because the header
/// is not "SQLite format 3\0". `Cache::open` routes to recovery, renames
/// the file to `cache.sqlite.corrupt-<ts>`, and opens a fresh empty cache.
///
/// The deterministic byte pattern (`Knuth multiplicative hash` of the
/// position) avoids pulling in the `rand` crate as a new workspace
/// dependency — the recovery routine doesn't care WHAT the bytes are, only
/// that the header is invalid.
pub struct DevonCacheCorruptFixture {
    pub temp: TempDir,
}

impl DevonCacheCorruptFixture {
    /// Build a fresh fixture with a corrupt cache.sqlite pre-installed.
    pub fn build() -> Self {
        let temp = TempDir::new().expect("create devon-cache-corrupt tempdir");
        let xdg_modeltap = temp.path().join("xdg-data").join("modeltap");
        std::fs::create_dir_all(&xdg_modeltap).expect("create xdg-data/modeltap");
        // Pre-create the test-tool model dir so cold-start discovery has
        // something to find — matches the M1 fixture's invariant that the
        // fresh recovered cache still finds the TestTool's model.
        let model_dir = temp.path().join("test-tool").join("models");
        std::fs::create_dir_all(&model_dir).expect("create test-tool/models");
        let model_path = model_dir.join(TEST_MODEL_FILENAME);
        std::fs::write(&model_path, b"synthetic-walking-skeleton-gguf-bytes")
            .expect("seed synthetic gguf");
        // logs/, modeltap-home/, and the corrupt cache itself.
        std::fs::create_dir_all(temp.path().join("logs")).expect("create logs/");
        std::fs::create_dir_all(temp.path().join("modeltap-home"))
            .expect("create modeltap-home/");

        // 16 KB of deterministic non-SQLite bytes. The first 16 bytes alone
        // are enough to fail the SQLite header check; 16 KB is the size the
        // step 04-01 spec calls out so the fixture matches the acceptance
        // test expectations.
        let bytes: Vec<u8> = (0..16_384u32)
            .map(|i| ((i.wrapping_mul(2654435761)) >> 24) as u8)
            .collect();
        let cache_path = xdg_modeltap.join("cache.sqlite");
        std::fs::write(&cache_path, bytes).expect("write corrupt cache.sqlite");
        Self { temp }
    }

    /// Absolute path to the corrupt `cache.sqlite` (pre-existing on disk).
    pub fn cache_path(&self) -> PathBuf {
        self.temp
            .path()
            .join("xdg-data")
            .join("modeltap")
            .join("cache.sqlite")
    }

    /// Diagnostics dir for `MODELTAP_DIAGNOSTICS_DIR`.
    pub fn diagnostics_dir(&self) -> PathBuf {
        self.temp.path().join("modeltap-home")
    }

    /// Log dir for `MODELTAP_LOG_DIR`.
    pub fn log_dir(&self) -> PathBuf {
        self.temp.path().join("logs")
    }

    /// Test-tool model root (matches DevonCacheEmptyFixture).
    pub fn test_tool_root(&self) -> PathBuf {
        self.temp.path().join("test-tool").join("models")
    }
}

// ---------------------------------------------------------------------------
// Step 04-03 — per-tool TTL stale fixture (US-25 AC-25-2 / AC-25-4).
// Pre-installs a populated cache.sqlite where one tool's `last_scan_at` is
// 25h old (stale w.r.t. the 24h default TTL) and the others are 2h / 1h fresh.
// The fixture also seeds at least one model row per tool so the warm-start
// orchestrator's `models_for_tool` returns a non-empty list for the fresh
// tools and the partition step can observe (a) the fresh tools' models
// painted from cache and (b) the stale tool's id returned as cold-scan work.
// ---------------------------------------------------------------------------

/// Pre-installed stale-tool cache fixture: writes a valid SQLite with three
/// `cache_tools` rows (one stale, two fresh) plus one `cache_models` row per
/// tool. The scenario points `MODELTAP_CACHE_PATH` at the seeded file.
///
/// Timeline (relative to `SystemTime::now()` at construction):
///   - `ollama`     → `last_scan_at = now - 25h` (stale; > 24h TTL)
///   - `llama-cli`  → `last_scan_at = now - 2h`  (fresh)
///   - `hf`         → `last_scan_at = now - 1h`  (fresh)
pub struct DevonCacheStaleToolFixture {
    pub temp: TempDir,
}

impl DevonCacheStaleToolFixture {
    /// Stable tool_id strings the fixture seeds. Exposed so step-definitions
    /// can assert on the warm-start orchestrator's `stale_tool_ids` output.
    pub const STALE_TOOL_ID: &'static str = "ollama";
    pub const FRESH_TOOL_ID_LLAMA_CLI: &'static str = "llama-cli";
    pub const FRESH_TOOL_ID_HF: &'static str = "hf";

    /// Build a fresh fixture with a populated cache.sqlite. Per-tool
    /// timestamps follow the structdoc above.
    pub fn build() -> Self {
        use modeltap_core::types::ToolId as RealToolId;
        use modeltap_store::types::{CachedModel, CachedTool};
        use modeltap_store::{Cache, CacheOpenResult};
        use std::collections::BTreeMap;
        use std::time::{Duration, SystemTime};

        let temp = TempDir::new().expect("create devon-cache-stale-tool tempdir");
        let xdg_modeltap = temp.path().join("xdg-data").join("modeltap");
        std::fs::create_dir_all(&xdg_modeltap).expect("create xdg-data/modeltap");
        std::fs::create_dir_all(temp.path().join("logs")).expect("create logs/");
        std::fs::create_dir_all(temp.path().join("modeltap-home"))
            .expect("create modeltap-home/");

        // Open a fresh cache and seed it. `Cache::open` runs the v1
        // migration on the first call so the schema is valid.
        let cache_path = xdg_modeltap.join("cache.sqlite");
        let cache = match Cache::open(&cache_path).expect("seed open") {
            CacheOpenResult::OpenedFresh(c) => c,
            other => panic!("expected OpenedFresh on seed, got {:?}", other),
        };

        // Leak each tool_id string once so we have a stable `'static`
        // reference (the `RealToolId` API requires `&'static str`).
        let stale: RealToolId = RealToolId(Box::leak(Self::STALE_TOOL_ID.to_string().into_boxed_str()));
        let llama: RealToolId = RealToolId(Box::leak(Self::FRESH_TOOL_ID_LLAMA_CLI.to_string().into_boxed_str()));
        let hf: RealToolId = RealToolId(Box::leak(Self::FRESH_TOOL_ID_HF.to_string().into_boxed_str()));

        let now = SystemTime::now();
        // 25h: stale w.r.t. the 24h default tool_ttl_seconds.
        let stale_at = now - Duration::from_secs(25 * 3600);
        let fresh_2h_at = now - Duration::from_secs(2 * 3600);
        let fresh_1h_at = now - Duration::from_secs(3600);

        for (tool, last_scan_at, label) in [
            (stale, stale_at, "Ollama"),
            (llama, fresh_2h_at, "llama-cli"),
            (hf, fresh_1h_at, "Hugging Face"),
        ] {
            cache
                .write_tool(&CachedTool {
                    tool_id: tool,
                    install_path: PathBuf::from(format!("/tmp/{}-install", tool.0)),
                    detected_version: Some("1.0.0".to_string()),
                    plugin_version: "0.0.0".to_string(),
                    model_count: 1,
                    disk_usage_bytes: 1024,
                    largest_model_id: Some(format!("{}-model", tool.0)),
                    last_scan_at,
                    last_scan_duration_ms: 0,
                    last_error: None,
                    last_error_at: None,
                    search_paths: Vec::new(),
                })
                .expect("write_tool stale-tool fixture");
            cache
                .write_models(
                    &tool,
                    &[CachedModel {
                        model_id: format!("{}-model", tool.0),
                        tool_id: tool,
                        display_name: format!("{label} cached model"),
                        format: Some("gguf".to_string()),
                        quantisation: Some("Q4_K_M".to_string()),
                        size_bytes: 1024,
                        sha256: None,
                        architecture: None,
                        parameters_billions: None,
                        context_length: None,
                        dedup_group_id: None,
                        metadata_kv: BTreeMap::new(),
                        metadata_introspected_at: None,
                        last_seen_at: now,
                        last_validated_at: None,
                    }],
                )
                .expect("write_models stale-tool fixture");
        }

        Self { temp }
    }

    /// Absolute path to the seeded `cache.sqlite`.
    pub fn cache_path(&self) -> PathBuf {
        self.temp
            .path()
            .join("xdg-data")
            .join("modeltap")
            .join("cache.sqlite")
    }

    /// Diagnostics dir for `MODELTAP_DIAGNOSTICS_DIR`.
    pub fn diagnostics_dir(&self) -> PathBuf {
        self.temp.path().join("modeltap-home")
    }

    /// Log dir for `MODELTAP_LOG_DIR`.
    pub fn log_dir(&self) -> PathBuf {
        self.temp.path().join("logs")
    }
}

// ---------------------------------------------------------------------------
// Step 04-05 — fully-populated warm-cache fixture (K-INFO-1 / K-INFO-7 /
// K3a). Pre-installs a populated cache.sqlite with 4 tools × ~15 models each
// (58 total — Devon's typical inventory size per outcome-kpis.md). Every
// tool's `last_scan_at` is recent (within the 24h default TTL → fresh) so
// the warm-start orchestrator paints the full inventory from cache and the
// K-INFO-1 (≤ 150 ms p90 warm paint) and K-INFO-7 (≤ 100 ms p90 cache-open
// overhead) budgets can be asserted against a realistic workload.
// ---------------------------------------------------------------------------

/// Pre-installed warm-cache fixture: writes a valid SQLite with 4
/// `cache_tools` rows (ollama, hf, lm-studio, atomic-chat) plus 15
/// `cache_models` rows for the first three tools and 13 for atomic-chat —
/// total 58 models. All `last_scan_at` values are recent (within the 24h
/// default TTL → fresh) so warm-start paints every tool from cache.
///
/// Model bytes are deterministic: per-tool slugs + a 1-based index ensure
/// the seed is reproducible across CI runs. `size_bytes` is randomised
/// within `[100 MB, 8 GB]` via a Knuth-multiplicative hash of the model
/// index (avoids a `rand` workspace-dep) so the fixture mimics Devon's
/// realistic per-model byte spread (a few small adapters + several large
/// quantised checkpoints).
pub struct DevonCacheWarmFixture {
    pub temp: TempDir,
}

impl DevonCacheWarmFixture {
    /// Total model count seeded across all four tools. Acceptance tests
    /// assert `inventory.entries.len() == TOTAL_MODELS` after warm-start.
    pub const TOTAL_MODELS: usize = 58;
    /// Number of seeded tools. Acceptance tests assert each tool's row
    /// is TTL-fresh and contributes to the warm-paint inventory.
    pub const TOTAL_TOOLS: usize = 4;

    /// Stable tool_ids the fixture seeds. Exposed so step-definitions can
    /// pass them to `CacheVerifier::model_count_for` without re-stringing.
    pub const TOOL_ID_OLLAMA: &'static str = "ollama";
    pub const TOOL_ID_HF: &'static str = "hf";
    pub const TOOL_ID_LM_STUDIO: &'static str = "lm-studio";
    pub const TOOL_ID_ATOMIC_CHAT: &'static str = "atomic-chat";

    /// Build the fixture: seeds the cache with 58 models across 4 tools
    /// (all TTL-fresh) and pre-creates the `logs/` + `modeltap-home/`
    /// subdirs used by acceptance scenarios.
    pub fn build() -> Self {
        use modeltap_core::types::ToolId as RealToolId;
        use modeltap_store::types::{CachedModel, CachedTool};
        use modeltap_store::{Cache, CacheOpenResult};
        use std::collections::BTreeMap;
        use std::time::{Duration, SystemTime};

        let temp = TempDir::new().expect("create devon-cache-warm tempdir");
        let xdg_modeltap = temp.path().join("xdg-data").join("modeltap");
        std::fs::create_dir_all(&xdg_modeltap).expect("create xdg-data/modeltap");
        std::fs::create_dir_all(temp.path().join("logs")).expect("create logs/");
        std::fs::create_dir_all(temp.path().join("modeltap-home"))
            .expect("create modeltap-home/");

        let cache_path = xdg_modeltap.join("cache.sqlite");
        let cache = match Cache::open(&cache_path).expect("seed open") {
            CacheOpenResult::OpenedFresh(c) => c,
            other => panic!("expected OpenedFresh on warm-cache seed, got {:?}", other),
        };

        // Per-tool model counts: 15 / 15 / 15 / 13 = 58. The first three
        // tools share the canonical 15-model shape; atomic-chat ships fewer
        // models in Devon's real install so the fixture matches that
        // empirical skew.
        let per_tool: [(&'static str, usize, &'static str); 4] = [
            (Self::TOOL_ID_OLLAMA, 15, "Ollama"),
            (Self::TOOL_ID_HF, 15, "Hugging Face"),
            (Self::TOOL_ID_LM_STUDIO, 15, "LM Studio"),
            (Self::TOOL_ID_ATOMIC_CHAT, 13, "Atomic Chat"),
        ];

        // Recent-but-different timestamps so the launches see a realistic
        // spread of freshness. All ≤ 2h old → well within the 24h default
        // TTL gate.
        let now = SystemTime::now();
        let per_tool_age_secs: [u64; 4] = [60, 30 * 60, 60 * 60, 2 * 3600];

        for (idx, (tool_str, model_count, label)) in per_tool.iter().enumerate() {
            let tool: RealToolId =
                RealToolId(Box::leak(tool_str.to_string().into_boxed_str()));
            let last_scan_at = now - Duration::from_secs(per_tool_age_secs[idx]);

            cache
                .write_tool(&CachedTool {
                    tool_id: tool,
                    install_path: PathBuf::from(format!("/opt/{tool_str}")),
                    detected_version: Some("1.0.0".to_string()),
                    plugin_version: "0.0.0".to_string(),
                    model_count: *model_count as u64,
                    disk_usage_bytes: 0,
                    largest_model_id: None,
                    last_scan_at,
                    last_scan_duration_ms: 0,
                    last_error: None,
                    last_error_at: None,
                    search_paths: Vec::new(),
                })
                .expect("write_tool warm-cache fixture");

            // Build the per-tool model batch. Deterministic shape so any
            // future debugging can re-derive the byte counts from the model
            // index.
            let models: Vec<CachedModel> = (0..*model_count)
                .map(|i| {
                    let one_based = (i + 1) as u32;
                    // Knuth multiplicative hash of (tool_idx, model_idx) so
                    // the size spread covers ~100 MB .. ~8 GB without a
                    // `rand` dep. The +1 avoids size_bytes = 0 which the
                    // compatibility engine would treat as a malformed row.
                    let mix = (idx as u32)
                        .wrapping_mul(0x9E37_79B1)
                        .wrapping_add(one_based.wrapping_mul(2_654_435_761));
                    let scaled = (mix as u64) % (8_000_000_000u64 - 100_000_000u64);
                    let size_bytes = 100_000_000u64 + scaled + 1;
                    CachedModel {
                        model_id: format!("{tool_str}-model-{one_based:02}"),
                        tool_id: tool,
                        display_name: format!("{label} Model #{one_based:02}"),
                        format: Some("gguf".to_string()),
                        quantisation: Some("Q4_K_M".to_string()),
                        size_bytes,
                        sha256: None,
                        architecture: None,
                        parameters_billions: None,
                        context_length: None,
                        dedup_group_id: None,
                        metadata_kv: BTreeMap::new(),
                        metadata_introspected_at: None,
                        last_seen_at: now,
                        last_validated_at: None,
                    }
                })
                .collect();
            cache
                .write_models(&tool, &models)
                .expect("write_models warm-cache fixture");
        }

        Self { temp }
    }

    /// Absolute path to the seeded `cache.sqlite`.
    pub fn cache_path(&self) -> PathBuf {
        self.temp
            .path()
            .join("xdg-data")
            .join("modeltap")
            .join("cache.sqlite")
    }

    /// Diagnostics dir for `MODELTAP_DIAGNOSTICS_DIR`.
    pub fn diagnostics_dir(&self) -> PathBuf {
        self.temp.path().join("modeltap-home")
    }

    /// Log dir for `MODELTAP_LOG_DIR`.
    pub fn log_dir(&self) -> PathBuf {
        self.temp.path().join("logs")
    }
}

/// Pre-installed future-version cache fixture: writes a valid SQLite at
/// `<temp>/xdg-data/modeltap/cache.sqlite` and sets `PRAGMA user_version = 99`.
/// `Cache::open` reads `user_version > EXPECTED_SCHEMA_VERSION`, routes to
/// recovery, renames the file to `cache.sqlite.future-version-99`, and opens
/// a fresh empty cache.
pub struct DevonCacheFutureVersionFixture {
    pub temp: TempDir,
}

impl DevonCacheFutureVersionFixture {
    /// Build a fresh fixture with a future-version cache.sqlite pre-installed.
    pub fn build() -> Self {
        let temp = TempDir::new().expect("create devon-cache-future-v tempdir");
        let xdg_modeltap = temp.path().join("xdg-data").join("modeltap");
        std::fs::create_dir_all(&xdg_modeltap).expect("create xdg-data/modeltap");
        let model_dir = temp.path().join("test-tool").join("models");
        std::fs::create_dir_all(&model_dir).expect("create test-tool/models");
        let model_path = model_dir.join(TEST_MODEL_FILENAME);
        std::fs::write(&model_path, b"synthetic-walking-skeleton-gguf-bytes")
            .expect("seed synthetic gguf");
        std::fs::create_dir_all(temp.path().join("logs")).expect("create logs/");
        std::fs::create_dir_all(temp.path().join("modeltap-home"))
            .expect("create modeltap-home/");

        // Seed a valid SQLite with PRAGMA user_version = 99.
        let cache_path = xdg_modeltap.join("cache.sqlite");
        let conn = Connection::open(&cache_path).expect("seed future-version sqlite");
        conn.pragma_update(None, "user_version", 99_i64)
            .expect("set user_version=99");
        conn.close()
            .map_err(|(_, e)| e)
            .expect("close future-version seed");
        Self { temp }
    }

    /// Absolute path to the future-version `cache.sqlite`.
    pub fn cache_path(&self) -> PathBuf {
        self.temp
            .path()
            .join("xdg-data")
            .join("modeltap")
            .join("cache.sqlite")
    }

    /// Diagnostics dir for `MODELTAP_DIAGNOSTICS_DIR`.
    pub fn diagnostics_dir(&self) -> PathBuf {
        self.temp.path().join("modeltap-home")
    }

    /// Log dir for `MODELTAP_LOG_DIR`.
    pub fn log_dir(&self) -> PathBuf {
        self.temp.path().join("logs")
    }

    /// Test-tool model root (matches DevonCacheEmptyFixture).
    pub fn test_tool_root(&self) -> PathBuf {
        self.temp.path().join("test-tool").join("models")
    }
}

// ---------------------------------------------------------------------------
// CACHE seam helper — acceptance-test-plan.md §1 CM-A.
// ---------------------------------------------------------------------------

/// Read-only view over the cache file. Opens via
/// `rusqlite::Connection::open_with_flags(_, SQLITE_OPEN_READ_ONLY)` so the
/// helper never contends with the `modeltap` process for the write lock.
///
/// Only the surface the walking-skeleton scenario needs is exposed:
///
/// - `pragma_user_version()` — proves the v0→v1 migrator landed (AC-23-3).
/// - `count_rows(table, where_clause)` — proves the warm-start reconcile
///   wrote the TestTool's row (AC for cache_models / cache_tools).
///
/// Step-definitions-skeleton.md §A maps these to the
/// `@cache-introspection`-tagged step assertions.
pub struct CacheVerifier {
    conn: Connection,
}

impl CacheVerifier {
    /// Open the cache file at `path` read-only. Returns `Err` if the file is
    /// absent — callers use `path.exists()` to disambiguate "process A never
    /// wrote" from "open failed".
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;
        Ok(Self { conn })
    }

    /// Return `PRAGMA user_version` as a `u32`. The walking-skeleton scenario
    /// asserts this equals 1 (proves the embedded migration in
    /// `crates/modeltap-store/migrations/0001_initial.sql` ran).
    pub fn pragma_user_version(&self) -> rusqlite::Result<u32> {
        self.conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
    }

    /// Count rows in `table`. If `where_clause` is `Some(s)`, the helper
    /// appends `WHERE {s}` to the query. The clause's parameters are inlined
    /// by the caller (the walking-skeleton scenarios use a small fixed set of
    /// literal values — `tool_id = 'test-tool'` — so this is safe; broader
    /// queries lands in phase 04 with proper parameter binding).
    pub fn count_rows(&self, table: &str, where_clause: Option<&str>) -> rusqlite::Result<i64> {
        let sql = match where_clause {
            Some(clause) => format!("SELECT COUNT(*) FROM {table} WHERE {clause}"),
            None => format!("SELECT COUNT(*) FROM {table}"),
        };
        self.conn.query_row(&sql, [], |row| row.get::<_, i64>(0))
    }

    /// Convenience: read the `model_count` column from the single
    /// `cache_tools` row matching `tool_id`. Returns `Ok(None)` if no row
    /// matches so the caller can distinguish "no row" from "row with zero".
    pub fn model_count_for(&self, tool_id: &str) -> rusqlite::Result<Option<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT model_count FROM cache_tools WHERE tool_id = ?1")?;
        let mut rows = stmt.query([tool_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Read the `last_scan_at` ISO-8601 column from the single `cache_tools`
    /// row matching `tool_id`. Returns `Ok(None)` if no row matches.
    ///
    /// Added in step 04-04 (US-23 Scenarios 4-5 / AC-23-10): the concurrent-
    /// writers scenario captures this value after process A's commit, then
    /// re-reads after process B's commit and asserts the string advanced —
    /// proving last-writer-wins semantics on `ON CONFLICT(tool_id) DO UPDATE`.
    /// The column convention writes ISO-8601 UTC strings which are
    /// lexicographically orderable, so a plain `>` comparison works.
    pub fn last_scan_at_for(&self, tool_id: &str) -> rusqlite::Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT last_scan_at FROM cache_tools WHERE tool_id = ?1")?;
        let mut rows = stmt.query([tool_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }
}

/// Returns the stable `tool_id` string the TestTool registers under. Exposed
/// so step-definition helpers can pass it to `CacheVerifier::count_rows` /
/// `model_count_for` without re-importing `TEST_TOOL_NAME` from the parent
/// module.
pub fn test_tool_id_str() -> &'static str {
    TEST_TOOL_NAME.0
}

// ---------------------------------------------------------------------------
// Unit tests — exercise the CACHE seam against an in-memory SQLite + a real
// tempdir-backed cache so the helper is self-validating before the walking-
// skeleton scenario consumes it.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use modeltap_store::types::{CachedModel, CachedTool};
    use modeltap_store::{Cache, CacheOpenResult};
    use std::collections::BTreeMap;
    use std::time::SystemTime;

    fn seed_one_row_cache(path: &Path) {
        let cache = match Cache::open(path).expect("seed open") {
            CacheOpenResult::OpenedFresh(c) => c,
            other => panic!("expected OpenedFresh, got {:?}", other),
        };
        let now = SystemTime::now();
        cache
            .write_tool(&CachedTool {
                tool_id: TEST_TOOL_NAME,
                install_path: PathBuf::from("/tmp/test-tool-install"),
                detected_version: Some("test-1.0.0".to_string()),
                plugin_version: "test-fixture 0.0.0".to_string(),
                model_count: 1,
                disk_usage_bytes: 1024,
                largest_model_id: Some("test-model-7b".to_string()),
                last_scan_at: now,
                last_scan_duration_ms: 0,
                last_error: None,
                last_error_at: None,
                search_paths: Vec::new(),
            })
            .expect("write_tool seed");
        cache
            .write_models(
                &TEST_TOOL_NAME,
                &[CachedModel {
                    model_id: "test-model-7b".to_string(),
                    tool_id: TEST_TOOL_NAME,
                    display_name: "Test Model 7B".to_string(),
                    format: Some("gguf".to_string()),
                    quantisation: None,
                    size_bytes: 1024,
                    sha256: None,
                    architecture: None,
                    parameters_billions: None,
                    context_length: None,
                    dedup_group_id: None,
                    metadata_kv: BTreeMap::new(),
                    metadata_introspected_at: None,
                    last_seen_at: now,
                    last_validated_at: None,
                }],
            )
            .expect("write_models seed");
    }

    #[test]
    fn cache_verifier_reads_pragma_user_version_after_seed() {
        let tmp = TempDir::new().expect("tmpdir");
        let path = tmp.path().join("cache.sqlite");
        seed_one_row_cache(&path);

        let verifier = CacheVerifier::open(&path).expect("open verifier");
        assert_eq!(
            verifier.pragma_user_version().expect("pragma"),
            1,
            "v1 migrator must have landed user_version = 1"
        );
    }

    #[test]
    fn cache_verifier_counts_rows_with_and_without_where_clause() {
        let tmp = TempDir::new().expect("tmpdir");
        let path = tmp.path().join("cache.sqlite");
        seed_one_row_cache(&path);

        let verifier = CacheVerifier::open(&path).expect("open verifier");
        assert_eq!(
            verifier.count_rows("cache_tools", None).expect("count"),
            1,
            "one seeded cache_tools row"
        );
        assert_eq!(
            verifier
                .count_rows("cache_models", Some("tool_id = 'test-tool'"))
                .expect("count where"),
            1,
            "one seeded cache_models row for test-tool"
        );
        assert_eq!(
            verifier
                .count_rows("cache_models", Some("tool_id = 'not-a-tool'"))
                .expect("count where empty"),
            0,
            "no rows for an unknown tool_id"
        );
    }

    #[test]
    fn cache_verifier_model_count_for_returns_some_for_seeded_tool() {
        let tmp = TempDir::new().expect("tmpdir");
        let path = tmp.path().join("cache.sqlite");
        seed_one_row_cache(&path);

        let verifier = CacheVerifier::open(&path).expect("open verifier");
        assert_eq!(
            verifier.model_count_for("test-tool").expect("query"),
            Some(1),
            "model_count for test-tool equals 1"
        );
        assert_eq!(
            verifier.model_count_for("missing-tool").expect("query"),
            None,
            "missing tool returns None"
        );
    }

    #[test]
    fn devon_cache_corrupt_fixture_pre_installs_invalid_sqlite_bytes() {
        let fix = DevonCacheCorruptFixture::build();
        let cache_path = fix.cache_path();
        assert!(
            cache_path.exists(),
            "fixture must pre-install corrupt cache.sqlite"
        );
        let bytes = std::fs::read(&cache_path).expect("read corrupt cache");
        assert_eq!(bytes.len(), 16_384, "must be exactly 16 KB");
        // The SQLite file header is "SQLite format 3\0" — verify the fixture
        // does NOT have this header so Cache::open reliably routes to
        // recovery.
        assert!(
            !bytes.starts_with(b"SQLite format 3\0"),
            "corrupt fixture must NOT have a valid SQLite header"
        );
    }

    #[test]
    fn devon_cache_future_version_fixture_seeds_user_version_99() {
        let fix = DevonCacheFutureVersionFixture::build();
        let cache_path = fix.cache_path();
        assert!(
            cache_path.exists(),
            "fixture must pre-install future-version cache.sqlite"
        );
        // Confirm PRAGMA user_version = 99 round-trips. Use the read-only
        // CacheVerifier so we exercise the same path the recovery scenarios
        // will use.
        let verifier = CacheVerifier::open(&cache_path).expect("open verifier");
        assert_eq!(
            verifier.pragma_user_version().expect("pragma"),
            99,
            "future-version fixture must have user_version = 99"
        );
    }

    #[test]
    fn devon_cache_empty_fixture_layout_matches_spec() {
        let fix = DevonCacheEmptyFixture::build();
        // cache.sqlite must NOT exist at construction time.
        assert!(
            !fix.cache_path().exists(),
            "fixture must not pre-create cache.sqlite"
        );
        // Parent of cache_path must exist (xdg-data/modeltap/).
        assert!(
            fix.cache_path().parent().unwrap().exists(),
            "cache_path parent dir must exist"
        );
        // The synthetic model file must exist where TestTool::discover looks.
        let expected_model = fix.test_tool_root().join(TEST_MODEL_FILENAME);
        assert!(
            expected_model.exists(),
            "fixture must seed the TestTool's model file at {}",
            expected_model.display()
        );
        // log_dir exists.
        assert!(fix.log_dir().exists(), "log_dir must exist");
    }
}
