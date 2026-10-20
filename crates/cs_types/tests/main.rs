//! `cs_types`' single integration-test binary (task #1542).
//!
//! Every former `tests/accept_*.rs` target is a module here, so `cargo test`
//! links and runs one executable instead of one per file. Test prefixes still
//! select the same tests: libtest's filter is a substring match, so
//! `accept_f01_a_` matches `accept_f01_a_claim_records::accept_f01_a_…` and
//! `accept_t583_` matches `accept_t583_splitmix64_state::accept_t583_…`.
//!
//! `cs_xtask test-layout` fails when a `.rs` file under this directory is not
//! reached through these `mod` declarations, because `autotests = false` means
//! cargo would silently compile and run no test in it.

mod accept_f01_a_claim_records;
mod accept_f02_a_manifest_and_profile_schema;
mod accept_parity2_original_keybindings;
mod accept_t335_splitmix64;
mod accept_t337_verified_original_methods;
mod accept_t397_identity_contract;
mod accept_t583_splitmix64_state;
