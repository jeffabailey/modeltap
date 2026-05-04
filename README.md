# modeltap

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
