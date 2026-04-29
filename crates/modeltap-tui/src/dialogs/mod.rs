//! Modal dialog state machines (per ADR-006 + US-05).
//!
//! Dialogs are PURE state — they live as `Option<...>` fields on `AppState`
//! and are mutated by the same `update()` Elm-loop that handles all other
//! messages. The composition root never instantiates a dialog directly; it
//! only feeds messages to `update()`.
//!
//! Step 01-04 introduces the `zap_confirm` dialog (US-05). Subsequent steps
//! add unify-confirm (US-06), help-overlay (US-07), and detail-screen
//! (US-04) dialogs to this module.

pub mod zap_confirm;
