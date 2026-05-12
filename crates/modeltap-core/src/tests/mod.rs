//! Public test-harness module for plugin contract tests.
//!
//! Per `docs/feature/folder-group-bulk-delete/distill/plugin-contract-spec.md`
//! §2, this module hosts parametrized contract-test harnesses that EVERY
//! plugin crate invokes from its own integration test files. The harness
//! lives in `modeltap-core` (not in any plugin crate) so plugin crates do NOT
//! need to depend on each other — architecture rule R2 ("only the app crate
//! composes plugins") is preserved.
//!
//! Gated by the `test-helpers` feature flag. Plugin crates opt-in via:
//!
//! ```toml
//! [dev-dependencies]
//! modeltap-core = { path = "../../crates/modeltap-core", features = ["test-helpers"] }
//! ```
//!
//! Production builds (no `--features test-helpers`) do NOT compile this
//! module, so the binary's footprint is unaffected.

pub mod plugin_contract;
