#!/usr/bin/env bash
# Build modeltap from source and run it.
#
# `CC=/usr/bin/cc` works around `~/.pyenv/shims/cc` shadowing the system
# compiler on macOS dev machines (documented in CLAUDE.md).
#
# Any arguments are forwarded to the binary, so `./run.sh --no-cache`,
# `./run.sh --headless --quit-after-paint`, etc. work as expected.

set -euo pipefail
cd "$(dirname "$0")"
CC=/usr/bin/cc cargo build --release -p modeltap-app
exec ./target/release/modeltap "$@"
