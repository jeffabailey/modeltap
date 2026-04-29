//! Regression test for the inventory linker-elision issue (DELIVER step 01-03).
//!
//! Rust's linker elides static segments from crates that aren't directly
//! referenced. `inventory::submit!` in a plugin crate doesn't actually
//! register unless the binary crate has at least one source-level reference
//! (e.g., `use modeltap_plugin_X as _;`) to that crate. If this test fails,
//! a plugin crate is declared in `modeltap-app/Cargo.toml` but no
//! `use ... as _;` line in `modeltap-app/src/main.rs` forces the linker to
//! include it.
//!
//! Per ADR-001 §"Plugin registration mechanism".

// Force linkage in this integration test binary too, since the test crate is
// a separate compilation unit from the `modeltap` binary. Without these
// imports the test binary's `inventory::iter()` would also see zero entries
// even though the binary's iter() works fine.
use modeltap_plugin_hf as _;
use modeltap_plugin_llama_cli as _;
use modeltap_plugin_lm_studio as _;
use modeltap_plugin_ollama as _;

use inventory::iter;
use modeltap_core::PluginFactory;

#[test]
fn all_four_plugin_factories_are_registered_via_inventory() {
    let count = iter::<PluginFactory>().count();
    assert!(
        count >= 4,
        "expected >= 4 PluginFactory entries (ollama, llama-cli, hf, lm-studio); got {count}. \
         Likely cause: missing `use modeltap_plugin_X as _;` in modeltap-app/src/main.rs."
    );
}
