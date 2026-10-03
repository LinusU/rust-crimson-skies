//! F34 world actors: the motion/dependency contract (F34-A) and the
//! rail/road/water/kinematic runtime (F34-B)
//! (`specs/F34-ground-vehicles-boats-trains-and-mission-machinery.md`,
//! stages `### F34-A` and `### F34-B`).
//!
//! Everything here is designed behavior: no original rule is measured, so
//! nothing is an original-fidelity claim. The pickup, gate and cargo wiring
//! into the mission host is F34-C.
//!
//! - [`trajectory`]: [`trajectory::Trajectory`], whose position and velocity
//!   come from one tick-indexed function, so they cannot disagree. Sampling
//!   takes a [`cs_types::Tick`] and nothing else: culling cannot stop motion.
//! - [`anchor`]: [`anchor::AnchorSocket`] and the single
//!   [`anchor::anchor_sample`] the renderer and pickup eligibility both call,
//!   plus [`anchor::relative_velocity_m_s`]. F34-B adds
//!   [`anchor::pickup_eligible_pose`], which judges eligibility against the
//!   same runtime pose the renderer reads.
//! - [`graph`]: [`graph::SupportGraph`], explicit support/cargo edges whose
//!   destruction cascade flips geometry and collision in one step.
//! - [`release`]: [`release::release_payload`], a detached payload that
//!   inherits source motion and faction and keeps its objective identity.
//! - [`route`]: [`route::RoutePlan`], the validated gate-aware polyline a
//!   route follower drives, with [`route::RouteGate`] passages an actor's
//!   [`graph::Presence`] controls.
//! - [`runtime`]: [`runtime::WorldActorSet`], the per-session registry that
//!   steps [`runtime::ActorMotion`] one tick at a time, holds route
//!   followers at closed gates, destroys actors through the support graph
//!   as zero-velocity wrecks and registers released payloads as free
//!   drifting actors. Its `step` takes no visibility input: offscreen
//!   motion, destruction and timers cannot stop because nobody looks.

pub mod anchor;
pub mod graph;
mod math;
pub mod release;
pub mod route;
pub mod runtime;
pub mod trajectory;

pub use math::Quat;
