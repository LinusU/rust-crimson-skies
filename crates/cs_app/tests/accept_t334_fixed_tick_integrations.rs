//! Task #334: `SyntheticScene::step(n)` must run exactly `n` Avian
//! integration steps — including the very first declared tick.
//!
//! Measured cause of the former gap (recorded in
//! `docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`):
//! `TimeUpdateStrategy::ManualDuration` feeds
//! `Time<Real>::update_with_duration`, whose first call only establishes the
//! baseline instant and reports a zero delta, so frame 1 accumulated nothing
//! and ran no `FixedMain` — `step(n)` integrated `n - 1` times while the
//! `Tick` counter reached `n`.
//!
//! The fix seeds the manual clock's baseline instant at build time, so the
//! first counted update already produces the full manual delta. Decision and
//! before/after measurements:
//! `docs/findings/2026-09-23-t334-first-frame-fixed-step.md`.
//!
//! Observable failure if the seeding is removed: the `FixedPostUpdate`
//! counter below — the schedule `PhysicsPlugins::default()` integrates in —
//! stays one behind the declared tick count, and the first `step(1)` leaves
//! the body at its spawn pose.

use avian3d::prelude::PhysicsSystems;
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{FixedPostUpdate, ResMut, Resource},
    time::{Fixed, Time},
};
use cs_app::synthetic::SyntheticScene;
use cs_types::{BodyKind, SyntheticBodySpec, Tick};

/// Counts `FixedPostUpdate` executions — one per fixed step, each carrying
/// exactly one `PhysicsSystems::StepSimulation` run — so the test measures
/// real schedule executions, not the scene's own tick counter.
#[derive(Resource, Default)]
struct IntegrationCount(u64);

fn count_integration(mut count: ResMut<IntegrationCount>) {
    count.0 += 1;
}

fn scene_with_integration_counter() -> SyntheticScene {
    SyntheticScene::builder(SyntheticBodySpec::falling_box(BodyKind::Dynamic))
        .configure(|app| {
            app.init_resource::<IntegrationCount>().add_systems(
                FixedPostUpdate,
                count_integration.before(PhysicsSystems::Prepare),
            );
        })
        .build()
        .expect("the fixture spec must build a scene")
}

/// Every declared tick — the first one included — runs exactly one
/// integration, leaves no overstep debt and advances the fixed clock by one
/// timestep.
#[test]
fn accept_t334_every_declared_tick_integrates_exactly_once() {
    let start = SyntheticBodySpec::falling_box(BodyKind::Dynamic).position_m[1];
    let mut scene = scene_with_integration_counter();
    let timestep = scene.world().resource::<Time<Fixed>>().timestep();

    let mut previous_y = start;
    for tick in 1..=8u64 {
        scene.step(1);

        let integrations = scene.world().resource::<IntegrationCount>().0;
        assert_eq!(
            integrations, tick,
            "tick {tick} must be the {tick}-th integration, got {integrations}"
        );

        let world = scene.world();
        let fixed = world.resource::<Time<Fixed>>();
        assert_eq!(
            fixed.overstep(),
            core::time::Duration::ZERO,
            "tick {tick} must leave the accumulator empty, got {:?}",
            fixed.overstep()
        );
        assert_eq!(
            fixed.elapsed(),
            timestep * tick as u32,
            "tick {tick}: the fixed clock must have advanced {tick} timesteps"
        );

        let y = scene.sample().position_m[1];
        assert!(
            y < previous_y,
            "tick {tick} must really have integrated: y = {y}, previous = {previous_y}"
        );
        previous_y = y;
    }
}

/// AC03 from the task's angle: two full runs in a row, each ending at the
/// requested tick count with exactly that many integrations behind it.
#[test]
fn accept_t334_two_runs_end_at_requested_count_with_exactly_n_integrations() {
    for run in 0..2 {
        let mut scene = scene_with_integration_counter();
        scene.step(600);

        assert_eq!(
            scene.world().resource::<IntegrationCount>().0,
            600,
            "run {run}: 600 declared ticks must mean 600 integrations"
        );
        let sample = scene.sample();
        assert_eq!(
            sample.tick,
            Tick(600),
            "run {run}: the world must end at the requested tick count"
        );
        assert!(
            sample.position_m[1] < 9.0,
            "run {run}: the body must have fallen for all 600 ticks, y = {}",
            sample.position_m[1]
        );
    }
}
