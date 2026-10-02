//! F21 acceptance tests: camera modes, projection policy and the rigs.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stages
//! `### F21-A` and `### F21-B`; shared contract
//! `docs/contracts/UI-NETWORK.md`. Task test prefixes: `accept_f21_a_` and
//! `accept_f21_b_`.
//!
//! These tests drive production code only: `cs_content::cameras` owns the
//! declared records and their validation, and `cs_app::camera` owns the
//! lowering boundary, the projection math, the framing of a world-space
//! point, the rigs and the camera smoothing. No test carries its own
//! projection formula beyond the standard perspective oracle it asserts
//! against.
//!
//! Stage `### F21-A` — the declared records, the projection policy and the
//! aspect-correct framing:
//!
//! * `framing` owns the stage's minimum scenario, *compare framing at three
//!   aspect ratios using an invariant world-space target*, together with the
//!   vertical/horizontal field-of-view conversion;
//! * `projection` owns the lowering refusals and the conversion round trip;
//! * `modes` owns the mode-set lowering;
//! * `records` owns the declared-record validation in `cs_content::cameras`.
//!
//! Stage `### F21-B` — the cockpit, chase, look and spyglass rigs:
//!
//! * `rig` owns the cockpit binding, the chase offset and free look, and the
//!   refusals at the mode and lowering boundaries;
//! * `spyglass` owns the stage's minimum scenario, *destroy or switch the
//!   spyglass target mid-frame without stale entity access* (AC02), driven
//!   through the real targeting producer;
//! * `smoothing` owns frame-rate independence, the teleport/swap resets and
//!   preservation through an origin rebase.
//!
//! No original data and no `CS_GAME_DIR` access: every value here is
//! authored development content (`Origin::SyntheticFixture`), so these
//! tests prove the interface and the camera contract, never the original
//! game.

mod common;
mod framing;
mod modes;
mod projection;
mod records;
mod rig;
mod smoothing;
mod spyglass;
