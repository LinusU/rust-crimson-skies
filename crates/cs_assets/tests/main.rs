//! `cs_assets`' single integration-test binary (task #1558).
//!
//! Every former `tests/*.rs` target is a module here, so `cargo test` links
//! and runs one executable instead of 35. Test prefixes still select the same
//! tests (libtest's filter is a substring match). `cs_xtask test-layout` fails
//! when a `.rs` file under this directory is not reached through these `mod`
//! declarations.

mod common;

mod accept_f02_a_cased_host_path_inventory;
mod accept_f02_b_diagnosis_and_safe_discovery;
mod accept_f02_b_one_byte_edit_fingerprint_and_cache;
mod accept_f02_b_retail_installation;
mod accept_f02_b_sha256_hashing;
mod accept_f02_d_classification;
mod accept_f02_doc_module_doc;
mod accept_f04_a_asset_key_and_context;
mod accept_f04_a_two_worlds_each_resolve_own_texture;
mod accept_f04_b_mounted_sources_and_path_validation;
mod accept_f04_c_session_lifecycle_and_private_export;
mod accept_f04_d_collisions_and_async_cancel;
mod accept_f04_d_gos_registration_order;
mod accept_f04_d_gos_request_fold_rule;
mod accept_f04_d_member_collisions;
mod accept_f04_d_order_archives;
mod accept_f04_d_order_reader_loose;
mod accept_f04_d_order_reader_members;
mod accept_f04_d_original_order_retail_shadowing;
mod accept_f04_d_rof_member_collisions;
mod accept_f04_d_texture_member_collisions;
mod accept_f15_a_cache_contract;
mod accept_f15_a_conversion_boundary;
mod accept_f15_a_load_transaction;
mod accept_f15_b_atomic_cache;
mod accept_f15_b_load_driver;
mod accept_f15_c_load_session;
mod accept_f15_d_cold_warm_restart;
mod accept_f15_e_mission_closure_cold_warm_restart;
mod accept_t693_metaopenfile_name_matching;
mod accept_test_bin_rest_assets;
mod evidence_report_f02_b;
mod evidence_report_f04_d;
mod evidence_report_f15_d;
mod evidence_report_f15_e;
mod evidence_report_t693;
