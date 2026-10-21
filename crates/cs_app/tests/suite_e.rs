//! `cs_app`'s `cs_app_tests_e` integration-test binary (task #1543).
//!
//! The `vs_m01_*` runtime-host suites of the campaign tree and the M01 world evidence
//! that shares their report module. They are kept alone because `vs_m01_runtime` reaches
//! so much engine code that it adds ~5 MB of `__eh_frame` to any binary it joins.
//! `m01_chase_view_parity`, `m01_player_prop_blur` and `vs_m01_rt_player_visual` moved to
//! `campaign/mod.rs` (so `cs_app_tests_d`) during #1811's review: the binary was 4 KB past
//! the 16 MiB `__eh_frame` gate. `vs_m01_rt_hud_art` and `vs_m01_rt_host_cadence` followed
//! during #1812's review for the same reason (~3.5 KB past).
//!
//! See `docs/findings/2026-10-17-t1543-cs-app-test-binaries.md`.

mod accept_t336_suite_e_image;
mod eh_frame_gate;
mod campaign {
    mod common;
    mod m01_play_world_evidence;
    mod vs_m01_rt_content;
    mod vs_m01_rt_evidence_capture;
    mod vs_m01_rt_evidence_report;
    mod vs_m01_runtime;
}
