//! Acceptance scenario F28-A (AC01 minimum scenario and its failure
//! cases): the swept proximity fuse, its arming boundary, the lost-target
//! contract, the bounded status-effect ledger and the nitro ledger.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-A`. Task test prefix: `accept_f28_a_`.
//!
//! These tests drive production code only: `cs_sim::weapons::ordnance`'s
//! [`closest_approach`], [`OrdnanceState`], [`GuidanceSet`],
//! [`StatusEffectLedger`], [`NitroLedger`] and [`OrdnanceRegistry`]. Removing
//! the relative-motion subtraction, the arming gate, the trigger latch, the
//! once-only lost-target report, the session check, the expiry tick, the
//! whole-tick capacity arithmetic or the import refusal makes one of them
//! fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access: these tests prove the interface and
//! the contract, never the original game.

use std::collections::BTreeSet;

use cs_sim::damage::{ActorId, DamageNodeKey, SystemKind};
use cs_sim::time::TickRate;
use cs_sim::weapons::ordnance::{
    ArmingRule, CompatibilityVerdict, EquipmentRules, FuseDecision, FuseInert, FuseRule,
    FuseTrigger, GuidanceError, GuidanceRule, GuidanceSet, GuidanceTracker, GuidanceUpdate,
    LostTargetBehavior, LostTargetReason, NitroActivationRule, NitroError, NitroLedger,
    NitroParameters, NitroRefusal, NitroTradeoffs, OrdnanceComponent, OrdnanceDefinitionError,
    OrdnanceFamily, OrdnanceId, OrdnanceRegistryError, OrdnanceState, OrdnanceStatusEffect,
    ProximityFuse, SYNTHETIC_AREA_DENIAL_KEY, SYNTHETIC_ARMING_TICKS, SYNTHETIC_DIRECT_KEY,
    SYNTHETIC_FLAK_KEY, SYNTHETIC_FLAK_LIFETIME_TICKS, SYNTHETIC_GUIDED_KEY,
    SYNTHETIC_NITRO_CONSUMPTION_PER_S, SYNTHETIC_NITRO_KEY, SYNTHETIC_TORPEDO_KEY,
    SYNTHETIC_TRIGGER_RADIUS_M, StatusEffectError, StatusEffectKind, StatusEffectLedger,
    StatusEffectTarget, TargetObservation, TargetPath, closest_approach, synthetic_aerial_torpedo,
    synthetic_area_denial, synthetic_area_effect, synthetic_choke, synthetic_direct_explosive,
    synthetic_guided_rocket, synthetic_nitro, synthetic_nitro_parameters, synthetic_ordnance,
    synthetic_proximity_flak, synthetic_registry,
};
use cs_sim::weapons::{InheritanceRule, MountTransform, ProjectileId, ProjectileSegment};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 11;
const TICKS_PER_SECOND: u32 = 30;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION,
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

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

/// A one-tick swept segment from `from` to `to`.
fn segment(from: [f64; 3], to: [f64; 3]) -> ProjectileSegment {
    ProjectileSegment {
        projectile: projectile(1),
        previous: position(from),
        current: position(to),
    }
}

/// A zero-length segment at one point: the shell sitting still, used to show
/// that the pass in the test above is missed by an endpoint-only test.
fn stationary(at: WorldPosition) -> ProjectileSegment {
    ProjectileSegment {
        projectile: projectile(1),
        previous: at,
        current: at,
    }
}

/// The tick length in seconds, for the fixture's declared per-second rates.
fn tick_seconds() -> f64 {
    1.0 / f64::from(TICKS_PER_SECOND)
}

/// An in-flight synthetic proximity-flak shell, released on `launched_at`.
fn flak_in_flight(launched_at: Tick) -> OrdnanceState {
    let definition = synthetic_proximity_flak();
    OrdnanceState::launch(
        projectile(1),
        definition.ordnance().clone(),
        definition.family(),
        definition.arming(),
        definition.fuse(),
        definition.lifetime_ticks(),
        launched_at,
    )
}

/// AC01 minimum scenario: a proximity fuse triggers for a fast near-pass.
///
/// The shell crosses the target's path at 210 m/s — 7 m per tick at 30 Hz —
/// while the trigger radius is [`SYNTHETIC_TRIGGER_RADIUS_M`]. The pass is
/// built so **both** of the shell's endpoints are further from the target's
/// path than the trigger radius and the crossing happens inside the tick,
/// which is exactly the case an endpoint-only proximity test misses.
#[test]
fn accept_f28_a_proximity_fuse_triggers_for_a_fast_near_pass() {
    let fuse_radius = SYNTHETIC_TRIGGER_RADIUS_M;
    assert!(
        fuse_radius > 7.0,
        "the pass below is faster than the radius"
    );

    // The shell runs from z = -60 to z = +60 this tick, straight past the
    // origin. The target sits still at the origin for the whole tick, so
    // the two paths cross at the middle of the tick.
    let shell = segment([0.0, 0.0, -60.0], [0.0, 0.0, 60.0]);
    let target =
        TargetPath::try_new(actor(2), [0.0; 3], [0.0; 3]).expect("the target path is valid");

    // The endpoints are outside the radius ...
    let at_start = closest_approach(&stationary(shell.previous), &target);
    let at_end = closest_approach(&stationary(shell.current), &target);
    assert!(
        !at_start.is_within(fuse_radius),
        "the start is outside the radius"
    );
    assert!(
        !at_end.is_within(fuse_radius),
        "the end is outside the radius"
    );

    // ... but the swept segment comes within it, halfway through the tick.
    let approach = closest_approach(&shell, &target);
    assert!(
        approach.is_within(fuse_radius),
        "the swept segment reaches {approach:?}, expected within {fuse_radius}"
    );
    assert!(
        (approach.time - 0.5).abs() < 1e-9,
        "the crossing is at the middle of the tick: {approach:?}"
    );

    // An armed, released shell therefore detonates on that near-pass.
    let mut state = flak_in_flight(tick(0));
    for _ in 0..SYNTHETIC_ARMING_TICKS {
        state.advance(&segment([0.0, 0.0, -60.0], [0.0, 0.0, -53.0]));
    }
    assert!(state.is_armed(), "the fuse arms after its declared delay");
    let decision = state.fuse_decision(&shell, std::slice::from_ref(&target), &[]);
    assert!(
        matches!(
            decision,
            FuseDecision::Triggered(FuseTrigger::Proximity { target: t, .. }) if t == actor(2)
        ),
        "the near-pass triggers proximity against the target: {decision:?}"
    );
}

/// AC01's failure case: the fuse is not live before the arming delay, so the
/// identical near-pass triggers nothing.
///
/// This is the half of the minimum scenario that a "just check the distance"
/// implementation gets wrong: it detonates the shell in its own launcher's
/// lap on the launch tick.
#[test]
fn accept_f28_a_proximity_fuse_does_not_trigger_before_arming() {
    let mut state = flak_in_flight(tick(0));
    let shell = segment([0.0, 0.0, -60.0], [0.0, 0.0, 60.0]);
    let target =
        TargetPath::try_new(actor(2), [0.0; 3], [0.0; 3]).expect("the target path is valid");

    // Ticks 0..SYNTHETIC_ARMING_TICKS - 1 are all un-armed, and each one
    // presents the same near-pass.
    for elapsed in 0..SYNTHETIC_ARMING_TICKS {
        assert!(
            !state.is_armed(),
            "the fuse must not be live after {elapsed} tick(s)"
        );
        let decision = state.fuse_decision(&shell, std::slice::from_ref(&target), &[]);
        assert!(
            decision.is_not_armed(),
            "an un-armed fuse must not trigger on the near-pass at {elapsed}: {decision:?}"
        );
        assert!(
            !state.is_triggered(),
            "an un-armed fuse must not latch a trigger at {elapsed}"
        );
        state.advance(&segment([0.0, 0.0, -60.0], [0.0, 0.0, -53.0]));
    }

    assert!(
        state.is_armed(),
        "the fuse is live after the declared delay"
    );
}

/// A travel-based arming rule is satisfied by distance flown, not by ticks
/// elapsed — a shell that never leaves the rail never arms.
#[test]
fn accept_f28_a_travel_based_arming_follows_distance_not_ticks() {
    let definition = synthetic_guided_rocket();
    assert_eq!(
        definition.arming(),
        ArmingRule::AfterTravelMetres(60.0),
        "the guided fixture arms on distance"
    );
    let mut state = OrdnanceState::launch(
        projectile(2),
        definition.ordnance().clone(),
        definition.family(),
        definition.arming(),
        definition.fuse(),
        definition.lifetime_ticks(),
        tick(0),
    );

    // Ten ticks of 1 m each: past the arming delay in wall time, still
    // short of the arming distance.
    for _ in 0..10 {
        assert!(!state.is_armed(), "ten meters must not arm a 60 m fuse");
        state.advance(&segment([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    }
    assert!(
        !state.is_armed(),
        "ten ticks of one meter is ten meters, not sixty"
    );
    for _ in 0..50 {
        state.advance(&segment([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]));
    }
    assert!(
        state.is_armed(),
        "sixty meters of travel arms the fuse: {}",
        state.travelled_m()
    );
}

/// The closest-approach sweep subtracts the target's own motion, so a
/// target that crosses the shell's path between two ticks is still within
/// range even though neither endpoint pair is.
#[test]
fn accept_f28_a_the_proximity_sweep_follows_relative_target_motion() {
    let shell = segment([0.0, 0.0, -60.0], [0.0, 0.0, 60.0]);
    // The target moves across from x = -200 to x = +200: it is nowhere near
    // the shell at either end of the tick, and directly on it halfway.
    let crossing = TargetPath::try_new(actor(3), [-200.0, 0.0, 0.0], [200.0, 0.0, 0.0])
        .expect("the crossing path is valid");
    let approach = closest_approach(&shell, &crossing);
    assert!(
        approach.distance_m < 1.0,
        "a crossing target is caught by the swept distance: {approach:?}"
    );

    // A target that stays put, far to one side, is not.
    let parallel = TargetPath::try_new(actor(4), [500.0, 0.0, -60.0], [500.0, 0.0, 60.0])
        .expect("the parallel path is valid");
    assert!(
        !closest_approach(&shell, &parallel).is_within(SYNTHETIC_TRIGGER_RADIUS_M),
        "a target that flies alongside at range is not triggered"
    );
}

/// One item detonates once: the fuse decision latches, so a second query in
/// the same tick — or a second contact report — names nothing new.
#[test]
fn accept_f28_a_one_item_detonates_once_however_many_contacts_report_it() {
    let mut state = flak_in_flight(tick(0));
    for _ in 0..SYNTHETIC_ARMING_TICKS {
        state.advance(&segment([0.0, 0.0, -60.0], [0.0, 0.0, -53.0]));
    }
    let shell = segment([0.0, 0.0, -60.0], [0.0, 0.0, 60.0]);
    let near = TargetPath::try_new(actor(2), [0.0; 3], [0.0; 3]).expect("the near path is valid");
    let far = TargetPath::try_new(actor(5), [300.0; 3], [300.0; 3]).expect("the far path is valid");

    let first = state.fuse_decision(&shell, &[near, far, near], &[]);
    assert!(first.is_triggered(), "the first query triggers: {first:?}");
    assert!(
        matches!(first.trigger(), Some(FuseTrigger::Proximity { target, .. }) if target == actor(2)),
        "the nearest path is the one that triggers: {first:?}"
    );

    let second = state.fuse_decision(&shell, std::slice::from_ref(&near), &[]);
    assert_eq!(
        second,
        FuseDecision::Inert(FuseInert::AlreadyTriggered),
        "a second query in the same tick adds nothing"
    );
}

/// AC02's minimum scenario: guidance loses a target safely when the target
/// is destroyed, and never re-acquires it.
///
/// The loss is reported **once** with its cause; every later tick reports the
/// declared consequence and the tracker holds no target.
#[test]
fn accept_f28_a_guidance_loses_a_destroyed_target_and_never_reacquires_it() {
    let definition = synthetic_guided_rocket();
    let rule = definition.guidance();
    assert!(
        matches!(
            rule,
            GuidanceRule::Targeted {
                lost_target: LostTargetBehavior::Detonate
            }
        ),
        "the guided fixture detonates on target loss: {rule:?}"
    );
    let target = actor(9);
    let mut set = GuidanceSet::new(SESSION);
    set.insert(GuidanceTracker::new(
        SESSION,
        projectile(4),
        rule,
        Some(target),
    ));

    // Alive: tracking.
    let updates = set.session_tick(SESSION, TargetObservation::alive(target, tick(5)));
    assert_eq!(
        updates.get(&projectile(4)).copied(),
        Some(GuidanceUpdate::Tracked { target }),
        "a live target keeps tracking"
    );

    // Destroyed: the loss is named exactly once.
    let updates = set.session_tick(SESSION, TargetObservation::destroyed(target, tick(6)));
    let lost = updates.get(&projectile(4)).copied();
    assert!(
        matches!(
            lost,
            Some(GuidanceUpdate::Lost {
                reason: LostTargetReason::Destroyed,
                behavior: LostTargetBehavior::Detonate,
            })
        ),
        "destruction is reported as a loss with the declared behavior: {lost:?}"
    );
    let tracker = set
        .tracker(&projectile(4))
        .expect("the tracker is registered");
    assert_eq!(tracker.target(), None, "a lost target is dropped");
    assert_eq!(
        tracker.lost(),
        Some(LostTargetReason::Destroyed),
        "the cause is remembered"
    );

    // The target cannot come back, even if a live actor of the same id is
    // reported on a later tick.
    let updates = set.session_tick(SESSION, TargetObservation::alive(target, tick(7)));
    let after = updates.get(&projectile(4)).copied();
    assert!(
        !after.is_some_and(|update| update.is_tracked()),
        "a tracker never re-acquires its lost target: {after:?}"
    );
    assert!(
        !matches!(after, Some(GuidanceUpdate::Lost { .. })),
        "the cause is not re-announced on every tick: {after:?}"
    );
}

/// AC02's second half: a session change loses the target too, so a stale id
/// cannot be resolved in the next generation's roster.
#[test]
fn accept_f28_a_guidance_loses_its_target_on_a_session_change() {
    let definition = synthetic_guided_rocket();
    let target = actor(9);
    let mut set = GuidanceSet::new(SESSION);
    set.insert(GuidanceTracker::new(
        SESSION,
        projectile(4),
        definition.guidance(),
        Some(target),
    ));

    let updates = set.session_tick(SESSION + 1, TargetObservation::alive(target, tick(6)));
    let update = updates.get(&projectile(4)).copied();
    assert!(
        matches!(
            update,
            Some(GuidanceUpdate::Lost {
                reason: LostTargetReason::ForeignSession { expected, found },
                ..
            }) if expected == SESSION && found == SESSION + 1
        ),
        "another generation is a lost target, not a lookup: {update:?}"
    );
    assert_eq!(
        set.tracker(&projectile(4)).map(GuidanceTracker::target),
        Some(None),
        "the target id is not carried across the session"
    );
}

/// The three declared lost-target behaviors are three different outcomes,
/// and only one of them re-announces the cause.
#[test]
fn accept_f28_a_each_lost_target_behavior_has_its_own_outcome() {
    let target = actor(9);
    /// Whether an update reports this lost-target behavior's consequence.
    type Expects = fn(&GuidanceUpdate) -> bool;
    let outcomes: [(LostTargetBehavior, Expects); 3] = [
        (LostTargetBehavior::Detonate, |update| {
            matches!(
                update,
                GuidanceUpdate::Lost {
                    behavior: LostTargetBehavior::Detonate,
                    ..
                }
            )
        }),
        (LostTargetBehavior::Coast, |update| {
            matches!(update, GuidanceUpdate::Coast)
        }),
        (LostTargetBehavior::Disarm, |update| {
            matches!(update, GuidanceUpdate::Disarmed)
        }),
    ];
    for (lost_target, expect) in outcomes {
        let mut tracker = GuidanceTracker::new(
            SESSION,
            projectile(5),
            GuidanceRule::Targeted { lost_target },
            Some(target),
        );
        assert_eq!(
            tracker.hold(),
            GuidanceUpdate::Tracked { target },
            "{lost_target} tracks a live target"
        );
        let first = tracker.lose(LostTargetReason::Destroyed);
        assert!(
            expect(&first),
            "{lost_target} must report its own consequence: {first:?}"
        );
        assert_eq!(
            tracker.target(),
            None,
            "{lost_target} still drops the target"
        );
        let second = tracker.lose(LostTargetReason::Despawned);
        assert!(
            !matches!(second, GuidanceUpdate::Lost { .. }),
            "{lost_target} must not re-announce a cause on a later tick: {second:?}"
        );
        if lost_target != LostTargetBehavior::Detonate {
            assert!(
                expect(&second),
                "{lost_target} keeps reporting its own consequence: {second:?}"
            );
        } else {
            // A detonating item has already ended on the tick its target
            // died, so a later call reports that rather than naming a cause
            // again for an event that happened once.
            assert_eq!(
                second,
                GuidanceUpdate::Disarmed,
                "an item that already detonated reports that it is done"
            );
        }
    }
}

/// An unguided item has no target to lose, and says so rather than
/// inventing one.
#[test]
fn accept_f28_a_an_unguided_item_has_no_target_to_lose() {
    let flak = synthetic_proximity_flak();
    assert!(
        !flak.guidance().is_targeted(),
        "a flak shell is unguided by definition"
    );
    let mut tracker = GuidanceTracker::new(SESSION, projectile(6), flak.guidance(), None);
    assert_eq!(tracker.hold(), GuidanceUpdate::Unguided);
    assert_eq!(
        tracker.lose(LostTargetReason::Destroyed),
        GuidanceUpdate::Unguided,
        "an unguided item cannot detonate on a target loss"
    );
    assert_eq!(tracker.target(), None);
}

/// A targeted item launched with **no** target is a different fact from one
/// that lost its target, and it is resolved by the rule's own declared
/// behavior: the cause is recorded once, and never re-announced.
#[test]
fn accept_f28_a_a_targeted_item_launched_without_a_target_reports_one_unassigned_loss() {
    for lost_target in LostTargetBehavior::ALL {
        let mut tracker = GuidanceTracker::new(
            SESSION,
            projectile(7),
            GuidanceRule::Targeted {
                lost_target: *lost_target,
            },
            None,
        );
        let first = tracker.hold();
        // `Disarm` is in the table deliberately: a defaulting implementation
        // that reported `Coast` for an unassigned target would be caught
        // here, because the declared behavior is the only outcome this item
        // may have.
        let expected_first = match lost_target {
            LostTargetBehavior::Detonate => GuidanceUpdate::Lost {
                reason: LostTargetReason::Unassigned,
                behavior: LostTargetBehavior::Detonate,
            },
            LostTargetBehavior::Coast => GuidanceUpdate::Coast,
            LostTargetBehavior::Disarm => GuidanceUpdate::Disarmed,
        };
        assert_eq!(
            first, expected_first,
            "{lost_target} resolves an unassigned target by its own declared behavior"
        );
        assert_eq!(
            tracker.lost(),
            Some(LostTargetReason::Unassigned),
            "{lost_target} records the cause once, and it is not 'despawned'"
        );
        let second = tracker.hold();
        assert!(
            !matches!(second, GuidanceUpdate::Lost { .. }),
            "{lost_target} does not re-announce the cause on a later tick: {second:?}"
        );
    }

    // The same resolution happens through the session's own tick, so no
    // caller has to remember to make it.
    let mut set = GuidanceSet::new(SESSION);
    set.insert(GuidanceTracker::new(
        SESSION,
        projectile(8),
        GuidanceRule::Targeted {
            lost_target: LostTargetBehavior::Detonate,
        },
        None,
    ));
    let updates = set.session_tick(SESSION, TargetObservation::alive(actor(11), tick(3)));
    assert!(
        matches!(
            updates.get(&projectile(8)).copied(),
            Some(GuidanceUpdate::Lost {
                reason: LostTargetReason::Unassigned,
                behavior: LostTargetBehavior::Detonate,
            })
        ),
        "an observation does not hand an untracked item a target: {updates:?}"
    );
    assert_eq!(
        set.tracker(&projectile(8)).map(GuidanceTracker::target),
        Some(None),
        "an unassigned item is never given a target"
    );
}

/// A set that is asked about an item it does not hold refuses by name, the
/// same way the ordnance registry refuses an unknown installation, so an
/// absent tracker is never silently skipped.
#[test]
fn accept_f28_a_guidance_refuses_an_unregistered_item_rather_than_skipping_it() {
    let mut set = GuidanceSet::new(SESSION);
    assert_eq!(
        set.require(&projectile(12)),
        Err(GuidanceError::UnknownProjectile {
            projectile: projectile(12)
        }),
        "an item that was never registered is refused by name"
    );
    assert!(set.is_empty());

    let guided = synthetic_guided_rocket();
    set.insert(GuidanceTracker::new(
        SESSION,
        projectile(13),
        guided.guidance(),
        Some(actor(9)),
    ));
    let tracker = set
        .require(&projectile(13))
        .expect("the item is registered");
    assert_eq!(
        tracker.target(),
        Some(actor(9)),
        "the registered item resolves"
    );
}

/// AC03's minimum scenario: a timed engine-status effect expires on the
/// exact tick its declared duration names, and the expiry is reported once.
#[test]
fn accept_f28_a_a_timed_engine_status_effect_expires_on_exactly_its_tick() {
    let choke = synthetic_choke();
    let duration = choke.duration_ticks();
    let source = synthetic_ordnance(SYNTHETIC_AREA_DENIAL_KEY);
    let engine = StatusEffectTarget::actor_system(actor(1), SystemKind::Propulsion);
    let mut ledger = StatusEffectLedger::new(SESSION, tick(100));

    let instance = ledger
        .apply(SESSION, tick(100), engine, &source, &choke)
        .expect("the effect applies in its own session");
    assert!(
        ledger.is_under(&engine, StatusEffectKind::Choke),
        "the engine is choked from the first tick"
    );
    assert_eq!(
        ledger.get(&instance).map(|effect| effect.expires_at),
        Some(tick(100 + duration)),
        "the expiry tick is the start tick plus the declared duration"
    );

    // One tick before the boundary the effect is still live ...
    let expired = ledger
        .advance_to(SESSION, tick(100 + duration - 1))
        .expect("the advance is in this session");
    assert!(
        expired.is_empty(),
        "nothing expires before the boundary: {expired:?}"
    );
    assert!(ledger.is_under(&engine, StatusEffectKind::Choke));

    // ... and on the boundary tick it is gone.
    let expired = ledger
        .advance_to(SESSION, tick(100 + duration))
        .expect("the advance is in this session");
    assert_eq!(
        expired.len(),
        1,
        "exactly one effect expires on the boundary: {expired:?}"
    );
    assert_eq!(expired[0].instance, instance);
    assert_eq!(expired[0].kind, StatusEffectKind::Choke);
    assert_eq!(
        expired[0].expired_at,
        tick(100 + duration),
        "the reported boundary is the declared one, not the tick asked for"
    );
    assert!(
        !ledger.is_under(&engine, StatusEffectKind::Choke),
        "the choke is gone on its expiry tick"
    );
    assert!(ledger.is_empty());

    // A re-entered schedule step on the same tick reports nothing further.
    let again = ledger
        .advance_to(SESSION, tick(100 + duration))
        .expect("a repeat advance is idempotent");
    assert!(
        again.is_empty(),
        "an expiry is applied once, not once per schedule step: {again:?}"
    );
}

/// AC03's second half: a restart clears the ledger. Effects belong to the
/// session that applied them, and another generation cannot query, advance
/// or inherit them.
#[test]
fn accept_f28_a_a_status_effect_ledger_resets_on_restart() {
    let choke = synthetic_choke();
    let source = synthetic_ordnance(SYNTHETIC_AREA_DENIAL_KEY);
    let engine = StatusEffectTarget::actor_system(actor(1), SystemKind::Propulsion);

    let mut first = StatusEffectLedger::new(SESSION, tick(0));
    first
        .apply(SESSION, tick(0), engine, &source, &choke)
        .expect("the effect applies");
    assert!(first.is_under(&engine, StatusEffectKind::Choke));

    // Another session's ledger starts empty and cannot see the old effect.
    let second = StatusEffectLedger::new(SESSION + 1, tick(0));
    assert!(
        second.is_empty(),
        "a new session starts with no inherited effects"
    );
    assert!(
        !second.is_under(&engine, StatusEffectKind::Choke),
        "the previous session's choke does not survive a restart"
    );

    // The old ledger refuses the new session rather than quietly accepting
    // it, in both directions.
    assert_eq!(
        first.advance_to(SESSION + 1, tick(1)),
        Err(StatusEffectError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        }),
        "advancing a ledger from another generation is refused"
    );
    assert!(
        first.is_under(&engine, StatusEffectKind::Choke),
        "a refused operation changes nothing"
    );
    assert_eq!(
        first.apply(SESSION + 1, tick(1), engine, &source, &choke),
        Err(StatusEffectError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        }),
        "applying from another generation is refused"
    );
}

/// A recipient is a stable id: an engine's choke and the aircraft's own
/// stall are different records even on the same frame.
#[test]
fn accept_f28_a_status_recipients_separate_the_actor_from_its_systems() {
    let choke = synthetic_choke();
    let marker = OrdnanceStatusEffect::try_new(StatusEffectKind::Marker, 10, 1.0)
        .expect("the marker effect is valid");
    let source = synthetic_ordnance(SYNTHETIC_AREA_DENIAL_KEY);
    let engine = StatusEffectTarget::actor_system(actor(1), SystemKind::Propulsion);
    let whole = StatusEffectTarget::whole_actor(actor(1));

    let mut ledger = StatusEffectLedger::new(SESSION, tick(0));
    ledger
        .apply(SESSION, tick(0), engine, &source, &choke)
        .expect("the engine choke applies");
    ledger
        .apply(SESSION, tick(0), whole, &source, &marker)
        .expect("the actor marker applies");

    assert!(ledger.is_under(&engine, StatusEffectKind::Choke));
    assert!(
        !ledger.is_under(&whole, StatusEffectKind::Choke),
        "an actor-wide recipient does not read as the engine's choke"
    );
    assert!(ledger.is_under(&whole, StatusEffectKind::Marker));
    assert!(
        !ledger.is_under(&engine, StatusEffectKind::Marker),
        "the engine recipient does not read the actor's marker"
    );
    assert_eq!(ledger.len(), 2, "two distinct recipients are two records");
}

/// An area effect is bounded: it may not outlive the item that made it, and
/// a declared projectile that tried to is refused.
#[test]
fn accept_f28_a_an_area_effect_may_not_outlive_its_item() {
    let area = synthetic_area_effect();
    let definition = synthetic_area_denial();
    assert_eq!(
        definition.area_effect(),
        Some(area),
        "the area-denial fixture declares its bounded area"
    );
    assert!(
        area.lifetime_ticks() <= definition.lifetime_ticks(),
        "the declared area is bounded by the item's own lifetime"
    );

    // Assembling the same area with a longer lifetime than the item is
    // refused by the runtime, which is where the bound is enforced.
    let too_long = cs_sim::weapons::ordnance::AreaEffect::try_new(
        area.radius_m(),
        definition.lifetime_ticks() + 1,
    )
    .expect("the radius itself is valid");
    let refusal = cs_sim::weapons::ordnance::ProjectileOrdnance::try_new(
        definition.ordnance().clone(),
        definition.family(),
        definition.launch().clone(),
        *definition.stack(),
        definition.arming(),
        definition.fuse(),
        definition.guidance(),
        definition.lifetime_ticks(),
        Some(too_long),
        *definition.channels(),
        definition.status().to_vec(),
        definition.media().clone(),
        EquipmentRules::default(),
    );
    assert!(
        matches!(
            refusal,
            Err(OrdnanceDefinitionError::AreaOutlivesItem { .. })
        ),
        "an area outliving its item is refused: {refusal:?}"
    );
}

/// Non-negotiable 1: a family whose declared guidance or fuse contradicts
/// what that family *is* is refused, so every rocket cannot silently be one
/// homing missile.
#[test]
fn accept_f28_a_no_family_may_declare_rules_that_contradict_it() {
    let flak = synthetic_proximity_flak();
    // A flak shell declared as a seeker is the substitution the sheet
    // forbids.
    let guided_flak = cs_sim::weapons::ordnance::ProjectileOrdnance::try_new(
        flak.ordnance().clone(),
        OrdnanceFamily::ProximityFlak,
        flak.launch().clone(),
        *flak.stack(),
        flak.arming(),
        flak.fuse(),
        GuidanceRule::Targeted {
            lost_target: LostTargetBehavior::Coast,
        },
        flak.lifetime_ticks(),
        flak.area_effect(),
        *flak.channels(),
        flak.status().to_vec(),
        flak.media().clone(),
        EquipmentRules::default(),
    );
    assert!(
        matches!(
            guided_flak,
            Err(OrdnanceDefinitionError::IncoherentFamily {
                field: "guidance",
                ..
            })
        ),
        "an unguided family may not be declared as a seeker: {guided_flak:?}"
    );

    // A guided rocket declared unguided is the same contradiction the other
    // way round.
    let guided = synthetic_guided_rocket();
    let unguided_guided = cs_sim::weapons::ordnance::ProjectileOrdnance::try_new(
        guided.ordnance().clone(),
        OrdnanceFamily::GuidedRocket,
        guided.launch().clone(),
        *guided.stack(),
        guided.arming(),
        guided.fuse(),
        GuidanceRule::Unguided,
        guided.lifetime_ticks(),
        guided.area_effect(),
        *guided.channels(),
        guided.status().to_vec(),
        guided.media().clone(),
        EquipmentRules::default(),
    );
    assert!(
        matches!(
            unguided_guided,
            Err(OrdnanceDefinitionError::IncoherentFamily {
                field: "guidance",
                ..
            })
        ),
        "a guided family may not be declared unguided: {unguided_guided:?}"
    );

    // An area-denial item cannot be impact-fused: its effect is the area, so
    // a contact end would mean the area never happened.
    let denial = synthetic_area_denial();
    let impact_denial = cs_sim::weapons::ordnance::ProjectileOrdnance::try_new(
        denial.ordnance().clone(),
        OrdnanceFamily::AreaDenialEngine,
        denial.launch().clone(),
        *denial.stack(),
        denial.arming(),
        FuseRule::Impact,
        denial.guidance(),
        denial.lifetime_ticks(),
        denial.area_effect(),
        *denial.channels(),
        denial.status().to_vec(),
        denial.media().clone(),
        EquipmentRules::default(),
    );
    assert!(
        matches!(
            impact_denial,
            Err(OrdnanceDefinitionError::IncoherentFamily { field: "fuse", .. })
        ),
        "an area-denial family ends on its clock, not on contact: {impact_denial:?}"
    );

    // A nitro booster is not a launched item and may not carry a fuse.
    assert!(
        matches!(
            cs_sim::weapons::ordnance::ProjectileOrdnance::try_new(
                synthetic_nitro().ordnance().clone(),
                OrdnanceFamily::NitroBooster,
                flak.launch().clone(),
                *flak.stack(),
                flak.arming(),
                flak.fuse(),
                flak.guidance(),
                flak.lifetime_ticks(),
                None,
                *flak.channels(),
                Vec::new(),
                flak.media().clone(),
                EquipmentRules::default(),
            ),
            Err(OrdnanceDefinitionError::NotAProjectile { .. })
        ),
        "a booster family carries no fuse, lifetime and guidance rule"
    );
}

/// The five projectile families and the booster are five different declared
/// behaviors, not one behavior spelled six ways.
#[test]
fn accept_f28_a_the_registry_holds_six_distinct_declared_behaviors() {
    let registry = synthetic_registry();
    assert_eq!(
        registry.len(),
        6,
        "one synthetic component per family: {:?}",
        registry.families()
    );

    let behaviors: Vec<(OrdnanceFamily, String, bool, bool)> = registry
        .families()
        .iter()
        .filter_map(|(family, ids)| {
            let id = ids.first()?;
            let component = registry.get(id).expect("the id came from the registry");
            let projectile = component.as_projectile()?;
            Some((
                *family,
                projectile.fuse().label(),
                projectile.guidance().is_targeted(),
                projectile.area_effect().is_some(),
            ))
        })
        .collect();
    assert_eq!(behaviors.len(), 5, "five projectile components");

    // No two projectile components share a fuse label ...
    let mut fuses: Vec<String> = behaviors
        .iter()
        .map(|(_, fuse, _, _)| fuse.clone())
        .collect();
    fuses.sort();
    let unique = {
        let mut deduped = fuses.clone();
        deduped.dedup();
        deduped
    };
    assert!(
        !unique.is_empty() && unique.len() < fuses.len(),
        "at least two families differ in their fuse: {fuses:?}"
    );
    // ... and exactly one of them is a seeker.
    assert_eq!(
        behaviors
            .iter()
            .filter(|(_, _, targeted, _)| *targeted)
            .count(),
        1,
        "exactly one family is guided: {behaviors:?}"
    );
    // ... and exactly one leaves a bounded area.
    assert_eq!(
        behaviors.iter().filter(|(_, _, _, area)| *area).count(),
        1,
        "exactly one family denies an area: {behaviors:?}"
    );

    // The booster is reachable only as a booster.
    let nitro = synthetic_nitro();
    let component = registry
        .require(nitro.ordnance())
        .expect("the booster is registered");
    assert_eq!(component.family(), OrdnanceFamily::NitroBooster);
    assert!(component.as_projectile().is_none());
    assert!(component.as_nitro().is_some());
}

/// Non-negotiable 5: an import naming a component the registry does not have
/// is refused, not skipped, so it cannot reach a session by way of the
/// import path.
#[test]
fn accept_f28_a_an_unsupported_installation_is_refused_rather_than_skipped() {
    let registry = synthetic_registry();
    let supported = synthetic_ordnance(SYNTHETIC_DIRECT_KEY);
    let unknown = OrdnanceId::try_new(
        ContentId::from_source(ContentKind::Weapon, "synthetic.fixture_not_in_the_registry")
            .expect("valid id"),
    )
    .expect("the weapon namespace is correct");

    let equipment = BTreeSet::new();
    let resolved = registry
        .resolve_installation(std::slice::from_ref(&supported), &equipment)
        .expect("a known component resolves");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].family, OrdnanceFamily::DirectExplosive);
    assert_eq!(resolved[0].compatibility, CompatibilityVerdict::Compatible);

    assert_eq!(
        registry.resolve_installation(&[supported, unknown.clone()], &equipment),
        Err(OrdnanceRegistryError::UnknownOrdnance { ordnance: unknown }),
        "a loadout with a hole in it is refused whole"
    );
    let twice = [
        synthetic_ordnance(SYNTHETIC_DIRECT_KEY),
        synthetic_ordnance(SYNTHETIC_DIRECT_KEY),
    ];
    assert!(
        matches!(
            registry.resolve_installation(&twice, &equipment),
            Err(OrdnanceRegistryError::DuplicateInstallation { .. })
        ),
        "two components claiming one slot cannot both be installed"
    );

    // A duplicate catalog id is refused at registration rather than letting
    // the later component win.
    let mut registry = synthetic_registry();
    assert_eq!(
        registry.register(OrdnanceComponent::Nitro(Box::new(synthetic_nitro()))),
        Err(OrdnanceRegistryError::DuplicateOrdnance {
            ordnance: synthetic_ordnance(SYNTHETIC_NITRO_KEY),
            family: OrdnanceFamily::NitroBooster,
        }),
        "the registry never replaces a declared component"
    );
}

/// The equipment rules are one shared verdict, so the shop and an import
/// read the same answer: a missing requirement and a forbidden piece of
/// equipment are named apart.
#[test]
fn accept_f28_a_equipment_rules_name_each_incompatibility() {
    let required =
        ContentId::from_source(ContentKind::HardpointEquipment, "synthetic.fixture_rack")
            .expect("valid id");
    let forbidden =
        ContentId::from_source(ContentKind::HardpointEquipment, "synthetic.fixture_ballast")
            .expect("valid id");
    let rules = EquipmentRules::new(Some(required.clone()), BTreeSet::from([forbidden.clone()]));

    assert_eq!(
        rules.check(&BTreeSet::new()),
        CompatibilityVerdict::MissingRequired {
            required: required.clone()
        }
    );
    assert_eq!(
        rules.check(&BTreeSet::from([required.clone(), forbidden.clone()])),
        CompatibilityVerdict::Forbidden {
            forbidden: forbidden.clone()
        },
        "a forbidden item is refused by name"
    );
    let compatible = rules.check(&BTreeSet::from([required]));
    assert!(
        compatible.is_compatible(),
        "carrying the requirement and none of the prohibitions is compatible"
    );
}

/// The direct-explosive fixture fuses on contact, and the timed fuse fires
/// exactly on its declared tick rather than on the next one.
#[test]
fn accept_f28_a_a_timed_fuse_fires_exactly_on_its_declared_tick() {
    let definition = synthetic_direct_explosive();
    assert_eq!(definition.fuse(), FuseRule::Impact);
    let area = synthetic_area_effect();
    let ticks = 5;
    let mut state = OrdnanceState::launch(
        projectile(7),
        synthetic_ordnance(SYNTHETIC_AREA_DENIAL_KEY),
        OrdnanceFamily::AreaDenialEngine,
        ArmingRule::AfterTicks(0),
        FuseRule::Timed { ticks },
        ticks,
        tick(0),
    );
    let step = || segment([0.0, 0.0, 0.0], [0.0, 0.0, -1.0]);

    for elapsed in 0..ticks {
        assert!(
            !state.fuse_decision(&step(), &[], &[]).is_triggered(),
            "the timed fuse must not fire before tick {ticks} (at {elapsed})"
        );
        state.advance(&step());
    }
    let fired = state.fuse_decision(&step(), &[], &[]);
    assert_eq!(
        fired,
        FuseDecision::Triggered(FuseTrigger::Timed { ticks }),
        "the timed fuse fires on exactly its declared tick"
    );
    assert!(
        state.fuse_decision(&step(), &[], &[]) != fired,
        "a timed fuse fires once"
    );
    assert!(
        area.lifetime_ticks() <= SYNTHETIC_FLAK_LIFETIME_TICKS * 2,
        "the area-denial fixture's area lifetime is a small multiple of a shell's, not unbounded"
    );
    assert_eq!(
        state.fuse(),
        FuseRule::Timed { ticks },
        "the in-flight item carries the declared fuse"
    );
}

/// AC04's minimum scenario: nitro changes thrust and consumption, and it
/// has no way to move an airframe or scale a render frame.
#[test]
fn accept_f28_a_nitro_changes_thrust_and_consumption_and_nothing_else() {
    let parameters = synthetic_nitro_parameters();
    let per_tick = SYNTHETIC_NITRO_CONSUMPTION_PER_S * tick_seconds();
    let mut ledger = NitroLedger::new(SESSION, tick(0), rate(), parameters);
    assert_eq!(ledger.capacity_units(), parameters.capacity_units());

    let held = ledger
        .request(SESSION, tick(1), true)
        .expect("the request is in this session");
    assert!(held.is_active(), "a held control with capacity runs nitro");
    assert_eq!(
        held.extra_thrust_n,
        parameters.extra_thrust_n(),
        "nitro reports a thrust modifier"
    );
    assert!(
        (held.consumed_units - per_tick).abs() < 1e-9,
        "one tick of capacity is consumed: {held:?}"
    );
    assert_eq!(held.authority_multiplier, 1.0, "no tradeoff is invented");

    let released = ledger
        .request(SESSION, tick(2), false)
        .expect("the request is in this session");
    assert!(!released.is_active(), "a released control stops nitro");
    assert_eq!(released.extra_thrust_n, 0.0, "no thrust modifier when idle");
    assert_eq!(
        released.consumed_units, 0.0,
        "an idle tick consumes nothing"
    );
    assert_eq!(
        released.authority_multiplier, 1.0,
        "authority is untouched when idle"
    );
}

/// `FLIGHT-PHYSICS`: pressing the control while nitro is unavailable
/// consumes no capacity, and the refusal is named.
#[test]
fn accept_f28_a_pressing_nitro_without_capacity_consumes_nothing() {
    // A capacity of exactly one tick's consumption, so it is empty after one
    // accepted tick.
    let per_tick = SYNTHETIC_NITRO_CONSUMPTION_PER_S * tick_seconds();
    let parameters = NitroParameters::try_new(
        per_tick,
        SYNTHETIC_NITRO_CONSUMPTION_PER_S,
        0.0,
        100.0,
        NitroActivationRule::WhileHeld,
        NitroTradeoffs::UNMEASURED,
    )
    .expect("the parameters are valid");
    let mut ledger = NitroLedger::new(SESSION, tick(0), rate(), parameters);

    let first = ledger.request(SESSION, tick(1), true).expect("valid");
    assert!(first.is_active(), "the first tick drains the capacity");
    assert_eq!(ledger.capacity_units(), 0.0);

    let second = ledger
        .request(SESSION, tick(2), true)
        .expect("the request is in this session");
    assert_eq!(
        second.refused,
        Some(NitroRefusal::CapacityExhausted),
        "an unavailable boost is refused by name"
    );
    assert!(!second.is_active());
    assert_eq!(
        second.consumed_units, 0.0,
        "an unavailable boost consumes no capacity"
    );
    assert_eq!(
        ledger.capacity_units(),
        0.0,
        "a refused request changes nothing"
    );
}

/// A fixed-duration burn runs for its declared number of whole ticks, so the
/// burn length is the same on every host and at every frame rate.
///
/// The refusal applies to *starting another* activation, never to the burn
/// already accepted: holding the control through the burn must not stop it.
#[test]
fn accept_f28_a_a_fixed_nitro_burn_lasts_exactly_its_declared_ticks() {
    let ticks = 3u64;
    let parameters = NitroParameters::try_new(
        100.0,
        1.0,
        0.0,
        100.0,
        NitroActivationRule::FixedTicks { ticks },
        NitroTradeoffs::UNMEASURED,
    )
    .expect("the parameters are valid");
    let mut ledger = NitroLedger::new(SESSION, tick(0), rate(), parameters);

    let first = ledger
        .request(SESSION, tick(1), true)
        .expect("the activation is accepted");
    assert!(first.is_active(), "the burn starts on the accepted tick");

    // A second request while the burn is running is refused by name and
    // changes nothing — and the accepted burn keeps running.
    let second = ledger
        .request(SESSION, tick(2), true)
        .expect("the request is in this session");
    assert_eq!(
        second.refused,
        Some(NitroRefusal::BurnAlreadyRunning { until: tick(4) }),
        "a running burn refuses a second activation by name"
    );
    assert!(
        second.is_active(),
        "a refused second activation does not stop the accepted burn: {second:?}"
    );
    assert_eq!(
        second.extra_thrust_n,
        parameters.extra_thrust_n(),
        "the running burn still reports its thrust modifier: {second:?}"
    );
    assert!(
        second.consumed_units > 0.0,
        "a running burn still consumes capacity: {second:?}"
    );

    // Releasing the control does not end a fixed burn either.
    let third = ledger
        .request(SESSION, tick(3), false)
        .expect("the request is in this session");
    assert!(
        third.is_active(),
        "a fixed burn runs for its declared length whatever the control does: {third:?}"
    );

    // The burn is over on the tick its declared length reaches.
    assert!(ledger.burn_running());
    let fourth = ledger
        .request(SESSION, tick(4), false)
        .expect("the request is in this session");
    assert!(!fourth.is_active(), "the burn ends on its declared tick");
    assert_eq!(
        fourth.consumed_units, 0.0,
        "an idle tick after a fixed burn consumes nothing"
    );
}

/// The idle-only recovery rule holds during a fixed burn: capacity is being
/// spent for as long as the burn runs, and a refused second activation must
/// not pay out recovery on the ticks the pilot is already paying for.
#[test]
fn accept_f28_a_a_running_fixed_nitro_burn_never_recovers_capacity() {
    let recovery = 5.0;
    let parameters = NitroParameters::try_new(
        50.0,
        1.0,
        recovery,
        100.0,
        NitroActivationRule::FixedTicks { ticks: 3 },
        NitroTradeoffs::UNMEASURED,
    )
    .expect("the parameters are valid");
    let per_tick = parameters.consumption_per_s() * tick_seconds();
    let recovery_per_tick = recovery * tick_seconds();
    assert!(
        recovery_per_tick > per_tick,
        "a one-tick recovery outweighs a one-tick consumption, so a stray \
         recovery would be visible in the capacity"
    );

    let mut ledger = NitroLedger::new(SESSION, tick(0), rate(), parameters);
    ledger
        .request(SESSION, tick(1), true)
        .expect("the activation is accepted");
    let mut previous = ledger.capacity_units();
    for step in 2..=3 {
        let update = ledger
            .request(SESSION, tick(step), true)
            .expect("the request is in this session");
        assert!(
            update.is_active(),
            "the burn is still running on tick {step}: {update:?}"
        );
        assert_eq!(
            update.consumed_units, per_tick,
            "each running tick consumes exactly one tick of capacity: {update:?}"
        );
        assert!(
            ledger.capacity_units() < previous,
            "a running burn only ever spends capacity: {} -> {}",
            previous,
            ledger.capacity_units()
        );
        previous = ledger.capacity_units();
    }

    // Once the burn is over, the same ledger does recover while idle.
    let idle = ledger
        .request(SESSION, tick(4), false)
        .expect("the request is in this session");
    assert!(!idle.is_active());
    assert!(
        ledger.capacity_units() > previous,
        "an idle tick recovers capacity again: {} -> {}",
        previous,
        ledger.capacity_units()
    );
}

/// Nitro capacity is measured in whole ticks of the declared rate: the same
/// number is consumed whether the caller walks one tick or jumps several.
#[test]
fn accept_f28_a_nitro_consumption_is_whole_ticks_not_frame_time() {
    let parameters = synthetic_nitro_parameters();
    let per_tick = SYNTHETIC_NITRO_CONSUMPTION_PER_S * tick_seconds();

    let mut walked = NitroLedger::new(SESSION, tick(0), rate(), parameters);
    for step in 1..=10 {
        walked
            .request(SESSION, tick(step), true)
            .expect("the request is in this session");
    }
    let mut jumped = NitroLedger::new(SESSION, tick(0), rate(), parameters);
    let jumped_tick = jumped
        .request(SESSION, tick(10), true)
        .expect("the request is in this session");

    assert!(
        (walked.capacity_units() - jumped.capacity_units()).abs() < 1e-9,
        "ten ticks cost the same walked or jumped: {} vs {}",
        walked.capacity_units(),
        jumped.capacity_units()
    );
    assert!(
        (jumped_tick.consumed_units - per_tick * 10.0).abs() < 1e-9,
        "a ten-tick jump consumes ten ticks of capacity: {jumped_tick:?}"
    );
}

/// A nitro ledger belongs to one session and one tick direction: a foreign
/// generation or a tick from the past is refused rather than applied.
#[test]
fn accept_f28_a_nitro_refuses_a_foreign_session_and_a_backwards_tick() {
    let mut ledger = NitroLedger::new(SESSION, tick(10), rate(), synthetic_nitro_parameters());
    assert_eq!(
        ledger.request(SESSION + 1, tick(11), true),
        Err(NitroError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        }),
        "another generation is refused"
    );
    assert_eq!(
        ledger.request(SESSION, tick(9), true),
        Err(NitroError::NonMonotonicTick {
            at: tick(10),
            found: tick(9),
        }),
        "time does not run backwards"
    );
    assert_eq!(
        ledger.capacity_units(),
        synthetic_nitro_parameters().capacity_units(),
        "a refused request consumes nothing"
    );
}

/// The launch pose is supplied by the hierarchy, and the declared
/// inheritance rule — not an assumed one — composes the release velocity.
#[test]
fn accept_f28_a_the_launch_velocity_composes_the_declared_inheritance_rule() {
    let torpedo = synthetic_aerial_torpedo();
    let transform = MountTransform::try_new(
        position([10.0, 0.0, 0.0]),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("forward is a unit axis"),
        [100.0, 0.0, 0.0],
    )
    .expect("the transform is valid");

    assert_eq!(
        torpedo.launch().inheritance(),
        InheritanceRule::None,
        "the fixture inherits none of the launcher's velocity"
    );
    let released = torpedo.launch().release_velocity_mps(&transform);
    assert!(
        released[0].abs() < 1e-12,
        "no inherited lateral velocity is invented: {released:?}"
    );
    assert_eq!(
        released[2],
        -torpedo.launch().launch_speed_mps(),
        "the declared launch speed is along the mount's forward axis"
    );
}

/// A launcher mount is the F29 weapon-mount damage node, so the node a
/// destroyed mount disables is literally the launcher that stops firing.
#[test]
fn accept_f28_a_the_launcher_mount_is_the_f29_weapon_mount_node() {
    let flak = synthetic_proximity_flak();
    let mount: &DamageNodeKey = flak.launch().mount();
    assert!(
        DamageNodeKey::new(mount.as_str()).is_ok(),
        "the launcher mount is a damage-node key: {mount}"
    );
    assert_ne!(
        mount.as_str(),
        flak.ordnance().as_str(),
        "a mount and a component are different identities"
    );
    assert_eq!(
        flak.stack().loaded_mass_kg(),
        flak.stack().unit_mass_kg() * flak.stack().capacity_units() as f64,
        "the declared stack mass is capacity times unit mass"
    );
}

/// The synthetic fixture's own lifetime is bounded and its arming delay is
/// inside it, so the fixture is internally consistent.
#[test]
fn accept_f28_a_the_synthetic_fixture_is_internally_consistent() {
    let flak = synthetic_proximity_flak();
    assert_eq!(flak.lifetime_ticks(), SYNTHETIC_FLAK_LIFETIME_TICKS);
    assert!(
        SYNTHETIC_ARMING_TICKS < flak.lifetime_ticks(),
        "the fixture arms before it expires"
    );
    assert_eq!(
        flak.fuse(),
        FuseRule::Proximity(
            ProximityFuse::try_new(SYNTHETIC_TRIGGER_RADIUS_M)
                .expect("the fixture radius is valid")
        ),
        "the fixture's fuse is the declared proximity fuse"
    );
    assert!(
        flak.status().is_empty(),
        "a proximity shell declares no timed status effect"
    );
    let denial = synthetic_area_denial();
    assert_eq!(
        denial.status().len(),
        1,
        "the area-denial fixture declares its choke"
    );
    assert_eq!(denial.status()[0].kind(), StatusEffectKind::Choke);
}

/// The synthetic nitro fixture declares **no** invented tradeoff, because
/// the original's tradeoff is unknown.
#[test]
fn accept_f28_a_the_nitro_fixture_invents_no_tradeoff() {
    let tradeoffs = synthetic_nitro_parameters().tradeoffs();
    assert!(
        tradeoffs.is_unmeasured(),
        "the fixture must not declare an authority penalty nobody measured"
    );
    assert_eq!(tradeoffs.authority_multiplier(), 1.0);
}

/// The direct-explosive and torpedo fixtures differ in their declared
/// behavior, which is what keeps the catalogue from collapsing to one
/// component.
#[test]
fn accept_f28_a_the_direct_and_torpedo_fixtures_differ() {
    let direct = synthetic_direct_explosive();
    let torpedo = synthetic_aerial_torpedo();
    assert_eq!(direct.family(), OrdnanceFamily::DirectExplosive);
    assert_eq!(torpedo.family(), OrdnanceFamily::AerialTorpedo);
    assert_ne!(
        direct.fuse(),
        torpedo.fuse(),
        "the two families do not share a fuse"
    );
    assert_ne!(
        direct.ordnance().as_str(),
        torpedo.ordnance().as_str(),
        "the two families are distinct catalog entries"
    );
}

/// A component's media is presentation only: the status ledger and the fuse
/// state never reach it, so dropping the particle costs a picture and never
/// a detonation.
#[test]
fn accept_f28_a_media_is_presentation_only() {
    let flak = synthetic_proximity_flak();
    assert!(
        flak.media().particles().is_some(),
        "the fixture declares a particle resource"
    );
    assert_eq!(
        flak.media().particles(),
        flak.media().particles(),
        "the particle is reachable only through the media record"
    );
    // Neither the fuse state nor the ledger carries a media field, which is
    // the structural form of non-negotiable 3.
    let mut state = flak_in_flight(tick(0));
    for _ in 0..SYNTHETIC_ARMING_TICKS {
        state.advance(&segment([0.0, 0.0, -60.0], [0.0, 0.0, -53.0]));
    }
    let decision = state.fuse_decision(
        &segment([0.0, 0.0, -60.0], [0.0, 0.0, 60.0]),
        &[TargetPath::try_new(actor(2), [0.0; 3], [0.0; 3]).expect("the target path is valid")],
        &[],
    );
    let FuseDecision::Triggered(FuseTrigger::Proximity { target, .. }) = decision else {
        panic!("the fixture fuse triggers: {decision:?}");
    };
    assert_eq!(
        target,
        actor(2),
        "the trigger names a target, not a particle"
    );
}

/// The catalogue keys the tests use are distinct, so a fixture id cannot
/// shadow another family's entry.
#[test]
fn accept_f28_a_the_synthetic_catalogue_keys_are_distinct() {
    let keys = [
        SYNTHETIC_DIRECT_KEY,
        SYNTHETIC_FLAK_KEY,
        SYNTHETIC_GUIDED_KEY,
        SYNTHETIC_AREA_DENIAL_KEY,
        SYNTHETIC_TORPEDO_KEY,
        SYNTHETIC_NITRO_KEY,
    ];
    let mut unique = keys.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), keys.len(), "the fixture keys are distinct");
    assert_eq!(
        synthetic_registry().len(),
        keys.len(),
        "the fixture registry holds every family exactly once"
    );
    assert_eq!(SYNTHETIC_AREA_DENIAL_KEY, keys[3]);
}
