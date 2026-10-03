//! F32 acceptance tests for the runtime combat-AI contract.
//!
//! Task test prefix: `accept_f32_a_` for the roles/skill/decision-trace
//! stage and `accept_f32_b_` for the maneuvers/firing-solution stage. Spec:
//! `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stages
//! `### F32-A` and `### F32-B`.
//!
//! Every test drives production code: `cs_sim::ai::combat`'s
//! [`CombatPlanner::decide`], its role/skill/formation records, its
//! maneuver and firing-solution vocabulary and its synthetic fixture.
//! Removing the priority scoring, the authoritative threat evidence, the
//! hostility gate, the reaction gate, the recovery reporting, the arsenal
//! snapshot, the maneuver map or the per-mount availability classification
//! fails the tests that name them.

mod accept_f32_a_combat;
mod accept_f32_a_combat_failures;
mod accept_f32_b_combat;
