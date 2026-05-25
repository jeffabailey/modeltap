//! Step-definitions for the concurrent-process cache scenarios (US-23
//! Scenarios 4-5 / AC-23-10).
//!
//! tool-model-info-sqlite-cache step 04-04. Mirrors the pattern established
//! by `cache_lifecycle.rs` (step 01-05) and `cache_opt_out.rs` (step 04-02):
//! each Gherkin phrase becomes a plain Rust function over a per-scenario
//! `ConcurrentWorld` struct; the driver file invokes them in scenario order.
//!
//! Two scenarios:
//!
//!   1. "Two modeltap processes can read the cache concurrently via SQLite
//!      WAL" — process A writes one row in a cold-start launch, then two
//!      processes B and C launch back-to-back against the same
//!      `MODELTAP_CACHE_PATH`. Both exit 0 — neither encounters
//!      `SQLITE_BUSY` because WAL allows concurrent readers.
//!
//!   2. "Concurrent cache writes serialise via busy_timeout" — process A
//!      launches with `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS=2000` so its
//!      reconcile writeback holds the per-tool transaction for 2 seconds
//!      BEFORE COMMIT. Process B launches ~100 ms after A and contests for
//!      the write lock. Both processes exit 0, process B's `launch.log`
//!      contains a `cache.write_wait_ms` event with `wait_ms` in
//!      `[0, 5000]`, and the final `cache_tools.last_scan_at` reflects
//!      process B's later write.

#![allow(dead_code)] // Step phrases are referenced by the cache_concurrent
                     // driver; the rest of the workspace doesn't import them.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use modeltap_acceptance::fixtures::cache_fixtures::{CacheVerifier, DevonCacheEmptyFixture};
use modeltap_acceptance::test_tool::TEST_MODEL_FILENAME;
use serde_json::Value;

/// Per-tool wait budget the busy_timeout PRAGMA enforces (`apply_open_pragmas`
/// in `modeltap-store/src/open.rs`). Process B's `cache.write_wait_ms` event
/// must fall within `[0, BUSY_TIMEOUT_MS]` — exceeding it would mean SQLite
/// returned `SQLITE_BUSY` to the writer, which the acceptance contract
/// forbids (AC-23-10).
pub const BUSY_TIMEOUT_MS: u64 = 5000;

/// How long process A holds the write lock for the busy_timeout scenario.
/// Chosen so process B's wait is large enough to be observable (~1.5 s
/// budget on slow CI) without exceeding the 5 s busy_timeout cap.
pub const HOLD_LOCK_MS: u64 = 2000;

/// Stagger between A's start and B's start in the busy_timeout scenario.
/// Long enough that A is reliably inside its BEGIN..COMMIT window when B
/// fires; short enough that B contests the lock instead of arriving after
/// A's transaction commits.
pub const STAGGER_MS: u64 = 100;

/// Mutable scenario state. One per scenario.
pub struct ConcurrentWorld {
    pub fixture: DevonCacheEmptyFixture,
    /// stdout/exit captured from process A in scenario order.
    pub process_a: Option<ProcessResult>,
    /// stdout/exit captured from process B in scenario order.
    pub process_b: Option<ProcessResult>,
    /// In the concurrent-reads scenario, a third process C launches in
    /// parallel with B. Both contend only for the read path.
    pub process_c: Option<ProcessResult>,
    /// `cache_tools.last_scan_at` observed after process A commits its
    /// row, captured by the busy_timeout scenario so the post-B
    /// assertion can prove B's write supplanted A's.
    pub last_scan_at_after_a: Option<String>,
}

impl ConcurrentWorld {
    pub fn new() -> Self {
        Self {
            fixture: DevonCacheEmptyFixture::build(),
            process_a: None,
            process_b: None,
            process_c: None,
            last_scan_at_after_a: None,
        }
    }
}

/// Captured outcome of one modeltap binary invocation.
pub struct ProcessResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

// ---------------------------------------------------------------------------
// Given steps
// ---------------------------------------------------------------------------

pub fn given_the_cache_file_does_not_exist(world: &ConcurrentWorld) {
    let path = world.fixture.cache_path();
    assert!(
        !path.exists(),
        "precondition violated: cache.sqlite already exists at {}",
        path.display()
    );
}

pub fn given_the_test_tool_will_discover_one_model(world: &ConcurrentWorld) {
    let model_path = world.fixture.test_tool_root().join(TEST_MODEL_FILENAME);
    assert!(
        model_path.exists(),
        "TestTool's seed model must exist at {} before the binary launches",
        model_path.display()
    );
}

/// Spawn a modeltap process A that performs cold-start discovery and
/// reconcile-writeback, then exits. Used as the seed for both concurrent
/// scenarios so process B has a cache.sqlite to read.
pub fn given_process_a_has_written_an_initial_cache(world: &mut ConcurrentWorld) {
    let result = run_modeltap_command(world, None);
    assert_eq!(
        result.exit_code, 0,
        "seed process A must exit 0; stderr=\n{}",
        result.stderr
    );
    world.process_a = Some(result);
    // Capture last_scan_at so the post-B assertion can compare. Use the
    // CacheVerifier seam — read-only, no contention with future writers.
    let verifier = CacheVerifier::open(&world.fixture.cache_path())
        .expect("open cache verifier after process A");
    world.last_scan_at_after_a = verifier.last_scan_at_for("test-tool").ok().flatten();
}

// ---------------------------------------------------------------------------
// When steps
// ---------------------------------------------------------------------------

/// `When two modeltap processes B and C launch concurrently against the same
/// cache file`
///
/// Spawns process B and process C in parallel against the same
/// `MODELTAP_CACHE_PATH`. Each is given its own MODELTAP_LOG_DIR so the
/// reader scenarios do not contest a shared log file (the cache file IS
/// the shared resource — that is what we test).
///
/// Truncates process A's launch.log first so any post-launch JSONL
/// inspection observes ONLY the new processes' events.
pub fn when_two_modeltap_processes_launch_concurrently(world: &mut ConcurrentWorld) {
    // Process B uses the fixture's primary log dir. Process C gets a
    // sibling tempdir so its log file does not contend with B's.
    let log_dir_b = world.fixture.log_dir();
    truncate_launch_log(&log_dir_b);

    let log_dir_c = world.fixture.temp.path().join("logs-c");
    std::fs::create_dir_all(&log_dir_c).expect("create logs-c");

    // Spawn both processes. `Child` is held across the join so neither is
    // dropped (which would orphan-reap) before we capture its output.
    let cache_path = world.fixture.cache_path();
    let test_tool_root = world.fixture.test_tool_root();

    let mut child_b = spawn_modeltap_child(&cache_path, &test_tool_root, &log_dir_b, None);
    let mut child_c = spawn_modeltap_child(&cache_path, &test_tool_root, &log_dir_c, None);

    let result_b = wait_for_child(&mut child_b, "B");
    let result_c = wait_for_child(&mut child_c, "C");

    world.process_b = Some(result_b);
    world.process_c = Some(result_c);
}

/// `When process A launches holding the write lock for 2 seconds and process
/// B launches 100 ms later`
///
/// Spawns process A with `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS=2000` then,
/// after `STAGGER_MS`, spawns process B against the same cache file.
/// Process A's reconcile-writeback transaction sleeps for 2 s BEFORE
/// COMMIT (the cfg-gated seam in `Cache::reconcile_tool`); process B's
/// `BEGIN IMMEDIATE` must busy-wait under the busy_timeout PRAGMA.
///
/// Both children are awaited and their outcomes captured. The driver
/// then asserts both exited 0, B's `cache.write_wait_ms` event fired,
/// and `cache_tools.last_scan_at` advanced.
pub fn when_process_a_holds_write_lock_then_b_contests(world: &mut ConcurrentWorld) {
    truncate_launch_log(&world.fixture.log_dir());

    let cache_path = world.fixture.cache_path();
    let test_tool_root = world.fixture.test_tool_root();

    // Process A: shares the fixture log dir so the test can inspect its
    // log if needed, but the cache.write_wait_ms event we assert is
    // process B's.
    let log_dir_a = world.fixture.log_dir();
    let mut child_a =
        spawn_modeltap_child(&cache_path, &test_tool_root, &log_dir_a, Some(HOLD_LOCK_MS));

    // Stagger so A is reliably inside the BEGIN..COMMIT window when B
    // fires its own BEGIN IMMEDIATE.
    thread::sleep(Duration::from_millis(STAGGER_MS));

    // Process B uses a sibling log dir so its cache.write_wait_ms event
    // is in a file we can read in isolation from A's.
    let log_dir_b = world.fixture.temp.path().join("logs-b");
    std::fs::create_dir_all(&log_dir_b).expect("create logs-b");
    let mut child_b = spawn_modeltap_child(&cache_path, &test_tool_root, &log_dir_b, None);

    let result_a = wait_for_child(&mut child_a, "A");
    let result_b = wait_for_child(&mut child_b, "B");

    world.process_a = Some(result_a);
    world.process_b = Some(result_b);
}

// ---------------------------------------------------------------------------
// Then steps
// ---------------------------------------------------------------------------

pub fn then_both_processes_exit_zero(world: &ConcurrentWorld) {
    let b = world.process_b.as_ref().expect("process B captured");
    let c = world.process_c.as_ref().expect("process C captured");
    assert_eq!(
        b.exit_code, 0,
        "process B must exit 0; stderr=\n{}",
        b.stderr
    );
    assert_eq!(
        c.exit_code, 0,
        "process C must exit 0; stderr=\n{}",
        c.stderr
    );
}

pub fn then_process_a_and_b_both_exit_zero(world: &ConcurrentWorld) {
    let a = world.process_a.as_ref().expect("process A captured");
    let b = world.process_b.as_ref().expect("process B captured");
    assert_eq!(
        a.exit_code, 0,
        "process A must exit 0; stderr=\n{}",
        a.stderr
    );
    assert_eq!(
        b.exit_code, 0,
        "process B must exit 0; stderr=\n{}",
        b.stderr
    );
}

/// `Then neither process surfaces SQLITE_BUSY in its stderr`
///
/// rusqlite formats `SQLITE_BUSY` errors with the substring "database is
/// locked" (per rusqlite's Display impl). The reconcile-writeback path
/// also logs the raw error to stderr on failure, so a `BUSY` would
/// surface there. The acceptance contract is that the busy_timeout
/// PRAGMA absorbs the wait before the error ever reaches the user.
pub fn then_neither_process_emits_sqlite_busy(world: &ConcurrentWorld) {
    for (name, result) in [
        ("B", world.process_b.as_ref()),
        ("C", world.process_c.as_ref()),
    ] {
        let result = result.unwrap_or_else(|| panic!("process {name} captured"));
        let combined = format!("{}\n{}", result.stdout, result.stderr).to_ascii_lowercase();
        assert!(
            !combined.contains("database is locked"),
            "process {name} stderr/stdout must NOT contain 'database is locked'; got:\n{}",
            result.stderr
        );
        assert!(
            !combined.contains("sqlite_busy"),
            "process {name} stderr/stdout must NOT contain 'sqlite_busy'; got:\n{}",
            result.stderr
        );
    }
}

/// `Then process B's launch.log contains a cache.write_wait_ms event with
/// 0 <= wait_ms <= 5000`
///
/// Returns the observed wait so the driver can also assert it advanced
/// (i.e. process A's hold-lock seam actually fired). Looks in the
/// `logs-b` sibling tempdir written by `when_process_a_holds_write_lock_then_b_contests`.
pub fn then_process_b_emits_cache_write_wait_event(world: &ConcurrentWorld) -> u64 {
    let log_dir_b = world.fixture.temp.path().join("logs-b");
    let events = read_launch_log(&log_dir_b);
    let event = events
        .iter()
        .find(|e| e.get("event").and_then(|v| v.as_str()) == Some("cache.write_wait_ms"))
        .unwrap_or_else(|| {
            panic!(
                "process B's launch.log must contain a cache.write_wait_ms event; got events: {:?}",
                events
                    .iter()
                    .filter_map(|e| e.get("event").and_then(|v| v.as_str()))
                    .collect::<Vec<_>>()
            )
        });
    let wait_ms = event
        .get("wait_ms")
        .and_then(|v| v.as_u64())
        .expect("cache.write_wait_ms event must carry wait_ms as a non-negative integer");
    assert!(
        wait_ms <= BUSY_TIMEOUT_MS,
        "process B's cache.write_wait_ms must be <= busy_timeout ({BUSY_TIMEOUT_MS} ms); got {wait_ms} ms"
    );
    wait_ms
}

/// `Then cache_tools.last_scan_at for test-tool reflects process B's later
/// write`
///
/// Reads `cache_tools.last_scan_at` via the read-only CacheVerifier and
/// asserts it is greater than the value captured after process A. ISO-8601
/// UTC strings are lexicographically orderable (per the column convention
/// in `format_iso8601_utc`), so a string `>` comparison gives the right
/// answer.
pub fn then_last_scan_at_reflects_process_b_write(world: &ConcurrentWorld) {
    let verifier = CacheVerifier::open(&world.fixture.cache_path())
        .expect("open cache verifier after process B");
    let after_b = verifier
        .last_scan_at_for("test-tool")
        .expect("query cache_tools.last_scan_at after B")
        .expect("cache_tools row must exist after process B");
    let after_a = world
        .last_scan_at_after_a
        .as_deref()
        .expect("last_scan_at_after_a captured during seed");
    assert!(
        after_b.as_str() > after_a,
        "cache_tools.last_scan_at must advance past process A's write; \
         after_a={after_a}, after_b={after_b}"
    );
}

// ---------------------------------------------------------------------------
// Helpers (internal plumbing, not Gherkin step phrases)
// ---------------------------------------------------------------------------

/// Build a one-shot `std::process::Command` for the modeltap binary,
/// configured with all the env vars the concurrent scenarios need.
///
/// `hold_lock_ms` plumbs `MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS`. The binary's
/// `test-harness` feature (default-on for `cargo test`) makes the seam
/// observe this env var via the cfg-gated branch in `Cache::reconcile_tool`.
fn run_modeltap_command(world: &ConcurrentWorld, hold_lock_ms: Option<u64>) -> ProcessResult {
    let log_dir = world.fixture.log_dir();
    let mut child = spawn_modeltap_child(
        &world.fixture.cache_path(),
        &world.fixture.test_tool_root(),
        &log_dir,
        hold_lock_ms,
    );
    wait_for_child(&mut child, "A-seed")
}

/// Spawn a modeltap child process and return its `Child` handle so the
/// caller can decide when to await it. Stdout/stderr are piped so the
/// captured `ProcessResult` carries both for diagnostic dumps.
fn spawn_modeltap_child(
    cache_path: &Path,
    test_tool_root: &Path,
    log_dir: &Path,
    hold_lock_ms: Option<u64>,
) -> Child {
    let bin = modeltap_binary_path();
    let mut cmd = Command::new(bin);
    cmd.arg("--quit-after-paint")
        .env("MODELTAP_HEADLESS", "1")
        .env("MODELTAP_TERM_COLS", "100")
        .env("MODELTAP_TEST_PLUGINS", "test-tool")
        .env("MODELTAP_TEST_TOOL_ROOT", test_tool_root)
        .env("MODELTAP_CACHE_PATH", cache_path)
        .env("MODELTAP_LOG_DIR", log_dir)
        .env("MODELTAP_OLLAMA_DIR", "/nonexistent/no-such-ollama")
        .env("MODELTAP_LOOSE_GGUF_DIRS", "/nonexistent/no-such-llama-cli")
        .env("MODELTAP_LMSTUDIO_DIRS", "/nonexistent/no-such-lm-studio")
        .env(
            "MODELTAP_ATOMIC_CHAT_DIRS",
            "/nonexistent/no-such-atomic-chat",
        )
        .env("MODELTAP_GPT4ALL_DIRS", "/nonexistent/no-such-gpt4all")
        .env("MODELTAP_CONFIG_PATH", "/nonexistent/no-such-config.toml")
        .env("HF_HOME", "/nonexistent/no-such-hf-cache")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(ms) = hold_lock_ms {
        cmd.env("MODELTAP_DEBUG_HOLD_WRITE_LOCK_MS", ms.to_string());
    }
    cmd.spawn()
        .unwrap_or_else(|e| panic!("spawn modeltap: {e}"))
}

/// Locate the modeltap binary built by `cargo test`. Uses the
/// `CARGO_BIN_EXE_modeltap` env var if Cargo set it (preferred), otherwise
/// falls back to `target/debug/modeltap` at the workspace root. The
/// fallback is required because `modeltap-acceptance` (this crate) does
/// not declare modeltap as a `[[bin]]` dependency — Cargo only injects
/// `CARGO_BIN_EXE_*` when the test binary lives in the same package as
/// the binary.
fn modeltap_binary_path() -> PathBuf {
    if let Ok(p) = std::env::var("CARGO_BIN_EXE_modeltap") {
        return PathBuf::from(p);
    }
    // Walk up from CARGO_MANIFEST_DIR to the workspace root, then into
    // target/debug/.
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo test");
    let mut workspace_root = PathBuf::from(&manifest_dir);
    // `manifest_dir` ends at `<root>/tests`; pop once.
    workspace_root.pop();
    let candidate = workspace_root.join("target").join("debug").join("modeltap");
    assert!(
        candidate.exists(),
        "modeltap binary not found at {}; run `cargo build -p modeltap-app` first",
        candidate.display()
    );
    candidate
}

/// Block until `child` exits, capturing stdout/stderr. Per CLAUDE.md
/// §Running Tests Fast on macOS, the concurrent scenarios cannot use
/// `assert_cmd::Command::output()` for both processes simultaneously
/// (it blocks until exit, which would serialize them); we go through
/// `std::process::Command::spawn` + `wait_with_output`.
fn wait_for_child(child: &mut Child, name: &str) -> ProcessResult {
    let start = Instant::now();
    // Defensive timeout: the slowest concurrent-writers scenario is
    // ~2 s for process A + ~5 s busy_timeout cap for B. 30 s is the
    // same upper bound the walking-skeleton uses.
    const TIMEOUT_SECS: u64 = 30;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    // Spawn reader threads so reads do not block waitpid.
    let stdout_thread = stdout.map(|mut s| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut s, &mut buf).ok();
            String::from_utf8_lossy(&buf).into_owned()
        })
    });
    let stderr_thread = stderr.map(|mut s| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut s, &mut buf).ok();
            String::from_utf8_lossy(&buf).into_owned()
        })
    });

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = stdout_thread
                    .map(|t| t.join().unwrap_or_default())
                    .unwrap_or_default();
                let stderr = stderr_thread
                    .map(|t| t.join().unwrap_or_default())
                    .unwrap_or_default();
                return ProcessResult {
                    exit_code: status.code().unwrap_or(-1),
                    stdout,
                    stderr,
                };
            }
            Ok(None) => {
                if start.elapsed() > Duration::from_secs(TIMEOUT_SECS) {
                    let _ = child.kill();
                    panic!("process {name} timed out after {TIMEOUT_SECS} s");
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("wait for process {name}: {e}"),
        }
    }
}

/// Truncate `<log_dir>/launch.log` so a fresh JSONL inspection observes
/// ONLY the events emitted by the upcoming launch.
fn truncate_launch_log(log_dir: &Path) {
    let path = log_dir.join("launch.log");
    if path.exists() {
        std::fs::write(&path, b"").expect("truncate launch.log");
    }
}

/// Read every JSONL line in `<log_dir>/launch.log`. Empty lines and
/// unparseable lines are skipped. Returns an empty Vec if the file is
/// absent (best-effort emission per `emit_cache_write_wait_event`).
pub fn read_launch_log(log_dir: &Path) -> Vec<Value> {
    let path = log_dir.join("launch.log");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Drop-in convenience: keep the SystemTime import meaningful so the
/// timestamp comparison helper (above) compiles. Not a step phrase.
fn _silence_unused_systemtime() -> SystemTime {
    SystemTime::now()
}
