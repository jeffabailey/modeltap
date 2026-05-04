# ADR-014: Formula templating — Tera in xtask, not inline shell

## Status

Proposed (2026-05-03 — DESIGN wave for `release-process-homebrew-github`).

## Context

The `bump-tap-formula` job must render `Formula/modeltap.rb` with:

- 1 version field (`version "0.2.0"`)
- 4 platform blocks (`on_macos.on_arm`, `on_macos.on_intel`, `on_linux.on_arm`, `on_linux.on_intel`), each with a `url` and `sha256` (US-10)
- A `test do` block that verifies `modeltap --version` output

This is non-trivial templating: 1 + 4 × 2 + 1 = ~10 substitution sites, with the sha256 values read from `.sha256` artifact files (US-06.AC-4 / US-10.AC-3). Walking-skeleton (WS) renders only 1 platform block; R1 renders all 4.

Three rendering approaches exist:

1. **Tera (Rust crate) called from `cargo xtask render-formula`**: a typed FormulaCtx struct passed to the template engine; Rust validates inputs before rendering.
2. **Inline workflow shell** (`sed`/`envsubst`/heredoc) in the bump-tap-formula job: the formula content is inlined in `release.yml` with `${VAR}` substitution.
3. **Minijinja or Handlebars (Rust crates)**: alternatives to Tera with similar capability.

Constraints:

- US-10.AC-3: sha256 read from artifact, NEVER recomputed. The renderer must enforce this.
- US-10.AC-4: missing `.sha256` artifact must fail the job with clear error.
- US-14: `release.yml` ≤250 lines. Inlining a 50-line Ruby template would consume 20% of the budget.
- Mutation testing target ≥80% kill rate (CLAUDE.md). Pure Rust functions are testable; shell substitutions are not.
- Maintainability for Riley (K-CONTRIB).

## Decision

**Implement formula rendering as `cargo xtask render-formula`, using the `tera` crate to render `release/templates/modeltap.rb.tera` (a checked-in template) given a `FormulaCtx` struct constructed from the `.sha256` artifact directory.**

Workflow snippet (illustrative):

```yaml
- name: Render formula
  run: |
    cargo run -p xtask --quiet -- render-formula \
      --version ${{ env.VERSION }} \
      --template release/templates/modeltap.rb.tera \
      --output tap-repo/Formula/modeltap.rb \
      --sha256-dir . \
      --release-base-url https://github.com/jeffabailey/modeltap/releases/download/v${{ env.VERSION }}
```

Template file (`release/templates/modeltap.rb.tera`) is checked into the source repo. Render output is written into the tap-repo checkout at `tap-repo/Formula/modeltap.rb`, ready to be committed and pushed.

The rendering function in xtask is pure:

```text
pub fn render(template_text: &str, ctx: &FormulaCtx) -> Result<String, FormulaError>;
```

It takes a string template and a typed context, returns a string. No I/O. The CLI wrapper handles file reads/writes around this pure function.

`FormulaError` distinguishes:
- `MissingSha256(target)` — `.sha256` file for the target was not found
- `InvalidSha256(target, content)` — content was not 64 lowercase hex chars
- `TemplateRenderError(...)` — Tera rendering failed (syntax, missing variable)

## Alternatives Considered

### Alternative 1: Inline shell (`sed` / `envsubst` / heredoc) in workflow YAML

```yaml
- name: Render formula
  run: |
    SHA_MAC_ARM=$(cat modeltap-${VERSION}-aarch64-apple-darwin.tar.gz.sha256)
    SHA_MAC_X86=$(cat modeltap-${VERSION}-x86_64-apple-darwin.tar.gz.sha256)
    SHA_LIN_ARM=$(cat modeltap-${VERSION}-aarch64-unknown-linux-gnu.tar.gz.sha256)
    SHA_LIN_X86=$(cat modeltap-${VERSION}-x86_64-unknown-linux-gnu.tar.gz.sha256)
    cat > tap-repo/Formula/modeltap.rb <<EOF
    class Modeltap < Formula
      version "${VERSION}"
      on_macos do
        on_arm do
          url "https://.../modeltap-${VERSION}-aarch64-apple-darwin.tar.gz"
          sha256 "${SHA_MAC_ARM}"
        end
        # ... 3 more blocks ...
      end
      # ...
    end
    EOF
```

- **Pros**: zero new code; everything visible in `release.yml`.
- **Cons**:
  - **Consumes ~50 lines of `release.yml`** — 20% of US-14's 250-line budget for a single substitution step.
  - **No validation of sha256 format**: a malformed `.sha256` file (e.g., contains GNU `sha256sum` two-field output `<hash>  <filename>`) is silently embedded into the formula, producing a broken `brew install` for users.
  - **No type checking on missing files**: `cat`-ing a missing file gives empty output; the rendered formula has `sha256 ""` which `brew test-bot audit` does flag, but only after the PR is opened (wasted CI cycles).
  - **Untestable in isolation**: bash substitution cannot be unit-tested; cannot be mutation-tested (zero kill rate).
  - **Variable proliferation**: 4 `SHA_*` variables × N future targets = combinatorial explosion.
  - **Hard to evolve**: adding a new platform block requires editing both the heredoc AND the variable declarations AND the URL templates — three edits in one shell snippet, easy to miss one.
- **Rejection rationale**: violates US-14 budget, violates US-10.AC-4 (missing-artifact handling), and is untestable. The exact failure modes the design must prevent are precisely the ones inline shell makes invisible.

### Alternative 2: Minijinja (Rust)

- **Pros**: smaller dep than Tera; very fast; Jinja2-compatible syntax.
- **Cons**:
  - **Less battle-tested in the Rust ecosystem**: Tera has 5k+ stars, used by Zola (static site generator) and many CLI tools.
  - **Marginally smaller dep tree than Tera**, but the difference is irrelevant for a build-time tool.
- **Rejection rationale**: Tera is the more conservative, more-widely-used choice. Minijinja is fine; not enough reason to prefer it.

### Alternative 3: Handlebars (Rust)

- **Pros**: well-known syntax; helper functions.
- **Cons**:
  - **Mustache-style syntax is more verbose** than Jinja for the conditional-block needs of WS-vs-R1 rendering.
  - **Helper functions are overkill** for our needs (simple substitution + one conditional).
- **Rejection rationale**: more powerful than needed.

### Alternative 4: Hand-roll string substitution in Rust (no template engine)

- **Pros**: zero deps; simplest possible.
- **Cons**:
  - **Conditional blocks (WS vs R1) become string concatenation**: brittle.
  - **Loss of template-as-data benefit**: the formula structure is no longer visible as a single readable file; it's split across Rust string literals.
  - **No syntax checking** on the template before runtime.
- **Rejection rationale**: Tera's overhead is trivial (single dep, well-licensed); the structural-clarity benefit of a template-file-on-disk is meaningful for K-CONTRIB.

## Consequences

### Positive

- **`release.yml` stays small**: render step is one shell line invoking xtask. Saves ~40 lines vs inline shell.
- **Template is its own checkable file**: `release/templates/modeltap.rb.tera` can be syntax-checked, version-controlled, and reviewed independently of workflow logic.
- **Type-safe rendering**: FormulaCtx is a struct; missing fields fail at compile time, missing `.sha256` files fail at render time with a typed error.
- **Sha256 validation enforced**: pure Rust validates 64-hex-char format before substitution, preventing the GNU `sha256sum` two-field gotcha.
- **Unit-testable**: `xtask::formula::render` is a pure function; trivially testable with fixture FormulaCtx values.
- **Mutation-testable**: matches the project's ≥80% kill-rate strategy.
- **Conditional blocks**: Tera `{% if targets.aarch64_apple_darwin %}` cleanly handles WS-vs-R1 difference without separate templates.

### Negative

- **One additional Rust dep (`tera 1.20`)**: adds ~10 transitive deps to `xtask`'s tree. Acceptable; xtask is build-time only.
- **Template syntax is one-more-thing-to-learn** for contributors. Mitigation: Tera uses Jinja2 syntax which most contributors recognize; the template is short (~50 lines).
- **Slight indirection**: someone debugging the rendered formula must also look at the template. Mitigated by clear file naming (`modeltap.rb.tera` → `Formula/modeltap.rb`).

### Quality attribute impact

| Attribute | Impact |
|---|---|
| Reliability | **Positive** — typed validation; missing inputs fail loudly |
| Maintainability | **Positive** — template visible as its own file; render logic unit-testable |
| Workflow size | **Positive** — saves ~40 lines vs inline shell |
| Build performance | **Neutral** — Tera adds <1 sec to xtask build time |
| Testability | **Positive** — pure function; mutation-testable |

## References

- DISCUSS `user-stories.md` US-06 (bump renders from template), US-10 (4 platform blocks; sha256 read not recomputed), US-14 (≤250 lines)
- DESIGN `data-models.md` §1 (FormulaCtx schema), §3 (rendered formula shape)
- DESIGN `component-boundaries.md` §2.1 (render-formula subcommand), §2.2 (interface)
- Tera: <https://github.com/Keats/tera> (MIT, 5k+ stars)
