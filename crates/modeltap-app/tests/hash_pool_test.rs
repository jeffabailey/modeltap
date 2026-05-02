//! Integration tests for the background SHA256 hash pool (ADR-013, step 01-07).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: Happy path — N jobs all complete → N HashComputed msgs, counters
//!         (total=N, completed=N, failed=0).
//!     B2: Failure path — job pointing at non-existent file → HashFailed msg,
//!         counters (completed=1, failed=1).
//!     B3: Cancellation — cancel token fires mid-flight → join completes within
//!         the 200 ms timeout, no panics.
//!     B4: Throttle — long-running job triggers HashProgressTick at ~250 ms
//!         cadence (allow 2x slack ≤500 ms between ticks).
//!     B5: MODELTAP_HASH_WORKERS env var caps concurrent worker count.
//!     B6: Dropped msg_tx receiver does not panic any worker (graceful send
//!         failure).
//!   budget = 6 × 2 = 12 tests max. We use 6.
//!
//! Per ADR-013, the pool is a fixed pool of `min(num_cpus, 4)` `spawn_blocking`
//! workers consuming a bounded `tokio::sync::mpsc` queue. A separate throttle
//! task posts `Msg::HashProgressTick` at 250 ms cadence. Cooperative shutdown
//! uses `tokio_util::sync::CancellationToken`.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use modeltap_app::hash_pool::{spawn, HashJob};
use modeltap_app::sha256_cache::Sha256Cache;
use modeltap_core::ports::{HashProgress, Hasher};
use modeltap_core::{ContentHash, ToolId};
use modeltap_tui::msg::{HashFailureReason, Msg};
use tokio_util::sync::CancellationToken;

const TOOL: ToolId = ToolId("ollama");

// ---------------------------------------------------------------------------
// FakeHasher — deterministic 5-line `Hasher` for tests.
// Returns canned hash; optionally sleeps to simulate a long file. Records
// invocation count + max-concurrent invocations to verify B5.
// ---------------------------------------------------------------------------

struct FakeHasher {
    canned_hash: ContentHash,
    sleep_ms: u64,
    in_flight: Arc<std::sync::atomic::AtomicUsize>,
    max_in_flight: Arc<std::sync::atomic::AtomicUsize>,
}

impl FakeHasher {
    fn new(canned_hash: ContentHash) -> Self {
        Self {
            canned_hash,
            sleep_ms: 0,
            in_flight: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            max_in_flight: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    fn slow(canned_hash: ContentHash, sleep_ms: u64) -> Self {
        let mut h = Self::new(canned_hash);
        h.sleep_ms = sleep_ms;
        h
    }
}

impl Hasher for FakeHasher {
    fn sha256_streaming(
        &self,
        path: &Path,
        _progress: &mut dyn FnMut(HashProgress),
    ) -> std::io::Result<ContentHash> {
        // Surface a real I/O failure when the file does not exist — the worker
        // is expected to translate this into Msg::HashFailed.
        if !path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("test: fake hasher saw missing path: {}", path.display()),
            ));
        }
        let cur = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        // Race: track the max observed concurrency to verify B5 worker cap.
        self.max_in_flight.fetch_max(cur, Ordering::SeqCst);
        if self.sleep_ms > 0 {
            std::thread::sleep(Duration::from_millis(self.sleep_ms));
        }
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(self.canned_hash)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const HASH_A: ContentHash = ContentHash([0xAA; 32]);

fn write_temp_file(dir: &tempfile::TempDir, name: &str, contents: &[u8]) -> PathBuf {
    let p = dir.path().join(name);
    std::fs::write(&p, contents).expect("write temp file");
    p
}

fn make_job(tool: ToolId, model_id: &str, path: PathBuf) -> HashJob {
    let meta = std::fs::metadata(&path).ok();
    let (mtime, size) = meta
        .map(|m| {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (mtime, m.len())
        })
        .unwrap_or((0, 0));
    HashJob {
        tool,
        model_id: model_id.to_string(),
        path,
        mtime,
        size,
    }
}

/// Drain the receiver until the supplied predicate returns Some(value), or
/// the timeout elapses. Returns None on timeout.
async fn collect_until<F, T>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Msg>,
    timeout: Duration,
    mut pred: F,
) -> (Vec<Msg>, Option<T>)
where
    F: FnMut(&[Msg]) -> Option<T>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    let mut collected = Vec::new();
    loop {
        if let Some(v) = pred(&collected) {
            return (collected, Some(v));
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return (collected, None);
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(msg)) => collected.push(msg),
            Ok(None) => {
                let v = pred(&collected);
                return (collected, v);
            }
            Err(_) => {
                let v = pred(&collected);
                return (collected, v);
            }
        }
    }
}

fn count_completed(msgs: &[Msg]) -> usize {
    msgs.iter()
        .filter(|m| matches!(m, Msg::HashComputed { .. }))
        .count()
}

fn count_failed(msgs: &[Msg]) -> usize {
    msgs.iter()
        .filter(|m| matches!(m, Msg::HashFailed { .. }))
        .count()
}

fn count_ticks(msgs: &[Msg]) -> usize {
    msgs.iter()
        .filter(|m| matches!(m, Msg::HashProgressTick))
        .count()
}

// ---------------------------------------------------------------------------
// T1 — Happy path: 3 jobs all complete → 3 HashComputed msgs received,
// counters at total=3, completed=3, failed=0.
// ---------------------------------------------------------------------------

#[test]
fn t1_happy_path_three_jobs_complete() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let p1 = write_temp_file(&dir, "a.gguf", b"alpha");
        let p2 = write_temp_file(&dir, "b.gguf", b"beta");
        let p3 = write_temp_file(&dir, "c.gguf", b"gamma");

        let jobs = vec![
            make_job(TOOL, "alpha", p1),
            make_job(TOOL, "beta", p2),
            make_job(TOOL, "gamma", p3),
        ];

        let cache = Sha256Cache::new();
        let hasher: Arc<dyn Hasher + Send + Sync> = Arc::new(FakeHasher::new(HASH_A));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
        let cancel = CancellationToken::new();
        let handle = spawn(
            jobs,
            cache,
            hasher,
            tx,
            cancel,
            &tokio::runtime::Handle::current(),
        );

        let progress = handle.progress.clone();
        let (msgs, _) = collect_until(&mut rx, Duration::from_secs(5), |c| {
            (count_completed(c) >= 3).then_some(())
        })
        .await;

        assert_eq!(
            count_completed(&msgs),
            3,
            "expected 3 HashComputed msgs, got {} (msgs: {:?})",
            count_completed(&msgs),
            msgs
        );
        assert_eq!(count_failed(&msgs), 0, "no failures expected");
        assert_eq!(progress.total.load(Ordering::SeqCst), 3, "total=3");
        assert_eq!(progress.completed.load(Ordering::SeqCst), 3, "completed=3");
        assert_eq!(progress.failed.load(Ordering::SeqCst), 0, "failed=0");

        let _ = handle.shutdown().await;
    });
}

// ---------------------------------------------------------------------------
// T2 — Failure path: job with non-existent file → 1 HashFailed msg,
// completed=1, failed=1 (the worker IS done — just unsuccessfully).
// ---------------------------------------------------------------------------

#[test]
fn t2_failure_path_unreadable_file_emits_hash_failed() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        // Path that does not exist.
        let bad = PathBuf::from("/nonexistent/modeltap-test/missing-12345.gguf");
        let job = HashJob {
            tool: TOOL,
            model_id: "ghost".to_string(),
            path: bad,
            mtime: 0,
            size: 0,
        };

        let cache = Sha256Cache::new();
        let hasher: Arc<dyn Hasher + Send + Sync> = Arc::new(FakeHasher::new(HASH_A));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
        let cancel = CancellationToken::new();
        let handle = spawn(
            vec![job],
            cache,
            hasher,
            tx,
            cancel,
            &tokio::runtime::Handle::current(),
        );

        let progress = handle.progress.clone();
        let (msgs, _) = collect_until(&mut rx, Duration::from_secs(5), |c| {
            (count_failed(c) >= 1).then_some(())
        })
        .await;

        assert_eq!(count_failed(&msgs), 1, "expected one HashFailed msg");
        // Per Msg variant comment: "increments completed (the worker IS done
        // — just unsuccessfully)".
        assert_eq!(progress.completed.load(Ordering::SeqCst), 1, "completed=1");
        assert_eq!(progress.failed.load(Ordering::SeqCst), 1, "failed=1");

        // Verify the HashFailed reason is Io (not Cancelled or Other).
        let failed = msgs
            .iter()
            .find_map(|m| match m {
                Msg::HashFailed { reason, .. } => Some(reason.clone()),
                _ => None,
            })
            .expect("HashFailed present");
        assert!(
            matches!(failed, HashFailureReason::Io(_)),
            "expected Io reason, got {failed:?}"
        );

        let _ = handle.shutdown().await;
    });
}

// ---------------------------------------------------------------------------
// T3 — Cancellation: large job count, cancel mid-flight. Join completes within
// ≤200 ms and no panics surface.
// ---------------------------------------------------------------------------

#[test]
fn t3_cancellation_workers_exit_within_timeout() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        let dir = tempfile::tempdir().unwrap();
        // 50 jobs each sleeping 100ms — enough to keep workers busy.
        let mut jobs = Vec::new();
        for i in 0..50 {
            let name = format!("m{i}.gguf");
            let p = write_temp_file(&dir, &name, b"x");
            jobs.push(make_job(TOOL, &format!("m{i}"), p));
        }

        let cache = Sha256Cache::new();
        let hasher: Arc<dyn Hasher + Send + Sync> = Arc::new(FakeHasher::slow(HASH_A, 100));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
        let cancel = CancellationToken::new();
        let handle = spawn(
            jobs,
            cache,
            hasher,
            tx,
            cancel.clone(),
            &tokio::runtime::Handle::current(),
        );

        // Let a couple of workers actually start.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let started = tokio::time::Instant::now();
        // shutdown() cancels and awaits join with a 200ms timeout.
        let _ = handle.shutdown().await;
        let elapsed = started.elapsed();

        // Allow generous slack (500ms) over the 200ms internal cap to absorb
        // CI scheduling noise; the in-flight blocking task can take up to
        // ~sleep_ms (100ms) to return after cancel + the join window.
        assert!(
            elapsed < Duration::from_millis(800),
            "shutdown took too long: {:?}",
            elapsed
        );
    });
}

// ---------------------------------------------------------------------------
// T4 — Throttle ticks: 1 long-running job in flight; observe HashProgressTick
// at ~250 ms cadence (allow 2x slack ≤500ms between ticks).
// ---------------------------------------------------------------------------

#[test]
fn t4_throttle_emits_progress_ticks() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let p = write_temp_file(&dir, "long.gguf", b"x");
        let job = make_job(TOOL, "long", p);

        let cache = Sha256Cache::new();
        // 1 job, 1.5s sleep — the throttle should fire several times before
        // the job completes.
        let hasher: Arc<dyn Hasher + Send + Sync> = Arc::new(FakeHasher::slow(HASH_A, 1500));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
        let cancel = CancellationToken::new();
        let handle = spawn(
            vec![job],
            cache,
            hasher,
            tx,
            cancel.clone(),
            &tokio::runtime::Handle::current(),
        );

        // Collect for ~1.2s to observe at least 3 ticks (250ms cadence) without
        // waiting for the full hash to complete.
        let (msgs, _) = collect_until(&mut rx, Duration::from_millis(1200), |c| {
            (count_ticks(c) >= 3).then_some(())
        })
        .await;

        assert!(
            count_ticks(&msgs) >= 3,
            "expected ≥3 HashProgressTick at 250ms cadence in 1200ms, got {} (msgs: {:?})",
            count_ticks(&msgs),
            msgs
        );

        let _ = handle.shutdown().await;
    });
}

// ---------------------------------------------------------------------------
// T5 — MODELTAP_HASH_WORKERS env var caps the concurrent spawn_blocking count.
// Set to 1 → the FakeHasher's max_in_flight stays at 1.
// ---------------------------------------------------------------------------

#[test]
fn t5_env_var_caps_worker_concurrency() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        // SAFETY: tests run in the same process; set + restore.
        let prev = std::env::var("MODELTAP_HASH_WORKERS").ok();
        std::env::set_var("MODELTAP_HASH_WORKERS", "1");

        let dir = tempfile::tempdir().unwrap();
        let mut jobs = Vec::new();
        for i in 0..6 {
            let name = format!("p{i}.gguf");
            let p = write_temp_file(&dir, &name, b"y");
            jobs.push(make_job(TOOL, &format!("p{i}"), p));
        }

        let cache = Sha256Cache::new();
        let fake = FakeHasher::slow(HASH_A, 50);
        let max_in_flight = fake.max_in_flight.clone();
        let hasher: Arc<dyn Hasher + Send + Sync> = Arc::new(fake);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
        let cancel = CancellationToken::new();
        let handle = spawn(
            jobs,
            cache,
            hasher,
            tx,
            cancel,
            &tokio::runtime::Handle::current(),
        );

        // Wait for all 6 to complete.
        let (_, _) = collect_until(&mut rx, Duration::from_secs(5), |c| {
            (count_completed(c) >= 6).then_some(())
        })
        .await;

        let observed = max_in_flight.load(Ordering::SeqCst);
        assert_eq!(
            observed, 1,
            "MODELTAP_HASH_WORKERS=1 must cap concurrency at 1, observed {}",
            observed
        );

        let _ = handle.shutdown().await;

        // Restore env var.
        match prev {
            Some(v) => std::env::set_var("MODELTAP_HASH_WORKERS", v),
            None => std::env::remove_var("MODELTAP_HASH_WORKERS"),
        }
    });
}

// ---------------------------------------------------------------------------
// T6 — msg_tx receiver-dropped robustness: workers must not panic when the
// receiver is dropped mid-flight.
// ---------------------------------------------------------------------------

#[test]
fn t6_receiver_dropped_does_not_panic_workers() {
    let rt = tokio::runtime::Runtime::new().expect("rt");
    rt.block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let mut jobs = Vec::new();
        for i in 0..5 {
            let name = format!("d{i}.gguf");
            let p = write_temp_file(&dir, &name, b"z");
            jobs.push(make_job(TOOL, &format!("d{i}"), p));
        }

        let cache = Sha256Cache::new();
        let panics: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
        let panics_clone = panics.clone();
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |_| {
            *panics_clone.lock().unwrap() = true;
        }));

        let hasher: Arc<dyn Hasher + Send + Sync> = Arc::new(FakeHasher::slow(HASH_A, 30));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Msg>();
        let cancel = CancellationToken::new();
        let handle = spawn(
            jobs,
            cache,
            hasher,
            tx,
            cancel,
            &tokio::runtime::Handle::current(),
        );

        // Drop the receiver immediately so worker sends become SendError.
        drop(rx);

        // Wait long enough for workers to finish their jobs.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = handle.shutdown().await;

        std::panic::set_hook(prev_hook);
        assert!(
            !*panics.lock().unwrap(),
            "no worker may panic when msg_tx receiver is dropped"
        );
    });
}
