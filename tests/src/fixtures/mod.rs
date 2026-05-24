//! Per-feature fixture helpers reused across the acceptance suite.
//!
//! Each submodule owns one feature's fixture builders and seam helpers. Public
//! re-exports keep the call-sites at the test driver level short (e.g.
//! `modeltap_acceptance::fixtures::CacheVerifier::open(...)`).

pub mod cache_fixtures;
// tool-model-info-sqlite-cache step 04-02: recursive (path, size, mtime)
// directory snapshot used by the cache-opt-out acceptance suite to assert
// the "zero bytes written" invariant. See `dir_manifest.rs` for the API.
pub mod dir_manifest;
pub mod inspect_fixtures;
