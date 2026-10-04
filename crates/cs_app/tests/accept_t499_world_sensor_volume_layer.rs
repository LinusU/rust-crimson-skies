//! Task #499: the collision layer a world-authored sensor volume declares.
//!
//! Owner paths: F18's world spawn (`crates/cs_app/src/world/`) and F23-C's
//! spawn preflight (`crates/cs_app/src/physics/preflight.rs`). Task test prefix:
//! `accept_t499_`.
//!
//! `classify_hit` in the preflight classifies a cast hit by reading the hit
//! entity's [`BodyLayer`] and its [`Sensor`] marker, and before this task **no
//! world collider carried a `BodyLayer`**. A hit it cannot classify is taken as
//! solid (`is_none_or(|kind| kind == ContactKind::SolidContact)`) and is refused
//! by the sensor cast, so a world trigger volume was the worst of both: a spawn
//! whose first tick swept it was clamped against it with its into-volume
//! velocity removed — F23-C's `accept_f23_c_preflight_never_stops_on_a_sensor`
//! violated for exactly the volumes a mission cares about — and the crossing
//! left no record for the gameplay consumer. The measurement, the choice and its
//! rejected alternatives are in
//! `docs/findings/2026-10-04-t499-world-sensor-volume-collision-layer.md`.
//!
//! Every test drives production code: the world is built by
//! [`spawn_world`](cs_app::world::spawn_world) from the authored
//! [`arch_world`](cs_app::world::arch_world) and
//! [`harbor_world`](cs_app::world::harbor_world) records, the projectile is
//! spawned by [`spawn_body`](cs_app::physics::spawn_body) on the `Projectile`
//! layer — which is what gives it the swept-CCD components, the declared layer
//! and the marker a preflight mover must carry — and the preflight and the
//! crossing consumer are the production systems the fixture's composition
//! installs ([`spawn_preflight`](cs_app::world::WorldFixtureBuilder::spawn_preflight)).
//!
//! No original data and no `CS_GAME_DIR` access: every mass, extent, position
//! and speed below is synthetic fixture content, and nothing here claims what the
//! 2000 PC original did with a trigger volume.

use avian3d::prelude::{CollisionLayers as AvianCollisionLayers, LinearVelocity, Position, Sensor};
use bevy::prelude::Entity;
use cs_app::objectives::{CrossingSource, TriggerCrossings};
use cs_app::physics::{
    BASELINE_FIXED_HZ, BodyLayer, BodyMode, BodySpec, SpawnPreflight, SpawnPreflightEvent,
    SpawnPreflightLog, spawn_body,
};
use cs_app::world::fixture::{ARCH_LEG_HALF_M, ARCH_LEG_Z_M};
use cs_app::world::{
    HARBOR_OBJECT_HANGAR, HARBOR_OBJECT_SENSOR, MESH_SETTLE_UPDATES, OBJECT_LEG_RIGHT,
    OBJECT_SENSOR, SENSOR_HALF_M, SENSOR_POS_M, WorldFixture, arch_world, harbor_meshes,
    harbor_world, static_world_layers,
};
use cs_content::world::WorldObjectId;
use cs_sim::collision::{CollisionLayer, ContactKind, ShapeClass, classify_contact};

/// The declared fixed rate, and the rate every cell below is measured at.
const RATE_HZ: u32 = BASELINE_FIXED_HZ;

/// Ticks of travel between the projectile's leading face and the volume's near
/// face at spawn: F23-B's first-tick hole, so the body is not yet in the broad
/// phase and the engine's own discrete phase has nothing to report.
const SPAWN_IN_HOLE_TICKS: f32 = 0.4;

/// The projectile's box half extent: 10 cm, the body F23-D's contact probe fires.
const PROJECTILE_HALF_M: f32 = 0.05;

/// The cell whose spawn tick *ends inside* the volume: 0.5 m of travel against a
/// 1.5 m thickness, so a discrete sample lands in the volume too and the engine
/// is not blind to this crossing.
const DWELL_SPEED_M_S: f32 = 60.0;

/// The boundary cell: one tick of travel exactly spans the volume's thickness
/// plus the projectile, so the body leaves the volume on the tick it entered it.
const BOUNDARY_SPEED_M_S: f32 = 192.0;

/// The **pass-through** cell: one tick of travel is three times the volume's
/// thickness, so no sample of the body ever lands inside it and the preflight's
/// swept record is the only trace of the crossing. This is the hole F23-D
/// measured, on the world's own volume.
const PASS_THROUGH_SPEED_M_S: f32 = 600.0;

/// Ticks flown in total, the spawn tick included.
const TICKS: u64 = 3;

/// The arch world's right leg: a solid box at `[0, ARCH_LEG_HALF_M[1],
/// ARCH_LEG_Z_M]`, the very record `OBJECT_LEG_RIGHT` is authored from. It is
/// the solid the regression arm is measured against.
const ARCH_LEG_POS_M: [f32; 3] = [0.0, ARCH_LEG_HALF_M[1] as f32, ARCH_LEG_Z_M as f32];

/// The near and far faces of the arch world's `trigger.sensor` volume along the
/// flight axis `z`, which is the volume's *thin* axis: a body can cross the whole
/// volume inside one tick without leaving the fixture's air, and a volume the
/// task could only be measured on at 8 m of length would need 1 km/s.
const ARCH_SENSOR_NEAR_Z_M: f32 = SENSOR_POS_M[2] as f32 - SENSOR_HALF_M[2] as f32;
const ARCH_SENSOR_FAR_Z_M: f32 = SENSOR_POS_M[2] as f32 + SENSOR_HALF_M[2] as f32;

/// One tick of travel at `speed_m_s`, in meters.
fn travel_m(speed_m_s: f32) -> f32 {
    speed_m_s / RATE_HZ as f32
}

/// The projectile: a 10 cm swept box on the `Projectile` layer, flying `+z`
/// through the centre of the arch world's sensor volume, spawned
/// [`SPAWN_IN_HOLE_TICKS`] of travel short of the volume's near face.
fn arch_projectile(speed_m_s: f32) -> BodySpec {
    let spawn_z =
        ARCH_SENSOR_NEAR_Z_M - SPAWN_IN_HOLE_TICKS * travel_m(speed_m_s) - PROJECTILE_HALF_M;
    BodySpec {
        layer: CollisionLayer::Projectile,
        shape: ShapeClass::Solid,
        mode: BodyMode::Dynamic,
        mass_kg: 1.0,
        half_extents_m: [PROJECTILE_HALF_M; 3],
        position_m: [SENSOR_POS_M[0] as f32, SENSOR_POS_M[1] as f32, spawn_z],
        linear_velocity_m_s: [0.0, 0.0, speed_m_s],
    }
}

/// A swept projectile on the `Projectile` layer, flying `+x` at the arch leg's
/// centre: the same body, aimed at the solid the regression arm needs.
fn arch_leg_projectile(speed_m_s: f32) -> BodySpec {
    let near_x = ARCH_LEG_POS_M[0] - ARCH_LEG_HALF_M[0] as f32;
    BodySpec {
        layer: CollisionLayer::Projectile,
        shape: ShapeClass::Solid,
        mode: BodyMode::Dynamic,
        mass_kg: 1.0,
        half_extents_m: [PROJECTILE_HALF_M; 3],
        position_m: [
            near_x - SPAWN_IN_HOLE_TICKS * travel_m(speed_m_s) - PROJECTILE_HALF_M,
            ARCH_LEG_POS_M[1],
            ARCH_LEG_POS_M[2],
        ],
        linear_velocity_m_s: [speed_m_s, 0.0, 0.0],
    }
}

/// A world fixture with the spawn preflight and its gameplay consumer installed,
/// and `spec` spawned into it and marked the way `PhysicsSession::spawn` marks a
/// moving body on a swept layer.
fn preflight_fixture(spec: &BodySpec) -> (WorldFixture, Entity) {
    let mut fixture = WorldFixture::builder(arch_world().expect("the arch world is well formed"))
        .spawn_preflight()
        .build()
        .expect("the arch fixture builds");
    let body = spawn_body(fixture.app_mut().world_mut(), spec).expect("the spec is valid");
    // The one line `PhysicsSession::spawn` adds for a body that needs a swept
    // first tick. It is a marker: the cast re-derives the distance from the
    // body's own velocity and the tick's own timestep.
    fixture
        .app_mut()
        .world_mut()
        .entity_mut(body)
        .insert(SpawnPreflight);
    (fixture, body)
}

/// The one object's collider entity, from a built world's own report.
fn collider_of(fixture: &WorldFixture, key: &str) -> Entity {
    let id = WorldObjectId::new(key).expect("the fixture object key is valid");
    fixture
        .spawned()
        .collider_for(&id)
        .unwrap_or_else(|| panic!("{key} carries a collider in this world"))
}

/// Every preflight record the log holds.
fn preflight_events(fixture: &WorldFixture) -> Vec<SpawnPreflightEvent> {
    fixture
        .world()
        .resource::<SpawnPreflightLog>()
        .events()
        .to_vec()
}

/// The record for one body, or a panic naming what was recorded instead.
fn preflight_of(fixture: &WorldFixture, body: Entity) -> SpawnPreflightEvent {
    let events = preflight_events(fixture);
    *events
        .iter()
        .find(|event| event.body == body)
        .unwrap_or_else(|| panic!("the swept body preflighted and left a record: {events:?}"))
}

/// The crossings the spawn-tick consumer delivered.
fn crossings(fixture: &WorldFixture) -> &TriggerCrossings {
    fixture
        .world()
        .get_resource::<TriggerCrossings>()
        .expect("the spawn-preflight composition installs the crossing resource")
}

/// One axis of a body's end-of-flight state, read from the engine.
fn axis(fixture: &WorldFixture, body: Entity, axis: usize) -> (f32, f32) {
    let world = fixture.world();
    let position = world
        .get::<Position>(body)
        .expect("the projectile still exists")
        .0;
    let velocity = world
        .get::<LinearVelocity>(body)
        .expect("the projectile still exists")
        .0;
    (position[axis], velocity[axis])
}

/// Every world contact that names the sensor volume.
fn volume_contacts(fixture: &WorldFixture) -> Vec<&cs_app::world::WorldContact> {
    fixture
        .contacts()
        .iter()
        .filter(|contact| contact.object.as_str() == OBJECT_SENSOR)
        .collect()
}

// ---------------------------------------------------------------------------

/// **A world collider declares the layer the sweep classifies it by**, on every
/// world layout, and the declared layer is the one the world membership already
/// assigned — one answer per collider, not a second one.
///
/// This is the criterion the repair rests on, so it is measured rather than
/// asserted through behaviour alone: the classification `classify_hit` reaches
/// is a function of exactly these two components, and both are read here.
///
/// Observable failure: the [`BodyLayer`] is dropped from any of the three spawn
/// bundles (the hand-built cuboid, the mesh-derived trigger volume, the
/// mesh-derived static body), or a layout declares a layer its membership does
/// not name — and the world returns to being unclassifiable, which the next two
/// tests show is a spawn being clamped by a trigger volume.
#[test]
fn accept_t499_a_world_collider_declares_the_layer_the_sweep_classifies_it_by() {
    let fixture = WorldFixture::arch();
    let world = fixture.world();

    for (key, is_sensor) in [(OBJECT_SENSOR, true), (OBJECT_LEG_RIGHT, false)] {
        let entity = collider_of(&fixture, key);
        let membership = world
            .get::<AvianCollisionLayers>(entity)
            .unwrap_or_else(|| panic!("{key} carries the world's designed layers"));
        assert_eq!(
            *membership,
            static_world_layers(),
            "{key}: the declared layer is the membership this world assigns, so the \
             assertion that follows is about that one layer and not about a second \
             declaration"
        );
        let declared = world
            .get::<BodyLayer>(entity)
            .unwrap_or_else(|| {
                panic!(
                    "{key} declares no collision layer, so the preflight cannot classify \
                     it: `classify_hit` returns None and an unclassifiable hit is taken \
                     as solid"
                )
            })
            .0;
        assert_eq!(
            u32::from(declared.bit()),
            membership.memberships.0,
            "{key}: the declared layer and the engine membership must be one fact"
        );
        assert_eq!(
            world.get::<Sensor>(entity).is_some(),
            is_sensor,
            "{key}: the shape class is the record's role and nothing else, and it is \
             what `classify_contact` reads to resolve a sensor pair"
        );
        assert_eq!(
            classify_contact(
                CollisionLayer::Projectile,
                declared,
                ShapeClass::Solid,
                if is_sensor {
                    ShapeClass::Sensor
                } else {
                    ShapeClass::Solid
                },
            ),
            if is_sensor {
                ContactKind::SensorOverlap
            } else {
                ContactKind::SolidContact
            },
            "{key}: the classification the preflight's matrix reaches for this pair"
        );
        // The camera layer stays inert against both, so the declared layer is
        // also what keeps the presentation query filter out of the pair.
        assert_eq!(
            classify_contact(
                CollisionLayer::Camera,
                declared,
                ShapeClass::Solid,
                if is_sensor {
                    ShapeClass::Sensor
                } else {
                    ShapeClass::Solid
                },
            ),
            ContactKind::Ignored,
            "{key}: the presentation camera must never generate a contact"
        );
    }

    // The import path: a mesh-derived trigger volume and a mesh-derived static
    // body are spawned by two different bundles, so each is asked separately.
    let mut mesh = WorldFixture::builder(harbor_world().expect("the harbor world is well formed"))
        .meshes(harbor_meshes())
        .spawn_preflight()
        .build()
        .expect("the harbor fixture builds");
    // A mesh-derived collider is derived by an `Update` system, so it exists only
    // after the fixture has settled.
    mesh.step(MESH_SETTLE_UPDATES);
    for (key, is_sensor) in [(HARBOR_OBJECT_SENSOR, true), (HARBOR_OBJECT_HANGAR, false)] {
        let entity = collider_of(&mesh, key);
        let declared = mesh
            .world()
            .get::<BodyLayer>(entity)
            .unwrap_or_else(|| panic!("{key}: the mesh path must declare its layer too"))
            .0;
        assert_eq!(
            u32::from(declared.bit()),
            mesh.world()
                .get::<AvianCollisionLayers>(entity)
                .unwrap_or_else(|| panic!("{key} carries the world's layers"))
                .memberships
                .0,
            "{key}: the mesh layout must declare the layer its membership names"
        );
        assert_eq!(
            classify_contact(
                CollisionLayer::Projectile,
                declared,
                ShapeClass::Solid,
                if is_sensor {
                    ShapeClass::Sensor
                } else {
                    ShapeClass::Solid
                },
            ),
            if is_sensor {
                ContactKind::SensorOverlap
            } else {
                ContactKind::SolidContact
            },
            "{key}: the import path must classify exactly as the cuboid path does, or \
             a mesh-authored trigger volume is a different kind of volume from a \
             hand-built one"
        );
    }
}

/// **No world sensor volume clamps, stops or delays a spawn**, at every one of
/// the three structural cells, and the body's whole flight is the flight nothing
/// touched.
///
/// F23-C's criterion is `accept_f23_c_preflight_never_stops_on_a_sensor`, which
/// was measured for bodies the physics session spawned. This is the same
/// criterion for the world's own volumes, which is where it was violated: the
/// preflight could not classify them, and an unclassifiable hit is solid.
///
/// Observable failure: the [`BodyLayer`] goes back off the sensor volume, and the
/// projectile is clamped onto its near face at 0.4 ticks of travel with its
/// into-volume velocity removed — measured at 0.200 m of a 0.500 m tick at
/// 60 m/s, 2.000 m of a 5.000 m tick at 600 m/s, and then it does not move again.
#[test]
fn accept_t499_a_world_sensor_volume_never_clamps_stops_or_delays_a_spawn() {
    for speed in [DWELL_SPEED_M_S, BOUNDARY_SPEED_M_S, PASS_THROUGH_SPEED_M_S] {
        let spec = arch_projectile(speed);
        let (mut fixture, body) = preflight_fixture(&spec);
        let spawn_z = spec.position_m[2];
        let dt = 1.0 / RATE_HZ as f32;

        // The spawn tick.
        fixture.step(1);
        let event = preflight_of(&fixture, body);
        assert_eq!(
            (event.hit, event.clamped, event.stopped),
            (None, false, false),
            "a world sensor volume must not stop a spawn at {speed} m/s: the preflight \
             clamped onto {:?} with the body stopped = {}",
            event.hit,
            event.stopped
        );
        assert!(
            (event
                .distance_m
                .expect("an unstopped cast measures a distance")
                - travel_m(speed))
            .abs()
                < 1e-4,
            "and the solid cast must have run the whole tick's travel: a shorter \
             distance means it found something solid on the way, at {speed} m/s: {:?}",
            event.distance_m
        );

        // Every later tick, value for value: nothing may delay the body either.
        // Both tolerances are a *fraction of the fired speed* rather than an
        // absolute number, because what is being excluded is "stopped" — a body
        // that lost its velocity or its travel — and that is a difference of
        // 100%, not of a few parts per million of f32 integration.
        for tick in 2..=TICKS {
            fixture.step(1);
            let (z, vz) = axis(&fixture, body, 2);
            let free = spawn_z + speed * tick as f32 * dt;
            assert!(
                (z - free).abs() < 1e-3 * speed * dt && (vz - speed).abs() < 1e-3 * speed,
                "a trigger volume must not delay a body either: at {speed} m/s tick \
                 {tick} ended at z = {z:.6}, vz = {vz}, against the free {free:.6} and \
                 {speed}"
            );
        }

        // However the engine's own discrete phase sees the pair, it sees it as
        // the sensor the record declared.
        for contact in volume_contacts(&fixture) {
            assert_eq!(
                contact.role,
                cs_content::world::WorldCollisionRole::Sensor,
                "the volume may report an overlap and nothing else: {contact:?}"
            );
        }
    }
}

/// **The pass-through is the only cell where the swept record is the whole
/// trace, and it reaches gameplay.** A body whose tick outruns the volume leaves
/// no contact report anywhere — the hole F23-D measured — so before this task the
/// crossing of a world trigger volume was unobservable, and now it is delivered
/// once, on the tick it happened, naming the volume.
///
/// That is what makes the F18-C overlay path and the spawn-tick path agree on
/// what a crossing is. **Both producers run in this fixture** — the composition
/// installs [`WorldSweptCrossingPlugin`](cs_app::world::WorldSweptCrossingPlugin)
/// as every world fixture does, and this one adds the spawn preflight — and both
/// hand the same `(actor, volume)` pair to the same
/// [`TriggerCrossings`](cs_app::objectives::TriggerCrossings) ledger, from
/// [`CrossingSource::SpawnTickPreflight`] here and
/// [`CrossingSource::OrdinaryFlightSweep`] there. The single delivery below is
/// therefore measured with *both* producers present, not only the one under test.
#[test]
fn accept_t499_a_spawn_tick_crossing_of_a_world_volume_reaches_the_consumer() {
    let spec = arch_projectile(PASS_THROUGH_SPEED_M_S);
    let (mut fixture, body) = preflight_fixture(&spec);
    let volume = collider_of(&fixture, OBJECT_SENSOR);

    // Both producers are in this composition; without that, the delivery count
    // below would say nothing about the two paths agreeing.
    assert!(
        fixture
            .world()
            .get_resource::<cs_app::world::SweptBodyTracks>()
            .is_some(),
        "the ordinary-flight crossing pass is part of the world composition, so this \
         test measures two producers sharing one pair ledger rather than one"
    );

    // Preconditions, asserted rather than assumed: the body must cross the whole
    // volume inside its first tick, or the engine's discrete phase would report
    // the overlap and this test would not be measuring the hole.
    let thickness = ARCH_SENSOR_FAR_Z_M - ARCH_SENSOR_NEAR_Z_M;
    assert!(
        travel_m(PASS_THROUGH_SPEED_M_S) > thickness + 2.0 * PROJECTILE_HALF_M,
        "the cell only measures a pass-through while one tick ({:.2} m) outruns the \
         volume's {thickness:.2} m thickness plus the projectile",
        travel_m(PASS_THROUGH_SPEED_M_S)
    );

    fixture.step(1);

    // The engine's own stream: nothing, over the whole flight.
    for _ in 1..TICKS {
        fixture.step(1);
    }
    assert!(
        volume_contacts(&fixture).is_empty(),
        "no sample of this body lands inside the volume, so the volume must report \
         nothing at all over {TICKS} ticks — a report here means the narrow phase \
         finds something this measurement does not account for, and the numbers have \
         to be re-measured rather than believed: {:?}",
        volume_contacts(&fixture)
    );
    let (_, vz) = axis(&fixture, body, 2);
    assert!(
        (vz - PASS_THROUGH_SPEED_M_S).abs() < 1e-3,
        "and the body must have flown the whole way through, vz = {vz}"
    );

    // The swept record, and what the consumer made of it.
    let event = preflight_of(&fixture, body);
    assert_eq!(
        event.passed,
        Some(volume),
        "the preflight's second, non-blocking cast must record the world sensor volume \
         the spawn crossed: {event:?}"
    );
    let crossed_at = event
        .passed_distance_m
        .expect("a recorded crossing carries the distance the cast measured");
    assert!(
        (crossed_at - SPAWN_IN_HOLE_TICKS * travel_m(PASS_THROUGH_SPEED_M_S)).abs() < 1e-3,
        "and the distance must be the cast's own time of impact — \
         {SPAWN_IN_HOLE_TICKS} ticks of travel before the tick's whole travel — not an \
         invented geometry: it is {crossed_at}"
    );

    let delivered = crossings(&fixture).crossings();
    assert_eq!(
        delivered.len(),
        1,
        "the consumer must deliver exactly one crossing for the pair, however often \
         the record is re-read: {delivered:?}"
    );
    let crossing = delivered[0];
    assert_eq!((crossing.actor, crossing.volume), (body, volume));
    assert_eq!(
        crossing.source,
        CrossingSource::SpawnTickPreflight,
        "and it must say which producer decided it, or the ordinary-flight pass's \
         crossing for the same pair would be indistinguishable"
    );
    assert!(crossing.is_entry(), "a spawn-tick crossing is an entry");
    assert_eq!(
        crossing.tick, 1,
        "stamped with the tick it happened on, not a later one"
    );
    assert_eq!(
        crossing.distance_m, crossed_at,
        "and at the distance the preflight measured, so a consumer can order the \
         crossing against a solid stop on the same tick"
    );
    assert_eq!(
        crossings(&fixture).duplicates(),
        TICKS - 1,
        "the record is re-read on every later tick of the frame and the pair ledger is \
         what refuses the repeats, so {TICKS} ticks of flight must produce one \
         delivery and {} refusals — a second delivery would be a trigger that fires \
         again",
        TICKS - 1
    );
}

/// **The distance the preflight measures is the same invariant at every speed.**
///
/// F23-B's first-tick hole is a geometric offset, not a speed: the body's
/// leading face is `SPAWN_IN_HOLE_TICKS` of travel short of the volume's near
/// face at spawn, so the cast's time of impact is that distance into the tick
/// whatever the tick's length is. Pinned as a ratio rather than as a count,
/// because the count is a property of this fixture's speeds and the ratio is
/// what makes the record comparable across them — and across the two producers,
/// which measure from the same reference (the spawn pose).
#[test]
fn accept_t499_the_crossing_distance_is_the_spawn_hole_offset_at_every_speed() {
    for speed in [DWELL_SPEED_M_S, BOUNDARY_SPEED_M_S, PASS_THROUGH_SPEED_M_S] {
        let (mut fixture, body) = preflight_fixture(&arch_projectile(speed));
        fixture.step(1);
        let event = preflight_of(&fixture, body);
        let crossed_at = event
            .passed_distance_m
            .unwrap_or_else(|| panic!("the spawn tick recorded no crossing at {speed} m/s"));
        assert!(
            (crossed_at / travel_m(speed) - SPAWN_IN_HOLE_TICKS).abs() < 1e-4,
            "at {speed} m/s the crossing must land {SPAWN_IN_HOLE_TICKS} of a tick's \
         travel in, and it landed {crossed_at} m of {} m",
            travel_m(speed)
        );
        assert!(
            crossed_at < travel_m(speed),
            "and strictly inside the tick's travel, or the body had not reached the \
             volume at all"
        );
    }
}

/// **Solid world geometry still clamps a spawn.** The regression arm: giving
/// world colliders a declared layer must not make the world's own geometry
/// invisible to the preflight, which is what a layer the matrix resolves as
/// `Ignored` — or one the cast's filter does not reach — would do.
///
/// Before this task the leg clamped too, but by *absence*: an unclassifiable hit
/// is taken as solid, which is a coincidence, not a decision. This is the
/// classification.
#[test]
fn accept_t499_solid_world_geometry_still_clamps_a_spawn() {
    let (mut fixture, body) = preflight_fixture(&arch_leg_projectile(DWELL_SPEED_M_S));
    let leg = collider_of(&fixture, OBJECT_LEG_RIGHT);
    let near_x = ARCH_LEG_POS_M[0] - ARCH_LEG_HALF_M[0] as f32;

    fixture.step(1);

    let event = preflight_of(&fixture, body);
    assert_eq!(
        (event.hit, event.clamped, event.stopped),
        (Some(leg), true, true),
        "a spawn swept at world geometry must be clamped onto it and stopped there: \
         {event:?}"
    );
    assert!(
        (event.distance_m.expect("a clamp measures a distance")
            - SPAWN_IN_HOLE_TICKS * travel_m(DWELL_SPEED_M_S))
        .abs()
            < 1e-4,
        "at the cast's own time of impact, {SPAWN_IN_HOLE_TICKS} ticks of travel in: {:?}",
        event.distance_m
    );
    assert_eq!(
        event.passed, None,
        "and the solid leg is not a volume the spawn crossed"
    );
    let (x, vx) = axis(&fixture, body, 0);
    // The solver leaves a small residual against the face it is easing the body
    // onto — measured here at -0.000826 m/s, and #415 measured -0.0019 m/s on the
    // same pinned engine with its own geometry — so this bound is a thousandth of
    // the fired speed rather than an absolute number: what it has to exclude is
    // "not stopped", which is 60 m/s.
    assert!(
        x <= near_x + 1e-3 && vx.abs() < 1e-3 * DWELL_SPEED_M_S,
        "the body must rest at the leg's near face ({near_x}) with its into-obstacle \
         motion ended: x = {x}, vx = {vx}"
    );
}
