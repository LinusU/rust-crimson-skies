//! `cs_formats`'s single integration-test binary (task #1560).
//!
//! Every former `tests/*.rs` target is a module here, so `cargo test` links and
//! runs one executable instead of 21. Test prefixes still select the same
//! tests: libtest's filter is a substring match, so `accept_f03_a_` matches
//! `accept_f03_a_checked_lengths::accept_f03_a_…`.
//!
//! `cs_xtask test-layout` fails when a `.rs` file under this directory is not
//! reached through these `mod` declarations, because `autotests = false` means
//! cargo would silently compile and run no test in it.

mod accept_doclib_conflict;
mod accept_f03_a_checked_lengths;
mod accept_f03_a_truncation_boundaries;
mod accept_f03_a_unaligned_and_nested;
mod accept_f03_b_allocation_budget;
mod accept_f03_b_no_large_allocation;
mod accept_f03_b_recursion_budget;
mod accept_f03_b_u32_max_and_extent_overflow;
mod accept_f03_c_entrypoint_returns_borrowed_input;
mod accept_f03_c_error_propagation;
mod accept_f03_c_teardown_and_retry;
mod accept_f03_c_unaligned_entrypoint;
mod accept_f03_d_fuzz_nested_range_and_string;
mod accept_f03_d_truncation_corpus;
mod accept_f12_f_reader_absolute_offset_window;
mod accept_f64_a_legacy_profile_contracts;
mod accept_parity4_lens_flare_a;
mod accept_test_bin_rest_formats;
mod bm;
mod common;
mod corpus;
mod evidence_report_f10_b_gamez;
mod gamez;
mod interp;
mod rof;
mod script_raw;
mod string_id;
mod texture;
mod zbd;
