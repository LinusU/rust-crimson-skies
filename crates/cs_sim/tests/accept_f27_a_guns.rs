//! Acceptance scenario F27-A (AC01 minimum scenario and its failure cases):
//! the swept-segment hit test, the once-per-projectile ledger, the
//! once-per-intent fire resolution and the per-mount weapon state.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
//! stage `### F27-A`. Task test prefix: `accept_f27_a_`.
//!
//! These tests drive production code only: [`cs_sim::weapons`]'s
//! [`Ballistics`], [`FireResolver`], [`WeaponState`] and
//! [`GunDefinition`]. Removing the slab test, the relative-motion
//! subtraction, the time-of-impact ordering, the once-per-projectile ledger,
//! the once-per-intent refusal or the per-mount cooldown makes one of them
//! fail.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data. No `CS_GAME_DIR` access: these tests prove the
//! interface and the contract, never the original game.

use std::collections::BTreeMap;

use cs_sim::damage::{ActorId, DamageChannel, DamageNodeKey};
use cs_sim::targeting::Allegiance;
use cs_sim::weapons::{
    AmmunitionId, Ballistics, FireDenialReason, FireError, FireEvent, FireIntent, FireIntentId,
    FireResolver, FriendlyFireRule, GunBank, GunBankError, GunDefinition, GunDefinitionError,
    GunMountKind, GunRate, GunStateError, InheritanceRule, IntentRefusal, MountTransform,
    MountTransformError, ProjectileId, ProjectileSegment, SYNTHETIC_STARTING_ROUNDS,
    SYNTHETIC_TICKS_BETWEEN_SHOTS, SelfHitRule, SpreadCone, SweepTarget, SweepTargetError,
    WeaponDamage, WeaponRules, WeaponState, synthetic_ammunition, synthetic_gun_definition,
    synthetic_mount,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 11;

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION,
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test mount keys are valid")
}

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

fn projectile(serial: u64) -> ProjectileId {
    ProjectileId {
        session: SESSION,
        serial,
    }
}

fn intent(tick: u64, sequence: u32) -> FireIntent {
    FireIntent {
        id: FireIntentId {
            session: SESSION,
            tick: Tick(tick),
            producer: 1,
            sequence,
        },
        shooter: actor(1),
    }
}

/// A forward-facing mount transform at `origin` flying at `velocity`.
fn transform(origin: [f64; 3], velocity: [f64; 3]) -> MountTransform {
    MountTransform::try_new(
        position(origin),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("forward is a unit axis"),
        velocity,
    )
    .expect("test transforms are valid")
}

fn transforms_for(
    mount: &DamageNodeKey,
    origin: [f64; 3],
) -> BTreeMap<DamageNodeKey, MountTransform> {
    BTreeMap::from([(mount.clone(), transform(origin, [0.0; 3]))])
}

/// The simulation rate the fixture numbers are written against: 30 ticks a
/// second, the rate the game's own flight work uses.
const TICKS_PER_SECOND: f64 = 30.0;

fn tick_seconds() -> f64 {
    1.0 / TICKS_PER_SECOND
}

/// A target box centred on the world origin, thin along the flight axis.
fn thin_target(actor: ActorId, half_extents: [f64; 3]) -> SweepTarget {
    SweepTarget::try_new(actor, [0.0; 3], [0.0; 3], half_extents).expect("test targets are valid")
}

/// A resolver with the fixture gun registered on actor 1 and the fixture
/// bank selected.
fn resolver() -> FireResolver {
    let definition = synthetic_gun_definition();
    let mount = definition.mount().clone();
    let state = WeaponState::try_new(
        std::slice::from_ref(&definition),
        GunBank::try_new([mount.clone()]).expect("the fixture bank names a mount"),
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("the fixture state is valid");
    let mut resolver = FireResolver::new(SESSION, Tick(0));
    resolver
        .register(actor(1), vec![definition], state)
        .expect("the fixture gun registers");
    resolver
}

/// AC01 minimum scenario: at high velocity a projectile crosses a thin
/// target and hits once.
///
/// 1200 m/s at a 1/30 s tick advances 40 m per tick, against a target 0.5 m
/// thick along the flight axis — `speed * dt` is eighty times the
/// thickness, exactly the case `FLIGHT-PHYSICS` says a discrete
/// endpoint-only implementation must provably fail. Both segment endpoints
/// are asserted outside the box, so only a swept test can find the hit; the
/// hit lands at the expected time of impact and lands **once**.
#[test]
fn accept_f27_a_high_velocity_projectile_crosses_a_thin_target_and_hits_once() {
    let speed_mps = 1200.0;
    let travel_m = speed_mps * tick_seconds();
    let thickness_m = 0.5;
    let half = [3.0, 3.0, thickness_m / 2.0];

    // The projectile starts 10 m short of the box's near face and ends 29.5 m
    // past its far face: neither endpoint is inside. Forward is canonical -Z,
    // so it travels from high +Z toward -Z.
    let start_z = half[2] + 10.0;
    let segment = ProjectileSegment {
        projectile: projectile(1),
        previous: position([0.0, 0.0, start_z]),
        current: position([0.0, 0.0, start_z - travel_m]),
    };
    let target = thin_target(actor(2), half);

    assert!(
        (segment.previous.z() - target.previous.z()).abs() > half[2],
        "the sweep's start point is outside the thin target, so an endpoint-only check cannot \
         see the hit"
    );
    assert!(
        (segment.current.z() - target.current.z()).abs() > half[2],
        "the sweep's end point is outside the thin target too"
    );
    assert!(
        travel_m > thickness_m * 10.0,
        "the test must keep speed*dt far larger than the obstacle thickness"
    );

    let mut ballistics = Ballistics::new();
    let hits = ballistics.sweep(&segment, std::slice::from_ref(&target));

    assert_eq!(
        hits.len(),
        1,
        "the swept segment crosses the thin target exactly once"
    );
    let hit = hits[0];
    assert_eq!(hit.projectile, projectile(1));
    assert_eq!(hit.target, actor(2));

    // The hit is where the leading face is entered: the near face sits at
    // z = +0.25 (canonical forward is -Z), 10 m into a 40 m sweep.
    let expected_toi = (start_z - half[2]) / travel_m;
    assert!(
        (hit.time_of_impact - expected_toi).abs() < 1e-9,
        "the hit's time of impact is the entry into the near face, got {} expected {expected_toi}",
        hit.time_of_impact
    );
    assert!(
        (0.0..=1.0).contains(&hit.time_of_impact),
        "a time of impact is always a fraction of the tick"
    );

    // "Hits once": the same segment swept again, and the same projectile
    // presented with a fresh target, still apply one hit.
    assert!(
        ballistics
            .sweep(&segment, std::slice::from_ref(&target))
            .is_empty(),
        "a projectile applies a hit on an actor at most once"
    );
    assert!(ballistics.has_hit(projectile(1), actor(2)));
    assert_eq!(
        ballistics.len(),
        1,
        "the ledger records exactly one application"
    );

    // A *different* projectile over the same path is a separate projectile
    // and still hits: the once-only rule is per projectile, not a global
    // suppression.
    let other = ProjectileSegment {
        projectile: projectile(2),
        ..segment
    };
    assert_eq!(
        ballistics
            .sweep(&other, std::slice::from_ref(&target))
            .len(),
        1,
        "the once-only rule is per (projectile, actor), not global"
    );
    assert_eq!(ballistics.len(), 2);
}

/// AC01's relative-motion requirement: a target that crosses the
/// projectile's path *between* ticks is still hit, and a target that never
/// crosses is not.
#[test]
fn accept_f27_a_sweep_follows_relative_target_motion() {
    let mut ballistics = Ballistics::new();
    // The box's z extent spans the whole tick, so the *lateral* motion alone
    // decides whether the target crosses into the flight line.
    let half = [2.0, 2.0, 20.0];

    // A static segment straight along -Z at x = 0.
    let segment = ProjectileSegment {
        projectile: projectile(1),
        previous: position([0.0, 0.0, 10.0]),
        current: position([0.0, 0.0, -10.0]),
    };

    // The target is 20 m to the side at the start of the tick and sweeps
    // 40 m across to x = 0 by the end: a point-in-time test at either end
    // misses it.
    let crossing = SweepTarget::try_new(actor(2), [20.0, 0.0, 0.0], [0.0, 0.0, 0.0], half)
        .expect("test targets are valid");
    assert!(
        crossing.previous.x().abs() > half[0] && crossing.current.x().abs() <= half[0],
        "the target starts outside the flight line and ends on it"
    );

    let hits = ballistics.sweep(&segment, std::slice::from_ref(&crossing));
    assert_eq!(
        hits.len(),
        1,
        "a target that crosses the projectile path between ticks is hit"
    );

    // A target that crosses elsewhere — 40 m away in Y throughout — is not.
    let elsewhere = SweepTarget::try_new(actor(3), [20.0, 40.0, 0.0], [0.0, 40.0, 0.0], half)
        .expect("test targets are valid");
    assert!(
        ballistics
            .sweep(&segment, std::slice::from_ref(&elsewhere))
            .is_empty(),
        "a target that never reaches the flight line is not hit"
    );
}

/// Ordering policy: hits come back in ascending time of impact, and an
/// exact tie breaks on the actor id — never on the caller's target order
/// (`FLIGHT-PHYSICS`, "stable tie-breakers").
#[test]
fn accept_f27_a_sweep_orders_by_earliest_impact_with_a_stable_tie_breaker() {
    let mut ballistics = Ballistics::new();
    let half = [1.0, 1.0, 1.0];
    // One long segment down the -Z axis past three boxes at different depths.
    let segment = ProjectileSegment {
        projectile: projectile(1),
        previous: position([0.0, 0.0, 100.0]),
        current: position([0.0, 0.0, -100.0]),
    };

    // Near, mid and far, so ascending depth along -Z is ascending distance.
    let near = SweepTarget::try_new(actor(9), [0.0, 0.0, 40.0], [0.0, 0.0, 40.0], half)
        .expect("valid target");
    let mid = SweepTarget::try_new(actor(2), [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], half)
        .expect("valid target");
    let far = SweepTarget::try_new(actor(5), [0.0, 0.0, -40.0], [0.0, 0.0, -40.0], half)
        .expect("valid target");

    // Deliberately feed the targets in an order that is *not* the answer.
    let hits = ballistics.sweep(&segment, &[far, mid, near]);
    let order: Vec<ActorId> = hits.iter().map(|hit| hit.target).collect();
    assert_eq!(
        order,
        vec![actor(9), actor(2), actor(5)],
        "hits are ordered by time of impact, not by the caller's target order"
    );
    // Each hit lands when its box's near face is entered: 59, 99 and 139 m
    // into a 200 m sweep, so 0.295, 0.495 and 0.695 of the tick.
    for (hit, expected_toi) in hits.iter().zip([0.295_f64, 0.495, 0.695]) {
        assert_eq!(hit.projectile, projectile(1));
        assert!(
            (hit.time_of_impact - expected_toi).abs() < 1e-9,
            "each hit's time of impact is where its near face is entered, got {} expected \
             {expected_toi}",
            hit.time_of_impact
        );
    }

    // Two boxes at the *same* depth: the tie must break on actor id.
    let mut tied = Ballistics::new();
    let tie_low = SweepTarget::try_new(actor(4), [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], half)
        .expect("valid target");
    let tie_high = SweepTarget::try_new(actor(3), [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], half)
        .expect("valid target");
    let tied_hits = tied.sweep(&segment, &[tie_high, tie_low]);
    assert_eq!(
        tied_hits.iter().map(|hit| hit.target).collect::<Vec<_>>(),
        vec![actor(3), actor(4)],
        "an exact time-of-impact tie breaks on the actor id"
    );
}

/// Non-negotiable 3 / AC01: a swept segment that passes *through* a target
/// and out the other side is one hit, and re-sweeping the same projectile
/// against the same target — the case where several collision features
/// report the same contact — applies nothing more.
#[test]
fn accept_f27_a_one_projectile_applies_a_hit_at_most_once() {
    let mut ballistics = Ballistics::new();
    let target = thin_target(actor(2), [2.0, 2.0, 0.25]);
    let segment = ProjectileSegment {
        projectile: projectile(7),
        previous: position([0.0, 0.0, 5.0]),
        current: position([0.0, 0.0, -5.0]),
    };

    // Both endpoints are past the target entirely: the projectile has left
    // through the far face, so nothing overlaps at either instant.
    assert_eq!((segment.previous.z() - target.previous.z()).abs(), 5.0);
    let first = ballistics.sweep(&segment, std::slice::from_ref(&target));
    assert_eq!(first.len(), 1, "passing through a target is one hit");

    // The very same segment again — two collision features reporting one
    // contact — applies nothing.
    assert!(
        ballistics
            .sweep(&segment, std::slice::from_ref(&target))
            .is_empty(),
        "a repeated report of the same contact applies no second hit"
    );
    assert!(
        ballistics.sweep(&segment, &[target, target]).is_empty(),
        "duplicated candidate entries cannot re-apply a hit either"
    );
    assert_eq!(ballistics.len(), 1);
}

/// The declared interaction rules filter the sweep's candidate list: the
/// shooter is excluded under `SelfHitRule::Excluded`, an ally is excluded
/// under `FriendlyFireRule::HostileOnly`, and an *undeclared* pair is not
/// silently treated as friendly (F27 non-negotiable 4).
#[test]
fn accept_f27_a_declared_rules_filter_the_sweep_candidate_list() {
    let rules = WeaponRules {
        self_hit: SelfHitRule::Excluded,
        friendly_fire: FriendlyFireRule::HostileOnly,
        penetration: false,
        ricochet: false,
        ammo_switching: false,
    };
    let shooter = actor(1);
    let hostile = actor(2);
    let ally = actor(3);
    let undeclared = actor(4);

    let candidate = |target: ActorId| {
        (
            thin_target(target, [2.0, 2.0, 2.0]),
            Some(Allegiance::Hostile),
        )
    };
    let eligible = rules.eligible(
        shooter,
        [
            // The shooter's own actor, offered as hostile so only the
            // self-hit rule can exclude it.
            (thin_target(shooter, [2.0; 3]), Some(Allegiance::Hostile)),
            candidate(hostile),
            (thin_target(ally, [2.0; 3]), Some(Allegiance::Friendly)),
            (thin_target(undeclared, [2.0; 3]), None),
        ],
    );
    assert_eq!(
        eligible
            .iter()
            .map(|target| target.actor)
            .collect::<Vec<_>>(),
        vec![hostile],
        "self is excluded, an ally is excluded and an undeclared pair is not admitted as hostile"
    );

    // Widen the declared rules and the same list widens with them.
    let mut permissive = rules;
    permissive.friendly_fire = FriendlyFireRule::Everyone;
    let still_excluded = permissive.eligible(shooter, [(thin_target(shooter, [2.0; 3]), None)]);
    assert!(
        still_excluded.is_empty(),
        "permissive friendly fire still does not override the declared self-hit rule"
    );

    permissive.self_hit = SelfHitRule::Allowed;
    let admitted = permissive.eligible(shooter, [(thin_target(shooter, [2.0; 3]), None)]);
    assert_eq!(
        admitted.len(),
        1,
        "only a declared allowed self-hit admits the shooter"
    );

    // And a hostile-only gun admits an undeclared pair only when its rules
    // say everyone is fair game.
    let hostile_only = WeaponRules {
        friendly_fire: FriendlyFireRule::HostileOnly,
        ..rules
    };
    assert!(
        !hostile_only.admits(shooter, undeclared, None),
        "an undeclared relation is not a guessed hostility"
    );
}

/// Non-negotiable 5: an accepted fire event is the authority for consuming
/// a round, starting a cooldown and naming sound and muzzle effect. The
/// event carries the mount's *supplied* transform — never a fixed
/// center-screen origin.
#[test]
fn accept_f27_a_an_accepted_event_consumes_a_round_and_starts_the_cooldown() {
    let mut resolver = resolver();
    let mount = synthetic_mount();
    let muzzle = [12.0, -3.0, 40.0];
    let transforms = transforms_for(&mount, muzzle);

    let before = resolver
        .state(&actor(1))
        .expect("the fixture actor is registered")
        .ammunition(&mount);
    let resolution = resolver
        .resolve(&intent(0, 0), &transforms)
        .expect("the fixture intent resolves");

    assert!(
        resolution.refused.is_empty(),
        "the fixture gun is enabled and loaded"
    );
    assert_eq!(
        resolution.accepted.len(),
        1,
        "one mount in the bank fires once"
    );

    let event: &FireEvent = &resolution.accepted[0];
    assert_eq!(event.shooter, actor(1));
    assert_eq!(event.mount, mount);
    assert_eq!(event.mount_kind, GunMountKind::Nose);
    assert_eq!(event.ammunition, synthetic_ammunition());
    assert_eq!(event.projectile.origin, position(muzzle));
    assert_eq!(
        event.projectile.velocity_mps,
        transform(muzzle, [0.0; 3]).world_velocity_mps(
            InheritanceRule::Full,
            synthetic_gun_definition().muzzle_velocity_mps()
        ),
        "the spawn velocity is the declared inheritance rule applied to the supplied transform"
    );
    assert_eq!(
        event.projectile.lifetime_ticks,
        synthetic_gun_definition().lifetime_ticks()
    );

    let after = resolver
        .state(&actor(1))
        .expect("registered")
        .ammunition(&mount);
    assert_eq!(
        before - after,
        1,
        "exactly one round is consumed by an accepted shot"
    );
    assert_eq!(
        resolver
            .state(&actor(1))
            .expect("registered")
            .cooldown_ticks(&mount),
        u64::from(SYNTHETIC_TICKS_BETWEEN_SHOTS),
        "the accepted shot starts the declared cooldown in ticks"
    );

    // The sound and effect ride on the event, which is the only place a
    // consumer reads them from.
    let gun = synthetic_gun_definition();
    assert_eq!(event.sound, *gun.sound());
    assert_eq!(event.effect, *gun.effect());
}

/// AC02's semantic half at this stage: a disabled mount emits neither a
/// projectile nor a sound nor an ammunition decrement. The disable arrives
/// as the mount's own damage-graph key, and the state gate is idempotent.
#[test]
fn accept_f27_a_a_disabled_mount_emits_no_event_and_conserves_ammo() {
    let mut resolver = resolver();
    let mount = synthetic_mount();
    let transforms = transforms_for(&mount, [1.0, 2.0, 3.0]);

    resolver
        .state_mut(&actor(1))
        .expect("the fixture actor is registered")
        .disable(&mount);
    // The damage resolver may report the same destruction twice; disabling
    // again is not a second event and must not change anything.
    resolver
        .state_mut(&actor(1))
        .expect("registered")
        .disable(&mount);

    let before = resolver
        .state(&actor(1))
        .expect("registered")
        .ammunition(&mount);
    let resolution = resolver
        .resolve(&intent(0, 0), &transforms)
        .expect("a disabled mount still resolves the intent");

    assert!(
        resolution.accepted.is_empty(),
        "a disabled mount emits no projectile and names no sound"
    );
    assert_eq!(
        resolution.refused,
        vec![FireDenialReason::MountDisabled {
            mount: mount.clone()
        }],
        "the refusal names the disabled mount"
    );
    assert_eq!(
        resolver
            .state(&actor(1))
            .expect("registered")
            .ammunition(&mount),
        before,
        "a disabled mount consumes no ammunition"
    );
    assert_eq!(
        resolver
            .state(&actor(1))
            .expect("registered")
            .cooldown_ticks(&mount),
        0,
        "a denied shot starts no cooldown"
    );

    // Repairing the mount lets it fire again, from the same rounds. The
    // resolver's own tick has to advance with it.
    resolver
        .state_mut(&actor(1))
        .expect("registered")
        .enable(&mount);
    resolver.advance_to(Tick(1));
    assert!(
        !resolver
            .resolve(&intent(1, 0), &transforms)
            .expect("resolves")
            .accepted
            .is_empty(),
        "a repaired mount fires again"
    );
}

/// Non-negotiable 5: a duplicate network packet drains nothing. A repeated
/// intent id is refused whole, before any mount is consulted.
#[test]
fn accept_f27_a_a_duplicate_fire_intent_is_refused_and_drains_nothing() {
    let mut resolver = resolver();
    let mount = synthetic_mount();
    let transforms = transforms_for(&mount, [0.0, 0.0, 0.0]);

    let first = resolver
        .resolve(&intent(0, 0), &transforms)
        .expect("the first packet resolves");
    assert_eq!(first.accepted.len(), 1);
    let rounds_after_first = resolver
        .state(&actor(1))
        .expect("registered")
        .ammunition(&mount);

    let replay = resolver
        .resolve(&intent(0, 0), &transforms)
        .expect_err("a duplicate intent id is refused");
    assert_eq!(
        replay,
        IntentRefusal::DuplicateIntent {
            id: intent(0, 0).id
        },
        "the duplicate is refused by name, not silently swallowed"
    );
    assert_eq!(
        resolver
            .state(&actor(1))
            .expect("registered")
            .ammunition(&mount),
        rounds_after_first,
        "a duplicate packet consumes no further ammunition"
    );

    // A *different* sequence in the same tick is a different intent and is
    // still refused only by its own cooldown, not by being a duplicate.
    let second = resolver
        .resolve(&intent(0, 1), &transforms)
        .expect("a fresh intent resolves");
    assert_eq!(
        second.refused,
        vec![FireDenialReason::Cooldown {
            mount: mount.clone(),
            remaining_ticks: u64::from(SYNTHETIC_TICKS_BETWEEN_SHOTS),
        }],
        "the cooldown, not duplication, is what stops the second intent"
    );
}

/// The cooldown is in ticks and counts down one per tick: a gun fires again
/// exactly `ticks_between_shots` ticks later, never earlier.
#[test]
fn accept_f27_a_cooldown_expires_exactly_after_the_declared_ticks() {
    let mut resolver = resolver();
    let mount = synthetic_mount();
    let transforms = transforms_for(&mount, [0.0, 0.0, 0.0]);

    resolver
        .resolve(&intent(0, 0), &transforms)
        .expect("the first shot resolves");

    // One tick early: still cooling.
    for tick in 1..u64::from(SYNTHETIC_TICKS_BETWEEN_SHOTS) {
        resolver.advance_to(Tick(tick));
        let denied = resolver
            .resolve(&intent(tick, 0), &transforms)
            .expect("resolves");
        assert_eq!(
            denied.accepted.len(),
            0,
            "the gun must still be cooling at tick {tick}"
        );
    }

    resolver.advance_to(Tick(u64::from(SYNTHETIC_TICKS_BETWEEN_SHOTS)));
    assert!(
        resolver
            .resolve(
                &intent(u64::from(SYNTHETIC_TICKS_BETWEEN_SHOTS), 0),
                &transforms
            )
            .expect("resolves")
            .accepted
            .len()
            == 1,
        "the gun fires again on the first tick its cooldown has elapsed"
    );
}

/// AC03's state half: switching the selected bank mid-cooldown duplicates no
/// fire and refills no ammunition. Cooldowns and rounds live per mount and
/// are untouched by a selection change.
#[test]
fn accept_f27_a_switching_bank_neither_duplicates_fire_nor_refills_ammo() {
    let nose = synthetic_mount();

    // Give the actor a second mount so a bank switch has somewhere to go.
    let wing = key("fixture_wing_mount");
    let wing_gun = GunDefinition::try_new(
        wing.clone(),
        GunMountKind::WingLeft,
        "synthetic fixture caliber",
        synthetic_ammunition(),
        GunRate::try_new(1).expect("a valid rate"),
        600.0,
        60,
        SpreadCone::try_new(0.01).expect("a valid spread"),
        WeaponDamage::try_new(4.0, 2.0).expect("valid damage"),
        InheritanceRule::Full,
        synthetic_gun_definition().effect().clone(),
        synthetic_gun_definition().sound().clone(),
    )
    .expect("the second fixture gun is valid");
    let both = GunBank::try_new([nose.clone(), wing.clone()]).expect("a valid bank");
    let state = WeaponState::try_new(
        &[synthetic_gun_definition(), wing_gun.clone()],
        both,
        SYNTHETIC_STARTING_ROUNDS,
    )
    .expect("a valid state");
    let mut resolver = FireResolver::new(SESSION, Tick(0));
    resolver
        .register(actor(1), vec![synthetic_gun_definition(), wing_gun], state)
        .expect("both guns register");
    let both_transforms = BTreeMap::from([
        (nose.clone(), transform([0.0, 0.0, 0.0], [0.0; 3])),
        (wing.clone(), transform([-4.0, 0.0, 1.0], [0.0; 3])),
    ]);

    // Both mounts fire, and both enter cooldown and lose a round.
    let rounds_before: Vec<u64> = [nose.clone(), wing.clone()]
        .iter()
        .map(|mount| {
            resolver
                .state(&actor(1))
                .expect("registered")
                .ammunition(mount)
        })
        .collect();
    let fired = resolver
        .resolve(&intent(0, 0), &both_transforms)
        .expect("the whole bank fires");
    assert_eq!(
        fired.accepted.len(),
        2,
        "a two-mount bank fires both mounts"
    );

    // Switching to the nose alone mid-cooldown.
    let nose_only = GunBank::try_new([nose.clone()]).expect("a valid bank");
    resolver
        .state_mut(&actor(1))
        .expect("registered")
        .select(nose_only.clone());

    let rounds_after: Vec<u64> = [nose.clone(), wing.clone()]
        .iter()
        .map(|mount| {
            resolver
                .state(&actor(1))
                .expect("registered")
                .ammunition(mount)
        })
        .collect();
    assert_eq!(
        rounds_after,
        vec![rounds_before[0] - 1, rounds_before[1] - 1],
        "selecting a different bank neither refills nor drains either mount"
    );

    // The next tick the nose bank is still cooling: no duplicate fire.
    resolver.advance_to(Tick(1));
    let switched = resolver
        .resolve(&intent(1, 0), &both_transforms)
        .expect("the switched bank resolves");
    assert!(
        switched.accepted.is_empty(),
        "switching bank during a cooldown does not let the mount fire again"
    );
    assert_eq!(
        switched.refused,
        vec![FireDenialReason::Cooldown {
            mount: nose.clone(),
            remaining_ticks: u64::from(SYNTHETIC_TICKS_BETWEEN_SHOTS - 1),
        }],
        "the switched-to mount is still the one that was cooling"
    );
}

/// The resolver is session- and tick-confined: a foreign session or tick is
/// refused by name, and an unknown shooter or empty selection is refused
/// rather than silently doing nothing.
#[test]
fn accept_f27_a_foreign_sessions_ticks_and_unknown_shooters_are_refused() {
    let mut resolver = resolver();
    let mount = synthetic_mount();
    let transforms = transforms_for(&mount, [0.0; 3]);

    let foreign = FireIntent {
        id: FireIntentId {
            session: SESSION + 1,
            tick: Tick(0),
            producer: 1,
            sequence: 0,
        },
        shooter: actor(1),
    };
    assert_eq!(
        resolver.resolve(&foreign, &transforms),
        Err(IntentRefusal::ForeignSession {
            expected: SESSION,
            found: SESSION + 1
        })
    );

    assert_eq!(
        resolver.resolve(&intent(7, 0), &transforms),
        Err(IntentRefusal::ForeignTick {
            expected: Tick(0),
            found: Tick(7)
        })
    );

    let stranger = FireIntent {
        id: intent(0, 5).id,
        shooter: actor(99),
    };
    assert_eq!(
        resolver.resolve(&stranger, &transforms),
        Err(IntentRefusal::UnknownShooter { shooter: actor(99) })
    );

    // An empty selection fires nothing at all.
    resolver
        .state_mut(&actor(1))
        .expect("registered")
        .select(GunBank::default());
    assert_eq!(
        resolver.resolve(&intent(0, 0), &transforms),
        Err(IntentRefusal::NoSelectedBank { shooter: actor(1) })
    );

    // None of those refusals touched the weapon state.
    assert_eq!(
        resolver
            .state(&actor(1))
            .expect("registered")
            .ammunition(&mount),
        SYNTHETIC_STARTING_ROUNDS
    );
}

/// Non-negotiable 2: a mount with no supplied transform cannot fire, so no
/// fixed center-screen origin can leak into the weapon path.
#[test]
fn accept_f27_a_a_mount_without_a_transform_refuses_to_fire() {
    let mut resolver = resolver();
    let mount = synthetic_mount();
    let no_poses = BTreeMap::new();

    let resolution = resolver
        .resolve(&intent(0, 0), &no_poses)
        .expect("the intent still resolves");
    assert!(
        resolution.accepted.is_empty(),
        "with no hierarchy pose there is no muzzle position, so nothing fires"
    );
    assert_eq!(
        resolution.refused,
        vec![FireDenialReason::MissingMountTransform {
            mount: mount.clone()
        }]
    );
    assert_eq!(
        resolver
            .state(&actor(1))
            .expect("registered")
            .ammunition(&mount),
        SYNTHETIC_STARTING_ROUNDS,
        "refusing for want of a pose consumes nothing"
    );
}

/// The definition's input boundary refuses corrupt values instead of
/// clamping them into plausible ones (`FLIGHT-PHYSICS`: "do not silently
/// clamp corrupted tuning into plausible values").
#[test]
fn accept_f27_a_gun_definitions_refuse_corrupt_values() {
    let mount = synthetic_mount();
    let ammo = synthetic_ammunition();
    let gun = synthetic_gun_definition();
    let rate = || GunRate::try_new(4).expect("a valid rate");
    let spread = || SpreadCone::try_new(0.004).expect("a valid spread");
    let damage = || WeaponDamage::try_new(6.0, 3.0).expect("valid damage");
    let effect = || gun.effect().clone();
    let sound = || gun.sound().clone();

    let build = |caliber: &str, muzzle: f64, lifetime: u64, effect: ContentId, sound: ContentId| {
        GunDefinition::try_new(
            mount.clone(),
            GunMountKind::Nose,
            caliber,
            ammo.clone(),
            rate(),
            muzzle,
            lifetime,
            spread(),
            damage(),
            InheritanceRule::Full,
            effect,
            sound,
        )
    };

    // A zero rate interval.
    assert_eq!(
        GunRate::try_new(0),
        Err(GunDefinitionError::ZeroRateInterval),
        "even the fastest gun needs one tick between shots"
    );

    // An empty caliber.
    assert!(matches!(
        build("", 640.0, 90, effect(), sound()),
        Err(GunDefinitionError::EmptyCaliber)
    ));

    // Non-finite and non-positive muzzle velocities.
    assert!(matches!(
        build("c", f64::NAN, 90, effect(), sound()),
        Err(GunDefinitionError::NonFiniteMuzzleVelocity)
    ));
    assert!(matches!(
        build("c", 0.0, 90, effect(), sound()),
        Err(GunDefinitionError::NonPositiveMuzzleVelocity {
            muzzle_velocity_mps: 0.0
        })
    ));

    // A zero lifetime: a round that dies on the muzzle is not a projectile.
    assert!(matches!(
        build("c", 640.0, 0, effect(), sound()),
        Err(GunDefinitionError::ZeroLifetime)
    ));

    // Negative and non-finite damage.
    assert!(matches!(
        WeaponDamage::try_new(-1.0, 3.0),
        Err(GunDefinitionError::NegativeDamage { amount: -1.0 })
    ));
    assert_eq!(
        WeaponDamage::try_new(f64::INFINITY, 3.0),
        Err(GunDefinitionError::NonFiniteDamage)
    );

    // A spread cone outside [0, π/2] and a non-finite one.
    assert!(matches!(
        SpreadCone::try_new(2.0),
        Err(GunDefinitionError::SpreadOutOfRange {
            half_angle_radians: 2.0
        })
    ));
    assert_eq!(
        SpreadCone::try_new(f64::NAN),
        Err(GunDefinitionError::NonFiniteSpread)
    );

    // A wrong-namespace effect or sound id.
    let not_a_sound =
        ContentId::from_source(ContentKind::Ammo, "synthetic.nope").expect("a valid content id");
    assert!(matches!(
        build("c", 640.0, 90, effect(), not_a_sound.clone()),
        Err(GunDefinitionError::SoundKindMismatch { id } ) if id == not_a_sound
    ));
    let not_an_effect =
        ContentId::from_source(ContentKind::Weapon, "synthetic.nope").expect("a valid content id");
    assert!(matches!(
        build("c", 640.0, 90, not_an_effect.clone(), sound()),
        Err(GunDefinitionError::EffectKindMismatch { id }) if id == not_an_effect
    ));

    // An inheritance share outside [0, 1].
    assert!(matches!(
        GunDefinition::try_new(
            mount.clone(),
            GunMountKind::Nose,
            "c",
            ammo,
            rate(),
            640.0,
            90,
            spread(),
            damage(),
            InheritanceRule::Fraction { share: 1.5 },
            effect(),
            sound(),
        ),
        Err(GunDefinitionError::InvalidInheritanceShare { share: 1.5 })
    ));

    // A non-finite inherited velocity at the transform boundary.
    assert_eq!(
        MountTransform::try_new(position([0.0; 3]), UnitVec3::FORWARD, [f64::NAN, 0.0, 0.0],),
        Err(MountTransformError::NonFiniteInheritedVelocity)
    );
}

/// The weapon-state input boundary: a zero starting load, a duplicated
/// mount and a bank naming an absent mount are all refused.
#[test]
fn accept_f27_a_weapon_state_and_bank_validation_refuse_malformed_records() {
    let gun = synthetic_gun_definition();

    assert_eq!(
        WeaponState::try_new(
            std::slice::from_ref(&gun),
            GunBank::try_new([gun.mount().clone()]).expect("valid"),
            0
        ),
        Err(GunStateError::InvalidStartingRounds),
        "a weapon that starts empty could never fire"
    );

    assert_eq!(
        WeaponState::try_new(
            &[gun.clone(), gun.clone()],
            GunBank::try_new([gun.mount().clone()]).expect("valid"),
            10
        ),
        Err(GunStateError::DuplicateMount {
            mount: gun.mount().clone()
        }),
        "two guns on one mount would make a disable ambiguous"
    );

    let absent = key("never_mounted");
    assert_eq!(
        WeaponState::try_new(
            std::slice::from_ref(&gun),
            GunBank::try_new([absent.clone()]).expect("valid"),
            10
        ),
        Err(GunStateError::UnknownSelection { mount: absent }),
        "a bank cannot name a mount the actor does not carry"
    );

    assert_eq!(
        GunBank::try_new([]),
        Err(GunBankError::Empty),
        "a bank with no mounts could never fire"
    );
}

/// The registration and identifier boundaries are typed rather than
/// stringly: the ammunition id's namespace, the resolver's duplicate
/// registration, and the projectile serial's non-recycling.
#[test]
fn accept_f27_a_identities_are_session_qualified_and_namespaced() {
    // An ammunition id outside the `ammo` namespace is refused.
    let not_ammo = ContentId::from_source(ContentKind::Weapon, "synthetic.not_ammo")
        .expect("a valid content id");
    assert!(AmmunitionId::try_new(not_ammo.clone()).is_err());

    // Registering the same actor twice is refused.
    let mut existing = resolver();
    assert_eq!(
        existing.register(
            actor(1),
            vec![synthetic_gun_definition()],
            WeaponState::try_new(
                std::slice::from_ref(&synthetic_gun_definition()),
                GunBank::try_new([synthetic_mount()]).expect("valid"),
                10
            )
            .expect("a valid state"),
        ),
        Err(FireError::DuplicateShooter { shooter: actor(1) })
    );

    // Two guns on one mount are refused at registration.
    let duplicate = GunDefinition::try_new(
        synthetic_mount(),
        GunMountKind::WingRight,
        "synthetic fixture caliber",
        synthetic_ammunition(),
        GunRate::try_new(2).expect("valid"),
        500.0,
        60,
        SpreadCone::try_new(0.005).expect("valid"),
        WeaponDamage::try_new(5.0, 5.0).expect("valid"),
        InheritanceRule::Full,
        synthetic_gun_definition().effect().clone(),
        synthetic_gun_definition().sound().clone(),
    )
    .expect("the duplicate-mount gun is valid");
    let state = WeaponState::try_new(
        &[synthetic_gun_definition(), duplicate],
        GunBank::try_new([synthetic_mount()]).expect("valid"),
        10,
    );
    // The state itself catches the ambiguity first, which is the point.
    assert_eq!(
        state,
        Err(GunStateError::DuplicateMount {
            mount: synthetic_mount()
        })
    );

    // Projectile serials are never recycled inside a session.
    let mut resolver = resolver();
    let first = resolver.next_projectile_id();
    let second = resolver.next_projectile_id();
    assert_eq!(first.session, SESSION);
    assert_eq!(second.session, SESSION);
    assert_ne!(
        first, second,
        "a projectile id names exactly one projectile"
    );
}

/// The swept target's geometry is validated: a negative half extent would
/// silently mirror the box and make the slab test answer nonsense.
#[test]
fn accept_f27_a_sweep_targets_refuse_corrupt_geometry() {
    assert!(matches!(
        SweepTarget::try_new(actor(2), [0.0; 3], [0.0; 3], [1.0, -1.0, 1.0]),
        Err(SweepTargetError::NegativeHalfExtent {
            axis: 1,
            value: -1.0
        })
    ));
    assert!(matches!(
        SweepTarget::try_new(actor(2), [0.0; 3], [0.0; 3], [f64::NAN, 1.0, 1.0]),
        Err(SweepTargetError::NonFiniteHalfExtent { axis: 0 })
    ));
    assert!(matches!(
        SweepTarget::try_new(actor(2), [f64::NAN, 0.0, 0.0], [0.0; 3], [1.0; 3]),
        Err(SweepTargetError::Position(_))
    ));
}

/// The synthetic fixture is what the acceptance tests drive, and it says
/// what it is: one mount on the fixture airframe's weapon-mount damage
/// node, one `ammo` type, and designed values throughout.
#[test]
fn accept_f27_a_synthetic_fixture_declares_one_mount_and_one_ammo_type() {
    let gun = synthetic_gun_definition();
    assert_eq!(gun.mount(), &synthetic_mount());
    assert_eq!(gun.kind(), GunMountKind::Nose);
    assert!(!gun.kind().is_wing(), "the fixture mount is the nose gun");
    assert_eq!(
        gun.ammunition().as_str(),
        "ammo/synthetic.fixture_slug",
        "the ammunition type is an opaque catalog id, not an enum variant"
    );
    assert_eq!(gun.sound().kind(), ContentKind::Sound);
    assert_eq!(gun.effect().kind(), ContentKind::HardpointEquipment);
    assert_eq!(gun.inheritance(), InheritanceRule::Full);
    assert_eq!(
        gun.damage().amount_on(DamageChannel::Armor),
        gun.damage().armor
    );
    assert_eq!(
        gun.damage().amount_on(DamageChannel::Internal),
        gun.damage().internal
    );

    // Every wing kind exists so AC02's "disabled wing gun" case is
    // expressible, and the fixture deliberately uses the nose one.
    assert!(
        GunMountKind::ALL.iter().any(|kind| kind.is_wing()),
        "the mount vocabulary includes wing guns"
    );
    assert_eq!(GunMountKind::ALL.len(), 5);
}
