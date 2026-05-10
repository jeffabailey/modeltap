#!/usr/bin/env bash
# Run `cargo test` with a parallel Gatekeeper pre-warm on macOS.
#
# Background — why this exists:
#   On macOS Sonoma+, every freshly-linked Mach-O binary triggers a synchronous
#   syspolicyd / XProtect yara-rules scan on first execution. The scan can take
#   10–100+ seconds per binary. With ~75 integration test binaries in this
#   workspace, cargo test (which launches each binary serially) spends most of
#   its time waiting for these scans, not running test code.
#
# What this script does:
#   1. Asks cargo for the EXACT current test binaries via
#      `--message-format=json` (avoids touching the hundreds of stale builds
#      that accumulate in target/debug/deps from incremental rebuilds).
#   2. Invokes each with `--list` (a sub-millisecond no-op that still triggers
#      the scan) in parallel batches, so multiple Gatekeeper scans queue up at
#      once instead of strictly serializing behind cargo's runner.
#   3. Hands off to `cargo test --workspace`.
#
# Realistic expectations:
#   The kernel scan queue parallelizes only weakly (~1.3-1.5x at 16-wide), so
#   this is a partial speed-up, not a magic bullet. For a *real* fix, add your
#   terminal emulator to System Settings > Privacy & Security > Developer Tools
#   — binaries spawned by an allowed dev tool skip the first-run scan entirely
#   and a fresh test build drops from ~25 min to ~1-2 min.
#
# Linux/Windows: the macOS-specific branch is skipped; behaves like
# `cargo test --workspace`.
#
# Usage:
#   scripts/test.sh                        # cargo test --workspace
#   scripts/test.sh -p modeltap-core       # forward args to cargo test
#   scripts/test.sh -- --nocapture         # forward post-`--` args

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" == "Darwin" ]]; then
    echo "→ building test binaries..."
    BIN_LIST=$(cargo test --workspace --no-run --message-format=json "$@" \
        | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' \
        | sort -u)
    BIN_COUNT=$(printf '%s\n' "$BIN_LIST" | grep -c . || true)
    echo "→ pre-warming Gatekeeper for $BIN_COUNT current test binaries (parallel x16)..."
    printf '%s\n' "$BIN_LIST" \
        | xargs -P 16 -n 1 -I{} bash -c '"{}" --list >/dev/null 2>&1 || true'
    echo "→ pre-warm complete; running cargo test"
else
    cargo test --workspace --no-run "$@"
fi

exec cargo test --workspace "$@"
