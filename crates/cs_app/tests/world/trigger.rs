//! Task #401: a world trigger volume and Avian's swept CCD.
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stages `### F18-B` (world import and static collision generation) and
//! `### F18-C` (mission overlays). Task test prefix: `accept_f18_b_`.
//!
//! F18-A measured that Avian's swept CCD stops a body at the first time of
//! impact against **any** collider its swept path reaches, `Sensor` included,
//! with no filter for the role: a 400 m/s probe crossing the `trigger.sensor`
//! volume lost 2.416 m of the crossing tick — exactly the distance from its
//! previous sample to the volume's near face. The role's own contract says the
//! opposite: `cs_sim::collision::classify_contact` makes every sensor pair a
//! `ContactKind::SensorOverlap` and never a `SolidContact`, so on the pinned
//! engine the physics contradicted a contract this workspace already declares.
//!
//! The decision (`crates/cs_app/src/world/spawn.rs`; recorded in
//! `docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md`) is that a
//! `WorldCollisionRole::Sensor` object is spawned on an entity with **no** rigid
//! body. It is the only layout on the pinned engine that stops the sweep
//! without silencing the report, and these tests are the measurement of it:
//!
//! * `accept_f18_b_a_swept_body_crosses_a_world_trigger_volume_untouched` — the
//!   decision itself, on the volume F18-A measured it on: free travel on every
//!   tick, unchanged velocity, and the volume **still reported once**.
//! * `accept_f18_b_a_mesh_trigger_volume_reports_a_swept_body_where_a_sample_lands`
//!   — the same on the import path, plus the boundary of what a mesh volume can
//!   report. The far half of that test is a **known limitation**, not a claim.
//! * `accept_f18_b_a_body_bearing_trigger_volume_holds_a_swept_body_in_four_ticks`
//!   — the layout that was rejected, measured through the same production spawn:
//!   one rigid body on the trigger costs 14.25 m of a 400 m/s flight and a second
//!   report, which is why it is not the layout.
//! * `accept_f18_b_the_layout_is_the_recorded_role_and_nothing_else` — the
//!   decision comes from the record's role and from nothing else.
//! * `accept_f18_b_the_trigger_and_solid_mesh_paths_differ_only_in_the_body` —
//!   the mesh path, where the trigger bundle is written separately from the
//!   collider-on-body one, so nothing but the body and the marker may differ.
//! * `accept_f18_b_the_swept_visible_body_audit_still_holds` — #424's enforced
//!   invariant is unaffected: a trigger volume is not a rigid body, so it neither
//!   violates the collider-on-body rule nor has to declare itself swept-invisible.
//!
//! No original data and no `CS_GAME_DIR` access: both fixtures are authored
//! development content, and every statement here is about the pinned engine and
//! about what this stage does, never about the 2000 PC original.

use avian3d::prelude::{
    Collider, ColliderOf, CollisionLayers as AvianCollisionLayers, Position, RigidBody, Sensor,
};
use bevy::asset::Assets;
use bevy::mesh::{Mesh, Mesh3d};
use bevy::prelude::{Entity, Transform, Vec3, World};
use bevy::time::{Fixed, Time};
use cs_app::world::{
    HARBOR_HANGAR_TRIANGLES, HARBOR_OBJECT_HANGAR, HARBOR_OBJECT_SENSOR, HARBOR_SENSOR_HALF_M,
    HARBOR_SENSOR_POS_M, MESH_SETTLE_UPDATES, OBJECT_LEG_RIGHT, OBJECT_SENSOR, ProbeSpec,
    SENSOR_HALF_M, SENSOR_POS_M, SpawnedWorld, WorldFixture, arch_world, harbor_meshes,
    harbor_world, static_world_layers,
};
use cs_app::world::{WorldColliderInstance, WorldContact, WorldObjectBinding};
use cs_content::world::{WorldCollisionRole, WorldObjectId};

use crate::common;

/// The speed at which one tick outruns the geometry these tests are about, in
/// m/s: 3.33 m per tick at the workspace's 120 Hz fixed rate.
const TUNNELLING_SPEED_M_S: f64 = 400.0;

/// The speed at which a tick is a quarter of a trigger volume's thickness, in
/// m/s: 0.25 m per tick.
const CROSSING_SPEED_M_S: f64 = 30.0;

/// Where a body starts on the flight axis in meters, and how long each flight
/// is. Both fixtures are entered from the same side, so the two speeds below
/// are measured on the same line through the same volumes.
const START_X_M: f64 = -28.5;

/// Ticks at [`TUNNELLING_SPEED_M_S`]: 50 m, past the arch world's volume and
/// well past the harbor world's.
const TUNNELLING_TICKS: u64 = 15;

/// Ticks at [`CROSSING_SPEED_M_S`]: 37.5 m, which carries a body from
/// [`START_X_M`] to `x = 9.0`, inside the harbor volume at `x ∈ [6, 14]`.
const CROSSING_TICKS: u64 = 150;

/// The flight height every volume in these fixtures sits inside.
const FLIGHT_Y_M: f64 = 1.5;

/// A probe spec flying `+x` at `z` from [`START_X_M`], at the production probe's
/// own size and mass.
fn flight(z: f64, speed_m_s: f64) -> ProbeSpec {
    ProbeSpec {
        position_m: [START_X_M, FLIGHT_Y_M, z],
        velocity_m_s: [speed_m_s, 0.0, 0.0],
        half_extents_m: [
            common::PROBE_HALF_M,
            common::PROBE_HALF_M,
            common::PROBE_HALF_M,
        ],
        mass_kg: 250.0,
    }
}

/// The arch world — the F18-A cuboid records, including `trigger.sensor` — with
/// a swept probe flying through that volume.
fn arch_swept(speed_m_s: f64) -> (WorldFixture, Entity) {
    let fixture =
        WorldFixture::builder(arch_world().expect("the synthetic arch world is well formed"))
            .probe(flight(SENSOR_POS_M[2], speed_m_s))
            .build()
            .expect("the arch fixture builds");
    let probe = fixture.probe().expect("the builder spawned the probe");
    (fixture, probe)
}

/// The harbor world — the F18-B mesh-authored records, including the mesh
/// `trigger.sensor` — with a swept probe flying through that volume.
///
/// The probe is spawned *after* the mesh colliders are derived
/// ([`MESH_SETTLE_UPDATES`]), because a probe spawned at build time would already
/// be through the geometry before the collision it is meant to meet exists.
fn harbor_swept(speed_m_s: f64) -> (WorldFixture, Entity) {
    let mut fixture =
        WorldFixture::builder(harbor_world().expect("the harbor world is well formed"))
            .meshes(harbor_meshes())
            .build()
            .expect("the harbor fixture builds");
    fixture.step(MESH_SETTLE_UPDATES);
    let probe = fixture
        .spawn_swept_probe(flight(HARBOR_SENSOR_POS_M[2], speed_m_s))
        .expect("the probe spec is valid");
    (fixture, probe)
}

/// Mutable access to the fixture's Bevy world, for the queries that take one
/// (the swept-invisible audit) and for the component reads after a step.
fn world_mut(fixture: &mut WorldFixture) -> &mut World {
    fixture.app_mut().world_mut()
}

/// One body's position, read from the engine.
fn position(world: &World, entity: Entity) -> Vec3 {
    world
        .get::<Position>(entity)
        .expect("the body still exists")
        .0
}

/// The fixed tick, in seconds, as the runtime has it.
fn tick_seconds(world: &World) -> f32 {
    world.resource::<Time<Fixed>>().timestep().as_secs_f32()
}

/// The position a body that nothing touched would reach after `ticks` steps.
fn free_flight(start: Vec3, speed_m_s: f64, ticks: u64, dt: f32) -> Vec3 {
    start + Vec3::X * (speed_m_s as f32 * ticks as f32 * dt)
}

/// Flies `probe` for `ticks` fixed steps, returning its position after each of
/// them.
fn fly(fixture: &mut WorldFixture, probe: Entity, ticks: u64) -> Vec<Vec3> {
    (0..ticks)
        .map(|_| {
            fixture.step(1);
            fixture
                .world()
                .get::<Position>(probe)
                .expect("the probe still exists")
                .0
        })
        .collect()
}

/// The contacts recorded for one object, by authored key.
fn contacts_for<'a>(fixture: &'a WorldFixture, key: &str) -> Vec<&'a WorldContact> {
    let wanted = object(key);
    fixture
        .contacts()
        .iter()
        .filter(|contact| contact.object == wanted)
        .collect()
}

/// An authored object key as the record's own id.
fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object key is valid")
}

/// The one object's collider entity, from a built world's own report.
fn collider_of(report: &SpawnedWorld, key: &str) -> Entity {
    let id = object(key);
    report
        .collider_for(&id)
        .unwrap_or_else(|| panic!("{key} carries a collider in this world"))
}

/// **The decision, measured on the volume F18-A measured it on:** a swept body
/// crosses a world trigger volume at full speed, with no tick held at its face,
/// and the volume is still reported exactly once.
///
/// Both preconditions are asserted rather than assumed. A tick of travel
/// (3.33 m) is far smaller than the volume is long (8 m along the flight axis),
/// so a discrete sample lands inside it and the report is a real overlap rather
/// than a lucky one; and it is larger than the volume is thick plus the probe,
/// so nothing here can be explained by the body never reaching the volume.
///
/// Observable failure: `spawn_object` puts a rigid body back on a sensor (or the
/// Avian `Sensor` marker is dropped, or the report is filtered away), and the
/// trace loses a tick's worth of travel at the volume's near face — F18-A's
/// measurement, 2.416 m of the crossing tick.
#[test]
fn accept_f18_b_a_swept_body_crosses_a_world_trigger_volume_untouched() {
    let (mut fixture, probe) = arch_swept(TUNNELLING_SPEED_M_S);
    let start = fixture.probe_position().expect("the probe was spawned");
    let dt = tick_seconds(fixture.world());
    let free = TUNNELLING_SPEED_M_S as f32 * dt;
    let volume_len = 2.0 * SENSOR_HALF_M[0] as f32;
    assert!(
        volume_len > free,
        "the volume must be longer than a tick ({free} m against {volume_len} m), or \
         this test measures a body no sample of which got inside it"
    );
    assert!(
        free > 2.0 * SENSOR_HALF_M[2] as f32 + 2.0 * common::PROBE_HALF_M as f32,
        "and the tick must outrun the volume's thickness plus the probe, or the \
         discrete path alone would have caught the crossing and the sweep's part in \
         it would not be measured"
    );

    let positions = fly(&mut fixture, probe, TUNNELLING_TICKS);

    let end = fixture.probe_position().expect("the probe still exists");
    let expected = free_flight(start, TUNNELLING_SPEED_M_S, TUNNELLING_TICKS, dt);
    let drift = (end - expected).length();
    assert!(
        drift < 0.01,
        "a trigger volume must not hold a swept body: the probe drifted {drift} m and \
         ended at {end:?} instead of {expected:?} from {positions:?}. A tick shorter \
         than the rest is the clamp at the volume's near face, and means a rigid \
         body is back on the sensor (task #401)"
    );
    let velocity = fixture.probe_velocity().expect("the probe still exists");
    assert!(
        (velocity.x - TUNNELLING_SPEED_M_S as f32).abs() < 1.0,
        "and it must not slow the body either, velocity is {velocity:?}"
    );
    assert!(
        end.x > SENSOR_POS_M[0] as f32 + SENSOR_HALF_M[0] as f32,
        "the body must have crossed the whole volume, it ended at {end:?}"
    );

    let reported = contacts_for(&fixture, OBJECT_SENSOR);
    assert_eq!(
        reported.len(),
        1,
        "the crossing must be reported exactly once — not zero times, and not twice: \
         {:?}",
        fixture
            .contacts()
            .iter()
            .map(|contact| contact.object.as_str().to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        reported[0].role,
        WorldCollisionRole::Sensor,
        "and the report must carry the role the record declared"
    );
    assert_eq!(reported[0].other, probe, "naming the body that reached it");
}

/// **The import path, and the boundary of what a mesh trigger volume can
/// report.** Two measurements of the same production spawn at two speeds:
///
/// * [`CROSSING_SPEED_M_S`] — 0.25 m per tick, a quarter of the volume's
///   thickness: the swept body crosses free **and** the mesh volume reports it,
///   once. A mesh volume is a working trigger volume, not only a cuboid one.
/// * [`TUNNELLING_SPEED_M_S`] — 3.33 m per tick, 1.7× the volume's thickness:
///   the swept body crosses free (the decision, on the mesh path too) and the
///   volume reports **nothing**.
///
/// That second half is a **known limitation of the pinned engine**, not a
/// property of the role and not a claim about the original. Avian's narrow phase
/// asks parry for a manifold within `max_contact_distance`
/// (`collision/narrow_phase/system_param.rs`), which is `dt · |v₂ − v₁|` after
/// each side's velocity is clamped to its own `SpeculativeMargin / dt`; a swept
/// body carries `SpeculativeMargin::ZERO`, so that distance collapses to the
/// contact tolerance, and a margin on the *volume* cannot widen it (measured:
/// `SpeculativeMargin(4.0)` on the trigger still reports nothing). A triangle
/// mesh only reports contacts near a triangle, so a body that lands deep inside
/// a mesh volume with no triangle within reach is never reported. A cuboid pair
/// has no such gap — parry's EPA produces a deep-penetration contact, which is
/// why the volume in the test above is reported at 400 m/s.
///
/// Affected content: any retail trigger or objective volume whose collision is
/// mesh-derived and whose thickness is under one tick of travel at the reaching
/// body's speed. The body no longer being held is not the cause — a body parked
/// inside such a volume was never reported either; the clamp only *looked* like
/// a report because it forced the body onto the surface. Resolving task: the
/// swept crossing report, which is F39's trigger semantics.
#[test]
fn accept_f18_b_a_mesh_trigger_volume_reports_a_swept_body_where_a_sample_lands() {
    // The crossing a discrete sample can see.
    let (mut crossing, probe) = harbor_swept(CROSSING_SPEED_M_S);
    let start = position(crossing.world(), probe);
    let dt = tick_seconds(crossing.world());
    let free = CROSSING_SPEED_M_S as f32 * dt;
    let thickness = 2.0 * HARBOR_SENSOR_HALF_M[2] as f32;
    assert!(
        free * 2.0 < thickness,
        "a tick of {free} m must be well under the volume's {thickness} m thickness, \
         or the slow arm measures the same thing as the fast one"
    );
    let positions = fly(&mut crossing, probe, CROSSING_TICKS);
    let end = position(crossing.world(), probe);
    let expected = free_flight(start, CROSSING_SPEED_M_S, CROSSING_TICKS, dt);
    assert!(
        (end - expected).length() < 0.01,
        "a mesh trigger volume must not hold a swept body either: {end:?} instead of \
         {expected:?} from {positions:?}"
    );
    assert_eq!(
        contacts_for(&crossing, HARBOR_OBJECT_SENSOR).len(),
        1,
        "a body that overlaps the volume must be reported by it, once: {:?}",
        crossing
            .contacts()
            .iter()
            .map(|contact| contact.object.as_str().to_owned())
            .collect::<Vec<_>>()
    );

    // The crossing no discrete sample can see, and the two stated together so
    // neither can be read as the other.
    let (mut tunnelling, probe) = harbor_swept(TUNNELLING_SPEED_M_S);
    let start = position(tunnelling.world(), probe);
    let dt = tick_seconds(tunnelling.world());
    let free = TUNNELLING_SPEED_M_S as f32 * dt;
    assert!(
        free > thickness + 2.0 * common::PROBE_HALF_M as f32,
        "this arm measures anything only while a tick outruns the volume's thickness \
         plus the probe, and the tick is {free} m against {thickness} m"
    );
    let positions = fly(&mut tunnelling, probe, TUNNELLING_TICKS);
    let end = position(tunnelling.world(), probe);
    let expected = free_flight(start, TUNNELLING_SPEED_M_S, TUNNELLING_TICKS, dt);
    assert!(
        (end - expected).length() < 0.01,
        "a body that outruns the volume's thickness must still cross it: {end:?} \
         instead of {expected:?} from {positions:?}"
    );
    assert!(
        contacts_for(&tunnelling, HARBOR_OBJECT_SENSOR).is_empty(),
        "and nothing is reported of it, because no sample lands near one of its \
         triangles. That is the measured limitation, not a claim that a mesh trigger \
         volume is silent — the slow arm above reports this same volume at this same \
         pose. A report appearing here would mean the narrow phase finds something \
         the source does not account for, and the finding has to be re-measured before \
         it is believed"
    );
}

/// **The layout that was rejected, measured through the same production spawn.**
///
/// One `RigidBody::Static` on the trigger volume — all it takes to get the old
/// behaviour back — and a 400 m/s body flying the mesh volume loses 11.08 m over
/// fifteen ticks, four of which it is held in: the swept clamp truncates the
/// crossing tick, and the discrete phase then walks the body along the volume's
/// surface (the same creep task #420 measured against a wall).
///
/// What the clamp buys is stated rather than left out: at this speed the crossing
/// **is** reported, because holding the body at the surface is what gives the
/// narrow phase something to see. That is the trade this task made, measured on
/// both sides — the neighbouring test records what a body-less volume does not
/// report — and the flight it costs is the reason the trade was taken.
///
/// The test exists so the decision stays a decision, and it is also the
/// re-measure signal: when a future avian release teaches the swept CCD to skip
/// sensors, this arm fails, and the finding is re-read — at that point the body
/// could go back on the trigger and this test be inverted.
#[test]
fn accept_f18_b_a_body_bearing_trigger_volume_holds_a_swept_body_in_four_ticks() {
    let (mut fixture, probe) = harbor_swept(TUNNELLING_SPEED_M_S);
    let report = fixture.spawned().clone();
    let trigger = collider_of(&report, HARBOR_OBJECT_SENSOR);
    assert_eq!(
        fixture.world().get::<RigidBody>(trigger),
        None,
        "the production spawn gives a trigger volume no rigid body; this test measures \
         the layout that is *rejected*, so it puts one back"
    );
    world_mut(&mut fixture)
        .entity_mut(trigger)
        .insert(RigidBody::Static);
    // Avian binds a collider to its body through an observer, so let the world run
    // before the body is flown at it.
    fixture.step(1);
    assert_eq!(
        fixture.world().get::<ColliderOf>(trigger).map(|of| of.body),
        Some(trigger),
        "the added body must be the one the collider is bound to, or the swept CCD \
         still cannot see it and this test measures nothing"
    );

    let start = position(fixture.world(), probe);
    let dt = tick_seconds(fixture.world());
    let free = TUNNELLING_SPEED_M_S as f32 * dt;
    assert!(
        free_flight(start, TUNNELLING_SPEED_M_S, TUNNELLING_TICKS, dt).x
            > HARBOR_SENSOR_POS_M[0] as f32 + HARBOR_SENSOR_HALF_M[0] as f32,
        "a body this fast must clear the whole volume, or a body that never got past \
         it would prove nothing about being held"
    );
    let positions = fly(&mut fixture, probe, TUNNELLING_TICKS);
    let travel: Vec<f32> = positions
        .windows(2)
        .map(|pair| pair[1].x - pair[0].x)
        .collect();
    let end = *positions.last().expect("the probe was flown");

    let lost: f32 = travel.iter().map(|step| free - step).sum();
    let held = travel.iter().filter(|step| **step < free * 0.5).count();
    let free_end = free_flight(start, TUNNELLING_SPEED_M_S, TUNNELLING_TICKS, dt);
    assert!(
        end.x < free_end.x - 2.0 * free,
        "a body-bearing trigger volume must cost the body real distance: this flight \
         lost {lost} m of {free} m per tick over {travel:?}, ending at {end:?} instead \
         of {free_end:?}. If every tick is now free, a future avian release may have \
         taught the swept CCD to skip sensors: re-measure and re-decide rather than \
         relaxing this test"
    );
    assert!(
        held >= 3,
        "and the loss must come from ticks the body was held in, not from many small \
         ones: {held} of {} ticks were under half a free tick, over {travel:?}",
        travel.len()
    );
    // The cost this layout bought, stated rather than left out: with a body on
    // the volume the crossing *is* reported at this speed, because the clamp puts
    // the body onto the volume's surface. The decision trades that report for the
    // body's flight, and the neighbouring test states the other side of it.
    assert_eq!(
        contacts_for(&fixture, HARBOR_OBJECT_SENSOR).len(),
        1,
        "and the crossing is reported here, where the body-less layout reports \
         nothing at this speed: that is the one thing the clamp buys, paid for with \
         the distance above. {:?}",
        fixture
            .contacts()
            .iter()
            .map(|contact| contact.object.as_str().to_owned())
            .collect::<Vec<_>>()
    );
}

/// **The layout follows the record's role and from nothing else.** A `Sensor`
/// object is spawned on an entity with no rigid body and reports `None` for one;
/// a `Solid` object is spawned on a static body of its own and reports it.
///
/// Observable failure if the role stops deciding the layout: a trigger volume
/// gains a body (a swept body can be held by a checkpoint), or solid geometry
/// loses one (every swept body flies through the world — the #420 defect arriving
/// from the other direction, and the one #424's audit exists to catch).
#[test]
fn accept_f18_b_the_layout_is_the_recorded_role_and_nothing_else() {
    let mut fixture = WorldFixture::arch();
    let report = fixture.spawned().clone();
    let trigger = collider_of(&report, OBJECT_SENSOR);
    let solid = collider_of(&report, OBJECT_LEG_RIGHT);
    // One update, so Avian has resolved each collider's body binding.
    fixture.step(1);
    let world = world_mut(&mut fixture);

    assert_eq!(
        world.get::<RigidBody>(solid),
        Some(&RigidBody::Static),
        "solid world geometry is a static rigid body: that is what a swept body is \
         stopped by (the collider-on-body rule, task #424)"
    );
    assert_eq!(
        world.get::<ColliderOf>(solid).map(|of| of.body),
        Some(solid),
        "and its collider is bound to that body, which is what makes it a candidate \
         for Avian's `SweptCcdBodyQuery`"
    );
    assert_eq!(
        world.get::<RigidBody>(trigger),
        None,
        "a trigger volume must carry no rigid body: with one, Avian binds the collider \
         to it, `solve_swept_ccd` resolves that body and holds a swept body at the \
         volume's near face (task #401)"
    );
    assert_eq!(
        world.get::<ColliderOf>(trigger),
        None,
        "so the collider is bound to no body, which is the whole mechanism: \
         `solve_swept_ccd` reaches its candidates through \
         `Query<(&Collider, &ColliderOf)>` and a standalone collider is never one"
    );
    assert!(
        world.get::<Collider>(trigger).is_some(),
        "while the collider itself exists, at the same place with the same shape: a \
         missing collider would collide with nothing and could not report either"
    );
    assert!(
        world.get::<Sensor>(trigger).is_some(),
        "and it is still marked as a sensor, so the narrow phase reports the overlap \
         and no solver ever answers it"
    );
    assert_eq!(
        world
            .get::<WorldColliderInstance>(trigger)
            .map(|marker| marker.role()),
        Some(WorldCollisionRole::Sensor),
        "the entity still names the role its record declared"
    );

    // The report agrees with what was built, entity for entity.
    let sensor = report
        .object(&object(OBJECT_SENSOR))
        .expect("the sensor is in the report");
    let sensor_collider = sensor.collider.as_ref().expect("the sensor is collided");
    assert_eq!(
        sensor_collider.body, None,
        "a consumer asking where the body is must be told there is none, not handed \
         an entity that carries no body"
    );
    assert_eq!(
        sensor.entities(),
        vec![sensor.visual, sensor_collider.entity],
        "and the load transaction despawns exactly the entities that exist: a body the \
         spawn never made must not be listed"
    );
    let solid_object = report
        .object(&object(OBJECT_LEG_RIGHT))
        .expect("the leg is in the report");
    assert_eq!(
        solid_object
            .collider
            .as_ref()
            .expect("the leg is collided")
            .body,
        Some(solid),
        "while a solid object's body is reported, because it has one"
    );
}

/// **The two mesh layouts differ in the body and the marker, and in nothing
/// else.** The trigger bundle is written separately from the collider-on-body
/// one, so this is what holds the two honest: the same uploaded mesh handle
/// behind both, the same stored triangles, the same pose, the same layers, the
/// same binding — the only differences being the `RigidBody` the solid path has
/// and the `Sensor` marker the trigger path has.
///
/// Observable failure if the two drift: a trigger volume that lost triangles (a
/// hull or a box substituted for the authored mesh), a solid object marked as a
/// sensor, or a trigger volume that acquired a body.
#[test]
fn accept_f18_b_the_trigger_and_solid_mesh_paths_differ_only_in_the_body() {
    let (mut fixture, _probe) = harbor_swept(CROSSING_SPEED_M_S);
    let report = fixture.spawned().clone();
    fixture.step(1);
    let trigger = collider_of(&report, HARBOR_OBJECT_SENSOR);
    let solid = collider_of(&report, HARBOR_OBJECT_HANGAR);
    let world = world_mut(&mut fixture);

    // Each object's own authored triangle count: a box is six quads, the hangar
    // shell is three of them.
    for (key, entity, triangles) in [
        (HARBOR_OBJECT_SENSOR, trigger, 12_usize),
        (HARBOR_OBJECT_HANGAR, solid, HARBOR_HANGAR_TRIANGLES),
    ] {
        // One upload behind both halves: the handle resolves in the world's own
        // asset stack, so the drawn and the collided geometry are one asset.
        let handle = world
            .get::<Mesh3d>(entity)
            .unwrap_or_else(|| panic!("{key} presents the mesh it collides with"))
            .0
            .clone();
        assert!(
            world.resource::<Assets<Mesh>>().get(&handle).is_some(),
            "{key}: the presented handle must resolve in the world's asset stack"
        );

        // The same geometry it was authored with, and no substituted primitive
        // (a hull, a bounding box) in its place.
        let collider = world
            .get::<Collider>(entity)
            .unwrap_or_else(|| panic!("{key} has the collider Avian derived"));
        let stored = collider
            .shape()
            .as_trimesh()
            .unwrap_or_else(|| panic!("{key}: the derived collider is a triangle mesh"))
            .indices()
            .len();
        assert_eq!(
            stored, triangles,
            "{key}: the collider must carry every triangle the upload stored, got \
             {stored}"
        );

        // The same pose, layers and identity.
        let position = world
            .get::<Position>(entity)
            .unwrap_or_else(|| panic!("{key} carries a position"));
        let transform = world
            .get::<Transform>(entity)
            .unwrap_or_else(|| panic!("{key} carries a transform"));
        assert_eq!(
            position.0, transform.translation,
            "{key}: the collision and the presentation must be placed from one pose, or \
             the two halves of the object disagree about where it is"
        );
        assert_eq!(
            *world
                .get::<AvianCollisionLayers>(entity)
                .unwrap_or_else(|| panic!("{key} carries the designed layers")),
            static_world_layers(),
            "{key}: both layouts are world geometry and carry the same layer set"
        );
        assert!(
            world.get::<WorldObjectBinding>(entity).is_some(),
            "{key}: both layouts carry the object's identity, so a report can name it"
        );
    }

    // And the two differences, which are the decision.
    assert_eq!(
        world.get::<RigidBody>(solid),
        Some(&RigidBody::Static),
        "solid mesh geometry is on a static body, so swept CCD sees it"
    );
    assert_eq!(
        world.get::<RigidBody>(trigger),
        None,
        "a mesh trigger volume is on no body at all, so swept CCD does not"
    );
    assert!(
        world.get::<Sensor>(trigger).is_some(),
        "and the trigger is marked as a sensor"
    );
    assert!(
        world.get::<Sensor>(solid).is_none(),
        "while the solid object is not, or it would stop nothing"
    );
}

/// **#424's enforced invariant is unaffected by the decision.** The audit that
/// keeps every production body-spawning path on the collider-on-body rule
/// (`asset_stack::undeclared_swept_invisible_bodies`) must find nothing to report
/// in a world that contains trigger volumes, and no body may have to declare
/// itself swept-invisible because of one.
///
/// A trigger volume is not a rigid body at all, so it is outside the audit's
/// query in the first place — which is the part worth pinning: the decision does
/// not weaken the invariant, it sits beside it.
#[test]
fn accept_f18_b_the_swept_visible_body_audit_still_holds() {
    let (mut fixture, _probe) = harbor_swept(CROSSING_SPEED_M_S);
    let report = fixture.spawned().clone();
    fixture.step(1);
    let trigger = collider_of(&report, HARBOR_OBJECT_SENSOR);

    let invisible = cs_app::asset_stack::swept_invisible_bodies(world_mut(&mut fixture));
    let undeclared =
        cs_app::asset_stack::undeclared_swept_invisible_bodies(world_mut(&mut fixture));
    assert!(
        invisible.is_empty() && undeclared.is_empty(),
        "no body in this world may carry its colliders on descendants — neither \
         undeclared nor declared: {:?}",
        invisible
            .iter()
            .chain(undeclared.iter())
            .map(|body| format!("{:?} declared {:?}", body.body, body.declared))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fixture.world().get::<RigidBody>(trigger),
        None,
        "the trigger volume is not a rigid body, which is why the audit has nothing \
         to say about it and why nothing has to declare it swept-invisible"
    );
}
