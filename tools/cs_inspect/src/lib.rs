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
//! full-content readiness check).

pub mod evidence;
pub mod install;
