//! Reproducible testing, coverage and packaging helpers.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. It launches the application as
//! a subprocess for GPU/audio evidence rather than linking renderer code, and
//! ships the gates an agent runs locally:
//!
//! * [`pins`] verifies that the committed `Cargo.lock` and
//!   `rust-toolchain.toml` still pin the intended Bevy/Avian baseline (F00-B).
//! * [`test_select`] runs a task's positive test selection and refuses an
//!   empty or failing one (F00-C).
//! * [`ci`] checks that the owner-maintained workflow keeps running the
//!   workspace gates (F00-C).
//!
//! The `cs_xtask` binary exposes `test-select` and `verify-ci`; packaging
//! and coverage commands arrive with later tooling tasks.

pub mod ci;
pub mod pins;
pub mod test_select;
