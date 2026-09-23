//! Engine-facing script IR, validation and the pure evaluator.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. No implementation yet: the IR
//! and evaluator land with the F10+ scripting tasks. Allowed dependency:
//! [`cs_types`]. This crate must never depend on Bevy or Avian.
