//! Reproducible testing, coverage and packaging helpers.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. Gates and packaging land with
//! the F00-C/D and later tooling tasks; [`pins`] is the first one: it verifies
//! that the committed `Cargo.lock` and `rust-toolchain.toml` still pin the
//! intended Bevy/Avian baseline. It launches the application as a subprocess
//! for GPU/audio evidence rather than linking renderer code.

pub mod pins;
