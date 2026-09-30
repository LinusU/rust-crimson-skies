//! F18-A acceptance tests: world instances, sectors and collision roles.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-A`. Task test prefix: `accept_f18_a_`.
//!
//! These tests drive production code only: `cs_content::world` owns the
//! records, `cs_app::world::spawn_world` is the one conversion into
//! Bevy/Avian, `cs_app::world::WorldContacts` is the contact log, and the
//! synthetic arch is `cs_app::world::arch_world` — the same functions the
//! runtime will call. No test carries its own world builder or its own
//! collision path.
//!
//! No original data and no `CS_GAME_DIR` access: the fixture is authored
//! development content (`Origin::SyntheticFixture`) and proves the interface
//! and the collision contract, never the original game.
//!
//! AC01 is the stage's minimum scenario: *fly a swept body through a narrow
//! synthetic arch at high speed without collision mismatch.* It is covered
//! from both sides — a probe through the opening must not touch anything
//! (no phantom wall), and a probe aimed at a leg must be stopped by it (no
//! ghost opening) — plus a record-level check that the visual entity, the
//! collider entity and the authored instance agree. `sweep` owns that
//! geometry, `spawn` owns the spawn boundary (what it refuses as a whole,
//! and what each declared collision role produces) and `records` owns the
//! typed contract in `cs_content::world`.

mod common;
mod records;
mod spawn;
mod sweep;
