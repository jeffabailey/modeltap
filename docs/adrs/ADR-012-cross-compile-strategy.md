# ADR-012: Cross-compile strategy for `aarch64-unknown-linux-gnu` — `cross` tool

## Status

Proposed (2026-05-03 — DESIGN wave for `release-process-homebrew-github`).

## Context

US-07 requires release builds for 4 targets. Three targets have native runners on GitHub Actions:

| Target | Native runner | Notes |
|---|---|---|
| `aarch64-apple-darwin` | `macos-14` | Apple Silicon hosted runner; native |
| `x86_64-apple-darwin` | `macos-13` | Intel macOS hosted runner; native |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | Native |

**`aarch64-unknown-linux-gnu` has no stable native hosted runner** at design time (May 2026). GitHub announced `ubuntu-22.04-arm` runners in beta in late 2025, but they are not yet in general availability and have observed flakiness. Per C7 (toolchain stability) and K-PIPE (≥95% success rate), depending on a beta runner is unacceptable for v1.

Three approaches exist for producing an `aarch64-unknown-linux-gnu` binary on an `ubuntu-22.04` (x86_64) runner:

1. **`cross` tool** (Docker-based cross-compile, maintained by the cross-rs project)
2. **`rustup target add aarch64-unknown-linux-gnu` + manual linker setup** (`gcc-aarch64-linux-gnu`, env var `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER`)
3. **Self-hosted aarch64 runner** (the maintainer's RPi or AWS Graviton)

Constraints:

- **OSS-only** (`technology-stack.md` policy)
- **K-PIPE ≥95%** — cross-compile must be reliable
- **K-T2T budget**: aarch64-linux cell ≤5 min (per architecture-design §8.4)
- **No proprietary licensing**
- **Maintainability**: contributor must be able to reproduce the cross-compile locally

## Decision

**Use `cross` (cross-rs/cross) v0.2.5 to cross-compile `aarch64-unknown-linux-gnu` on the `ubuntu-22.04` runner.**

Workflow snippet (illustrative; software-crafter writes the exact YAML):

```yaml
- name: Install cross
  if: matrix.target == 'aarch64-unknown-linux-gnu'
  run: cargo install cross --version 0.2.5 --locked

- name: Build (cross)
  if: matrix.target == 'aarch64-unknown-linux-gnu'
  run: cross build --release --locked --target ${{ matrix.target }} --package modeltap-app

- name: Build (native)
  if: matrix.target != 'aarch64-unknown-linux-gnu'
  run: cargo build --release --locked --target ${{ matrix.target }} --package modeltap-app
```

`cross` uses pre-built Docker images (`ghcr.io/cross-rs/aarch64-unknown-linux-gnu:0.2.5`) containing the appropriate cross-toolchain and a recent glibc. The build runs inside the container; the produced binary is statically aware of the correct linker.

## Alternatives Considered

### Alternative 1: `rustup target add` + manual `gcc-aarch64-linux-gnu`

```yaml
- run: rustup target add aarch64-unknown-linux-gnu
- run: sudo apt-get install -y gcc-aarch64-linux-gnu
- run: cargo build --release --locked --target aarch64-unknown-linux-gnu
  env:
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER: aarch64-linux-gnu-gcc
```

- **Pros**: no Docker; faster cold-cache start (no image pull, ~1 min savings); simpler conceptually.
- **Cons**:
  - **C-FFI dependency landmines**: any crate that builds C code via `cc-rs` needs a fully cross-aware build environment. Configuring `CC_aarch64_unknown_linux_gnu`, `AR_aarch64_unknown_linux_gnu`, `PKG_CONFIG_*` per dep is fiddly and breaks silently when a new dep is added (transitive C-FFI is invisible until it bites).
  - **glibc version mismatch risk**: ubuntu-22.04 ships glibc 2.35; the resulting binary requires glibc ≥2.35 on the target. End users on Ubuntu 20.04 (still supported through 2025) can't run it. `cross`'s images use older glibc baselines.
  - **Reproducibility**: maintainer's local cross-compile setup must mirror CI's, which requires per-machine config files and is annoying.
  - **Documented project history**: the `requirements.md` risk register flags exactly this scenario ("Cross-compile of aarch64-unknown-linux-gnu breaks due to dependency adding C-FFI without aarch64 support" — Medium probability).
- **Rejection rationale**: lower reliability for a 1-minute speed gain. K-PIPE matters more than cell duration.

### Alternative 2: Self-hosted aarch64 runner

- **Pros**: native build, no cross-compile concerns, fastest possible build time.
- **Cons**:
  - **Operational burden**: the maintainer must keep a physical/cloud aarch64 machine online and registered as a GH Actions runner; security posture (a runner with write access to release artifacts) needs hardening; uptime monitoring becomes a thing.
  - **Single point of failure**: if the maintainer's RPi is down, no aarch64-linux release ships.
  - **Cost**: AWS Graviton is non-zero ongoing spend; project is OSS with no funding.
  - **Conway's Law**: this introduces operational responsibility outside the maintainer's stated comfort zone (intake-brief: "comfortable with git, GitHub Actions, and Homebrew").
- **Rejection rationale**: violates simplest-solution-first; introduces operational complexity disproportionate to the benefit.

### Alternative 3: GitHub `ubuntu-22.04-arm` beta runner

- **Pros**: native, no cross-compile, GitHub-managed (no operational burden).
- **Cons**:
  - **Beta status**: K-PIPE ≥95% target is incompatible with beta-runner reliability (observed availability dips and image-update incidents).
  - **Capacity limits**: beta runners have lower concurrency caps; release builds could queue.
  - **Reversibility**: trivially adoptable post-GA; the build matrix declaration is a one-line change.
- **Rejection rationale**: not GA-stable. **Revisit when GitHub announces general availability** (per OQ-3 in architecture-design).

## Consequences

### Positive

- **Reliability**: `cross` is the most-used cross-compile tool in Rust CI; well-tested across the ecosystem (10k+ GH stars; used by `actix-web`, `tokio`, `ripgrep`, etc.).
- **C-FFI compatibility**: `cross`'s Docker images include the full cross-toolchain (gcc, ar, pkg-config, common -dev libs) for the target.
- **glibc baseline control**: `cross`'s images target older glibc (default ~2.27), so the produced binary runs on Ubuntu 20.04+, RPi OS Bullseye+, and AWS Graviton AMIs. End-user reach is broader than a manual ubuntu-22.04 cross would give.
- **Reproducibility**: maintainer can `cross build --target aarch64-unknown-linux-gnu` locally and get bit-identical-ish output.
- **Pinning**: `--version 0.2.5 --locked` makes `cross` itself a reproducible build input.

### Negative

- **Cold-cache cost**: first run pulls the Docker image (~500 MB, ~1 min); cached for subsequent runs in the same workflow but not across workflows by default.
- **Docker dependency**: `ubuntu-22.04` has Docker pre-installed, so this is a non-issue on hosted runners. Self-hosted runners would need Docker; out of scope.
- **Indirection**: builds run inside a container, so debugging cross-compile failures (rare) requires dropping into the container with `cross --shell`.
- **Future obsolescence**: when GitHub's `ubuntu-22.04-arm` GA, this ADR will be superseded.

### Quality attribute impact

| Attribute | Impact |
|---|---|
| Reliability | **Positive** — mature, widely-used tool |
| Performance | **Slightly negative** — ~1 min cold-cache overhead vs manual setup |
| Maintainability | **Positive** — standard pattern; reproducible locally |
| Cross-platform reach | **Positive** — older glibc baseline broadens user base |
| Cost | **Neutral** — runs on hosted runners; no new spend |

### Verification

`brew test-bot` runs the install + test stanzas on `ubuntu-22.04` via QEMU user-space emulation for the aarch64 binary. This is not a perfect aarch64-native test but catches gross failures (binary crashes on launch, dynamic linker errors). The maintainer can supplement with periodic manual verification on a real aarch64 device (e.g., RPi 5, MacBook Air via Asahi Linux). Tracked in RELEASING.md release-log "platforms verified" column.

## References

- cross-rs/cross: <https://github.com/cross-rs/cross>
- DISCUSS `requirements.md` (NFR cross-platform table; risk register C-FFI row)
- DISCUSS `user-stories.md` US-07 (build matrix), US-15 (end-user install across platforms)
- DESIGN `architecture-design.md` §8.5 (cross-platform coverage), §11 OQ-3 (future native arm runner)
- DESIGN `technology-stack.md` §2
