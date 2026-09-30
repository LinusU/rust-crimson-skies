//! The spawn boundary itself: what `spawn_world` refuses, and how each
//! declared [`WorldCollisionRole`] decides what exists in the engine (F18-A).
//!
//! The sweep tests prove the *geometry*; these prove the *report*:
//!
//! * an authored matrix no runtime transform can hold is refused **whole**,
//!   before any entity exists — a half-built world would carry some objects
//!   and not others while the caller holds no `SpawnedWorld` to ask which;
//! * `None` means presented and never colliding, `Solid` means a collider
//!   that is *not* a sensor, and `Sensor` means a collider Avian will only
//!   report — three different outcomes from three declared roles;
//! * a sensor reports the body that reached it and does not stop it.

use avian3d::prelude::{RigidBody, Sensor};
use bevy::prelude::{App, Time, Vec3, World};
use bevy::time::Fixed;
use cs_app::world::{
    WorldFixture, WorldSpawnError, arch_world,
    fixture::{SENSOR_HALF_M, SENSOR_POS_M},
    spawn_world,
};
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    Aabb, Sector, SectorId, SurfaceRole, WorldCollisionRole, WorldCollisionShape, WorldDefinition,
    WorldId, WorldObjectInstance,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use crate::common;

/// How long the sensor probe is flown: long enough to cross the volume and
/// clear it entirely.
const TICKS: u64 = 15;

fn provenance(key: &str) -> Provenance {
    Provenance::designed(ClaimId::new(&format!("spawn.{key}")).expect("the claim id is valid"))
}

fn known<T>(value: T, key: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance(key)))
}

fn object(key: &str) -> cs_content::world::WorldObjectId {
    cs_content::world::WorldObjectId::new(key).expect("the test object key is valid")
}

/// A one-object world whose single instance carries a **shear**: a linear
/// map no translation/rotation/scale triple can rebuild, and therefore one
/// [`spawn_world`] must refuse instead of approximating.
fn sheared_world() -> WorldDefinition {
    let sheared = CanonicalTransform::try_new(
        [[1.0, 0.5, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [1.0, 2.0, 3.0],
    )
    .expect("the shear is finite");
    let instance = WorldObjectInstance::try_new(
        object("sheared.panel"),
        known(
            ContentId::from_source(ContentKind::Mesh, "synthetic.sheared_panel")
                .expect("the mesh id is valid"),
            "mesh",
        ),
        sheared,
        known(WorldCollisionRole::Solid, "collision"),
        known(
            WorldCollisionShape::cuboid([1.0, 1.0, 1.0]).expect("the box is valid"),
            "shape",
        ),
        known(SurfaceRole::Ground, "surface"),
        vec![SectorId::new("only").expect("the sector key is valid")],
        provenance("sheared.panel"),
    )
    .expect("the sector list has no duplicates");

    WorldDefinition::try_new(
        WorldId::from_key("test.sheared_world").expect("the world key is valid"),
        Origin::SyntheticFixture,
        known(cs_content::world::WorldBoundary::default(), "boundary"),
        vec![Sector::new(
            SectorId::new("only").expect("the sector key is valid"),
            Aabb::try_new([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).expect("the bounds are valid"),
        )],
        vec![instance],
        provenance("definition"),
    )
    .expect("the definition is structurally valid: one sector, one object")
}

/// **AC01's precondition, on the refusal side:** a matrix the runtime cannot
/// hold stops the whole build *before the first entity exists*.
///
/// Observable failure if the build is not atomic: the caller gets an error
/// while the app has already been filled with the objects that came before
/// the bad one, and no `SpawnedWorld` survives to say which were built.
#[test]
fn accept_f18_a_spawn_refuses_a_matrix_no_runtime_transform_can_hold_before_spawning_anything() {
    let definition = sheared_world();
    // The record itself is legal; only its placement has no runtime form.
    assert_eq!(
        definition.objects().len(),
        1,
        "the refusal must be about the transform, not the record structure"
    );

    let mut app = App::new();
    let entities_before = app.world().entities().len();

    let error = spawn_world(&mut app, &definition)
        .expect_err("a sheared matrix must be refused, not approximated");
    assert!(
        matches!(
            &error,
            WorldSpawnError::UnrepresentableTransform { object } if object.as_str() == "sheared.panel"
        ),
        "the refusal must name the object whose matrix was refused, got {error:?}"
    );

    assert_eq!(
        app.world().entities().len(),
        entities_before,
        "a refused build must leave no entity behind: a half-built world has no \
         `SpawnedWorld` to ask which objects it got"
    );
    let visuals = count_world_visuals(app.world_mut());
    assert_eq!(
        visuals, 0,
        "no visual may be spawned for a definition that was refused"
    );
}

fn count_world_visuals(world: &mut World) -> usize {
    let mut query = world.query::<&cs_app::world::WorldVisual>();
    query.iter(world).count()
}

/// Every declared collision role produces its own outcome, and only its own:
/// `None` is presented without a collider, `Solid` is a collider that is not
/// a sensor, `Sensor` is a collider Avian will only report.
///
/// Observable failure if a role stops being consulted: the banner acquires a
/// collider (a wall the record never declared), the leg becomes a sensor
/// (motion it must block goes unpunished), or the trigger stops reporting.
#[test]
fn accept_f18_a_every_collision_role_decides_what_is_spawned() {
    let fixture = WorldFixture::arch();
    let spawned = fixture.spawned();

    // `None`: presented, never blocking, and reported as a deliberate answer.
    let banner = object("banner.non_colliding");
    assert!(
        spawned.visual_for(&banner).is_some(),
        "an object with role `None` is still presented"
    );
    assert!(
        spawned.collider_for(&banner).is_none(),
        "role `None` must never produce a collider"
    );
    assert_eq!(
        spawned.non_colliding().to_vec(),
        vec![banner.clone()],
        "role `None` must be reported, not silently dropped"
    );

    // `Sensor`: a collider, marked as a sensor, and reported with its role.
    let trigger = object("trigger.sensor");
    let trigger_entity = spawned
        .collider_for(&trigger)
        .expect("role `Sensor` produces a collider");
    assert!(
        fixture.world().get::<Sensor>(trigger_entity).is_some(),
        "role `Sensor` must be marked as a sensor so Avian never solves it"
    );
    assert!(
        fixture.world().get::<RigidBody>(trigger_entity) == Some(&RigidBody::Static),
        "the sensor is static world geometry"
    );
    let reported = spawned
        .colliders()
        .iter()
        .find(|entry| entry.object == trigger)
        .expect("the sensor collider is in the report");
    assert_eq!(
        reported.role,
        WorldCollisionRole::Sensor,
        "the report must carry the role the record declared"
    );

    // `Solid`: the same spawn must not make it a sensor.
    let leg = object("arch.leg_right");
    let leg_entity = spawned
        .collider_for(&leg)
        .expect("role `Solid` produces a collider");
    assert!(
        fixture.world().get::<Sensor>(leg_entity).is_none(),
        "a `Solid` role must never be marked as a sensor: it has to stop bodies"
    );
    assert!(
        spawned.visual_for(&leg).is_some(),
        "a solid object is presented as well as collided with"
    );
}

/// **The sensor half of the role contract:** the body that reaches the
/// trigger volume is named in the contact log, and the volume does not stop
/// it (F18's "reports an overlap and never blocks motion").
///
/// The probe is spawned **without** [`avian3d::prelude::SweptCcd`] on
/// purpose: measured on the pinned pair, Avian's swept CCD stops a body at
/// the first time of impact against any collider, a `Sensor` volume
/// included, so a swept body would be held at the sensor's near face for the
/// crossing frame. That interaction is recorded in the findings doc; the
/// role's own claim is what this test measures.
#[test]
fn accept_f18_a_a_sensor_reports_the_probe_and_never_blocks_it() {
    let mut fixture = WorldFixture::builder(arch_world().expect("the arch world is well formed"))
        .probe_discrete(common::probe_at(SENSOR_POS_M[2]))
        .build()
        .expect("the fixture builds");
    let start = fixture.probe_position().expect("the probe was spawned");
    let dt = fixture
        .world()
        .resource::<Time<Fixed>>()
        .timestep()
        .as_secs_f32();

    fixture.step(TICKS);

    let trigger = object("trigger.sensor");
    let sensor_contacts: Vec<_> = fixture
        .contacts()
        .iter()
        .filter(|contact| contact.object == trigger)
        .collect();
    assert!(
        !sensor_contacts.is_empty(),
        "the sensor must report the probe that reached it; contacts: {:?}",
        fixture
            .contacts()
            .iter()
            .map(|contact| contact.object.as_str())
            .collect::<Vec<_>>()
    );
    let contact = sensor_contacts[0];
    assert_eq!(
        contact.role,
        WorldCollisionRole::Sensor,
        "the contact must carry the role its record declared"
    );
    assert_eq!(
        contact.other,
        fixture.probe().expect("the probe was spawned"),
        "the contact must name the body that reached the sensor"
    );

    // The volume must not stop the body: it travelled the whole way.
    let end = fixture.probe_position().expect("the probe still exists");
    let expected = start + Vec3::X * common::PROBE_SPEED_M_S as f32 * (TICKS as f32 * dt);
    let drift = (end - expected).length();
    assert!(
        drift < 0.01,
        "a sensor must report and never block: the probe drifted {drift} m, \
         ended at {end:?} instead of {expected:?}"
    );
    let velocity = fixture.probe_velocity().expect("the probe still exists");
    assert!(
        (velocity.x - common::PROBE_SPEED_M_S as f32).abs() < 1.0,
        "a sensor must not slow the body that crossed it, velocity is {velocity:?}"
    );
    assert!(
        end.x > SENSOR_POS_M[0] as f32 + SENSOR_HALF_M[0] as f32,
        "the probe must have crossed the whole volume, it ended at {end:?}"
    );
}
