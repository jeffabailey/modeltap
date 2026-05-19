//! Per-feature fixture helpers reused across the acceptance suite.
//!
//! Each submodule owns one feature's fixture builders and seam helpers. Public
//! re-exports keep the call-sites at the test driver level short (e.g.
//! `modeltap_acceptance::fixtures::CacheVerifier::open(...)`).

pub mod cache_fixtures;
pub mod inspect_fixtures;
