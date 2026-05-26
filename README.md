# modeltap
[![FOSSA Status](https://app.fossa.com/api/projects/git%2Bgithub.com%2Fjeffabailey%2Fmodeltap.svg?type=shield)](https://app.fossa.com/projects/git%2Bgithub.com%2Fjeffabailey%2Fmodeltap?ref=badge_shield)


Terminal UI for discovering, inspecting, and cleaning up local AI model files
across multiple tools (Ollama, Hugging Face cache, LM Studio, Atomic Chat).

## Supported platforms (v1)

| Platform | Status |
|---|---|
| macOS x86_64 (Intel) | Supported |
| macOS aarch64 (Apple Silicon) | Supported |
| Linux x86_64 | Supported |
| Linux aarch64 | Supported |
| Windows via [WSL2](https://learn.microsoft.com/windows/wsl/install) | Supported (treated as Linux) |
| Windows native (`x86_64-pc-windows-*`) | **Not supported** — refuses with exit code 64 |

WSL is treated identically to native Linux. There is no WSL-specific code path
because, from `modeltap`'s perspective, the WSL filesystem is a Linux
filesystem.

If you run the native Windows binary, `modeltap` exits 64 and prints:

```
Windows is supported only via WSL — see https://learn.microsoft.com/windows/wsl/install
```

## Installing

```sh
cargo install --path crates/modeltap-app
```

(Windows users: install WSL2 first, then run the above inside your WSL shell.)

## Inventory cache

On every launch, `modeltap` paints from a local SQLite cache of the
inventory discovered on the previous launch, then runs a background reconcile
to refresh it. Warm-paint completes in well under 150 ms even when the live
tool directories take seconds to walk.

### Where the cache lives

| OS | Default path | Override |
|---|---|---|
| macOS | `~/Library/Application Support/modeltap/cache.sqlite` | `MODELTAP_CACHE_PATH=/path/to/cache.sqlite` |
| Linux / WSL | `$XDG_DATA_HOME/modeltap/cache.sqlite` (or `~/.local/share/modeltap/cache.sqlite` when `XDG_DATA_HOME` is unset) | `MODELTAP_CACHE_PATH=/path/to/cache.sqlite` |

The path resolver and PRAGMA invariants are documented in `lat.md/warm-start.md`
and `lat.md/modeltap-store.md`.

### What's stored

Four tables — `cache_meta`, `cache_tools`, `cache_models`, `cache_model_files` —
hold the per-tool last scan timestamp, every discovered model row, and one
row per on-disk file (mtime, size, inode, device) so drift can be detected
without rehashing. SHA-256 dedup keys are present but sparse (computed lazily;
persistence across runs is deferred to a future release).

The cache is **never authoritative for destructive actions**. Before any
unify / zap / delete operation, a pre-mutate revalidator re-stats the
target files and either proceeds, re-introspects on drift, or refuses on
"file gone." This is the K5 invariant — see ADR-015 and the R9
architecture lint in `crates/modeltap-app/tests/architecture.rs`.

### Refreshing the cache

| Key | Scope |
|---|---|
| `r` | Reconcile the selected tool |
| `Shift+R` | Reconcile every tool |

The summary bar shows provenance (`as of 2 min ago`) with a `refreshing
<tool>…` or `reconciling…` suffix while a reconcile is in flight.

### Opting out

Two ways to disable the cache for a single launch or permanently:

```sh
modeltap --no-cache                    # one-shot bypass; never reads or writes the cache
```

```toml
# ~/.modeltap/config.toml
[cache]
enabled = false                        # permanent opt-out; same behavior as --no-cache
```

`--no-cache` overrides the config file. With the cache disabled, every
launch walks every tool directory live — the v0.2.x stateless behavior.

### Tuning per-tool freshness

Rows older than the TTL window are reconciled on launch; rows inside the
window paint instantly from cache and reconcile in the background.

```toml
# ~/.modeltap/config.toml
[cache]
tool_ttl_seconds = 86400               # default: 24 h
```

Shorten this for rapidly changing tool inventories; lengthen it if your
tools rarely change and you want maximum warm-paint hit rate.

### Inspecting the cache by hand

The cache is a plain SQLite file in WAL mode. The `sqlite3` CLI can open
it read-only without contending with a running `modeltap` process:

```sh
# macOS
sqlite3 -readonly "$HOME/Library/Application Support/modeltap/cache.sqlite" \
  "SELECT tool_id, last_scan_at FROM cache_tools;"

# Linux / WSL
sqlite3 -readonly "${XDG_DATA_HOME:-$HOME/.local/share}/modeltap/cache.sqlite" \
  "SELECT tool_id, last_scan_at FROM cache_tools;"
```

If you ever need to nuke the cache to force a clean rediscovery, deleting
the file is safe — `modeltap` recreates it on next launch (and falls back
to a cold-start scan if it can't).

### When the cache is corrupted or version-skewed

If `cache.sqlite` is unreadable, on a schema version newer than the binary
understands, or migration fails, the TUI shows a recovery banner and falls
back to cold-start. No data is lost — the source of truth is always each
tool's own directory.

## Continuous integration

The CI matrix (`.github/workflows/ci.yml`) runs `cargo build` + `cargo test`
on `ubuntu-latest` and `macos-latest`. Both must pass before any PR can merge.
The K3 first-paint smoke benchmark (`k3-bench` job) runs on `ubuntu-latest`
only — performance budgets are validated against a single canonical runner
to keep the signal stable.

For per-OS testing in a single CI job, set `MODELTAP_FORCE_PLATFORM` to one of:

- `macos-x86_64`
- `macos-aarch64`
- `linux-x86_64`
- `linux-aarch64`
- `windows-x86_64`

The override takes precedence over the host's compiled-in target triple. An
unrecognized value falls back to the host platform — a typo cannot brick the
binary in production.

## Installation troubleshooting

### macOS Gatekeeper warning on first run

The first time you run `modeltap` on macOS 13 or 14, you may see a warning:

> "modeltap" cannot be opened because Apple cannot check it for malicious software.

This is because we do not currently sign or notarize the macOS binaries
(tracked as a future enhancement). To proceed:

```sh
xattr -d com.apple.quarantine $(brew --prefix)/bin/modeltap
modeltap --version
```

After this one-time step, `modeltap` runs without further prompts.

### Verifying the binary's build provenance (optional)

Every release archive carries a SLSA Level 3 build provenance attestation,
proving the binary was built by the official GitHub Actions workflow on
`jeffabailey/modeltap`. To verify:

```sh
# Download the archive matching your platform
gh release download v0.2.0 --pattern 'modeltap-*-aarch64-apple-darwin.tar.gz'

# Verify the attestation
gh attestation verify modeltap-0.2.0-aarch64-apple-darwin.tar.gz --owner jeffabailey
```

A successful verification confirms:

- The archive was built by `.github/workflows/release.yml` in `jeffabailey/modeltap`
- The build was triggered by a tag push (not a manual workflow_dispatch)
- The archive's SHA-256 matches what was attested at build time

### Linux: aarch64 binaries are cross-compiled

Linux ARM64 binaries (used by Raspberry Pi 4/5, Apple Silicon Macs in Linux
VMs, ARM-based EC2 instances) are cross-compiled on x86_64 build runners using
`cross`. They run on native aarch64 Linux as well as via QEMU on x86_64 hosts.
WSL2 on Windows uses the Linux x86_64 binary (WSL2 transparently provides
x86_64 compatibility regardless of the host CPU).

### "I just released, but `brew install` still resolves to the previous version"

There is a short window (typically under two minutes) between the GitHub
Release being published and the Homebrew tap formula being updated. If
`brew install jeffabailey/modeltap/modeltap` still installs the previous
version more than a few minutes after a release was announced, run:

```sh
brew update
brew install jeffabailey/modeltap/modeltap
```

If it persists for more than ten minutes, the tap-bump step in the release
pipeline may have failed (see "If you're a maintainer" below).

### Where do I report install problems?

File an issue at <https://github.com/jeffabailey/modeltap/issues> with:

- Your OS and architecture (`uname -s -m`)
- Your Homebrew version (`brew --version`)
- The exact command you ran
- The full error output

### If you're a maintainer and you see a release-pipeline alert

The release pipeline opens a `release-pipeline-failure` issue when any
release workflow finishes with a non-success conclusion. Common root causes
and the standard fix:

| Symptom | Typical fix |
|---|---|
| `validate-tag` failed within ~30 seconds | Re-tag with the version in `Cargo.toml`; delete the bad tag |
| `cargo fmt` / `cargo clippy` / `cargo test` failed in `build` | Fix forward in source; cut a new patch tag |
| `cargo deny check` failed (license/advisory regression) | Update `deny.toml` or rotate the offending dependency |
| Network blip during `cross` Docker pull on aarch64-linux | Re-run the failed cell from the GH Actions UI |
| `gh release create` failed | GitHub Releases API outage (rare); re-run |
| `bump-tap-formula` got HTTP 401 | Rotate `GH_TAP_TOKEN` (see `RELEASING.md` "operational notes"), then re-run only the `bump-tap-formula` job |
| `brew test-bot` red-flagged the tap PR | Investigate the platform-specific failure; auto-merge will not fire until it passes |

The triage checklist is captured in the issue body itself — fill it in so
the quarterly K-PIPE review can spot recurring failure modes.

### If you're a maintainer and `GH_TAP_TOKEN` has expired

Symptoms: a `tap-token-expiry` issue is opened by the
`token-expiry-warning.yml` workflow, AND any in-flight release fails at the
`bump-tap-formula` step with HTTP 401. Recovery (full procedure in
`RELEASING.md`):

1. Generate a replacement fine-grained PAT at
   <https://github.com/settings/personal-access-tokens> (repository:
   `jeffabailey/homebrew-modeltap`; permissions: Contents R/W, Pull requests
   R/W, Metadata R; expiration: 365 days).
2. Update `GH_TAP_TOKEN` under
   `Settings → Secrets and variables → Actions → Repository secrets` on
   `jeffabailey/modeltap`.
3. Re-run the failed `bump-tap-formula` job from the GH Actions UI. The job
   is idempotent (force-with-lease push + `gh pr list` check before
   `gh pr create`) — re-running after the token rotation produces the same
   end state with no duplicate PR.
4. Close the `tap-token-expiry` issue with a comment recording the rotation
   date.

### If you're a maintainer and you need to yank a release

If a critical defect ships in a release, do NOT edit a published release.
Instead:

1. Mark the buggy release as draft so end users stop seeing it:
   `gh release edit v${VERSION} --draft --repo jeffabailey/modeltap`
2. Revert the tap-bump PR in the tap repo so `brew install` resolves to the
   prior version:
   open a PR against `jeffabailey/homebrew-modeltap` reverting
   `Formula/modeltap.rb` to the previous version's content; merge after
   `brew test-bot` passes.
3. Annotate the `## [<version>]` section in `CHANGELOG.md` with `(yanked)`.
4. Cut a new patch release with the fix following the standard
   `RELEASING.md` procedure.

## License

Apache-2.0 OR MIT (per workspace `Cargo.toml`).


[![FOSSA Status](https://app.fossa.com/api/projects/git%2Bgithub.com%2Fjeffabailey%2Fmodeltap.svg?type=large)](https://app.fossa.com/projects/git%2Bgithub.com%2Fjeffabailey%2Fmodeltap?ref=badge_large)	