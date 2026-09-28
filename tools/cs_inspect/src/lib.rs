//! `cs-inspect` library surface: command-line inspection and conversion
//! diagnostics.
//!
//! The binary entry point lives in `main.rs`. This library hosts the typed
//! inspection machinery the CLI commands consume as their stages land;
//! [`evidence`] is the F01-A claim-record admission check and the F01-B
//! ledger-validation front end, and [`install`] is the F02-A synthetic
//! installation-inventory fixture, the F02-C `inventory` command's
//! report wiring (the dependency-impact report and the JSON renderer)
//! and the F02-D `audit` command (per-file classification and the
//! full-content readiness check). [`resolve`] is the F04-C `resolve`
//! command: one lookup in a mounted content session, reported with its
//! resolution trace, plus the explicit private export. [`zbd`] is the
//! F06-D `zbd-audit` command: every ZBD container and member of an
//! installation, family by family, with a strict status. [`interp`] is the
//! F07-B consumer of the INTERP loading-script decoder: one container decoded
//! and validated, reported with its lossless tokens and the findings the
//! decoder retained.

pub mod evidence;
pub mod install;
pub mod interp;
pub mod resolve;
pub mod zbd;
