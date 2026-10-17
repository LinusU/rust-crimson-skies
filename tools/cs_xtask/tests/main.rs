//! `cs_xtask`'s single integration-test binary (task #1562, convention from
//! task #1542).
//!
//! Every former `tests/accept_*.rs` target is a module here, so `cargo test`
//! links and runs one executable instead of one per file. Test prefixes still
//! select the same tests: libtest's filter is a substring match, so
//! `accept_t430_` matches `accept_t430_ci_disk_budget::accept_t430_…`.
//!
//! `cs_xtask test-layout` fails when a `.rs` file under this directory is not
//! reached through these `mod` declarations, because `autotests = false`
//! means cargo would silently compile and run no test in it.

mod accept_f00_b_pinned_dependencies;
mod accept_f00_c_ci_gates;
mod accept_f00_c_test_selection;
mod accept_f00_d_workspace_bootstrap;
mod accept_f54_x7_missing_harness_binary;
mod accept_f61_a_release_contents;
mod accept_f61_a_user_data_policy;
mod accept_t1170_push_guard;
mod accept_t383_per_worktree_target_dir;
mod accept_t430_ci_disk_budget;
mod accept_t433_stale_target_dir;
mod accept_t437_runtime_workspace_root;
mod accept_t440_live_foreign_target_dir;
mod accept_t610_transient_target_dir;
mod accept_t696_test_disk_footprint;
mod accept_test_bin_guard;
mod accept_test_bin_rest_xtask;
