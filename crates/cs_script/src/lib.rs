//! Engine-facing script IR, validation and the pure evaluator.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. Allowed dependency: [`cs_types`].
//! This crate must never depend on Bevy or Avian.
//!
//! [`ir`] is the F37-A typed mission IR and its pre-launch validation;
//! [`runtime`] is the mutable state and stable event ordering. The bounded
//! evaluator, timers and snapshots are F37-B/F37-C.

pub mod ir;
pub mod runtime;
