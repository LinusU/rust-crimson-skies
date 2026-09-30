//! F23 acceptance tests: the verified Avian schedule adapter, the declared
//! collision layers, the body-creation path, the wake/drop accounting of the
//! force queue, the swept crossing detection and the contact reporter's
//! refusal paths.
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stages `### F23-A` and `### F23-B`. Task test prefixes: `accept_f23_a_`
//! (stage A) and `accept_f23_b_` (stage B).
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
mod schedule;
mod session;
mod sweeps;
mod wake;
