//! F21-A acceptance tests: camera modes and projection policy.
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-A`; shared contract `docs/contracts/UI-NETWORK.md`.
//! Task test prefix: `accept_f21_a_`.
//!
//! These tests drive production code only: `cs_content::cameras` owns the
//! declared records and their validation, and `cs_app::camera` owns the
//! lowering boundary, the projection math and the framing of a world-space
//! point. No test carries its own projection formula beyond the standard
//! perspective oracle it asserts against.
//!
//! The stage's minimum scenario — *compare framing at three aspect ratios
//! using an invariant world-space target* — lives in `framing`, together
//! with the vertical/horizontal field-of-view conversion that makes
//! aspect-correct framing possible. `projection` owns the lowering refusals
//! and the conversion round trip; `modes` owns the mode-set lowering;
//! `records` owns the declared-record validation in `cs_content::cameras`.
//!
//! No original data and no `CS_GAME_DIR` access: every value here is
//! authored development content (`Origin::SyntheticFixture`), so these
//! tests prove the interface and the projection contract, never the
//! original game.

mod common;
mod framing;
mod modes;
mod projection;
mod records;
