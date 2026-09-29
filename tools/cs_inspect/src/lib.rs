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
//! installation, family by family, with a strict status. [`rof`] is the
//! F05-C `rof` command: one ROF container mounted and reported, with an
//! optional bounded member read and explicit export. [`interp`] is the
//! F07-B consumer of the INTERP loading-script decoder — one container decoded
//! and validated, reported with its lossless tokens and the findings the
//! decoder retained — and the F07-C consumer of the loading plan: with
//! `--plan` the same container is classified against a command table, its
//! registered commands are resolved through a content session, and every
//! failure is reported with its source offset and the world it affects.
//! [`textures`] is the F08-D `texture-audit` command: every ZBD texture
//! decoded through the F08-C upload boundary and compared texel by texel
//! with a pinned reference extraction, with an optional private contact
//! sheet. [`catalog`] is the F14-A synthetic content-catalog fixture: ready
//! and unsupported stable-id rows through the canonical `cs_content`
//! constructor, never a retail entry. [`reference_capture`] is the
//! REF-CAPTURE-PROTOCOL record model (#357): the operator worksheet shape
//! for an original-game capture — fingerprints, timebase, units, artifacts,
//! observer and capture method — with its three-valued admission rules
//! (valid, invalid, unavailable) and the reserved-holdout gate that keeps
//! fitting acceleration alone from standing in for handling fidelity.
//! [`config`] is the F12-C `config` command: one configuration member or PE
//! resource image routed by its observed rule, read through the
//! `cs_content::config` consumers, with declared tuning fields resolved to
//! checked constants and localized string ids resolved through the catalog.
//! [`script_discovery`] is the F13-B `scripts` command: every ZBD container
//! of an installation is routed and its loading, mission and animation
//! programs located and classified, with an optional coverage check that
//! fails closed on a container that hides its programs.

pub mod catalog;
pub mod config;
pub mod evidence;
pub mod install;
pub mod interp;
pub mod reference_capture;
pub mod resolve;
pub mod rof;
pub mod script_discovery;
pub mod textures;
pub mod zbd;
