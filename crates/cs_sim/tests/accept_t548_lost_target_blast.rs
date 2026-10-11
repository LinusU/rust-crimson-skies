//! Task #548 (F28-C.1 follow-up): the measured original blast rule.
//!
//! Task test prefix: `accept_t548_`. The measurement is in
//! `docs/findings/2026-10-11-f28-c-2-lost-target-blast-measured.md`: the
//! original's area routine damages every actor inside `IMPACT_PROXIMITY`,
//! scaled by `1 - d^2 / r^2`, instead of one tracked actor.
//!
//! The geometry here is newly authored synthetic fixture data, never original
//! game data; the retail numbers are re-measured by
//! `crates/cs_content/tests/accept_t548_retail_blast.rs`.

use std::collections::BTreeSet;

use cs_sim::damage::{ActorId, DamageNodeKey};
use cs_sim::time::TickRate;
use cs_sim::weapons::ordnance::{
    AreaEffect, ArmingRule, BlastRecipient, EquipmentRules, FuseRule, GuidanceRule,
    LostTargetBehavior, OrdnanceFamily, OrdnanceRuntime, OrdnanceRuntimeError, ProjectileOrdnance,
    SYNTHETIC_ORDNANCE_ARMOR_DAMAGE, SYNTHETIC_ORDNANCE_INTERNAL_DAMAGE, TargetObservation,
    synthetic_channels, synthetic_launch_geometry, synthetic_media, synthetic_ordnance,
    synthetic_stack_load,
};
use cs_sim::weapons::{GuidanceDetonation, MountTransform, ProjectileId};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 11;
const RADIUS_M: f64 = 40.0;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SessionId::new(SESSION).expect("a nonzero session generation"),
        serial,
    }
}

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

fn node() -> DamageNodeKey {
    DamageNodeKey::new("fuselage").expect("the fixture node key is valid")
}

fn guided(area: Option<AreaEffect>) -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance("synthetic.t548_blast"),
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

/// Launches one item at the origin and destroys its target, returning the
/// runtime and the detonation record the loss produced.
fn detonation(area: Option<AreaEffect>) -> (OrdnanceRuntime, GuidanceDetonation) {
    let mut runtime = OrdnanceRuntime::new(SESSION, Tick(0), TickRate::new(30).expect("rate"), 7);
    let mount = MountTransform::try_new(
        position([0.0, 0.0, 0.0]),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("unit vector"),
        [0.0, 0.0, 0.0],
    )
    .expect("the fixture mount is valid");
    let target = actor(9);
    runtime
        .launch(
            actor(1),
            ProjectileId {
                session: SESSION,
                serial: 1,
            },
            &guided(area),
            &mount,
            Some(target),
            [0.0, 0.0, 0.0],
        )
        .expect("the fixture launches");
    let mut lost = runtime.guidance_tick(SESSION, TargetObservation::destroyed(target, Tick(1)));
    let detonation = lost.detonated.remove(0);
    (runtime, detonation)
}

fn recipient(serial: u64, distance_m: f64) -> BlastRecipient {
    BlastRecipient::new(actor(serial), node(), position([distance_m, 0.0, 0.0]))
}

/// The measured falloff: full at the centre, `1 - d^2/r^2` inside, nothing at or
/// beyond the radius, and no fraction for a distance that is not a distance.
#[test]
fn accept_t548_the_measured_falloff_is_one_minus_squared_ratio() {
    let area = AreaEffect::try_new(RADIUS_M, 10).expect("a valid area");
    assert_eq!(area.blast_fraction(0.0), Some(1.0));
    assert_eq!(area.blast_fraction(RADIUS_M / 2.0), Some(0.75));
    assert_eq!(area.blast_fraction(RADIUS_M), Some(0.0));
    assert_eq!(area.blast_fraction(RADIUS_M * 2.0), Some(0.0));
    assert_eq!(area.blast_fraction(-1.0), None);
    assert_eq!(area.blast_fraction(f64::NAN), None);
}

/// A lost-target blast damages every recipient inside the radius, each scaled
/// by its own distance, and nobody outside — not one tracked actor.
#[test]
fn accept_t548_a_blast_damages_every_actor_inside_the_radius_with_falloff() {
    let area = AreaEffect::try_new(RADIUS_M, 10).expect("a valid area");
    let (mut runtime, detonation) = detonation(Some(area));
    let hits = runtime
        .route_detonation_area(
            SESSION,
            Tick(1),
            &detonation,
            &[
                recipient(20, 0.0),
                recipient(21, RADIUS_M / 2.0),
                recipient(22, RADIUS_M),
                recipient(23, RADIUS_M * 3.0),
            ],
        )
        .expect("the blast routes");
    let damage_on = |serial: u64| -> Vec<f64> {
        hits.iter()
            .filter(|hit| hit.target == actor(serial))
            .map(|hit| hit.damage)
            .collect()
    };
    let full = damage_on(20);
    assert_eq!(full.len(), 2, "both declared channels reach the centre");
    assert!(full.contains(&SYNTHETIC_ORDNANCE_ARMOR_DAMAGE));
    assert!(full.contains(&SYNTHETIC_ORDNANCE_INTERNAL_DAMAGE));
    let half = damage_on(21);
    assert_eq!(half.len(), 2);
    assert!(half.contains(&(SYNTHETIC_ORDNANCE_ARMOR_DAMAGE * 0.75)));
    assert!(half.contains(&(SYNTHETIC_ORDNANCE_INTERNAL_DAMAGE * 0.75)));
    assert!(damage_on(22).is_empty(), "the radius itself takes nothing");
    assert!(damage_on(23).is_empty(), "outside takes nothing");
    assert!(hits.iter().all(|hit| hit.attacker == Some(actor(1))));
}

/// Damage is applied once: the item latches like a trigger does.
#[test]
fn accept_t548_a_blast_routes_once_and_needs_a_declared_area() {
    let area = AreaEffect::try_new(RADIUS_M, 10).expect("a valid area");
    let (mut runtime, detonation) = detonation(Some(area));
    runtime
        .route_detonation_area(SESSION, Tick(1), &detonation, &[recipient(20, 1.0)])
        .expect("the first routing succeeds");
    let again = runtime.route_detonation_area(SESSION, Tick(1), &detonation, &[recipient(20, 1.0)]);
    assert!(matches!(
        again,
        Err(OrdnanceRuntimeError::AlreadyRouted { .. })
    ));

    let (mut runtime, bare) = detonation_without_area();
    let refused = runtime.route_detonation_area(SESSION, Tick(1), &bare, &[recipient(20, 1.0)]);
    assert!(matches!(
        refused,
        Err(OrdnanceRuntimeError::NoAreaEffect { .. })
    ));
    // The refusal latched nothing: the single-target path is still open.
    runtime
        .route_detonation(SESSION, Tick(1), &bare, actor(20), node())
        .expect("a refused area blast leaves the item routable");
}

fn detonation_without_area() -> (OrdnanceRuntime, GuidanceDetonation) {
    detonation(None)
}

/// A recipient position with no finite distance is refused before anything is
/// latched.
#[test]
fn accept_t548_a_non_finite_recipient_is_refused_without_latching() {
    let area = AreaEffect::try_new(RADIUS_M, 10).expect("a valid area");
    let (mut runtime, detonation) = detonation(Some(area));
    let far = BlastRecipient::new(actor(20), node(), position([f64::MAX, f64::MAX, f64::MAX]));
    let refused = runtime.route_detonation_area(SESSION, Tick(1), &detonation, &[far]);
    assert!(matches!(
        refused,
        Err(OrdnanceRuntimeError::NonFiniteRecipient { .. })
    ));
    runtime
        .route_detonation_area(SESSION, Tick(1), &detonation, &[recipient(20, 1.0)])
        .expect("the item was not latched by the refusal");
}
