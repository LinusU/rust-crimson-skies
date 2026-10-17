//! `cs_script`'s single integration-test binary (task #1559).
//!
//! Every former `tests/*.rs` target is a module here, so `cargo test` links and
//! runs one executable instead of 24. Test prefixes still select the same
//! tests: libtest's filter is a substring match, so `accept_t583_` matches
//! `accept_t583_rng_state_record::accept_t583_…`.
//!
//! `cs_xtask test-layout` fails when a `.rs` file under this directory is not
//! reached through these `mod` declarations, because `autotests = false` means
//! cargo would silently compile and run no test in it.

mod accept_f37_a;
mod accept_f37_b;
mod accept_f37_c;
mod accept_f37_d;
mod accept_f37_d_fu2;
mod accept_f37_d_fu3;
mod accept_f37_d_fu3_doc;
mod accept_f37_d_fu4;
mod accept_f37_d_fu5;
mod accept_f37_d_fu7;
mod accept_f37_d_fu8_countdown_end_guards;
mod accept_f38_a_host_bindings;
mod accept_f38_b_observed_bindings;
mod accept_f38_b_retail_host_call_corpus;
mod accept_f38_c_located_diagnostics;
mod accept_f38_c_retail_source_map;
mod accept_f38_d1_native_rekey;
mod accept_f38_d1_retail_native_census;
mod accept_m01_lc_lowering_conditions;
mod accept_m01_lc_lowering_signatures;
mod accept_t583_rng_state_record;
mod accept_test_bin_rest_script;
mod evidence_report_f38_b;
mod evidence_report_f38_c;
mod evidence_report_f38_d1;
