//! Task #552 (F28-AE1): the runtime half of the declared area effect.
//!
//! Task test prefix: `accept_f28_ae1_`. Synthetic fixture geometry only; the
//! original's area semantics are not claimed.

use std::collections::BTreeSet;

use cs_sim::damage::ActorId;
use cs_sim::time::TickRate;
use cs_sim::weapons::ordnance::{
    AreaEffect, ArmingRule, EquipmentRules, FuseRule, GuidanceRule, LostTargetBehavior,
    OrdnanceFamily, OrdnanceRuntime, OrdnanceRuntimeError, ProjectileOrdnance, TargetObservation,
    synthetic_channels, synthetic_launch_geometry, synthetic_media, synthetic_ordnance,
    synthetic_stack_load,
};
use cs_sim::weapons::{GuidanceDetonation, MountTransform, ProjectileId, TargetPath};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::UnitVec3;

const SESSION: u64 = 12;
const RADIUS_M: f64 = 30.0;
const LIFETIME: u64 = 5;

fn actor_in(session: u64, serial: u64) -> ActorId {
    ActorId {
        session: SessionId::new(session).expect("a nonzero session generation"),
        serial,
    }
}

fn actor(serial: u64) -> ActorId {
    actor_in(SESSION, serial)
}

fn path(serial: u64, x: f64) -> TargetPath {
    TargetPath::try_new(actor(serial), [x, 0.0, 0.0], [x, 0.0, 0.0]).expect("finite")
}

fn guided(area: Option<AreaEffect>) -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance("synthetic.ae1_area"),
        OrdnanceFamily::GuidedRocket,
        synthetic_launch_geometry(),
        synthetic_stack_load(),
        ArmingRule::AfterTicks(1),
        FuseRule::Timed { ticks: 30 },
        GuidanceRule::Targeted {
            lost_target: LostTargetBehavior::Detonate,
        },
        30,
        area,
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the guided fixture is valid")
}

fn detonation(area: Option<AreaEffect>) -> (OrdnanceRuntime, GuidanceDetonation) {
    let mut runtime = OrdnanceRuntime::new(SESSION, Tick(0), TickRate::new(30).expect("rate"), 7);
    let mount = MountTransform::try_new(
        cs_types::space::WorldPosition::try_new([0.0; 3]).expect("finite"),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("unit vector"),
        [0.0; 3],
    )
    .expect("the fixture mount is valid");
    runtime
        .launch(
            actor(1),
            ProjectileId {
                session: SESSION,
                serial: 1,
            },
            &guided(area),
            &mount,
            Some(actor(9)),
            [0.0; 3],
        )
        .expect("the fixture launches");
    let mut lost = runtime.guidance_tick(SESSION, TargetObservation::destroyed(actor(9), Tick(1)));
    let detonation = lost.detonated.remove(0);
    (runtime, detonation)
}

fn area() -> AreaEffect {
    AreaEffect::try_new(RADIUS_M, LIFETIME).expect("a valid area")
}

#[test]
fn accept_f28_ae1_the_application_pass_runs_once_and_lists_each_recipient_once() {
    let (mut runtime, detonation) = detonation(Some(area()));
    let targets = [
        path(22, RADIUS_M - 0.5),
        path(21, 0.0),
        path(21, 1.0),
        path(23, RADIUS_M),
    ];
    let instance = runtime
        .open_detonation_area(SESSION, Tick(1), &detonation, &targets)
        .expect("the area opens");
    let live = runtime.areas().get(&instance).expect("live");
    assert_eq!(live.recipients, vec![actor(21), actor(22)]);
    assert_eq!(live.expires_at, Tick(1 + LIFETIME));
    assert_eq!(
        runtime.open_detonation_area(SESSION, Tick(1), &detonation, &targets),
        Err(OrdnanceRuntimeError::AreaAlreadyOpened {
            projectile: detonation.projectile()
        }),
        "an area gets one application pass"
    );
    assert_eq!(runtime.areas().len(), 1);
}

#[test]
fn accept_f28_ae1_expiry_is_exact_and_reported_once() {
    let (mut runtime, detonation) = detonation(Some(area()));
    runtime
        .open_detonation_area(SESSION, Tick(1), &detonation, &[path(21, 0.0)])
        .expect("opens");
    let boundary = 1 + LIFETIME;
    assert!(
        runtime
            .advance_areas(SESSION, Tick(boundary - 1))
            .expect("advances")
            .is_empty(),
        "live one tick before the boundary"
    );
    let expired = runtime
        .advance_areas(SESSION, Tick(boundary))
        .expect("advances");
    assert_eq!(expired.len(), 1);
    assert_eq!(expired[0].expired_at, Tick(boundary));
    assert!(runtime.areas().is_empty());
    assert!(
        runtime
            .advance_areas(SESSION, Tick(boundary))
            .expect("advances")
            .is_empty()
    );
    assert!(runtime.advance_areas(SESSION, Tick(boundary - 1)).is_err());
}

#[test]
fn accept_f28_ae1_an_item_without_an_area_and_a_foreign_session_are_refused() {
    let (mut runtime, detonation) = detonation(None);
    assert!(matches!(
        runtime.open_detonation_area(SESSION, Tick(1), &detonation, &[]),
        Err(OrdnanceRuntimeError::NoAreaEffect { .. })
    ));
    let (mut runtime, detonation) = self::detonation(Some(area()));
    let foreign = TargetPath::try_new(actor_in(SESSION + 1, 5), [0.0; 3], [0.0; 3]).expect("ok");
    assert!(matches!(
        runtime.open_detonation_area(SESSION, Tick(1), &detonation, &[foreign]),
        Err(OrdnanceRuntimeError::ForeignSession { .. })
    ));
    assert!(matches!(
        runtime.open_detonation_area(SESSION + 1, Tick(1), &detonation, &[]),
        Err(OrdnanceRuntimeError::ForeignSession { .. })
    ));
    assert!(runtime.areas().is_empty(), "a refusal opens nothing");
}
