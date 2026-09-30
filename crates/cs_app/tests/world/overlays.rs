//! F18-C: mission overlays — the records, the producer, the consumer and the
//! effect (acceptance scenario **AC03**).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-C`. Task test prefix: `accept_f18_c_`.
//!
//! These tests drive production code only. The overlay record is
//! `cs_content::world::MissionOverlay`, the world is `cs_app::world::depot_world`
//! (the same stored arch the harbor world uses, with a door panel that fills its
//! opening and a sensor volume in front of it), the load is
//! `cs_app::world::load_world`, the producer is Avian's `CollisionStart` stream as
//! `cs_app::world::queue_overlay_triggers` reads it, and the consumer is
//! `cs_app::world::apply_overlay` / `apply_overlay_requests`. No test carries its
//! own world builder, its own trigger or its own transform writer.
//!
//! What is pinned here:
//!
//! * **AC03**: a body reaching the trigger opens the door, the door's *drawn*
//!   transform and its *collided* transform both move by the authored offset, and
//!   it happens once. The control is the same world loaded without the overlay,
//!   where the same body is stopped by the panel — the only difference between
//!   the two flights is the mission's own record.
//! * **the producer is the contact stream**, and a sensor volume with no overlay
//!   behind it produces neither an application nor a refusal.
//! * **the refusals name themselves and change nothing**: no overlay, already
//!   applied, target streamed out, entity vanished — and a refused overlay is
//!   still *pending*, so the next request applies it.
//! * **no leftovers**: a world unloaded and loaded again has its door shut and
//!   its overlay unapplied, because the applied set belongs to the load.
//! * **the swept production body configuration**, which is what the F18-A review
//!   asked this stage to verify against task #401, with the measured interaction
//!   asserted rather than left out.

use avian3d::prelude::{Collider, Position, RigidBody};
use bevy::prelude::{App, GlobalTransform, Transform, Vec3, With};
use cs_app::world::AppliedOverlay;
use cs_app::world::{
    DEPOT_DOOR_HALF_M, DEPOT_DOOR_OPEN_OFFSET_M, DEPOT_OBJECT_CRATE, DEPOT_OBJECT_DOOR,
    DEPOT_OBJECT_GROUND, DEPOT_OBJECT_HANGAR, DEPOT_OBJECT_TRIGGER, DEPOT_SECTOR_YARD,
    MESH_SETTLE_UPDATES, OverlayError, OverlayOutcome, ProbeSpec, SpawnedWorld, WorldContacts,
    WorldObjectBinding, apply_overlay, depot_meshes, depot_mission, depot_world, load_sector,
    load_world, overlay_log, request_overlay, spawn_discrete_probe, spawn_swept_probe,
    unload_sector, unload_world, world_app,
};
use cs_content::world::{
    MissionOverlay, OverlayEffect, WorldDefinition, WorldError, WorldObjectId,
};

/// The speed the door flight is measured at, in m/s.
///
/// At the workspace's 120 Hz fixed rate a tick is a quarter of a meter, so the
/// 1 m trigger volume cannot be stepped over between two samples and the
/// crossing is a real overlap rather than a lucky one.
const PROBE_SPEED_M_S: f64 = 30.0;

/// Where a body starts on the tunnel's centreline, in meters.
const PROBE_START_X_M: f64 = -12.0;

/// The tunnel's centreline height, inside both the opening (`y < 3`) and the
/// trigger volume (`y < 3`).
const PROBE_Y_M: f64 = 1.5;

/// Ticks a body needs to cross the trigger, the panel's old place and the far
/// side of the hangar: 120 ticks is 30 m, from `x = -12` past `x = 18`.
const TICKS: u64 = 120;

/// The speed the swept measurement is taken at: 400 m/s is 3.33 m per tick, more
/// than the trigger volume is thick.
const SWEPT_SPEED_M_S: f64 = 400.0;

/// The door panel's own half extents, as the runtime's vector.
fn door_half_extents() -> Vec3 {
    Vec3::new(
        DEPOT_DOOR_HALF_M[0] as f32,
        DEPOT_DOOR_HALF_M[1] as f32,
        DEPOT_DOOR_HALF_M[2] as f32,
    )
}

/// The door overlay's displacement, as the runtime's vector.
fn open_offset() -> Vec3 {
    Vec3::new(
        DEPOT_DOOR_OPEN_OFFSET_M[0] as f32,
        DEPOT_DOOR_OPEN_OFFSET_M[1] as f32,
        DEPOT_DOOR_OPEN_OFFSET_M[2] as f32,
    )
}

/// The depot world, built by production code.
fn depot() -> WorldDefinition {
    depot_world().expect("the synthetic depot world is well formed")
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object key is valid")
}

fn sector(key: &str) -> cs_content::world::SectorId {
    cs_content::world::SectorId::new(key).expect("the fixture sector key is valid")
}

/// A body flying the tunnel's centreline at the measured speed.
fn probe_at(start_x_m: f64) -> ProbeSpec {
    probe_at_z(start_x_m, 0.0)
}

/// A body flying down `+x` at the measured speed, at an explicit `z`.
fn probe_at_z(start_x_m: f64, z: f64) -> ProbeSpec {
    ProbeSpec {
        position_m: [start_x_m, PROBE_Y_M, z],
        velocity_m_s: [PROBE_SPEED_M_S, 0.0, 0.0],
        half_extents_m: [0.25, 0.25, 0.25],
        mass_kg: 250.0,
    }
}

/// The headless world with the depot world loaded and its mesh collider derived.
fn loaded(open_door: bool) -> (App, SpawnedWorld) {
    let definition = depot();
    let instance = depot_mission(&definition, open_door, &[]).expect("a valid depot mission");
    let mut app = world_app();
    let report = load_world(&mut app, &definition, &instance, &depot_meshes())
        .expect("the depot world loads");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    (app, report)
}

/// The headless world with a mission record that also declares `required`.
fn loaded_with(required: &[&str]) -> (App, SpawnedWorld) {
    let definition = depot();
    let instance = depot_mission(&definition, true, required).expect("a valid depot mission");
    let mut app = world_app();
    let report = load_world(&mut app, &definition, &instance, &depot_meshes())
        .expect("the depot world loads");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }
    (app, report)
}

fn step(app: &mut App, ticks: u64) {
    for _ in 0..ticks {
        app.update();
    }
}

fn contacts(app: &App) -> Vec<String> {
    app.world()
        .resource::<WorldContacts>()
        .contacts()
        .iter()
        .map(|contact| contact.object.as_str().to_owned())
        .collect()
}

fn outcomes(app: &App) -> Vec<OverlayOutcome> {
    overlay_log(app.world())
        .expect("the overlay trace is installed by the composition")
        .outcomes()
        .to_vec()
}

fn applied_count(app: &App) -> usize {
    outcomes(app)
        .iter()
        .filter(|outcome| outcome.is_applied())
        .count()
}

/// The translations the door's render and collision halves are at.
///
/// Read from the entities themselves and not from the residency record: the
/// record says the overlay was applied, and this says where the geometry
/// actually is. A panel whose *drawn* half moved and whose *collided* half did
/// not is the failure this triple exists to catch, and it cannot be caught by
/// reading the record.
fn door_translations(app: &App, report: &SpawnedWorld) -> (Vec3, Vec3, Vec3) {
    let door = report
        .object(&object(DEPOT_OBJECT_DOOR))
        .expect("the door panel is in the report");
    let collider = door
        .collider
        .as_ref()
        .expect("a solid cuboid object has a collider");
    let drawn = app
        .world()
        .get::<Transform>(door.visual)
        .expect("the door's presentation entity exists")
        .translation;
    let global = app
        .world()
        .get::<GlobalTransform>(door.visual)
        .expect("the door's presentation entity exists")
        .translation();
    let collided = app
        .world()
        .get::<Position>(collider.entity)
        .expect("the door's collider entity exists")
        .0;
    (drawn, global, collided)
}

/// **AC03: an authored door opens, and both its render and its collision move —
/// once.**
///
/// The panel is a cuboid, so its drawn geometry and its collided geometry are
/// two entities ([`SpawnedObject::entities`]), and the test reads all three
/// transforms the object carries: its `Transform`, its `GlobalTransform` and the
/// Avian `Position` its collider hangs off. The displacement is the record's own
/// offset, not a distance the test chose, and the panel's *shape* is asserted
/// unchanged: a door that turned into a smaller box, or into a marker that draws
/// nothing, would also have "opened", and neither is a door.
///
/// "Once" is checked twice, because there are two ways to get it wrong: the
/// flight is stepped past the door afterwards and the geometry must not move
/// again, and a second application of the same trigger is refused by name.
///
/// The body flies the tunnel afterwards, which is the behavioural half: a
/// collision left where the panel was would leave the tunnel sealed and the body
/// stopped, with the panel named in the contact log.
///
/// Observable failure: the panel's draw moved and its collider did not, the body
/// was stopped by a door that is visibly open, the panel moved twice, or the
/// overlay never fired because the trigger was not the contact stream the
/// consumer reads.
#[test]
fn accept_f18_c_opening_an_authored_door_moves_its_render_and_its_collision_once() {
    let (mut app, report) = loaded(true);
    let (drawn_before, global_before, collided_before) = door_translations(&app, &report);
    assert_eq!(
        drawn_before.z, 0.0,
        "the panel starts shut, across the middle of the opening"
    );
    assert!(
        (drawn_before - global_before).length() < 1e-5
            && (drawn_before - collided_before).length() < 1e-5,
        "the two halves start together: drawn {drawn_before:?}, global {global_before:?}, \
         collided {collided_before:?}"
    );

    let body = spawn_discrete_probe(&mut app, &probe_at(PROBE_START_X_M)).expect("valid probe");

    // Step one update at a time and read the door in **the same update the
    // overlay was applied in**. That is the discriminating read: Avian's
    // `transform_to_position` sync copies a `Transform` into the collider's
    // `Position` at the *start of the next step*, so a consumer that moved only
    // the drawn half would still have the right `Position` one update later and
    // a test that read after a hundred ticks could not tell the two apart. One
    // pass updating both halves, or the two can drift for a tick, is what
    // "render and collision update together" has to mean.
    let mut same_pass: Option<(Vec3, Vec3, Vec3)> = None;
    for _ in 0..TICKS {
        let applied_before = applied_count(&app);
        app.update();
        if applied_before == 0 && applied_count(&app) == 1 {
            same_pass = Some(door_translations(&app, &report));
            break;
        }
    }
    let (drawn_after, global_after, collided_after) = same_pass.expect(
        "the trigger volume is 4 m ahead of the panel and the body covers 0.25 m \
         per tick, so the overlay applies well inside the flight",
    );
    let offset = open_offset();
    assert!(
        (drawn_after - (drawn_before + offset)).length() < 1e-5,
        "the drawn panel must move by the overlay's own offset, **in the pass \
         that applied it**: {drawn_before:?} -> {drawn_after:?}, offset {offset:?}"
    );
    assert!(
        (global_after - (global_before + offset)).length() < 1e-5,
        "the drawn panel's world transform must move with it: {global_before:?} -> \
         {global_after:?}"
    );
    assert!(
        (collided_after - (collided_before + offset)).length() < 1e-5,
        "the *collided* half must move by the same offset **in the same pass**, \
         which is the part a shared mesh reference does not guarantee and which \
         Avian's `transform_to_position` sync would otherwise only repair one \
         step later: {collided_before:?} -> {collided_after:?}"
    );

    // The rest of the flight, and the panel's pose at the end of it.
    step(&mut app, TICKS);
    let (drawn_after, global_after, collided_after) = door_translations(&app, &report);
    assert!(
        (drawn_after - (drawn_before + offset)).length() < 1e-5
            && (global_after - (global_before + offset)).length() < 1e-5
            && (collided_after - (collided_before + offset)).length() < 1e-5,
        "and it stays where the overlay put it: {drawn_after:?} / {global_after:?} / \
         {collided_after:?}"
    );
    // The shape is untouched: the door moved, it did not shrink or vanish.
    let door = report
        .object(&object(DEPOT_OBJECT_DOOR))
        .expect("the door panel is in the report");
    let collider_entity = door
        .collider
        .as_ref()
        .expect("a solid cuboid object has a collider")
        .entity;
    let half = app
        .world()
        .get::<Collider>(collider_entity)
        .expect("the panel still has its collider")
        .shape_scaled()
        .as_cuboid()
        .expect("the panel is a box")
        .half_extents;
    assert!(
        (Vec3::new(half.x, half.y, half.z) - door_half_extents()).length() < 1e-5,
        "the panel keeps the opening's own box: {half:?}"
    );

    // 2. The collision really opened: the body that reached the trigger went
    //    through the tunnel the panel used to shut.
    let end = app
        .world()
        .get::<Position>(body)
        .expect("the body still exists")
        .0;
    assert!(
        end.x > 1.0,
        "a body that opened the door must be able to fly the tunnel: it ended at \
         {end:?}, still west of the hangar's far face (x = 0.5)"
    );
    let log = contacts(&app);
    assert!(
        log.iter().any(|name| name == DEPOT_OBJECT_TRIGGER),
        "the trigger volume is what fired it: {log:?}"
    );
    assert!(
        !log.iter().any(|name| name == DEPOT_OBJECT_DOOR),
        "the open panel must not have been touched: {log:?}"
    );

    // 3. Once. More ticks, and a second request by name.
    step(&mut app, 20);
    let (drawn_later, _, collided_later) = door_translations(&app, &report);
    assert!(
        (drawn_later - drawn_after).length() < 1e-5
            && (collided_later - collided_after).length() < 1e-5,
        "the panel must not move again: {drawn_after:?} -> {drawn_later:?} and \
         {collided_after:?} -> {collided_later:?}"
    );
    assert_eq!(
        apply_overlay(app.world_mut(), &object(DEPOT_OBJECT_TRIGGER)),
        Err(OverlayError::AlreadyApplied {
            trigger: object(DEPOT_OBJECT_TRIGGER),
            target: object(DEPOT_OBJECT_DOOR),
        }),
        "a second application of the same trigger is refused by name"
    );
    assert_eq!(
        applied_count(&app),
        1,
        "the load applied the door overlay exactly once: {:?}",
        outcomes(&app)
    );
    let applied: Vec<AppliedOverlay> = outcomes(&app)
        .iter()
        .filter_map(|outcome| outcome.applied())
        .cloned()
        .collect();
    assert_eq!(applied.len(), 1, "one application, with its own report");
    assert_eq!(
        applied[0].offset_m, DEPOT_DOOR_OPEN_OFFSET_M,
        "and it applied the record's own offset"
    );
}

/// The control for AC03: **the same world, the same body, the same flight — and
/// a load that declares no overlay.**
///
/// Without the record the panel *is* the opening, so the body must be stopped by
/// it. This is what makes the door test discriminating: if the door test passed
/// for a reason that had nothing to do with the overlay, this one would pass too.
///
/// Observable failure: the body gets through a shut door, or nothing stops it.
#[test]
fn accept_f18_c_a_load_with_no_overlay_keeps_its_door_shut() {
    let (mut app, report) = loaded(false);
    let (drawn_before, _, collided_before) = door_translations(&app, &report);
    let body = spawn_discrete_probe(&mut app, &probe_at(PROBE_START_X_M)).expect("valid probe");
    step(&mut app, TICKS);

    let (drawn_after, _, collided_after) = door_translations(&app, &report);
    assert!(
        (drawn_after - drawn_before).length() < 1e-5
            && (collided_after - collided_before).length() < 1e-5,
        "a load that declares no overlay has no way to move the panel: \
         {drawn_before:?} -> {drawn_after:?}, {collided_before:?} -> {collided_after:?}"
    );
    let end = app
        .world()
        .get::<Position>(body)
        .expect("the body still exists")
        .0;
    assert!(
        end.x < 0.0,
        "the panel *is* the opening while it is shut, so a body must be stopped by \
         it: it ended at {end:?}"
    );
    let log = contacts(&app);
    assert!(
        log.iter().any(|name| name == DEPOT_OBJECT_DOOR),
        "and the stop is a contact with the panel itself: {log:?}"
    );
    assert!(
        outcomes(&app).is_empty(),
        "a sensor volume with no overlay behind it produces no outcome at all: {:?}",
        outcomes(&app)
    );
}

/// **The production body configuration: a swept body, and what it does at a
/// trigger volume — the check task #401 asked this stage for.**
///
/// F23's aircraft bodies carry [`SweptCcd`](avian3d::prelude::SweptCcd), and
/// F18-A measured that Avian's swept CCD stops a body at a *sensor* volume's near
/// face with no `Sensor` filter in the sweep
/// (`avian3d-0.7.0/src/dynamics/ccd/mod.rs`, `solve_swept_ccd`). Task #401 owns
/// that interaction. What this stage owes it is the overlay half, and this is the
/// measurement:
///
/// * the swept body **does** reach the volume closely enough for the narrow phase
///   to report the contact, so **the overlay fires for the production body**;
/// * the CCD **does** cost it travel: on the crossing tick the body is clamped at
///   the volume's near face instead of continuing. Measured here, 0.58 m of the
///   3.33 m it would otherwise have covered, with the loss being the distance from
///   its previous sample to the face — the same rule F18-A measured on an 8 m
///   volume, scaled to a 1 m one;
/// * the volume is **not** a wall: on the next tick the body is through it and
///   covers the full 3.33 m again.
///
/// The hold is asserted rather than left out. A stage that verified only "the
/// overlay fires" would leave a mission whose every trigger volume costs an
/// aircraft part of a second of travel looking like a finished feature.
///
/// The panel is 4 m ahead of the volume — 1.2 ticks at this speed — so a body
/// this fast still meets the *closed* panel on the tick after the volume and is
/// stopped by it before the effect lands. That is a property of where the fixture
/// put the volume, not of the overlay, and it is why the AC03 traversal
/// assertion uses a speed whose sampling cannot outrun the distance between a
/// trigger and the geometry it opens.
///
/// If #401's fix lands, the clamp assertion fails, and that failure is the signal
/// to re-measure and update the finding — not a bug in this stage.
#[test]
fn accept_f18_c_a_swept_body_fires_the_overlay_and_pays_for_the_sensor_face() {
    let (mut app, report) = loaded(true);
    let body = spawn_swept_probe(
        &mut app,
        &ProbeSpec {
            position_m: [PROBE_START_X_M, PROBE_Y_M, 0.0],
            velocity_m_s: [SWEPT_SPEED_M_S, 0.0, 0.0],
            half_extents_m: [0.25, 0.25, 0.25],
            mass_kg: 250.0,
        },
    )
    .expect("valid probe");

    // The body's per-tick travel across the volume, one step at a time: the
    // clamp is a single tick's worth of travel, so only the trace can show it.
    let dt = app
        .world()
        .resource::<bevy::time::Time<bevy::time::Fixed>>()
        .timestep()
        .as_secs_f32();
    let free = SWEPT_SPEED_M_S as f32 * dt;
    let mut positions: Vec<f32> = Vec::new();
    for _ in 0..6 {
        app.update();
        positions.push(
            app.world()
                .get::<Position>(body)
                .expect("the body still exists")
                .0
                .x,
        );
    }
    let face = cs_app::world::DEPOT_TRIGGER_POS_M[0] as f32
        - cs_app::world::DEPOT_TRIGGER_HALF_M[0] as f32
        - 0.25;
    let travel: Vec<f32> = positions.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(
        travel.iter().any(|step| *step < free - 0.5),
        "a swept body must be clamped at a sensor volume's near face on the pinned \
         pair (task #401): the free tick is {free} m and the body covered {travel:?} \
         from {positions:?}, with the face at x = {face}. If every tick is now \
         free, #401 has been resolved: re-measure and update \
         docs/findings/2026-09-30-f18-c-mission-overlays-and-visibility-streaming.md"
    );
    let clamped = positions[2];
    assert!(
        (clamped - face).abs() < 0.05,
        "and the clamp is at the volume's face, not somewhere else: {clamped} against \
         {face}"
    );
    assert!(
        (travel[2] - free).abs() < 0.05,
        "while the volume is not a wall: the next tick is free travel again ({})",
        travel[2]
    );

    // The overlay fired, and both halves moved, once.
    let log = contacts(&app);
    assert!(
        log.iter().any(|name| name == DEPOT_OBJECT_TRIGGER),
        "a swept body must still reach the trigger volume closely enough to fire \
         the overlay, which is what the F18-A review asked this stage to verify: \
         {log:?}"
    );
    assert_eq!(
        applied_count(&app),
        1,
        "and the consumer applied it once: {:?}",
        outcomes(&app)
    );
    let (_, _, collided) = door_translations(&app, &report);
    assert!(
        (collided.z - DEPOT_DOOR_OPEN_OFFSET_M[2] as f32).abs() < 1e-5,
        "the panel's collision moved, so the overlay reached both halves: {collided:?}"
    );
}

/// **Every refusal names what it refused, and none of them changed anything.**
///
/// The refusals are the ways an overlay can fail to apply, and each is checked
/// for what it left behind as well as for what it said:
///
/// * a trigger the load declares no overlay for ([`OverlayError::NoSuchOverlay`]);
/// * a trigger already applied ([`OverlayError::AlreadyApplied`]);
/// * a target whose sector is streamed out ([`OverlayError::TargetNotPresent`]) —
///   and because that one records nothing, the *same* overlay still applies once
///   the sector is back, which is the retry a trigger needs;
/// * a presented object that has lost an entity
///   ([`OverlayError::VanishedEntity`]) — despawned behind the record's back, so
///   nothing moved and the load's "already applied" claim stayed false.
///
/// Observable failure: a refusal that moved half the panel, or one that recorded
/// an application that never happened.
#[test]
fn accept_f18_c_every_overlay_refusal_names_what_it_refused_and_changed_nothing() {
    // A trigger with no overlay behind it.
    let (mut app, report) = loaded(false);
    let (before, _, collided_before) = door_translations(&app, &report);
    assert_eq!(
        apply_overlay(app.world_mut(), &object(DEPOT_OBJECT_TRIGGER)),
        Err(OverlayError::NoSuchOverlay {
            trigger: object(DEPOT_OBJECT_TRIGGER),
        }),
        "a load that declares no overlay for a volume says so"
    );
    let (after, _, collided_after) = door_translations(&app, &report);
    assert!(
        (after - before).length() < 1e-5 && (collided_after - collided_before).length() < 1e-5,
        "and moved nothing: {before:?} -> {after:?}"
    );

    // A target streamed out, and the retry once it is back.
    let (mut app, _report) = loaded(true);
    unload_sector(&mut app, &sector(DEPOT_SECTOR_YARD)).expect("the yard is loaded");
    assert_eq!(
        apply_overlay(app.world_mut(), &object(DEPOT_OBJECT_TRIGGER)),
        Err(OverlayError::TargetNotPresent {
            trigger: object(DEPOT_OBJECT_TRIGGER),
            target: object(DEPOT_OBJECT_DOOR),
        }),
        "an overlay whose target is streamed out is refused by name, and the \
         refusal says which object is missing"
    );
    assert!(
        cs_app::world::residency(app.world())
            .expect("a world is loaded")
            .resident()
            .applied_overlays()
            .is_empty(),
        "and it is still *pending*: a refused overlay must be retryable"
    );
    load_sector(&mut app, &sector(DEPOT_SECTOR_YARD), &depot_meshes())
        .expect("the yard sector reloads");
    assert!(
        apply_overlay(app.world_mut(), &object(DEPOT_OBJECT_TRIGGER)).is_ok(),
        "the retry applies the same overlay once the target is present again"
    );

    // A presented object that has lost an entity.
    let (mut app, report) = loaded(true);
    let door = report
        .object(&object(DEPOT_OBJECT_DOOR))
        .expect("the door panel is in the report");
    let visual = door.visual;
    app.world_mut().entity_mut(visual).despawn();
    assert!(
        matches!(
            apply_overlay(app.world_mut(), &object(DEPOT_OBJECT_TRIGGER)),
            Err(OverlayError::VanishedEntity { target, .. })
                if target == object(DEPOT_OBJECT_DOOR)
        ),
        "an object that lost an entity is refused by name rather than half-moved"
    );
    assert!(
        cs_app::world::residency(app.world())
            .expect("a world is loaded")
            .resident()
            .applied_overlays()
            .is_empty(),
        "and the load does not claim it applied an overlay it could not"
    );
}

/// **The load owns the applied set: a second load of the same world starts with
/// a shut door.**
///
/// F18 non-negotiable behavior 5, applied to an overlay. The applied set is part
/// of the residency record, so [`unload_world`] takes it with everything else
/// and the next load has nothing to inherit: its door is shut, and firing the
/// same trigger again works.
#[test]
fn accept_f18_c_the_applied_overlay_is_the_loads_own_and_a_new_load_starts_shut() {
    let (app, _report) = loaded(true);
    let mut app = app;
    unload_world(&mut app).expect("the world unloads");
    let definition = depot();
    let instance = depot_mission(&definition, true, &[]).expect("a valid depot mission");
    let report = load_world(&mut app, &definition, &instance, &depot_meshes())
        .expect("the depot world loads again");
    step(&mut app, MESH_SETTLE_UPDATES);

    let (drawn, _, collided) = door_translations(&app, &report);
    assert!(
        drawn.z.abs() < 1e-5 && collided.z.abs() < 1e-5,
        "the second load's panel is shut, exactly as the record authored it: \
         drawn {drawn:?}, collided {collided:?}"
    );
    assert!(
        cs_app::world::residency(app.world())
            .expect("a world is loaded")
            .resident()
            .applied_overlays()
            .is_empty(),
        "and the new load has applied nothing: the previous run's overlays died \
         with its record"
    );
    assert!(
        apply_overlay(app.world_mut(), &object(DEPOT_OBJECT_TRIGGER)).is_ok(),
        "so the same trigger fires again in the new mission"
    );
    assert!(
        cs_app::world::residency(app.world())
            .expect("a world is loaded")
            .resident()
            .applied_overlays()
            .contains(&object(DEPOT_OBJECT_TRIGGER)),
        "and the new load records its own application"
    );
}

/// **A mission's own request reaches the same consumer as the contact stream.**
///
/// Two producers, one consumer: Avian's contact stream (which is what the AC03
/// test flies) and a mission's own request — a script branch that decides the
/// door opens without anything flying into the volume. Both go through
/// [`request_overlay`]'s hand-off, so there is one place that applies an effect
/// and one trace that says what it did.
///
/// The test also pins what a mission *sees* when a body reaches ordinary solid
/// world geometry: no overlay is applied and nothing is traced, because a solid
/// object is not a trigger and the load declares no overlay behind one.
#[test]
fn accept_f18_c_a_missions_own_request_reaches_the_same_consumer() {
    let (mut app, report) = loaded(true);
    assert!(
        outcomes(&app).is_empty(),
        "a load with no contact and no request has produced no outcome: {:?}",
        outcomes(&app)
    );
    request_overlay(&mut app, object(DEPOT_OBJECT_TRIGGER));
    step(&mut app, 1);
    assert_eq!(
        applied_count(&app),
        1,
        "the consumer applied the mission's request: {:?}",
        outcomes(&app)
    );
    let (_, _, collided) = door_translations(&app, &report);
    assert!(
        (collided.z - DEPOT_DOOR_OPEN_OFFSET_M[2] as f32).abs() < 1e-5,
        "and the panel's collision moved: {collided:?}"
    );

    // A body that hits **solid** world geometry is not a trigger. The hangar's
    // own leg is solid and is in the way of a body flying at `z = 1.5`, one
    // metre beside the tunnel's centreline, so this is a real contact with a
    // real world object.
    //
    // What it does **not** pin is the producer's own `role != Sensor` filter,
    // and the assertion must not pretend otherwise: the consumer independently
    // requires the load to *declare* an overlay for whatever it is handed, and a
    // load that declared one behind a solid trigger is refused at the door
    // (`WorldError::OverlayTriggerNotASensor`), so no reachable load lets the
    // two halves disagree — a producer that forwarded this contact would have its
    // request skipped for want of a declared overlay, and the count below would
    // be the same. The filter is the record's own claim about whether a body can
    // *enter* a volume, the consumer's filter is a different question, and both
    // are stated in `overlays::queue_overlay_triggers`; what is pinned here is
    // the outcome a mission sees, not which of the two checks produced it.
    spawn_discrete_probe(&mut app, &probe_at_z(PROBE_START_X_M, 1.5)).expect("valid probe");
    step(&mut app, TICKS);
    let log = contacts(&app);
    assert!(
        log.iter().any(|name| name == DEPOT_OBJECT_HANGAR),
        "the body reached the hangar's leg, so the solid contact really happened: \
         {log:?}"
    );
    assert_eq!(
        applied_count(&app),
        1,
        "and hitting a solid world object applies no overlay: {:?}",
        outcomes(&app)
    );
}

/// **The records refuse what the runtime could not honour.**
///
/// Every check is one [`WorldInstance::validate_against`] or
/// [`MissionOverlay::try_new`] refusal, driven through the same production path a
/// load uses. They matter because each is a load that would otherwise look
/// configured while doing nothing:
///
/// * a trigger that is a **solid** object is a wall a body is stopped by and
///   never enters, so its overlay could never fire;
/// * a trigger whose role is an **explicit unknown** cannot be established to be
///   enterable at all — a content gap, reported as its own fact rather than
///   treated as a sensor;
/// * a required object the load's population never activates names a state
///   nothing could show;
/// * two overlays sharing one trigger would make the load's declaration order the
///   whole behaviour;
/// * a non-finite offset is not a displacement.
///
/// The last assertion is the positive control: the depot's own door overlay
/// validates, so the refusals above are about the records and not the fixture.
#[test]
fn accept_f18_c_every_overlay_record_refusal_names_what_it_refused() {
    let depot_definition = depot();
    let depot_population: Vec<&str> = cs_app::world::depot_population();
    let displace = |trigger: &str, target: &str| {
        MissionOverlay::try_new(
            object(trigger),
            OverlayEffect::Displace {
                target: object(target),
                offset_m: DEPOT_DOOR_OPEN_OFFSET_M,
            },
            cs_app::world::fixture_provenance("test.overlay"),
        )
        .expect("a finite offset")
    };
    let depot_load = |overlays: Vec<MissionOverlay>, required: &[&str]| {
        cs_app::world::world_instance(&depot_definition, None, &depot_population, &[])
            .expect("a valid fixture load record")
            .with_mission_layer(overlays, cs_app::world::object_set(required))
            .expect("no duplicate triggers and no non-finite offsets")
    };

    // A solid trigger: the hangar shell is solid, so its overlay could never fire.
    assert_eq!(
        depot_load(vec![displace(DEPOT_OBJECT_HANGAR, DEPOT_OBJECT_DOOR)], &[])
            .validate_against(&depot_definition),
        Err(WorldError::OverlayTriggerNotASensor {
            object: object(DEPOT_OBJECT_HANGAR),
        }),
        "an overlay whose trigger is a solid wall is refused by name"
    );
    // A required object outside the population.
    let inactive = cs_app::world::world_instance(
        &depot_definition,
        None,
        &depot_population[..depot_population.len() - 1],
        &[],
    )
    .expect("a valid fixture load record")
    .with_mission_layer(
        vec![displace(DEPOT_OBJECT_TRIGGER, DEPOT_OBJECT_DOOR)],
        cs_app::world::object_set(&[DEPOT_OBJECT_CRATE]),
    )
    .expect("no duplicate triggers");
    assert_eq!(
        inactive.validate_against(&depot_definition),
        Err(WorldError::InactiveLoadObject {
            object: object(DEPOT_OBJECT_CRATE),
            context: "the required object set",
        }),
        "declaring an object gameplay requires, in a population that never \
         activates it, is refused"
    );
    // Two overlays on one trigger.
    assert_eq!(
        cs_app::world::world_instance(&depot_definition, None, &depot_population, &[])
            .expect("a valid fixture load record")
            .with_mission_layer(
                vec![
                    displace(DEPOT_OBJECT_TRIGGER, DEPOT_OBJECT_DOOR),
                    displace(DEPOT_OBJECT_TRIGGER, DEPOT_OBJECT_HANGAR),
                ],
                cs_app::world::object_set(&[]),
            ),
        Err(WorldError::DuplicateOverlayTrigger {
            object: object(DEPOT_OBJECT_TRIGGER),
        }),
        "two overlays sharing a trigger are refused"
    );
    // A non-finite offset.
    assert_eq!(
        MissionOverlay::try_new(
            object(DEPOT_OBJECT_TRIGGER),
            OverlayEffect::Displace {
                target: object(DEPOT_OBJECT_DOOR),
                offset_m: [0.0, f64::NAN, 0.0],
            },
            cs_app::world::fixture_provenance("test.overlay"),
        ),
        Err(WorldError::NonFiniteOverlayOffset {
            object: object(DEPOT_OBJECT_DOOR),
            axis: 1,
        }),
        "a displacement that is not a displacement is refused, naming the axis"
    );

    // An unresolved trigger role, on the record that has one: the arch world's
    // `sign.unevidenced_role`.
    let arch = cs_app::world::arch_world().expect("the arch world is well formed");
    let arch_overlay = MissionOverlay::try_new(
        object(cs_app::world::OBJECT_UNEVIDENCED_ROLE),
        OverlayEffect::Displace {
            target: object(cs_app::world::OBJECT_LEG_RIGHT),
            offset_m: DEPOT_DOOR_OPEN_OFFSET_M,
        },
        cs_app::world::fixture_provenance("test.overlay"),
    )
    .expect("a finite offset");
    let arch_load = cs_app::world::world_instance(
        &arch,
        None,
        &[
            cs_app::world::OBJECT_LEG_LEFT,
            cs_app::world::OBJECT_LEG_RIGHT,
            cs_app::world::OBJECT_LINTEL,
            cs_app::world::OBJECT_GROUND,
            cs_app::world::OBJECT_WATER,
            cs_app::world::OBJECT_NON_COLLIDING,
            cs_app::world::OBJECT_SENSOR,
            cs_app::world::OBJECT_UNEVIDENCED_ROLE,
            cs_app::world::OBJECT_UNEVIDENCED_SHAPE,
        ],
        &[],
    )
    .expect("a valid fixture load record")
    .with_mission_layer(vec![arch_overlay], cs_app::world::object_set(&[]))
    .expect("no duplicate triggers");
    assert_eq!(
        arch_load.validate_against(&arch),
        Err(WorldError::OverlayTriggerRoleUnknown {
            object: object(cs_app::world::OBJECT_UNEVIDENCED_ROLE),
        }),
        "a trigger whose role was never resolved is refused as unknown, not \
         treated as a sensor"
    );

    // The positive control: the depot's own overlay and required set validate.
    let good = depot_load(
        vec![displace(DEPOT_OBJECT_TRIGGER, DEPOT_OBJECT_DOOR)],
        &[DEPOT_OBJECT_DOOR],
    );
    assert_eq!(
        good.validate_against(&depot_definition),
        Ok(()),
        "the fixture's own overlay validates, so the refusals above are about the \
         records and not about the fixture"
    );
    assert_eq!(
        good.required_objects().len(),
        1,
        "and the required set is the load's own"
    );
    assert_eq!(
        good.overlay_for(&object(DEPOT_OBJECT_TRIGGER))
            .map(|overlay| overlay.effect().target().clone()),
        Some(object(DEPOT_OBJECT_DOOR)),
        "and the overlay is keyed by its trigger"
    );
    let _ = object(DEPOT_OBJECT_GROUND);
}

/// The load record's mission layer is **additive**: a record that declares no
/// overlays and no required objects is not a degraded load, it is a world that
/// never changes and never streams.
///
/// F18-A's and F18-B's fixtures are exactly that, and they keep validating. This
/// is the regression that would bite the previous stages if the mission layer
/// were required.
#[test]
fn accept_f18_c_a_load_that_declares_no_mission_layer_is_still_a_valid_load() {
    let arch = cs_app::world::arch_world().expect("the arch world is well formed");
    let harbor = cs_app::world::harbor_world().expect("the harbor world is well formed");
    let depot_definition = depot();
    let cases: Vec<(&WorldDefinition, Vec<&str>)> = vec![
        (
            &arch,
            vec![
                cs_app::world::OBJECT_LEG_LEFT,
                cs_app::world::OBJECT_LEG_RIGHT,
                cs_app::world::OBJECT_LINTEL,
                cs_app::world::OBJECT_GROUND,
                cs_app::world::OBJECT_WATER,
                cs_app::world::OBJECT_NON_COLLIDING,
                cs_app::world::OBJECT_SENSOR,
                cs_app::world::OBJECT_UNEVIDENCED_ROLE,
                cs_app::world::OBJECT_UNEVIDENCED_SHAPE,
            ],
        ),
        (
            &harbor,
            vec![
                cs_app::world::HARBOR_OBJECT_HANGAR,
                cs_app::world::HARBOR_OBJECT_SENSOR,
                cs_app::world::HARBOR_OBJECT_BANNER,
                cs_app::world::HARBOR_OBJECT_WATER,
                cs_app::world::HARBOR_OBJECT_GROUND,
                cs_app::world::HARBOR_OBJECT_ABSENT,
            ],
        ),
        (&depot_definition, cs_app::world::depot_population()),
    ];
    for (definition, population) in cases {
        let instance = cs_app::world::world_instance(definition, None, &population, &[])
            .expect("a valid fixture load record");
        assert_eq!(
            instance.validate_against(definition),
            Ok(()),
            "`{}` loads with no mission layer, as it did before F18-C",
            definition.id()
        );
        assert!(
            instance.overlays().is_empty() && instance.required_objects().is_empty(),
            "and it declares neither an overlay nor a required object"
        );
    }
}

/// Every entity a moved object owns still names it, so a query that starts
/// anywhere — the presentation marker, the collider, the body — resolves to the
/// same authored object. The overlay displaces the list the *report* gives, and
/// the binding is what makes that list checkable from the world afterwards.
#[test]
fn accept_f18_c_every_entity_of_a_moved_object_still_names_it() {
    let (mut app, report) = loaded(true);
    let door = object(DEPOT_OBJECT_DOOR);
    let spawned = report
        .object(&door)
        .expect("the door panel is in the report");
    assert!(
        spawned.entities().len() >= 2,
        "the panel is a cuboid, so its render and its collision are two entities: \
         {:?}",
        spawned.entities()
    );
    apply_overlay(app.world_mut(), &object(DEPOT_OBJECT_TRIGGER)).expect("the door opens");
    for entity in spawned.entities() {
        assert_eq!(
            app.world()
                .get::<WorldObjectBinding>(entity)
                .map(|binding| binding.object().clone()),
            Some(door.clone()),
            "entity {entity:?} still names the object the overlay moved"
        );
    }
    let mut query = app
        .world_mut()
        .query_filtered::<bevy::prelude::Entity, With<RigidBody>>();
    let bodies = query.iter(app.world()).count();
    assert!(
        bodies > 0,
        "and the world's static bodies are all still there: {bodies}"
    );
    let _ = loaded_with(&[]);
}
