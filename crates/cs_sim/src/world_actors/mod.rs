//! F34-A world-actor motion and dependency contract
//! (`specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`, stage
//! `### F34-A`).
//!
//! Typed inputs/outputs and minimal synthetic fixtures only; the rail, road,
//! water and kinematic runtime is F34-B and the pickup, gate and cargo wiring
//! is F34-C. Everything here is designed behavior: no original rule is
//! measured, so nothing is an original-fidelity claim.
//!
//! - [`trajectory`]: [`trajectory::Trajectory`], whose position and velocity
//!   come from one tick-indexed function, so they cannot disagree. Sampling
//!   takes a [`cs_types::Tick`] and nothing else: culling cannot stop motion.
//! - [`anchor`]: [`anchor::AnchorSocket`] and the single
//!   [`anchor::anchor_sample`] the renderer and pickup eligibility both call,
//!   plus [`anchor::relative_velocity_m_s`].
//! - [`graph`]: [`graph::SupportGraph`], explicit support/cargo edges whose
//!   destruction cascade flips geometry and collision in one step.
//! - [`release`]: [`release::release_payload`], a detached payload that
//!   inherits source motion and faction and keeps its objective identity.

pub mod anchor;
pub mod graph;
mod math;
pub mod release;
pub mod trajectory;

pub use math::Quat;
