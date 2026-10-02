//! F23 acceptance tests: the verified Avian schedule adapter, the declared
//! collision layers, the body-creation path, the wake/drop accounting of the
//! force queue, the swept crossing detection, the contact reporter's
//! refusal paths and the resting rule for a body resting against world
//! geometry.
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stages `### F23-A` and `### F23-B`. Task test prefixes: `accept_f23_a_`
//! (stage A), `accept_f23_b_` (stage B) and `accept_t428_` (the #428
//! contact/restitution follow-up).
//!
//! These tests drive production code only: [`cs_app::physics`] builds the real
//! pinned Bevy/Avian plugin group with the real adapters, spawns bodies
//! through the production creation path and reads the production contact
//! reporter; `cs_sim::collision` owns the layer vocabulary. No original data
//! and no `CS_GAME_DIR` access: they prove the interface and the schedule,
//! never the original game.
//!
//! Every value here is newly authored fixture data.

mod bodies;
mod common;
mod evidence;
mod forces;
mod layers;
mod reports;
mod resting;
mod schedule;
mod session;
mod sweeps;
mod wake;
