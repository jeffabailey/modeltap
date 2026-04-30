//! modeltap-app library surface.
//!
//! Most of `modeltap-app` is the binary `main()` (in `main.rs`); a small
//! library surface is exposed here so integration tests can call refresh /
//! action helpers directly. The bin re-uses these via `crate::*` paths.
//!
//! Library-only modules (no plugin linkage, no JSONL writer) — kept thin so
//! pulling them into a test binary does not drag the whole composition
//! root.

#![forbid(unsafe_code)]

pub mod inventory_build;
pub mod lsof_adapter;
pub mod refresh;
pub mod sha256_cache;
