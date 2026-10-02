//! F21 acceptance tests: camera modes, projection policy, the rigs and the
//! session that runs them.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stages
//! `### F21-A`, `### F21-B` and `### F21-C`; shared contract
//! `docs/contracts/UI-NETWORK.md`. Task test prefixes: `accept_f21_a_`,
//! `accept_f21_b_` and `accept_f21_c_`.
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
//! Stage `### F21-C` — script cameras, deterministic capture flags and the
//! session that runs them:
//!
//! * `script` owns the scripted camera request: the identity, span and shot a
//!   producer asks for, and the refusals that keep a bounded half-open span and
//!   a refused install that changes nothing;
//! * `capture` owns the deterministic capture request, the pinned projection
//!   derived from the frame's own lowered policy, the `f64 → f32` narrowing kept
//!   visible, and the override report;
//! * `session` owns the integration and the stage's minimum scenario, *swap
//!   aircraft during a scripted capture and verify the camera binds to the new
//!   player body* (AC03), together with teardown, retry and error propagation.
//!
//! No original data and no `CS_GAME_DIR` access: every value here is
//! authored development content (`Origin::SyntheticFixture`), so these
//! tests prove the interface and the camera contract, never the original
//! game.

mod capture;
mod common;
mod framing;
mod modes;
mod projection;
mod records;
mod rig;
mod script;
mod session;
mod smoothing;
mod spyglass;
