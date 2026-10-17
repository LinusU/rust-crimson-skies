//! `cs_net`'s single integration-test binary (task #1561).
//!
//! Every former `tests/*.rs` target is a module here, so `cargo test` links and
//! runs one executable instead of one per file. Test prefixes still select the
//! same tests because libtest's filter is a substring match.
//!
//! `cs_xtask test-layout` fails when a `.rs` file under this directory is not
//! reached through these `mod` declarations.

mod accept_f476_measured_mode_rules;
mod accept_f54_a_wire_contract;
mod accept_f54_b_pinned_transport;
mod accept_f54_c_lifecycle;
mod accept_f54_x10_loopback_loss;
mod accept_f55_a_lobby_protocol;
mod accept_f55_b_host_validation;
mod accept_f55_c_join_refusals;
mod accept_f56_a_rules;
mod accept_f56_b_rules;
mod accept_f56_c_launch;
mod accept_f57_a_snapshot_schema;
mod accept_f58_a_recovery_rules;
mod accept_f58_a_session_identity;
mod accept_f58_b_intent_validation;
mod accept_f58_b_rate_and_resource_caps;
mod accept_f58_b_recovery_claims;
mod accept_f58_c_departure_ledger;

// Helpers shared by the modules above: the F54-C adversarial corpus and the
// process-wide loopback-socket lock.
mod support;
