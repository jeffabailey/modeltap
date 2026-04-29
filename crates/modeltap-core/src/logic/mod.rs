//! Pure-domain logic functions used by the orchestration layer.
//!
//! These are pure functions over domain types — no I/O, no async. They are
//! their own driving ports (the function signature IS the public interface,
//! so calling them directly in tests IS port-to-port testing per the
//! `nw-tdd-methodology` convention).

pub mod canonical_selector;
pub mod compatibility;
pub mod dedup;
pub mod plan;
pub mod unification_status;
