//! F18-A, F18-B and F18-C acceptance tests: world instances, sectors, collision
//! roles, world import, static collision generation, mission overlays and
//! streaming.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-A`, `### F18-B` and `### F18-C`. Task test prefixes:
//! `accept_f18_a_`, `accept_f18_b_` and `accept_f18_c_`.
//!
//! These tests drive production code only: `cs_content::world` owns the
//! records, `cs_app::world::spawn_world` is the one conversion into Bevy/Avian,
//! `cs_app::world::load_world` and its sector calls are the load transaction,
//! `cs_app::world::WorldContacts` is the contact log, and the synthetic
//! fixtures (`arch_world` for the cuboid records, `harbor_world` for the
//! mesh-authored ones, `depot_world` for the door and the sectors a mission
//! overlay and a streaming pass act on) are the same functions the runtime will
//! call. No test carries its own world builder or its own collision path.
//!
//! No original data and no `CS_GAME_DIR` access: all three fixtures are authored
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
//! **#421** (`accept_f18_b_shear_`) is the F18-A follow-up that narrows the
//! whole-build refusal from "no translation/rotation/scale triple" to "no exact
//! placement at all": a sheared world object is placed, exactly, in the
//! presentation and in the collision. `shear` owns it.
//!
//! **F18-B** (`accept_f18_b_`) is the import path and the load transaction.
//! `import` owns mesh-derived static collision — one asset behind the drawn and
//! the collided geometry, each declared role honoured, water bounded, missing
//! geometry reported — and `residency` owns AC02: a damaged objective survives
//! unloading and reloading its sector, and a second mission loads its own
//! population and damage with nothing left over. `shared_asset` owns the
//! **engine-asset** half of that claim (#425): four records naming one stored
//! mesh share one `Assets<Mesh>` entry on all four mesh layouts, records naming
//! different meshes do not, and unloading the world releases the shared asset.
//!
//! **F18-D** (`accept_f18_d_`) is the evidence stage over the original
//! installation: `audit` owns AC04 — every discovered world group visited, its
//! representative geometry compared, and the traversal routes and stunt
//! openings reported as the blocker they are while the world placement is
//! undecoded — plus the real offscreen GPU capture that shows each group's
//! stored geometry is drawable as stored.
//!
//! **#427** (`accept_t427_`) measures the **original's** trigger volumes instead
//! of the fixtures': `triggers` owns the survey that reads every world
//! container's node array through the production node reader, and pins the
//! contract that a stored extent is never read as a length in metres — the
//! one-tick verdict refuses to decide while the stored-vertex unit is
//! unmeasured and reports the factor at which it would flip.
//!
//! **F18-C** (`accept_f18_c_`) is the mission layer on top of that transaction:
//! `overlays` owns AC03 — a body reaching a trigger volume opens an authored
//! door, and both the drawn and the collided half of the door move, once — and
//! the records and refusals around it; `visibility` owns the streaming policy:
//! a sector a gameplay-required object is in is held whatever the focus does, and
//! a sector that went comes back with the condition and the applied overlay its
//! load already held.
//!
//! **#629** (`accept_m01_lc_world_import_`) is the first-mission path's missing
//! hop: it turns an **original** `gamez.zbd` into the `WorldDefinition`
//! `spawn_world` consumes, through the world record's own partition grid.
//! `import_retail` owns it — the sector index an object's membership comes from,
//! the collision role that follows the index, every other role named as an
//! explicit unknown, the refusals a grid that contradicts itself triggers, the
//! spawn over the imported record, and (retail) the measured counts of
//! `ZBD/C1C/gamez.zbd` with the real container's geometry uploaded.
//!
//! **#498** (`accept_f18_c_` too) is the swept crossing report itself:
//! `crossing` owns the ordinary-flight producer — a body whose tick completely
//! outruns a thin cuboid trigger, or lands a mesh-derived volume's deep-inside
//! gap, is still reported exactly once and the overlay behind it still fires,
//! a body that turns away inside a volume earns nothing beyond the entry the
//! pair was owed, and the report is a read that never touches the body.
//!
//! **#499** (`accept_t499_`) is the spawn-tick half of that report:
//! `sensor_layer` owns the collision layer a world-authored sensor volume
//! declares — a spawn whose first tick sweeps a trigger volume is neither
//! clamped by it nor stopped at it, and the crossing reaches
//! `TriggerCrossings` from `deliver_spawn_tick_crossings`, once per pair, with
//! both producers installed against the same volume. It is a module here
//! rather than a separate test binary so the suite links one Bevy image fewer.

mod audit;
mod common;
mod crossing;
mod hierarchy;
mod import;
mod import_retail;
mod overlays;
mod records;
mod residency;
mod scene_ids;
mod sensor_layer;
mod shared_asset;
mod shear;
mod spawn;
mod sweep;
mod trigger;
mod triggers;
mod visibility;
