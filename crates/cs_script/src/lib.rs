//! Engine-facing script IR, validation and the pure evaluator.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. Allowed dependency: [`cs_types`].
//! This crate must never depend on Bevy or Avian.
//!
//! [`ir`] is the F37-A typed mission IR and its pre-launch validation;
//! [`runtime`] is the mutable state, stable event ordering, the bounded
//! evaluator, the deferred-work queue (F37-B) and the versioned save record
//! with its exact pending-timer restore (F37-C). Applying the effects a mission
//! asks for is the simulation's job (`cs_sim::mission`). [`bindings`] is the
//! F38-A host-binding registry that lowers an adapter's raw calls into the IR.

pub mod bindings;
pub mod ir;
pub mod runtime;
