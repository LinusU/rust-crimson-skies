//! F18-A and F18-B acceptance tests: world instances, sectors, collision roles,
//! world import and static collision generation.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-A` and `### F18-B`. Task test prefixes: `accept_f18_a_` and
//! `accept_f18_b_`.
//!
//! These tests drive production code only: `cs_content::world` owns the
//! records, `cs_app::world::spawn_world` is the one conversion into Bevy/Avian,
//! `cs_app::world::load_world` and its sector calls are the load transaction,
//! `cs_app::world::WorldContacts` is the contact log, and the synthetic
//! fixtures (`arch_world` for the cuboid records, `harbor_world` for the
//! mesh-authored ones) are the same functions the runtime will call. No test
//! carries its own world builder or its own collision path.
//!
//! No original data and no `CS_GAME_DIR` access: both fixtures are authored
//! development content (`Origin::SyntheticFixture`) and prove the interface and
//! the collision contract, never the original game.
//!
//! **F18-A** (`accept_f18_a_`) is the typed contract and the cuboid spawn. Its
//! minimum scenario is AC01: *fly a swept body through a narrow synthetic arch
//! at high speed without collision mismatch*, covered from both sides — a probe
//! through the opening must not touch anything, and a probe aimed at a leg must
//! be stopped by it. `sweep` owns that geometry, `spawn` owns the spawn boundary
//! and `records` owns the typed contract in `cs_content::world`.
//!
//! **F18-B** (`accept_f18_b_`) is the import path and the load transaction.
//! `import` owns mesh-derived static collision — one asset behind the drawn and
//! the collided geometry, each declared role honoured, water bounded, missing
//! geometry reported — and `residency` owns AC02: a damaged objective survives
//! unloading and reloading its sector, and a second mission loads its own
//! population and damage with nothing left over.

mod common;
mod import;
mod records;
mod residency;
mod spawn;
mod sweep;
