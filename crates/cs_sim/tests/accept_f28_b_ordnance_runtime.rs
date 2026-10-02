//! Acceptance scenario F28-B: the per-tick ordnance runtime and its
//! direct, proximity, guidance, status and nitro mechanisms.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-B`. Task test prefix: `accept_f28_b_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Minimum scenario: guidance loses a target safely on destruction or session
//! change. The tests drive production code only — `cs_sim::weapons::ordnance`'s
//! [`OrdnanceRuntime`] and the records it owns. Removing the launch path, the
//! swept advance, the fuse drive, the guidance loss handling, the status
//! bridge, the once-only damage routing or the nitro bridge makes one of them
//! fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access: these tests prove the interface and the
//! contract, never the original game.

use std::collections::BTreeSet;

use cs_sim::damage::{ActorId, DamageNodeKey, SystemKind};
use cs_sim::time::TickRate;
use cs_sim::weapons::ordnance::{
    ArmingRule, EquipmentRules, FuseInert, FuseRule, FuseTrigger, GuidanceRule, GuidanceUpdate,
    LostTargetBehavior, LostTargetReason, NitroActivationRule, NitroTradeoffs, OrdnanceFamily,
    OrdnanceRuntime, OrdnanceRuntimeError, ProjectileOrdnance, SYNTHETIC_CHOKE_TICKS,
    SYNTHETIC_NITRO_EXTRA_THRUST_N, StatusEffectKind, StatusEffectTarget, TargetObservation,
    TargetPath, synthetic_area_denial, synthetic_channels, synthetic_direct_explosive,
    synthetic_guided_rocket, synthetic_launch_geometry, synthetic_media,
    synthetic_nitro_parameters, synthetic_ordnance, synthetic_proximity_flak, synthetic_stack_load,
};
use cs_sim::weapons::{MountTransform, ProjectileId, SweptHit};
use cs_types::Tick;
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 11;
const TICKS_PER_SECOND: u32 = 30;
const PRODUCER: u32 = 7;
/// Still air: the fixture's own conversion must be the identity here.
const STILL_AIR: [f64; 3] = [0.0, 0.0, 0.0];

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

fn projectile(serial: u64) -> ProjectileId {
    ProjectileId {
        session: SESSION,
        serial,
    }
}

fn tick(value: u64) -> Tick {
    Tick(value)
}

fn rate() -> TickRate {
    TickRate::new(TICKS_PER_SECOND).expect("the test rate is non-zero")
}

fn tick_seconds() -> f64 {
    1.0 / f64::from(TICKS_PER_SECOND)
}

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

/// A runtime opened on the fixture session.
fn runtime() -> OrdnanceRuntime {
    OrdnanceRuntime::new(SESSION, tick(0), rate(), PRODUCER)
}

/// A launcher mount at the origin pointing down -Z, the canonical forward of
/// `FLIGHT-PHYSICS`.
fn transform() -> MountTransform {
    MountTransform::try_new(
        position([0.0, 0.0, 0.0]),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("the canonical forward is a unit vector"),
        [0.0, 0.0, 0.0],
    )
    .expect("the fixture mount is valid")
}

/// A guided component with one declared lost-target behavior, so every
/// behavior is exercised through the runtime rather than only the one the
/// synthetic fixture ships.
fn guided_with(key: &str, lost_target: LostTargetBehavior) -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance(key),
        OrdnanceFamily::GuidedRocket,
        synthetic_launch_geometry(),
        synthetic_stack_load(),
        ArmingRule::AfterTicks(1),
        FuseRule::Timed { ticks: 30 },
        GuidanceRule::Targeted { lost_target },
        30,
        None,
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the guided fixture is valid")
}

/// A damage node no other test depends on.
fn node() -> DamageNodeKey {
    DamageNodeKey::new("weapon_mount_1").expect("the fixture node key is valid")
}

/// AC02 minimum scenario: a guided item's target is destroyed, the loss is
/// reported once with the item's own declared behavior, and the item ends
/// safely — its tracker is gone and it never re-acquires.
#[test]
fn accept_f28_b_guidance_loses_a_destroyed_target_and_ends_safely() {
    let definition = synthetic_guided_rocket();
    let target = actor(9);
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &definition,
            &transform(),
            Some(target),
            STILL_AIR,
        )
        .expect("the fixture launches");
    assert_eq!(runtime.len(), 1);
    assert_eq!(
        runtime.guidance().len(),
        1,
        "a guided item registers a tracker"
    );

    let alive = runtime.guidance_tick(SESSION, TargetObservation::alive(target, tick(1)));
    assert_eq!(
        alive.update_for(&projectile(1)),
        Some(GuidanceUpdate::Tracked { target }),
        "a live target keeps tracking"
    );

    let destroyed = runtime.guidance_tick(SESSION, TargetObservation::destroyed(target, tick(2)));
    assert!(
        matches!(
            destroyed.update_for(&projectile(1)),
            Some(GuidanceUpdate::Lost {
                reason: LostTargetReason::Destroyed,
                behavior: LostTargetBehavior::Detonate,
            })
        ),
        "destruction is a loss with the declared behavior: {destroyed:?}"
    );
    assert_eq!(
        destroyed.detonated,
        vec![projectile(1)],
        "a detonating item ends where it is"
    );
    assert_eq!(runtime.len(), 0, "the ended item is no longer live");
    assert!(
        runtime.guidance().require(&projectile(1)).is_err(),
        "no stale tracker outlives the item"
    );

    // The target cannot come back: a later live observation finds no tracker
    // and does not re-acquire.
    let later = runtime.guidance_tick(SESSION, TargetObservation::alive(target, tick(3)));
    assert!(
        later.update_for(&projectile(1)).is_none(),
        "an ended item is never tracked again: {later:?}"
    );
}

/// AC02's second half: a session change is a loss, not a lookup that might
/// match an id in the next generation's roster.
#[test]
fn accept_f28_b_guidance_loses_its_target_on_a_session_change() {
    let definition = synthetic_guided_rocket();
    let target = actor(9);
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &definition,
            &transform(),
            Some(target),
            STILL_AIR,
        )
        .expect("the fixture launches");

    let changed = runtime.guidance_tick(SESSION + 1, TargetObservation::alive(target, tick(1)));
    assert!(
        matches!(
            changed.update_for(&projectile(1)),
            Some(GuidanceUpdate::Lost {
                reason: LostTargetReason::ForeignSession { expected, found },
                ..
            }) if expected == SESSION && found == SESSION + 1
        ),
        "another generation is a lost target: {changed:?}"
    );
    assert!(
        runtime.guidance().require(&projectile(1)).is_err(),
        "the tracker does not survive the session change"
    );
}

/// A `Coast` loss leaves the item flying its last vector, with no target and
/// no re-acquisition, and the cause is remembered exactly once.
#[test]
fn accept_f28_b_a_coasting_item_keeps_flying_after_losing_its_target() {
    let definition = guided_with("synthetic.fixture_guided_coast", LostTargetBehavior::Coast);
    let target = actor(9);
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &definition,
            &transform(),
            Some(target),
            STILL_AIR,
        )
        .expect("the fixture launches");

    let lost = runtime.guidance_tick(SESSION, TargetObservation::destroyed(target, tick(1)));
    assert_eq!(
        lost.update_for(&projectile(1)),
        Some(GuidanceUpdate::Coast),
        "the coasting item reports its own consequence"
    );
    assert!(
        lost.detonated.is_empty(),
        "a coasting item does not detonate"
    );
    assert_eq!(runtime.len(), 1, "a coasting item stays live");
    let tracker = runtime
        .guidance()
        .require(&projectile(1))
        .expect("the tracker is still registered");
    assert_eq!(tracker.target(), None, "a lost target is dropped");
    assert_eq!(
        tracker.lost(),
        Some(LostTargetReason::Destroyed),
        "the cause is remembered once"
    );

    let before = runtime.get(&projectile(1)).expect("still live").current();
    runtime
        .advance(tick_seconds(), STILL_AIR)
        .expect("a coasting item advances");
    assert_ne!(
        runtime.get(&projectile(1)).expect("still live").current(),
        before,
        "a coasting item keeps flying its last vector"
    );
}

/// A `Disarm` loss retires the item: it stays a visible dud but its fuse can
/// no longer fire and it deals nothing.
#[test]
fn accept_f28_b_a_disarmed_item_can_never_fire_its_fuse() {
    let definition = guided_with(
        "synthetic.fixture_guided_disarm",
        LostTargetBehavior::Disarm,
    );
    let target = actor(9);
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &definition,
            &transform(),
            Some(target),
            STILL_AIR,
        )
        .expect("the fixture launches");

    let lost = runtime.guidance_tick(SESSION, TargetObservation::destroyed(target, tick(1)));
    assert_eq!(
        lost.update_for(&projectile(1)),
        Some(GuidanceUpdate::Disarmed),
        "the disarming item reports its own consequence"
    );
    let live = runtime.get(&projectile(1)).expect("a dud stays visible");
    assert!(
        live.state().is_retired(),
        "a disarmed item is retired, not left firing"
    );
    let impact = SweptHit {
        projectile: projectile(1),
        target: actor(9),
        time_of_impact: 0.5,
    };
    assert_eq!(
        runtime
            .decide(&projectile(1), &[], &[impact])
            .expect("the dud is live"),
        cs_sim::weapons::ordnance::FuseDecision::Inert(FuseInert::AlreadyTriggered),
        "a retired item's fuse reports that it has already ended"
    );
}

/// Removing an item drops its tracker, so teardown cannot leave a stale id
/// behind; an unguided item registers no tracker at all.
#[test]
fn accept_f28_b_removing_an_item_drops_its_tracker() {
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &synthetic_guided_rocket(),
            &transform(),
            Some(actor(9)),
            STILL_AIR,
        )
        .expect("the fixture launches");
    assert!(runtime.guidance().require(&projectile(1)).is_ok());

    assert!(runtime.remove(&projectile(1)).is_some());
    assert!(
        runtime.remove(&projectile(1)).is_none(),
        "removal is idempotent"
    );
    assert!(
        runtime.guidance().require(&projectile(1)).is_err(),
        "the tracker is dropped with the item"
    );

    runtime
        .launch(
            actor(1),
            projectile(2),
            &synthetic_proximity_flak(),
            &transform(),
            None,
            STILL_AIR,
        )
        .expect("the unguided fixture launches");
    assert!(
        runtime.guidance().is_empty(),
        "an unguided item has no target to track"
    );
}

/// AC01 through the runtime: the proximity fuse is driven per tick and still
/// refuses before arming, then fires on the armed tick.
#[test]
fn accept_f28_b_the_runtime_drives_the_proximity_fuse_and_its_arming_gate() {
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &synthetic_proximity_flak(),
            &transform(),
            None,
            STILL_AIR,
        )
        .expect("the fixture launches");

    // Two ticks: not armed yet, whatever the geometry.
    for _ in 0..2 {
        runtime.advance(tick_seconds(), STILL_AIR).expect("advance");
        let at = runtime
            .get(&projectile(1))
            .expect("live")
            .current()
            .to_array();
        let path = TargetPath::try_new(actor(9), at, at).expect("a finite path");
        let decision = runtime
            .decide(&projectile(1), &[path], &[])
            .expect("the item is live");
        assert!(
            decision.is_not_armed(),
            "the fuse does not fire before the declared arming delay: {decision:?}"
        );
    }

    // The third tick arms the fuse; the target is on the item's path.
    runtime.advance(tick_seconds(), STILL_AIR).expect("advance");
    let at = runtime
        .get(&projectile(1))
        .expect("live")
        .current()
        .to_array();
    let path = TargetPath::try_new(actor(9), at, at).expect("a finite path");
    let decision = runtime
        .decide(&projectile(1), &[path], &[])
        .expect("the item is live");
    assert!(
        matches!(
            decision.trigger(),
            Some(FuseTrigger::Proximity { target, .. }) if target == actor(9)
        ),
        "the armed fuse fires on the near path: {decision:?}"
    );
}

/// Direct explosive through the runtime: an impact trigger routes its declared
/// channel damage once, and a second routing is refused by name.
#[test]
fn accept_f28_b_an_impact_trigger_routes_its_declared_damage_once() {
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &synthetic_direct_explosive(),
            &transform(),
            None,
            STILL_AIR,
        )
        .expect("the fixture launches");
    for _ in 0..3 {
        runtime.advance(tick_seconds(), STILL_AIR).expect("advance");
    }

    let impact = SweptHit {
        projectile: projectile(1),
        target: actor(9),
        time_of_impact: 0.5,
    };
    let decision = runtime
        .decide(&projectile(1), &[], &[impact])
        .expect("the item is live");
    assert!(
        matches!(
            decision.trigger(),
            Some(FuseTrigger::Impact { target, .. }) if target == actor(9)
        ),
        "the impact ends the item: {decision:?}"
    );

    let hits = runtime
        .route_trigger(SESSION, tick(3), &projectile(1), actor(9), node())
        .expect("a triggered item routes");
    assert_eq!(hits.len(), 2, "one hit per non-zero declared channel");
    assert!(
        hits.iter().all(|hit| hit.attacker == Some(actor(1))
            && hit.target == actor(9)
            && hit.node == node()),
        "each hit names the shooter, the target and the supplied node: {hits:?}"
    );

    assert_eq!(
        runtime.route_trigger(SESSION, tick(3), &projectile(1), actor(9), node()),
        Err(OrdnanceRuntimeError::AlreadyRouted {
            projectile: projectile(1)
        }),
        "one trigger routes its damage once"
    );
}

/// An un-triggered item routes no damage, and an unknown item is refused by
/// name rather than quietly skipped.
#[test]
fn accept_f28_b_routing_refuses_an_untriggered_or_unknown_item() {
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &synthetic_direct_explosive(),
            &transform(),
            None,
            STILL_AIR,
        )
        .expect("the fixture launches");
    assert_eq!(
        runtime.route_trigger(SESSION, tick(0), &projectile(1), actor(9), node()),
        Err(OrdnanceRuntimeError::NotTriggered {
            projectile: projectile(1)
        })
    );
    assert_eq!(
        runtime.route_trigger(SESSION, tick(0), &projectile(99), actor(9), node()),
        Err(OrdnanceRuntimeError::UnknownProjectile {
            projectile: projectile(99)
        })
    );
}

/// A launched item retires once its declared lifetime is spent, after its
/// final tick's segment.
#[test]
fn accept_f28_b_an_item_retires_after_its_declared_lifetime() {
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &synthetic_proximity_flak(),
            &transform(),
            None,
            STILL_AIR,
        )
        .expect("the fixture launches");
    let lifetime = runtime
        .get(&projectile(1))
        .expect("live")
        .state()
        .lifetime_ticks();
    let mut retired = false;
    for _ in 0..lifetime {
        let step = runtime.advance(tick_seconds(), STILL_AIR).expect("advance");
        if step.expired.contains(&projectile(1)) {
            retired = true;
        }
    }
    assert!(retired, "the spent item is reported expired");
    assert!(runtime.is_empty(), "the spent item is no longer live");
}

/// AC03 through the runtime: an item's declared status effect is applied to a
/// stable recipient, expires on its exact tick, and a restarted session starts
/// clean.
#[test]
fn accept_f28_b_a_timed_status_effect_expires_on_its_tick_and_resets() {
    let mut runtime = runtime();
    runtime
        .launch(
            actor(1),
            projectile(1),
            &synthetic_area_denial(),
            &transform(),
            None,
            STILL_AIR,
        )
        .expect("the fixture launches");
    let engine = StatusEffectTarget::actor_system(actor(2), SystemKind::Propulsion);
    let applied = runtime
        .apply_statuses(SESSION, tick(5), &projectile(1), engine)
        .expect("the fixture choke applies");
    assert_eq!(applied.len(), 1);
    assert!(
        runtime.status().is_under(&engine, StatusEffectKind::Choke),
        "the engine is choking"
    );
    assert!(
        !runtime.status().is_under(&engine, StatusEffectKind::Stall),
        "a choke is not a stall"
    );

    let expired = runtime
        .advance_status(SESSION, tick(5 + SYNTHETIC_CHOKE_TICKS))
        .expect("the ledger advances");
    assert_eq!(expired.len(), 1, "the effect expires exactly once");
    assert_eq!(
        expired[0].expired_at,
        tick(5 + SYNTHETIC_CHOKE_TICKS),
        "the expiry tick is the declared duration, in whole ticks"
    );
    assert!(runtime.status().is_empty(), "the expired effect is gone");

    let restarted = OrdnanceRuntime::new(SESSION + 1, tick(0), rate(), PRODUCER);
    assert!(
        restarted.status().is_empty(),
        "a restarted session inherits no effect"
    );
}

/// AC04 through the runtime: nitro changes thrust and consumption but carries
/// no pose or duration, and an unavailable activation consumes nothing.
#[test]
fn accept_f28_b_nitro_changes_thrust_and_consumption_but_never_a_pose() {
    let mut boosted = runtime();
    boosted.register_nitro(actor(1), synthetic_nitro_parameters());

    // The ledger opens at tick 0; the first elapsed tick is tick 1, which is
    // the first tick that can consume capacity.
    let active = boosted
        .request_nitro(&actor(1), tick(1), true)
        .expect("the booster is registered");
    assert!(active.is_active(), "the request is accepted");
    assert_eq!(
        active.extra_thrust_n, SYNTHETIC_NITRO_EXTRA_THRUST_N,
        "an accepted boost adds its declared thrust"
    );
    let spent = active.consumed_units;
    assert!(spent > 0.0, "an accepted boost consumes capacity");

    // A tiny tank exhausts in one tick; the next request is refused and
    // consumes nothing at all.
    let mut drained = runtime();
    drained.register_nitro(
        actor(1),
        cs_sim::weapons::ordnance::NitroParameters::try_new(
            0.1,
            3.0,
            0.0,
            4200.0,
            NitroActivationRule::WhileHeld,
            NitroTradeoffs::UNMEASURED,
        )
        .expect("the tiny fixture is valid"),
    );
    let first = drained
        .request_nitro(&actor(1), tick(1), true)
        .expect("the first request is accepted");
    assert!(first.is_active());
    let second = drained
        .request_nitro(&actor(1), tick(2), true)
        .expect("the refusal is not an error");
    assert!(second.is_refused(), "an empty tank refuses the request");
    assert_eq!(
        second.consumed_units, 0.0,
        "an unavailable boost consumes nothing"
    );
}

/// The launch boundary refuses a foreign session, a duplicate id and a
/// non-finite wind, each by name.
#[test]
fn accept_f28_b_launch_refuses_a_bad_request_by_name() {
    let definition = synthetic_proximity_flak();
    let mut runtime = runtime();
    assert_eq!(
        runtime.launch(
            actor(1),
            ProjectileId {
                session: SESSION + 1,
                serial: 1,
            },
            &definition,
            &transform(),
            None,
            STILL_AIR,
        ),
        Err(OrdnanceRuntimeError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        })
    );
    assert_eq!(
        runtime.launch(
            ActorId {
                session: session(SESSION + 1),
                serial: 1,
            },
            projectile(1),
            &definition,
            &transform(),
            None,
            STILL_AIR,
        ),
        Err(OrdnanceRuntimeError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        }),
        "a shooter from another generation is refused"
    );
    assert_eq!(
        runtime.launch(
            actor(1),
            projectile(1),
            &definition,
            &transform(),
            None,
            [f64::NAN, 0.0, 0.0],
        ),
        Err(OrdnanceRuntimeError::NonFiniteWind { component: 0 })
    );
    runtime
        .launch(
            actor(1),
            projectile(1),
            &definition,
            &transform(),
            None,
            STILL_AIR,
        )
        .expect("the first launch is accepted");
    assert_eq!(
        runtime.launch(
            actor(1),
            projectile(1),
            &definition,
            &transform(),
            None,
            STILL_AIR,
        ),
        Err(OrdnanceRuntimeError::DuplicateProjectile {
            projectile: projectile(1)
        })
    );
}
