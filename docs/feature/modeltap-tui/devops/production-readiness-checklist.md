# Production-Readiness Checklist — modeltap-tui v1.0.0

DELIVER must satisfy every item before tagging v1.0.0. Each checkbox requires evidence (link to PR, CI run, asciinema recording, or file path).

## 1. Build / CI Health (3 items)

- [ ] **CI green on macOS for last 7 days.** Evidence: GitHub Actions history showing 7 consecutive days of green `ci / test (macos-latest)`.
- [ ] **CI green on Linux for last 7 days.** Evidence: same for `ci / test (ubuntu-latest)`.
- [ ] **Architecture rule R1 passing on every PR for last 7 days.** Evidence: `ci / arch-rule` history.

## 2. Acceptance Criteria (2 items)

- [ ] **All US-01..US-20 + US-05b acceptance criteria met.** Evidence: link to acceptance test suite results in CI; spreadsheet or markdown table mapping each US to its passing test name.
- [ ] **HF + LM Studio linking spikes complete and outcomes documented in plugin code.** Evidence: code comments in `plugins/hf/src/lib.rs` and `plugins/lm-studio/src/lib.rs` documenting:
  - The exact mechanism used (e.g., "HF: replace blob in `~/.cache/huggingface/hub/blobs/<hash>` with hardlink; snapshot symlinks already point at blob; verified `huggingface-cli scan-cache` and `transformers.AutoModel.from_pretrained` both load OK")
  - Any limitations discovered (e.g., "LM Studio caches model handles; user must close LM Studio before unify; we detect via lsof and prompt")
  - Date the spike was performed and against which tool versions

## 3. Performance (1 item)

- [ ] **K3 benchmark passing in CI.** Evidence: `ci / k3-bench` job consistently green; `first_paint_ms` median over last 7 days < 1000 ms; `full_inventory_ms` < 5000 ms. Trend uploaded as artifact.

## 4. Release Tooling (3 items)

- [ ] **`cargo dist plan` produces expected artifact set.** Evidence: output of `cargo dist plan` showing 4 target tarballs + checksums + Homebrew formula.
- [ ] **Release dry-run completed successfully.** Evidence: a pre-1.0 tag (e.g., `v1.0.0-rc.1`) was pushed and the release workflow completed end-to-end including Homebrew tap update.
- [ ] **Cargo-dist generates binaries for all 4 targets without warnings.** Evidence: release workflow log showing 4 successful builds (x86_64-darwin, aarch64-darwin, x86_64-linux-gnu, aarch64-linux-gnu).

## 5. Distribution (2 items)

- [ ] **Homebrew tap repository created and formula publishable.** Evidence: `<org>/homebrew-modeltap` repo exists; pre-1.0 tap installation tested: `brew tap <org>/modeltap && brew install modeltap` works on a clean macOS.
- [ ] **Crates.io ownership verified.** Evidence: `cargo owner --list modeltap` shows the maintainer; (optional) `modeltap` name reserved with a 0.0.0 placeholder release if v1.0.0 isn't ready to publish yet.

## 6. Documentation (1 item, covering 4 documents)

- [ ] **CONTRIBUTING.md, SECURITY.md, INSTALL.md, RELEASE.md all present and accurate.** Evidence: files exist in repo root; each contains the sections specified below.
  - `CONTRIBUTING.md` must include: dev environment setup; how to run CI locally (`lefthook install`, `cargo fmt/clippy/test/deny`); how to add a plugin (link to plugin trait docs, worked example referencing `plugins/ollama/`); how to file a bug; CODEOWNERS expectations.
  - `SECURITY.md` must include: vulnerability reporting email or GitHub Security Advisories link; supported versions table; `cargo-audit` schedule; expected response time.
  - `INSTALL.md` must include: per-platform install commands (Homebrew, cargo install, manual download); macOS Gatekeeper workaround; how to verify checksums; uninstall instructions.
  - `RELEASE.md` must include: the 9-step release flow from `release-strategy.md` §8; hotfix flow; SemVer contract reminder.

## 7. License + Security (2 items)

- [ ] **`cargo deny check` passes.** No GPL deps. All transitive licenses are MIT / Apache-2.0 / BSD / ISC / Zlib / Unicode-DFS-2016 / MPL-2.0 / CC0-1.0. No outstanding RustSec advisories. Evidence: latest `ci / cargo-deny` run green.
- [ ] **Project license file present (`LICENSE`).** Evidence: `LICENSE` file at repo root; MIT recommended (see platform-design.md §13). Mentioned in `Cargo.toml` `[package] license = "MIT"`.

## 8. Manual End-to-End Validation (2 items)

- [ ] **At least one plugin (Ollama) round-trip tested manually end-to-end on macOS.** Evidence: asciinema recording showing: launch modeltap → see real Ollama models → zap one → confirm bytes reclaimed match `du -sh ~/.ollama/models/blobs/`. Recording attached to release notes.
- [ ] **Same end-to-end test on Linux.** Evidence: separate asciinema recording on Ubuntu 22.04 LTS or later.

## 9. Release Tag Gates (final pre-tag checks)

These are checked manually by the maintainer immediately before pushing the release tag:

- [ ] CHANGELOG.md has a complete `[1.0.0]` section
- [ ] No open issues with the `release-blocker` label
- [ ] No open PRs targeting `main` that should be in this release
- [ ] `cargo dist plan` runs cleanly on the release commit
- [ ] All boxes in sections 1-8 above are checked

## Item Count Summary

- Section 1 (CI health): 3
- Section 2 (acceptance): 2
- Section 3 (performance): 1
- Section 4 (release tooling): 3
- Section 5 (distribution): 2
- Section 6 (docs): 1 (covering 4 files)
- Section 7 (license/security): 2
- Section 8 (manual E2E): 2
- Section 9 (release-tag gates): 5 (procedural, not pre-staged work)

**Total work items DELIVER must complete: 16** (sections 1-8). Section 9 is a checklist for the moment of release, not separate work.

## DELIVER Responsibility Allocation

DELIVER (software-crafter) owns:
- All sections 1-8 evidence collection
- Authoring the four docs (CONTRIBUTING.md, SECURITY.md, INSTALL.md, RELEASE.md) using the templates and content sketched in `release-strategy.md`, `ci-pipeline.md`, and `platform-design.md`
- Running the HF + LM Studio spikes and updating plugin code with comments
- Producing the asciinema recordings on both macOS and Linux

DEVOPS (Apex) provides:
- The CI workflow files
- The architecture-rule test code (`tests/architecture.rs`)
- The K3 benchmark structure and threshold logic
- The cargo-dist configuration
- The `deny.toml` policy
- The lefthook configuration
- The launch.log JSONL schema and rotation policy
- This checklist
