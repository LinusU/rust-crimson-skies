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
//! * [`bootstrap`] requires every workspace member the F00 deliverable names
//!   to be listed with a real manifest, and composes the pin and CI guards so
//!   one command freezes the platform bootstrap (F00-D).
//! * [`budget`] requires the profiles CI builds under to stay inside the
//!   runner's disk budget, so the largest link in the job cannot be the one
//!   that exhausts it (task #430).
//! * [`target_dir`] requires the effective `CARGO_TARGET_DIR` to be private
//!   to this worktree, so concurrent agent builds cannot reuse each other's
//!   artifacts (task #383), requires it to hold no artifact a worktree that
//!   has since been removed produced (task #433), and requires it to hold no
//!   artifact a different, still-present checkout produced (task #440).
//! * [`package`] states what a release archive may contain, refuses a
//!   candidate that carries proprietary content or drops a required notice, and
//!   resolves the user-data base directory from the platform alone (F61-A).
//! * [`corpus`] declares the F62-A differential-corpus contract — the
//!   synthetic/private/regression separation, the per-container truncation
//!   oracle and the known-container manifest — and audits that no private
//!   bytes are tracked in the repo.
//! * [`transient`] retries a read or spawn that loses its file for the
//!   moment a writer is replacing it — the `NotFound` a concurrent cargo on
//!   the same target directory can put under a check (task #610).
//!
//! The `cs_xtask` binary exposes `test-select`, `verify-ci`,
//! `verify-bootstrap`, `verify-ci-budget`, `verify-target-dir`,
//! `verify-package` and `corpus`; coverage commands arrive with later tooling
//! tasks.

pub mod bootstrap;
pub mod budget;
pub mod ci;
pub mod corpus;
pub mod package;
pub mod pins;
pub mod target_dir;
pub mod test_select;
pub mod transient;
