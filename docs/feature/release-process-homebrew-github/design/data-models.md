# Data Models — release-process-homebrew-github

**Wave:** DESIGN (3 of 6)
**Date:** 2026-05-03

There is no persistent database in this pipeline. The "data models" are:

1. The Tera template rendering context (`FormulaCtx`) — the schema that bridges build outputs to formula content.
2. The shared-artifact flow — every `${variable}` in the journey, traced from producer to every consumer (the integration manifest).
3. The Homebrew formula itself (`Formula/modeltap.rb`) — the rendered output schema.
4. The release archive naming + sha256 sidecar — the build-output shape.

## 1. FormulaCtx Schema (Tera template rendering context)

The `cargo xtask render-formula` subcommand constructs a `FormulaCtx` and passes it to Tera. This is the single source of structure for the rendered formula.

```text
FormulaCtx {
    version: Version,                    // "0.2.0" (semver, no v prefix)
    release_base_url: String,            // "https://github.com/jeffabailey/modeltap/releases/download/v0.2.0"
    targets: Vec<TargetEntry>,           // exactly 4 entries in R1; 1 in WS
}

TargetEntry {
    triple: String,                      // "aarch64-apple-darwin"
    homebrew_block: HomebrewPlatform,    // ARM_MACOS | INTEL_MACOS | ARM_LINUX | INTEL_LINUX
    archive_name: String,                // "modeltap-0.2.0-aarch64-apple-darwin.tar.gz"
    sha256: String,                      // 64-hex-char string read from .sha256 artifact
}

HomebrewPlatform = ARM_MACOS | INTEL_MACOS | ARM_LINUX | INTEL_LINUX
                                         // Mapped to on_macos.on_arm / on_macos.on_intel /
                                         // on_linux.on_arm / on_linux.on_intel blocks.
```

### Mapping table (Rust target triple → Homebrew block)

| Rust target | HomebrewPlatform | Tera template path |
|---|---|---|
| `aarch64-apple-darwin` | ARM_MACOS | `on_macos.on_arm` |
| `x86_64-apple-darwin` | INTEL_MACOS | `on_macos.on_intel` |
| `aarch64-unknown-linux-gnu` | ARM_LINUX | `on_linux.on_arm` |
| `x86_64-unknown-linux-gnu` | INTEL_LINUX | `on_linux.on_intel` |

### Construction rules

- `version`: read from the validated tag (strip leading `v`). Validated equal to `Cargo.toml [workspace.package].version` per US-02.
- `release_base_url`: literal template `https://github.com/jeffabailey/modeltap/releases/download/v{version}`. Do NOT hardcode in template; pass via context so it's testable and future-changeable (org migration).
- `targets`: enumerate the artifacts directory (`*.sha256` files). The xtask refuses to render if any of the 4 expected targets is missing (US-10.AC-4). For walking-skeleton (US-04 / WS slice), only 1 target is expected and the unused platform blocks in the template are conditionally omitted via Tera `{% if %}`.
- `sha256`: read verbatim from the `.sha256` file. The xtask asserts it's exactly 64 lowercase hex characters; otherwise rejects with `error: invalid sha256 in <file>`.

### Integration invariants enforced at construction

- `archive_name == "modeltap-{version}-{triple}.tar.gz"` (US-04.AC-3) — built by formatting; never hand-typed.
- `sha256` length == 64; matches `[a-f0-9]{64}` — validated before render.
- `targets.len()` == 4 (R1) or 1 (WS) — validated before render. Mixed states (2 or 3 targets) are an error.

## 2. Shared-Artifact Flow Diagram

This diagram traces every shared `${variable}` from its single source of truth (per `discuss/shared-artifacts-registry.md`) through its consumers in the pipeline. It is the integration manifest for the release.

```mermaid
flowchart LR
  subgraph SOURCES["Single sources of truth"]
    CARGO[Cargo.toml<br/>workspace.package.version]
    CONV[Conventional commit log<br/>since previous tag]
    MATRIX[release.yml matrix.target<br/>4 entries]
    BUILD_ENV[release.yml build job env<br/>VERSION + TARGET]
    SHASH[sha256sum output<br/>per build cell]
    TEMPLATE[modeltap.rb.tera<br/>in source repo]
    TAP_NAME[jeffabailey/homebrew-modeltap<br/>repo name itself]
    SECRETS[GH_TAP_TOKEN<br/>repository secret]
    ATTEST[actions/attest-build-provenance@v2<br/>output]
  end

  subgraph PIPELINE["Pipeline activities"]
    PREP[xtask release-prep]
    TAG[git tag -a v + version]
    VAL[validate-tag job]
    GATES[CI parity gates]
    BLD[cargo build --release per target]
    PKG[strip + tar.gz + sha256sum]
    REL[gh release create]
    REND[xtask render-formula]
    BUMP[bump-tap-formula job]
    AUTOM[gh pr merge --auto]
    TESTBOT[brew test-bot in tap repo]
  end

  subgraph CONSUMERS["End-state consumers"]
    GHREL[GitHub Release page]
    FORMULA[Formula/modeltap.rb in tap repo]
    USERINSTALL[brew install on user machine]
    BINARY[modeltap --version output]
    NOTES[GitHub Release notes]
    CHANGELOG[CHANGELOG.md in repo]
  end

  CARGO -->|read| PREP
  CARGO -->|read| VAL
  CARGO -->|compile-time CARGO_PKG_VERSION| BINARY
  PREP -->|writes| CARGO
  PREP -->|writes| CHANGELOG

  CONV -->|git-cliff| PREP
  PREP -->|appends section| CHANGELOG

  CARGO -->|maintainer derives v + version| TAG
  TAG -->|github.ref_name| VAL
  VAL -->|asserts equality| CARGO

  MATRIX -->|fan-out| BLD
  BUILD_ENV -->|format| PKG
  BLD --> PKG
  PKG -->|writes| SHASH
  SHASH -->|.sha256 sidecar| REND
  ATTEST -->|signs| PKG

  PKG -->|upload-artifact| REL
  CHANGELOG -->|extract section| NOTES
  REL -->|publishes| GHREL
  REL -->|attaches| NOTES

  TEMPLATE -->|render with FormulaCtx| REND
  REND -->|writes| FORMULA
  TAP_NAME -->|target repo| BUMP
  SECRETS -->|auth| BUMP
  BUMP -->|push branch + open PR| FORMULA
  BUMP --> AUTOM
  AUTOM -->|gated by| TESTBOT
  TESTBOT -->|reads url+sha256| FORMULA
  TESTBOT -->|downloads archive| GHREL

  FORMULA -->|brew tap + install| USERINSTALL
  USERINSTALL -->|extracts + runs| BINARY
  GHREL -->|brew downloads| USERINSTALL
```

### Integration invariants represented in this graph

| Invariant | Source(s) | Consumer(s) | Where enforced |
|---|---|---|---|
| `${tag}` == `"v" + ${version}` | CARGO, TAG | VAL | `validate-tag` job |
| `${sha256[T]}` (artifact) == `${sha256[T]}` (formula) for each T | SHASH | REND, TESTBOT | xtask reads .sha256 verbatim; brew test-bot install verifies |
| `${release-url[T]}` (release page) == `${url[T]}` (formula) for each T | release_base_url + archive_name | GHREL, FORMULA | both derived from same xtask context |
| `${version}` (Cargo.toml) == `modeltap --version` stdout | CARGO | BINARY | clap `CARGO_PKG_VERSION` at compile time |
| publish runs ONLY if all 4 build cells succeeded | needs: DAG | REL | release.yml structure (US-08) |
| bump runs ONLY if publish succeeded | needs: DAG | BUMP | release.yml structure (US-08) |
| CI parity gates run BEFORE cargo build | step order | GATES → BLD | release.yml step ordering (US-03) |

## 3. Rendered Formula Shape (`Formula/modeltap.rb`)

The exact output schema produced by the Tera render. This is what end users' `brew` reads.

```ruby
class Modeltap < Formula
  desc "TUI for managing local AI models across Ollama, HF, LM Studio, Atomic Chat"
  homepage "https://github.com/jeffabailey/modeltap"
  version "{{ version }}"
  license "MIT OR Apache-2.0"

  on_macos do
    on_arm do
      url "{{ release_base_url }}/modeltap-{{ version }}-aarch64-apple-darwin.tar.gz"
      sha256 "{{ targets.aarch64_apple_darwin.sha256 }}"
    end
    on_intel do
      url "{{ release_base_url }}/modeltap-{{ version }}-x86_64-apple-darwin.tar.gz"
      sha256 "{{ targets.x86_64_apple_darwin.sha256 }}"
    end
  end

  on_linux do
    on_arm do
      url "{{ release_base_url }}/modeltap-{{ version }}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "{{ targets.aarch64_unknown_linux_gnu.sha256 }}"
    end
    on_intel do
      url "{{ release_base_url }}/modeltap-{{ version }}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "{{ targets.x86_64_unknown_linux_gnu.sha256 }}"
    end
  end

  def install
    bin.install "modeltap"
  end

  test do
    assert_match "modeltap #{version}", shell_output("#{bin}/modeltap --version")
  end
end
```

### Walking-skeleton variant (1 platform)

For US-04 / WS slice, the template uses Tera conditional blocks:

```text
{% if targets.x86_64_unknown_linux_gnu %}
  on_linux do
    on_intel do
      url "..."
      sha256 "..."
    end
  end
{% endif %}
```

A formula with only one platform block is valid Homebrew DSL; `brew install` on a non-supported platform fails informatively with "formula not available for this platform".

## 4. Release Archive Naming + sha256 Sidecar

### Archive naming

```
modeltap-{version}-{target}.tar.gz
```

Examples (post-WS):
- `modeltap-0.2.0-aarch64-apple-darwin.tar.gz`
- `modeltap-0.2.0-x86_64-apple-darwin.tar.gz`
- `modeltap-0.2.0-aarch64-unknown-linux-gnu.tar.gz`
- `modeltap-0.2.0-x86_64-unknown-linux-gnu.tar.gz`

WS example: `modeltap-0.0.1-rc1-x86_64-unknown-linux-gnu.tar.gz` (pre-release suffix preserved per US-04.AC-7).

### Archive contents

Single file at archive root: `modeltap` (stripped binary). No nested directory (US-04.AC-4). Verified by `tar -tzf <archive>` returning exactly one line.

### sha256 sidecar

Filename: `<archive>.sha256` (literal append).

Content: a single line containing the lowercase hex sha256 of the archive (64 chars), no filename, no newline beyond LF.

Example content:
```
e5f6789012abcdef0123456789abcdef0123456789abcdef0123456789abcdef
```

Format choice: bare hex (NOT `sha256sum` GNU format which includes filename). The xtask `render-formula` reads with `read_to_string` + `trim()`, expecting the bare format. Documented to avoid the `e5f6...  modeltap-0.2.0-...tar.gz` two-field gotcha.

### SLSA attestation

Filename: not stored as a file in the workflow workspace; uploaded as a sidecar by `actions/attest-build-provenance@v2` via the GitHub attestations API. End users verify with `gh attestation verify <archive> --owner jeffabailey` which queries GitHub's attestation store. Documented in README troubleshooting (US-09.AC-5).

## 5. Cross-Reference to `shared-artifacts-registry.md`

This document operationalizes every artifact in `discuss/shared-artifacts-registry.md`:

| Registry artifact | Where modeled in this document |
|---|---|
| `version` | §1 FormulaCtx.version; §2 CARGO source |
| `tag` | §2 TAG, validated by VAL |
| `changelog` | §2 CHANGELOG; extracted into NOTES by extract-changelog xtask subcommand |
| `binary-targets[]` | §1 FormulaCtx.targets; §2 MATRIX |
| `archive-name` | §1 TargetEntry.archive_name; §4 |
| `sha256[target]` | §1 TargetEntry.sha256; §4 sidecar; §2 SHASH → REND data flow |
| `release-url[target]` | §1 release_base_url + archive_name composition; §3 formula `url` field |
| `tap-name` | §2 TAP_NAME; ADR-013 |
| `formula-content` | §3 (the full rendered shape) |
| `install-command` | derived in README; not an internal data model |
| `release-runbook` | RELEASING.md per `component-boundaries.md` §7; not a data model |
| `tap-bump-token` | §2 SECRETS; ADR-013 |
| `slsa-attestation` | §4 (GitHub attestations API, not a file artifact) |
| `ci-parity-gates` | §2 GATES; release.yml step ordering enforces |
| `cli-vocabulary` | covered in `discuss/journey-tag-to-brew-install-visual.md`; not a data model |

All 14 registry artifacts have a single source of truth in this design. No artifact is consumed without a documented producer.
