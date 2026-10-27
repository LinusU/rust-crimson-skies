//! F32 acceptance tests for the runtime combat-AI contract.
//!
//! Task test prefix: `accept_f32_a_` for the roles/skill/decision-trace
//! stage, `accept_f32_b_` for the maneuvers/firing-solution stage,
//! `accept_f32_c_` for the session runtime that composes formations, aces and
//! difficulty, `accept_f32_d_` for the retail-measured stage: the
//! original's difficulty option and AI skill tiers, and the repeated
//! mission-combat probe run at every discovered difficulty, `accept_f32_r_`
//! for the unmeasured role set, `accept_f32_pg_` for the probe scenario's
//! designed-fixture provenance marker, and `accept_f32_fd_` for the
//! session-owned per-actor fire discipline the reported cadence needs
//! (#571), and `accept_f32_cadence_` for its tick monotonicity (#555). Spec:
//! `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stages
//! `### F32-A`, `### F32-B`, `### F32-C` and `### F32-D`.
//!
//! Every test drives production code: `cs_sim::ai::combat`'s
//! [`CombatPlanner::decide`], its role/skill/formation records, its
//! maneuver and firing-solution vocabulary, its per-session
//! [`CombatRuntime`] with its [`FormationCoordinator`], [`AceVariant`]s,
//! [`DifficultyRoster`] and [`FireDiscipline`], and its
//! [`DifficultyProbeReport`]. Removing the
//! priority scoring, the authoritative threat evidence, the hostility gate,
//! the reaction gate, the recovery reporting, the arsenal snapshot, the
//! maneuver map, the per-mount availability classification, the recovery
//! *application*, the profile selection, the session's fire-discipline
//! state or the difficulty probe's
//! per-tier aggregation fails the tests that name them.

mod accept_f32_a_combat;
mod accept_f32_a_combat_failures;
mod accept_f32_b_combat;
mod accept_f32_c_combat;
mod accept_f32_c_combat_failures;
mod accept_f32_cadence_combat;
mod accept_f32_d_combat;
mod accept_f32_d_combat_failures;
mod accept_f32_fd_combat;
mod accept_f32_pg_combat;
mod accept_f32_r_combat;
mod accept_f32_station_consume;
