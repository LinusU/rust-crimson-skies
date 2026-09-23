//! Protocol, codecs and connection/lobby state.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. No implementation yet: protocol
//! work lands with the F24+ networking tasks. Allowed dependency: [`cs_types`].
//! This crate must never depend on Bevy or Avian, and packets never serialize
//! Bevy `Entity` values.
