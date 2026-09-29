//! Fixed-step schedule authority: one integration per tick, constant dt and
//! the pinned pre-integration force hook (F23-A non-negotiable behavior 5).

use core::time::Duration;

use avian3d::prelude::{LinearVelocity, PhysicsSystems, RigidBody};
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{FixedPostUpdate, Query, ResMut, Resource, With},
    time::{Fixed, Time},
};
use cs_app::physics::{FixtureBodySpec, PhysicsFixture};

use crate::common;

/// Velocity observed in `FixedPostUpdate` before `PhysicsSystems::Prepare`.
#[derive(Resource, Default)]
struct PreIntegrationVelocities(Vec<[f32; 3]>);

fn record_velocity_before_integration(
    mut samples: ResMut<PreIntegrationVelocities>,
    bodies: Query<&LinearVelocity, With<RigidBody>>,
) {
    let velocity = bodies
        .single()
        .expect("the fixture has exactly one rigid body");
    samples.0.push(velocity.0.to_array());
}

/// Every declared tick runs exactly one integration, the accumulator is left
/// empty and the fixed timestep never varies.
///
/// Observable failure if a tick is skipped or doubled: `integrations` and
/// `ticks` diverge from the declared count, or the accumulator keeps an
/// overstep. Observable failure if dt became wall-clock variable: `timestep_s`
/// changes between ticks or `Time<Fixed>::elapsed` no longer equals
/// `ticks * timestep`.
#[test]
fn accept_f23_a_every_declared_tick_integrates_exactly_once_at_a_constant_dt() {
    let ticks = 7_u64;
    let mut fixture = common::fixture(1.0);
    let declared_dt = fixture.timestep_s();

    fixture.step(ticks);

    let ledger = fixture.ledger();
    assert_eq!(ledger.ticks, ticks, "one tick boundary per declared tick");
    assert_eq!(
        ledger.integrations, ticks,
        "one Avian integration per declared tick"
    );
    assert_eq!(
        ledger.timestep_s, declared_dt,
        "the adapter must report the declared fixed timestep, not a variable dt"
    );

    let world = fixture.world();
    let fixed = world.resource::<Time<Fixed>>();
    assert_eq!(
        fixed.overstep(),
        Duration::ZERO,
        "a declared tick must not leave accumulator debt"
    );
    assert_eq!(
        fixed.timestep().as_secs_f32(),
        declared_dt,
        "Time<Fixed> must carry the adapter's declared timestep"
    );
    assert_eq!(
        fixed.elapsed(),
        fixed.timestep() * ticks as u32,
        "the fixed clock must have advanced exactly {ticks} timesteps"
    );
}

/// The adapter applies queued forces before the integration step of the same
/// tick, so the pre-integration probe still sees the start velocity and the
/// tick's end already contains the delta-v.
///
/// Observable failure if the force hook is scheduled after the integration
/// step (`PhysicsSystems::StepSimulation`) or outside the fixed schedule: the
/// delta-v lands a tick late, so the probe/integration counts diverge and the
/// same tick's end velocity stays zero.
#[test]
fn accept_f23_a_force_hook_runs_before_the_integration_step() {
    let mass_kg = 2.0_f32;
    let force_n = [0.0, 0.0, 8.0];
    let mut fixture = PhysicsFixture::builder(FixtureBodySpec::at_origin(mass_kg))
        .configure(|app| {
            app.init_resource::<PreIntegrationVelocities>().add_systems(
                FixedPostUpdate,
                record_velocity_before_integration.before(PhysicsSystems::Prepare),
            );
        })
        .build()
        .expect("the fixture spec is valid");
    let dt = fixture.timestep_s();

    fixture.submit(common::force_request(fixture.body(), force_n));
    fixture.step(1);

    let pre = fixture.world().resource::<PreIntegrationVelocities>();
    assert_eq!(
        pre.0,
        vec![[0.0, 0.0, 0.0]],
        "the pre-integration probe must see the untouched start velocity"
    );

    let expected_delta_v = force_n[2] / mass_kg * dt;
    let velocity = fixture.sample().linear_velocity_m_s;
    assert!(
        (velocity[2] - expected_delta_v).abs() <= 1e-4 * expected_delta_v,
        "the same tick must already contain the delta-v {expected_delta_v}, got {velocity:?}"
    );
    assert_eq!(fixture.ledger().integrations, 1);
}
