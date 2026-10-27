//! `cs_app`'s `cs_app_tests_b` integration-test binary (task #1543).
//!
//! Groups (see `docs/findings/2026-10-17-t1543-cs-app-test-binaries.md`): a_f2a, playtest, d_render, a_f40.
//! The F40 cinematics members and their evidence harnesses moved here from
//! `cs_app_tests_a` (task #1615): this binary already reaches the device
//! playback/render code through `playtest_*` and `render/`, so they add
//! ~0.15 MiB here while they cost `cs_app_tests_a` ~4.8 MiB.
//!
//! The crate declares several test binaries rather than one because a single
//! binary's `__eh_frame` passes macOS's 16 MiB limit (task #336), and the
//! limit is set by how much dependency code a binary's tests reach, not by
//! their line count. `[package.metadata.cs_test_layout]` in `Cargo.toml` fixes the
//! count; `cs_xtask test-layout` requires every `.rs` file under `tests/` to
//! be reached from one target (a shared, test-free helper excepted).

mod accept_f11_e_session_producer;
mod accept_f11_f_session_scene;
mod accept_f13_d_fu1_keyword_reconciliation;
mod accept_f13_d_fu5_census_statements;
mod accept_f20_a_animation_boundary;
mod accept_f20_b_animation_playback;
mod accept_f20_c_01_attachment_hierarchy;
mod accept_f20_c_02_fixed_tick_instances_and_teardown;
mod accept_f20_c_03_visibility_lod_ownership;
mod accept_f20_c_04_collider_presence_consumer;
mod accept_f20_c_05_node_collider_spawn_wiring;
mod accept_f20_c_marker_consumer;
mod accept_f20_c_production_composition;
mod accept_f20_c_scene_teardown;
mod accept_f20_c_stale_consumer_docs;
mod accept_f20_c_wired_session;
mod accept_f20_d_validation;
mod accept_f22_b_device_adapters;
mod accept_f22_c_focus_ui_replay_ownership;
mod accept_f22_d_device_families_and_command_coverage;
mod accept_f22_e_profile_persistence;
mod accept_f22_wheel_mouse_wheel_producer;
mod accept_f24_c_flight_wiring;
mod accept_f40_d1_mpeg_playback;
mod accept_f40_d3_camera_anim;
mod accept_f40_d4_device_playback;
mod accept_f40_d_story_cinematics;
mod accept_sidewinder_ffb2;
mod accept_t336_suite_b_image;
mod eh_frame_gate;
mod evidence_report_f11_f;
mod evidence_report_f40_d;
mod evidence_report_f40_d1;
mod evidence_report_f40_d4;
mod playtest_b0004;
mod playtest_fly;
mod playtest_full_aircraft;
mod playtest_retail;
mod playtest_retail_launch;
mod render;
