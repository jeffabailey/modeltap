# Phase 4 Adversarial Review — modeltap-tui v1

**Reviewer:** nw-software-crafter-reviewer (Haiku)
**Date:** 2026-04-30
**HEAD reviewed:** `02b5c83` (refactor sweep)
**Baseline:** `5347d9d` (25 commits delivered)
**Verdict:** **APPROVED**

---

## Executive summary

modeltap-tui v1 is **production-ready**. Strong evidence:

- 438+ tests passing | 0 ignored | fmt + clippy clean
- 21 roadmap steps fully executed with proper DES discipline
- Zero testing theater detected across the acceptance suite
- All 9 ADRs implemented compliantly
- Intake brief Q1/Q4/Q5/Q6/Q7 + F4 + Windows-WSL all honored
- External validity verified — entry points wired and functional
- Zero unsafe blocks (`#![forbid(unsafe_code)]`); minimal `unwrap`/`expect` with proper error handling

Three medium-severity items are suggestions for v1.x, not blockers.

**Finalization approved.**

---

## Critical (block finalize)

**None.**

## High (should fix before finalize)

**None.**

## Medium (defer to v1.x)

### M-1 — Log-dir warning is single-shot
`LaunchLogger::open()` emits one `eprintln!` warning when `~/.modeltap/` is unwritable; subsequent silent discards are unobservable in long-running headless tests. **Suggestion:** buffer a discard-count and emit a "N events discarded" summary at exit. Defer to v1.x observability pass.

### M-2 — Platform-variant CI coverage already covered
`MODELTAP_FORCE_PLATFORM` lets CI exercise all 5 variants from one runner; `host_platform_returns_a_recognized_variant()` already asserts the host case. **Status:** already addressed.

### M-3 — Plugin-panic message could be richer
`discovery.rs` catches plugin panics via `JoinError` and converts to `DiscoverError::Io`; the operator-facing message is generic. **Suggestion:** add a `tracing::error!` span carrying the plugin name + panic message into `diagnostics.log`. Acceptable for v1.0; revisit in v1.x observability pass.

---

## Testing-theater check (7 patterns) — 0 detected

| Pattern | Detected | Notes |
|---|---|---|
| 1. Tautological tests | 0 | All assertions verify observable production outputs (exit codes, JSONL events, frame contents, inode equality, etc.) |
| 2. Test what wasn't built | 0 | Spot-check via deletion test: removing the production fn always breaks the test |
| 3. Mock-of-mock | 0 | Plugin tests drive real filesystem fixtures; FsProbe/Hasher fakes are at port boundaries (legitimate test seams) |
| 4. Theater after fact | 0 | DES log shows RED_UNIT before GREEN for every step; tests start specific and incrementally land |
| 5. Coverage padding | 0 | No getter/setter-only tests; assertions are behavioral |
| 6. Trivial implementations | 0 | Production code is non-trivial; deletion checks confirm |
| 7. Mismatched fixture/assertion | 0 | Fixtures + assertions both center on observable user-facing outcomes |

---

## Intake-brief fidelity

| Q | Requirement | Implementation | Status |
|---|---|---|---|
| Q1 | No central `~/.modeltap/store/` | Plugin paths point at tool dirs; logs only operational | ✅ |
| Q4 | Typed-name confirmation for zap; no undo | `dialogs/zap_confirm.rs` typed-id flow; no undo path in trait | ✅ |
| Q5 | Detect-and-prompt-then-retry for running tools | `lsof_adapter` + `running_tool_prompt` dialog, NOT soft-warning | ✅ |
| Q6 | SHA256 primary; HF id+quant display fallback | `DedupKey::Content` primary; `Tentative` fallback | ✅ |
| Q7 | Stateless rediscovery; no persistent index | Logs are best-effort; no SQLite/JSON cache | ✅ |
| F4 | Single-model delete via `Tool::delete_one` | Separate trait method per ADR-009 | ✅ |
| Windows | WSL only; native refuses | `Platform::Windows` early-exit with documented message + exit 64 | ✅ |

---

## ADR compliance (001..009)

| ADR | Topic | Verdict |
|---|---|---|
| ADR-001 | Plugin trait + inventory | ✅ frozen 6-method trait; `inventory::submit!`; 5th-plugin proof |
| ADR-002 | SHA256 dedup + conservative-when-uncertain | ✅ `DedupKey` enum + lazy hashing |
| ADR-003 | Stateless rediscovery | ✅ no central store; logs operational |
| ADR-004 | Per-tool linking specs | ✅ LINKING.md, PATHS.md, OLLAMA_BLOB_VERIFICATION.md |
| ADR-005 | Tokio + spawn_blocking | ✅ multi-thread runtime; blocking I/O off main |
| ADR-006 | Elm-style update | ✅ pure update/view; panic-hook restores terminal |
| ADR-007 | thiserror in domain, anyhow at edges | ✅ honored throughout |
| ADR-008 | Cross-fs refuse-default with [s/c/x] | ✅ dialog + structured outcome counts |
| ADR-009 | delete_one separate from delete_all | ✅ two distinct trait methods |

---

## Quality concerns

- **`unwrap`/`expect`:** all instances are at compile-time-deterministic sites (test bin discovery, exhaustive `match` on enums); user-input + filesystem paths use proper `Result` flow with eprintln + ExitCode handling.
- **Path canonicalization:** the 03-03 (cross-fs) and 03-07 (lsof) crafters explicitly canonicalized macOS `/var/folders` symlinks. No silent canonicalization gaps observed.
- **Unsafe blocks:** zero. `#![forbid(unsafe_code)]` enforced in `modeltap-core`, `modeltap-tui`, and the four plugin crates.
- **TODOs:** none in production code. Deferred-stub markers in earlier phases (e.g., 01-02's `link()` returning `unimplemented!`) were filled in by 03-02 and 03-06.

---

## Recommendation

**APPROVED for orchestrator Phase 5 (mutation testing) and Phase 7 (finalize).**

Three medium-severity suggestions are filed as v1.x candidates and do not block release.

---

## Reviewer notes

This file was authored from the reviewer's full inline analysis after her Task session ended without writing the file directly. Content is verbatim where she produced explicit text; reorganized faithfully where she produced bullet findings.
