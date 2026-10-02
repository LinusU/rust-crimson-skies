//! #416: the world bootstrap's substep policy is declared, not overwritten.
//!
//! The task asked every world bootstrap to run the F23-D declared
//! `DECLARED_SUBSTEP_COUNT`. Measuring the world path showed the declared count
//! is not adoptable there yet: on the pinned engine (`bevy 0.19.1` /
//! `avian3d 0.7.0`), a `SpeculativeMargin::ZERO` body — the body shape every
//! swept production layer carries — tunnels through a static trimesh wall at
//! ordinary speeds under exactly `SubstepCount(2)`, with contacts logged but
//! zero solver response. `docs/findings/2026-10-02-t416-world-substep-policy.md`
//! has the measurement; `world_app()` therefore declares its count through the
//! plugin seam as [`WORLD_FIXTURE_SUBSTEP_COUNT`] instead of being silently
//! overwritten after the fact.
//!
//! * `..._the_world_bootstrap_declares_its_substep_count` fails if the
//!   override is deleted (the world would then silently run the declared
//!   count and tunnel the trimesh wall), if it is restated as a literal, or
//!   if the constant drifts — the divergence cannot come back unnoticed in
//!   either direction.
//! * `..._the_declared_count_still_tunnels_the_mesh_leg` pins the defect that
//!   blocks adoption. When a fix lands — an engine change, a different
//!   collider strategy, a solver workaround — this test fails because the
//!   probe is then stopped; that failure is the signal to flip
//!   `WORLD_FIXTURE_SUBSTEP_COUNT` to `DECLARED_SUBSTEP_COUNT`, not a
//!   regression here.
//!
//! Both tests drive production code only: `world_app`, `load_world`,
//! `spawn_swept_probe` and the harbor fixture are the same composition the
//! runtime will call.

use avian3d::prelude::{Position, SubstepCount};
use cs_app::physics::{BASELINE_FIXED_HZ, DECLARED_SUBSTEP_COUNT, PhysicsAdapterPlugin};
use cs_app::world::{
    HARBOR_OBJECT_HANGAR, MESH_SETTLE_UPDATES, ProbeSpec, WORLD_FIXTURE_SUBSTEP_COUNT,
    harbor_meshes, harbor_world, load_world, spawn_swept_probe, world_app, world_instance,
};
use cs_content::world::WorldObjectId;

const POPULATION: [&str; 6] = [
    HARBOR_OBJECT_HANGAR,
    "trigger.sensor",
    "banner.non_colliding",
    "water.patch",
    "terrain.ground",
    "strip.absent_mesh",
];

/// The world path declares a count of its own, and the declared policy is not
/// it — the divergence is stated, not smuggled past the plugin's install.
#[test]
fn accept_t416_the_world_bootstrap_declares_its_substep_count() {
    let app = world_app();
    assert_eq!(
        app.world().resource::<SubstepCount>(),
        &SubstepCount(WORLD_FIXTURE_SUBSTEP_COUNT),
        "the world bootstrap's substep count is the declared fixture count: \
         WORLD_FIXTURE_SUBSTEP_COUNT, measured in \
         docs/findings/2026-10-02-t416-world-substep-policy.md"
    );
    assert_eq!(
        PhysicsAdapterPlugin::new(BASELINE_FIXED_HZ).substeps(),
        DECLARED_SUBSTEP_COUNT,
        "the adapter's default stays the F23-D declared count"
    );
    assert_eq!(
        cs_app::physics::PhysicsSession::new(BASELINE_FIXED_HZ)
            .world()
            .expect("the session built a world")
            .resource::<SubstepCount>(),
        &SubstepCount(DECLARED_SUBSTEP_COUNT),
        "the production session runs the declared count"
    );
}

/// The measured defect that blocks adoption, pinned so its resolution is
/// loud: at the declared two substeps, the production swept probe flies
/// through the harbor world's mesh leg untouched — same body, same wall, same
/// spawn as the F18-B tests that pass under the fixture count.
#[test]
fn accept_t416_the_declared_count_still_tunnels_the_mesh_leg() {
    let definition = harbor_world().expect("the harbor world is well formed");
    let meshes = harbor_meshes();
    let mut app = world_app();
    // Reproduce the divergence exactly: the same bootstrap at the declared
    // count, nothing else changed.
    app.insert_resource(SubstepCount(DECLARED_SUBSTEP_COUNT));
    let mission = world_instance(&definition, None, &POPULATION, &[])
        .expect("the fixture load record is valid");
    load_world(&mut app, &definition, &mission, &meshes).expect("the harbor world loads");
    for _ in 0..MESH_SETTLE_UPDATES {
        app.update();
    }

    let probe = spawn_swept_probe(
        &mut app,
        &ProbeSpec {
            position_m: [-12.0, 1.5, 1.5],
            velocity_m_s: [30.0, 0.0, 0.0],
            half_extents_m: [0.25, 0.25, 0.25],
            mass_kg: 250.0,
        },
    )
    .expect("the probe spec is valid");
    for _ in 0..80 {
        app.update();
    }

    let end = app
        .world()
        .get::<Position>(probe)
        .expect("the probe still exists")
        .0;
    let logged = app
        .world()
        .resource::<cs_app::world::WorldContacts>()
        .contacts()
        .iter()
        .any(|contact| {
            contact.object == WorldObjectId::new(HARBOR_OBJECT_HANGAR).expect("valid id")
        });
    assert!(
        logged,
        "the contact is still logged — the defect is a missing solver \
         response, not missing detection"
    );
    assert!(
        end.x > 1.0,
        "at the declared substep count the probe passes the solid mesh leg \
         (it ended at {end:?}); if it is now stopped, the blocker is resolved \
         — flip WORLD_FIXTURE_SUBSTEP_COUNT to DECLARED_SUBSTEP_COUNT"
    );
}
