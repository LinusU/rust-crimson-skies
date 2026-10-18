//! Task #1319 (T548 follow-up): the original's lost-target behavior.
//!
//! Task test prefix: `accept_t1319_`. The measurement is in
//! `docs/findings/2026-10-11-f28-c-2-lost-target-blast-measured.md`
//! ("What the projectile step then does"): a lost target is a cleared
//! pointer, the rocket flies on and detonates on its timer or at the end of
//! `RANGE`. `LostTargetBehavior::CoastThenDetonate` expresses that; `Detonate`
//! and `Coast` are unchanged.
//!
//! The geometry is newly authored synthetic fixture data, never original game
//! data.

use std::collections::BTreeSet;

use cs_sim::damage::ActorId;
use cs_sim::time::TickRate;
use cs_sim::weapons::ordnance::{
    ArmingRule, EquipmentRules, FuseDecision, FuseRule, FuseTrigger, GuidanceRule, GuidanceUpdate,
    LostTargetBehavior, OrdnanceFamily, OrdnanceRuntime, ProjectileOrdnance, TargetObservation,
    synthetic_channels, synthetic_launch_geometry, synthetic_media, synthetic_ordnance,
    synthetic_stack_load,
};
use cs_sim::weapons::{MountTransform, ProjectileId};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 11;
const LIFETIME_TICKS: u64 = 10;
const STILL_AIR: [f64; 3] = [0.0, 0.0, 0.0];

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(),
        serial,
    }
}

fn projectile(serial: u64) -> ProjectileId {
    ProjectileId {
        session: SESSION,
        serial,
    }
}

fn runtime() -> OrdnanceRuntime {
    OrdnanceRuntime::new(session(), Tick(0), TickRate::new(30).expect("rate"), 7)
}

fn transform() -> MountTransform {
    MountTransform::try_new(
        WorldPosition::try_new([0.0, 0.0, 0.0]).expect("origin"),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("forward"),
        [0.0, 0.0, 0.0],
    )
    .expect("the fixture mount is valid")
}

fn rocket(lost_target: LostTargetBehavior, fuse: FuseRule) -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance("synthetic.fixture_t1319"),
        OrdnanceFamily::GuidedRocket,
        synthetic_launch_geometry(),
        synthetic_stack_load(),
        ArmingRule::AfterTicks(1),
        fuse,
        GuidanceRule::Targeted { lost_target },
        LIFETIME_TICKS,
        None,
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the fixture is valid")
}

/// Launches `definition` at `actor(9)`; destroys that target when `lose`.
fn flown(definition: &ProjectileOrdnance, lose: bool) -> OrdnanceRuntime {
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            definition,
            &transform(),
            Some(actor(9)),
            STILL_AIR,
        )
        .expect("the fixture launches");
    if lose {
        let lost =
            runtime.guidance_tick(session(), TargetObservation::destroyed(actor(9), Tick(1)));
        assert_eq!(
            lost.update_for(&projectile(1)),
            Some(GuidanceUpdate::Coast),
            "a lost target is a cleared pointer: the rocket flies on"
        );
        assert!(lost.detonated.is_empty(), "no detonation on the loss tick");
        assert_eq!(runtime.len(), 1);
    }
    runtime
}

/// Flies to the end of life and returns every tick's range-end detonations.
fn fly_out(runtime: &mut OrdnanceRuntime) -> Vec<cs_sim::weapons::GuidanceDetonation> {
    let mut all = Vec::new();
    for _ in 0..LIFETIME_TICKS {
        let step = runtime.advance(1.0 / 30.0, STILL_AIR).expect("advance");
        all.extend(step.range_end_detonated);
    }
    all
}

#[test]
fn accept_t1319_a_lost_target_rocket_flies_on_and_detonates_at_the_end_of_its_range() {
    let definition = rocket(
        LostTargetBehavior::CoastThenDetonate,
        FuseRule::Timed {
            ticks: LIFETIME_TICKS,
        },
    );
    let mut runtime = flown(&definition, true);
    let detonations = fly_out(&mut runtime);
    assert_eq!(detonations.len(), 1, "exactly one range-end detonation");
    assert_eq!(detonations[0].projectile(), projectile(1));
    assert_eq!(detonations[0].shooter(), actor(1));
    assert!(
        detonations[0].position().to_array()[2] < 0.0,
        "it detonates where it flew to, not at the launcher"
    );
    assert!(runtime.is_empty(), "the detonated item is gone");
}

#[test]
fn accept_t1319_coast_and_detonate_are_unchanged() {
    // Coast: same flight, silent end of life.
    let coast = rocket(
        LostTargetBehavior::Coast,
        FuseRule::Timed {
            ticks: LIFETIME_TICKS,
        },
    );
    let mut runtime = flown(&coast, true);
    assert!(fly_out(&mut runtime).is_empty(), "Coast expires silently");
    assert!(runtime.is_empty());

    // Detonate: ends on the loss tick.
    let detonate = rocket(
        LostTargetBehavior::Detonate,
        FuseRule::Timed {
            ticks: LIFETIME_TICKS,
        },
    );
    let mut runtime = runtime_with(&detonate);
    let lost = runtime.guidance_tick(session(), TargetObservation::destroyed(actor(9), Tick(1)));
    assert_eq!(lost.detonated.len(), 1, "Detonate ends immediately");
}

fn runtime_with(definition: &ProjectileOrdnance) -> OrdnanceRuntime {
    flown(definition, false)
}

#[test]
fn accept_t1319_an_item_that_never_lost_its_target_expires_silently() {
    let definition = rocket(
        LostTargetBehavior::CoastThenDetonate,
        FuseRule::Timed {
            ticks: LIFETIME_TICKS,
        },
    );
    let mut runtime = flown(&definition, false);
    assert!(
        fly_out(&mut runtime).is_empty(),
        "the rule is about a lost target, not about tracking rockets"
    );
}

#[test]
fn accept_t1319_the_declared_timer_still_detonates_a_lost_target_rocket_early() {
    let definition = rocket(
        LostTargetBehavior::CoastThenDetonate,
        FuseRule::Timed { ticks: 4 },
    );
    let mut runtime = flown(&definition, true);
    for _ in 0..4 {
        runtime.advance(1.0 / 30.0, STILL_AIR).expect("advance");
    }
    let decision = runtime
        .decide(&projectile(1), &[], &[])
        .expect("the item is live");
    assert!(
        matches!(
            decision,
            FuseDecision::Triggered(FuseTrigger::Timed { ticks: 4 })
        ),
        "the timer fires before the end of range: {decision:?}"
    );
}
