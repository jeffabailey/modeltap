//! Side-effect descriptor types referenced by `Msg` variants.
//!
//! `modeltap-tui` cannot depend on `modeltap-app` (would invert the dep
//! direction), so types defined in `actions/*` that need to be carried in a
//! `Msg` payload are stubbed here under `effects/`. Each stub is a minimal
//! data shape; the canonical type lives in `modeltap-app` and is the source
//! of truth.
//!
//! Step 01-06 introduces `UnifyOutcome` here as a stub. Step 01-08 (wiring)
//! will replace this with a re-export or the canonical
//! `modeltap_app::actions::unify::UnifyOutcome` once the orchestrator wiring
//! lands. Until then, the stub is what `Msg::UnifyApplied` carries.

pub mod unify_outcome;
