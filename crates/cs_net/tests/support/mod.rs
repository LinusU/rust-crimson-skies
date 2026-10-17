//! Helpers shared by `cs_net`'s test modules, not a test target itself.
//!
//! `f54_c_fuzz` is the deterministic adversarial corpus the F54-C lifecycle
//! tests feed through the production codec, consumer and snapshot decode.
//! `loopback` is the process-wide socket lock every test that opens a real
//! loopback socket holds — the one-binary layout lets the modules that were
//! separate test binaries share it, which per-file statics could not do.

pub mod f54_c_fuzz;
pub mod loopback;
