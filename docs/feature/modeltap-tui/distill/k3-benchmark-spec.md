# K3 Latency Benchmark Specification — modeltap-tui

**Wave:** DISTILL (5 of 6)
**Author:** Quinn (nw-acceptance-designer)
**Purpose:** specify the CI benchmark that gates K3 (first paint < 1 s, full inventory < 3 s) per `outcome-kpis.md`, `architecture-design.md` § 7, and `ci-pipeline.md` § 2.

## 1. KPI Recap

Per `outcome-kpis.md` and `architecture-design.md` § 7:

| KPI | Target | Hard fail | Soft warn |
|---|---|---|---|
| K3.first_paint_ms | < 1 s on a 2020-or-later workstation | > 2 s (CI gate) | > 1 s (CI warning, not blocking) |
| K3.full_inventory_ms | < 3 s for ≤ 500 models (NFR target) | > 5 s (CI gate) | > 2 s (CI warning) |

The benchmark exists for two reasons:

1. **Per-PR regression detection.** Architecture decisions like "render skeleton first, parallel async discover" (§ 7) are easy to break; a benchmark catches the breakage before merge.
2. **Trend visibility.** GitHub Actions artifacts retain `timing.json` per CI run; the maintainer can plot K3 over time without shipping telemetry.

## 2. Fixture Specification

### 2.1 Fixture name

`k3-bench` — per `acceptance-test-plan.md` § 3 named-fixture inventory.

### 2.2 Fixture contents

A pre-built tree at `tests/fixtures/k3-bench/` (built by `tests/fixtures/build.sh k3-bench`). Total ≤ 200 models distributed across the 4 plugins:

| Plugin | Models | Fixture layout |
|---|---:|---|
| ollama | 50 | 50 manifest files at `${ROOT}/.ollama/models/manifests/registry.ollama.ai/library/<repo>/<tag>` (50 distinct repos), each pointing at a unique blob. Plus 10 additional manifests sharing blobs with the first 10 (testing the deduplication path in size accounting). Blobs are sparse files of average 4 GB. |
| llama-cli | 50 | 50 sparse `.gguf` files in `${ROOT}/llms/`, each with a valid GGUF header (8 KB header + sparse padding). Average 4.5 GB each. |
| hf | 50 | 50 model directories at `${ROOT}/.cache/huggingface/hub/models--<org>--<repo>/snapshots/<rev>/` — each containing a snapshot subdir with one symlink resolving to a sparse blob. Average 5 GB each. |
| lm-studio | 50 | 50 sparse files at `${ROOT}/.cache/lm-studio/models/<org>/<repo>/model.gguf`, average 3.5 GB each. |

**Total apparent size:** ≈ 850 GB. **Total actual disk usage** (sparse files): < 50 MB.

### 2.3 Why 200 models

- 200 is a realistic upper bound for a power user (Devon's first-order ceiling). Per `architecture-design.md` § 7 budget, ≤ 200 models comfortably fits the target K3 budget.
- 500 models would stress-test the system but is beyond current target users; a separate `k3-stress` fixture (deferred to v1.x) would cover that.

### 2.4 Fixture is idempotent and seed-stable

`build.sh k3-bench` is deterministic given a seed: same model names, same blob hashes, same sparse sizes across runs. This ensures CI run-to-run comparability.

```bash
# tests/fixtures/build.sh k3-bench
#
# Generates the K3 benchmark fixture deterministically.
# Idempotent: subsequent runs are no-ops if the fixture is already correct.

set -euo pipefail
SEED="${MODELTAP_FIXTURE_SEED:-k3-bench-2026-04}"
ROOT="${1:-tests/fixtures/k3-bench}"
# ... per-plugin sub-builders, ordered to stable model names ...
```

## 3. Benchmark Invocation

### 3.1 Command

```bash
MODELTAP_HEADLESS=1 \
MODELTAP_FIXTURES=tests/fixtures/k3-bench \
MODELTAP_OLLAMA_DIR=tests/fixtures/k3-bench/.ollama/models \
MODELTAP_LLAMACLI_DIRS=tests/fixtures/k3-bench/llms \
HF_HOME=tests/fixtures/k3-bench/.cache/huggingface \
MODELTAP_LMSTUDIO_DIR=tests/fixtures/k3-bench/.cache/lm-studio/models \
MODELTAP_LOG_DIR=$(mktemp -d) \
./target/release/modeltap --emit-timing > timing.json
```

### 3.2 `--emit-timing` flag contract

When `MODELTAP_HEADLESS=1` AND `--emit-timing` is passed:

- The binary launches, runs the full discovery + indicator computation, captures the `launch.timing` event payload, prints it to stdout as a single JSON object, and exits with code 0.
- No interactive event loop — the binary exits immediately after the timing event is emitted.
- Stdout JSON schema matches the `launch.timing` JSONL event schema in `kpi-instrumentation.md` § 2.1.

```json
{
  "schema": "modeltap.launch.v1",
  "ts": "2026-04-28T14:23:02.245Z",
  "session_id": "01HXY...",
  "event": "launch.timing",
  "modeltap_version": "1.0.0",
  "platform": "linux-x86_64",
  "process_start_to_first_paint_ms": 142,
  "first_paint_to_all_discovered_ms": 781,
  "all_discovered_to_indicators_ms": 187,
  "full_inventory_ms": 1110,
  "model_count": 200,
  "plugin_timings_ms": {
    "ollama": 412,
    "llama-cli": 89,
    "hf": 781,
    "lm-studio": 156
  }
}
```

### 3.3 Build configuration

The benchmark MUST run against a release build (`cargo build --release --package modeltap-app --locked`). Debug builds are 5-20× slower and would produce meaningless gates.

## 4. CI Assertions

Per `ci-pipeline.md` § 2 `k3-bench` job:

```bash
set -e
FIRST_PAINT_MS=$(jq -r '.process_start_to_first_paint_ms' timing.json)
FULL_INV_MS=$(jq -r '.full_inventory_ms' timing.json)
MODEL_COUNT=$(jq -r '.model_count' timing.json)

echo "first_paint_ms=$FIRST_PAINT_MS  full_inventory_ms=$FULL_INV_MS  model_count=$MODEL_COUNT"

# Hard fail: model_count != 200 (fixture corruption)
if [ "$MODEL_COUNT" -ne 200 ]; then
  echo "::error::Fixture corruption: expected 200 models, got $MODEL_COUNT"
  exit 1
fi

# Hard fail: first paint > 2 s (per outcome-kpis.md K3 guardrail)
if [ "$FIRST_PAINT_MS" -gt 2000 ]; then
  echo "::error::K3 regression: first paint ${FIRST_PAINT_MS}ms > 2000ms guardrail"
  exit 1
fi

# Soft warn: first paint > 1 s (target per architecture §7)
if [ "$FIRST_PAINT_MS" -gt 1000 ]; then
  echo "::warning::K3 budget exceeded: first paint ${FIRST_PAINT_MS}ms > 1000ms target"
fi

# Hard fail: full inventory > 5 s (NFR margin over 3 s target)
if [ "$FULL_INV_MS" -gt 5000 ]; then
  echo "::error::Full-inventory regression: ${FULL_INV_MS}ms > 5000ms"
  exit 1
fi

# Soft warn: full inventory > 2 s (target trending)
if [ "$FULL_INV_MS" -gt 2000 ]; then
  echo "::warning::Full inventory ${FULL_INV_MS}ms > 2000ms target"
fi
```

## 5. Per-Plugin Timing Sub-Assertions (advisory)

Beyond the aggregate gates, the timing JSON includes `plugin_timings_ms` per-plugin. CI does NOT hard-gate on these (per-plugin variance is high), but the maintainer can review them in artifacts to spot regressions:

| Plugin | Soft target (advisory) |
|---|---|
| ollama | < 500 ms for 50 manifests |
| llama-cli | < 200 ms for 50 GGUF files (header parse is the bottleneck) |
| hf | < 1000 ms for 50 model dirs (symlink resolution + tree walk) |
| lm-studio | < 300 ms for 50 model files |

If any plugin exceeds 2× the soft target on consecutive runs, open an investigation issue. No automated gating — per-plugin timing is informational.

## 6. Artifact Retention

Per `ci-pipeline.md` the benchmark artifact is retained for 30 days:

```yaml
- name: Upload timing artifact
  uses: actions/upload-artifact@v4
  with:
    name: k3-timing-${{ github.sha }}
    path: timing.json
    retention-days: 30
```

This gives the maintainer 30 days of trend data per branch. For longer-term tracking, the maintainer downloads artifacts and aggregates locally; no time-series database in v1.

## 7. Local Reproducibility

A developer can run the same benchmark locally:

```bash
cargo build --release --package modeltap-app
./tests/fixtures/build.sh k3-bench
make k3-bench   # wraps the env+command from §3.1
```

Recommended local Makefile target:

```makefile
.PHONY: k3-bench
k3-bench:
	@./tests/fixtures/build.sh k3-bench
	@MODELTAP_HEADLESS=1 \
	 MODELTAP_FIXTURES=tests/fixtures/k3-bench \
	 MODELTAP_OLLAMA_DIR=tests/fixtures/k3-bench/.ollama/models \
	 MODELTAP_LLAMACLI_DIRS=tests/fixtures/k3-bench/llms \
	 HF_HOME=tests/fixtures/k3-bench/.cache/huggingface \
	 MODELTAP_LMSTUDIO_DIR=tests/fixtures/k3-bench/.cache/lm-studio/models \
	 MODELTAP_LOG_DIR=$$(mktemp -d) \
	 ./target/release/modeltap --emit-timing | tee timing.json | jq .
```

## 8. Acceptance Test Coverage of K3 (cross-reference)

The master feature file (`features/master-acceptance.feature`) includes two `@k3-latency` scenarios that exercise the same path:

```gherkin
@k3-latency @kpi-instrumentation @real-io
Scenario: First paint latency under 1 second on K3 fixture
  Given fixture "k3-bench" with 200 models across 4 plugins
  When Devon runs "modeltap" in headless mode with --emit-timing
  Then the JSONL log "launch.timing" event has process_start_to_first_paint_ms < 1000
  And the JSONL log "launch.timing" event has full_inventory_ms < 5000
```

These scenarios exist so the K3 contract is covered by the cucumber-rs suite as well as the standalone CI benchmark. The standalone benchmark is faster and produces the artifact for trend tracking; the cucumber scenarios catch the same regressions inside `cargo test`.

## 9. Edge Cases and Caveats

| # | Item | Mitigation |
|---|---|---|
| EC1 | GitHub-hosted runners have variable performance day to day; one outlier run could trigger a false hard-fail. | The 2-second hard-fail threshold is 2× the 1-second target — that variance budget should absorb runner noise. If false fails accumulate (> 1/week), revisit. Recommended: track variance over 100 runs and adjust threshold if p99 > 1500 ms. |
| EC2 | macOS CI runners might be slower than Linux for the same fixture (less SSD throughput). | The benchmark runs on `ubuntu-latest` only per `ci-pipeline.md` § 2 `k3-bench`. macOS performance is validated by the matrix `test` job (which runs the cucumber `@k3-latency` scenarios) but does not gate on absolute timing. |
| EC3 | Sparse-file behavior could differ from real model files (real I/O on big files might be slower than the metadata-only sparse case). | The benchmark measures *discovery* + *plan computation* — both of which are O(model count), not O(byte count). No model file content is read during discovery (per architecture § 7 "no per-launch full-tree filesystem stat storms"). Sparse files are appropriate for this measurement. SHA256 (which IS O(byte count)) is lazy and not exercised by the K3 benchmark. |
| EC4 | The `--emit-timing` flag adds a code path that doesn't exist in production interactive mode. | Acceptable — same code paths run for discovery + indicator computation; only the event-loop entry/exit differs. The cucumber `@k3-latency` scenarios validate the same numbers from a normal interactive launch with `MODELTAP_HEADLESS=1`. |
| EC5 | First runs are slower (cold caches, cold mmaps); CI typically reuses runners but not always. | The 2-second hard-fail accommodates cold runs. |

## 10. What This Spec Does NOT Cover

- **K1 (bytes reclaimed)** — measured per-action by the JSONL event, not by a benchmark. See `kpi-instrumentation.md` and master feature file `@kpi-instrumentation` scenarios.
- **K2 (dedupable %)** — same; measured by `launch.inventory` event.
- **K4 (community plugins)** — process metric, not measured by binary.
- **K5 (accidental loss)** — process metric, tracked via GitHub issue label.
- **Real-cross-fs unify performance** — the K3 fixture is single-filesystem; cross-fs perf is dominated by user choice (skip vs copy), not by inherent system perf.

These KPIs are specified elsewhere; this document specifies K3 only.
