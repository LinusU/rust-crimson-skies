//! F23-A acceptance tests: the verified Avian schedule adapter, the one-tick
//! force/torque path and the declared collision layers.
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-A`. Task test prefix: `accept_f23_a_`.
//!
//! These tests drive production code only: [`cs_app::physics`] builds the real
//! pinned Bevy/Avian plugin group with the real adapter and a known `Mass`, and
//! `cs_sim::collision` owns the layer vocabulary. No original data and no
//! `CS_GAME_DIR` access: they prove the interface and the schedule, never the
//! original game.
//!
//! Every value here is newly authored fixture data.

mod common;
mod forces;
mod layers;
mod schedule;
