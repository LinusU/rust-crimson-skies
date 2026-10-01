//! F32-A acceptance tests for the runtime combat-AI contract.
//!
//! Task test prefix: `accept_f32_a_`. Spec:
//! `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-A`.
//!
//! Every test drives production code: `cs_sim::ai::combat`'s
//! [`CombatPlanner::decide`], its role/skill/formation records and its
//! synthetic fixture. Removing the priority scoring, the authoritative
//! threat evidence, the hostility gate, the reaction gate, the recovery
//! reporting or the arsenal snapshot fails the tests that name them.

mod accept_f32_a_combat;
mod accept_f32_a_combat_failures;
