//! The one integration-test binary of `cs_inspect`: every former
//! `tests/*.rs` file is a module below.

mod accept_f00_a_cli_smoke;
mod accept_f00_b_cli_help;
mod accept_f01_a_check_claims;
mod accept_f01_b_ledger_validation;
mod accept_f01_c_audit_wiring;
mod accept_f01_d_release_inventory;
mod accept_f02_a_synthetic_install_fixture;
mod accept_f02_c_dependency_impact;
mod accept_f02_c_inventory_command;
mod accept_f02_c_retail;
mod accept_f02_d_audit_command;
mod accept_f02_d_readiness;
mod accept_f02_d_retail;
mod accept_f14_e_campaign_report;
mod accept_f31_a_routes_command;
mod accept_f31_c_routes_wiring;
mod accept_f31_d_route_coverage;
mod accept_ref_capture_protocol;
mod accept_t447_loop_route_projection;
mod accept_test_bin_inspect;
mod evidence_report_f02_c;
mod evidence_report_f02_d;
mod evidence_report_f14_e;
mod evidence_report_f31_d;

// Fixture helpers shared by the modules above.
mod common;
