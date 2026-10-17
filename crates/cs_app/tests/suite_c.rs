//! `cs_app`'s `cs_app_tests_c` integration-test binary (task #1543).
//!
//! Groups (see `docs/findings/2026-10-17-t1543-cs-app-test-binaries.md`): world, f64, d_camera, d_text, d_accessibility, d_scrapbook, a_f0f1.
//!
//! The crate declares several test binaries rather than one because a single
//! binary's `__eh_frame` passes macOS's 16 MiB limit (task #336), and the
//! limit is set by how much dependency code a binary's tests reach, not by
//! their line count. `[package.metadata.cs_test_layout]` in `Cargo.toml` fixes the
//! count; `cs_xtask test-layout` requires every `.rs` file under `tests/` to
//! be reached from one target (a shared, test-free helper excepted).

mod campaign {
    mod common;
    mod vs_m01_geometry_verdict;
}
mod accept_f00_a_cli_smoke;
mod accept_f00_a_synthetic_body;
mod accept_f00_b_cli_help;
mod accept_f00_b_schedule_ordering;
mod accept_f00_c_synthetic_smoke;
mod accept_f16_b_origin_shift;
mod accept_f16_c_frame_rate_integration;
mod accept_f16_e3_zrd_angle_consumer;
mod accept_f17_b_rgb565_expansion;
mod accept_f60_a_benchmark_contracts;
mod accept_f64_c_import_dialog;
mod accept_f64_c_import_report;
mod accept_f64_c_persist_import;
mod accept_f64_d_optional_save_switch;
mod accept_f64_d_retail_custom_aircraft_and_optional_saves;
mod accessibility;
mod camera;
mod evidence_report_f64_c;
mod evidence_report_f64_d;
mod f64_c_support;
mod f64_d_support;
mod scrapbook;
mod text;
mod ui;
mod world;
