//! `cs_app`'s `cs_app_tests_d` integration-test binary (task #1543).
//!
//! Groups (see `docs/findings/2026-10-17-t1543-cs-app-test-binaries.md`): ev_f, a_f3a, a_f3b, d_physics, d_hud, c_m02up, c_f50, c_misc.
//! The m02-m05 campaign members moved here from `cs_app_tests_a` (task
//! #1615): this binary's `campaign/mod.rs` module root already carries their
//! m06-m24 siblings and the shared machinery they bind.
//!
//! The crate declares several test binaries rather than one because a single
//! binary's `__eh_frame` passes macOS's 16 MiB limit (task #336), and the
//! limit is set by how much dependency code a binary's tests reach, not by
//! their line count. `[package.metadata.cs_test_layout]` in `Cargo.toml` fixes the
//! count; `cs_xtask test-layout` requires every `.rs` file under `tests/` to
//! be reached from one target (a shared, test-free helper excepted).

mod accept_f30_a_targeting_boundary;
mod accept_f30_b_targeting_session;
mod accept_f30_c_target_consumers;
mod accept_f30_c_targeting_schedule;
mod accept_f32_e_combat_lowering;
mod accept_f33_a_roster_boundary;
mod accept_f33_b_briefing_and_wingmates;
mod accept_f33_c_ally_lifecycle;
mod accept_f33_d_neutral_traffic_population;
mod accept_f34_c_world_actors;
mod accept_f35_a_capital_boundary;
mod accept_f35_b_capital_boundary;
mod accept_f35_c_capital_boundary;
mod accept_f36_a_interaction_boundary;
mod accept_f36_b_moving_frame_transfer;
mod accept_f36_c2_consumer_rebind;
mod accept_f36_c_interaction_consumers;
mod accept_f39_c_objective_session;
mod accept_f39_d_marker_binding_collision;
mod accept_f39_d_objective_branching;
mod accept_f39_e1_retail_dormant_reveal;
mod accept_f39_e2_block_precedence;
mod accept_f39_e4_count_category_producers;
mod accept_f39_e5_completion_effects;
mod accept_f39_e6_repeated_effect_key;
mod accept_f39_e7_detached_condition_vocabulary;
mod accept_f39_e_actorid_space;
mod accept_t336_suite_d_image;
mod campaign;
mod eh_frame_gate;
mod evidence_report_f18_grid_collision_origin;
mod evidence_report_f19_weather_units_measure;
mod evidence_report_f20_d;
mod evidence_report_f21_d;
mod evidence_report_f33_d;
mod evidence_report_f39_d;
mod evidence_report_f39_e1;
mod evidence_report_f39_e2;
mod evidence_report_f39_e4;
mod evidence_report_f39_e6;
mod evidence_report_f39_e7;
mod evidence_report_f49_d_02;
mod evidence_report_f49_d_03;
mod evidence_report_f49_d_parent;
mod evidence_report_flight_original_levelop;
mod evidence_report_flight_original_playtest;
mod f21_d_evidence_doc_step1_env;
mod f45_d_evidence_doc_step1_env;
mod hud;
mod physics;
