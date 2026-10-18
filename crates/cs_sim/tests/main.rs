//! `cs_sim`'s single integration-test binary (task #1557, convention from
//! task #1542).
//!
//! Every former `tests/accept_*.rs` target and the former `tests/ai/`
//! directory target (`tests/ai/main.rs`, now `tests/ai/mod.rs`) is a module
//! here, so `cargo test` links and runs one executable instead of one per
//! file. Test prefixes still select the same tests: libtest's filter is a
//! substring match, so `accept_f32_a_` matches
//! `ai::accept_f32_a_combat::accept_f32_a_…` and `accept_f27_a_` matches
//! `accept_f27_a_guns::accept_f27_a_…`.
//!
//! `cs_xtask test-layout` fails when a `.rs` file under this directory is not
//! reached through these `mod` declarations, because `autotests = false`
//! means cargo would silently compile and run no test in it.

mod accept_doclib_conflict;
mod accept_f16_d_pause_and_probe_evidence;
mod accept_f16_f_original_clock_policy_comparison;
mod accept_f16_f_speed_up_reaches_gameplay_timers;
mod accept_f19_b_authoritative_wind_conversion;
mod accept_f20_a_animated_object;
mod accept_f22_a_one_frame_key_edge;
mod accept_f24_a_flight_equations;
mod accept_f25_a_exceptional_telemetry;
mod accept_f25_b_exceptional_control_law;
mod accept_f26_a_handling_envelopes;
mod accept_f26_b_headless_probes;
mod accept_f26_c_roster_audit;
mod accept_f27_a_guns;
mod accept_f27_b_guns_runtime;
mod accept_f27_c_sweep_query_and_hit_routing;
mod accept_f27_d_ammunition_registry;
mod accept_f28_a_ordnance;
mod accept_f28_ae1_area_ledger;
mod accept_f28_b_ordnance_runtime;
mod accept_f29_a_damage_resolver;
mod accept_f29_b_zones_armor_disablement;
mod accept_f29_c_04_bailout_mission_transition;
mod accept_f29_c_kill_award;
mod accept_f30_a_targeting;
mod accept_f30_b_selection_actions;
mod accept_f30_c_phase_clear_reason;
mod accept_f30_d_crosshair_eligibility;
mod accept_f31_a_navigation;
mod accept_f31_b_navigation;
mod accept_f31_c_navigation_follow;
mod accept_f31_d_navigation_coverage;
mod accept_f33_c2_ends_action_owner;
mod accept_f34_a_world_actors;
mod accept_f34_b_world_actors;
mod accept_f35_a_capital_ship;
mod accept_f35_b_capital_runtime;
mod accept_f35_c_capital_wiring;
mod accept_f36_a_interaction;
mod accept_f37_c_fu1_seed_and_reward_catalog;
mod accept_f37_d_fu6_mission_countdown_producer;
mod accept_f37_d_fu8_countdown_stale_guard;
mod accept_f37_e1_mission_actor_facts;
mod accept_f39_a_objective_trigger_spawn;
mod accept_f39_b_objective_runtime;
mod accept_f39_e_actorid_space;
mod accept_f41_a_audio_events;
mod accept_f41_b_engine_voice_smoothing;
mod accept_f41_b_spatial_emitters;
mod accept_f43_v2_reconciled_records;
mod accept_f56_a_final_result;
mod accept_f56_b_objective;
mod accept_f56_c_session;
mod accept_f57_a_net_state_authority;
mod accept_m01_lc_lowering_conditions_lifecycle;
mod accept_m02_b_fu3_objective_address;
mod accept_m02_b_fu4_directive_family;
mod accept_t1319_coast_then_detonate;
mod accept_t397_animation_event_identity;
mod accept_t442_shared_damage_identity;
mod accept_t447_loop_route_progress;
mod accept_t451_integrated_bank_sign;
mod accept_t496_audio_identity;
mod accept_t526_bank_hold_and_envelope;
mod accept_t548_lost_target_blast;
mod accept_t601_ordnance_identity;
mod accept_t761_deviating_command;
mod accept_test_bin_rest_sim;
mod ai;
