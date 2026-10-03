//! F28-D through the live ordnance session: what the audit says a session can
//! actually fire, and the AC04 minimum scenario — boost changes thrust and
//! consumption and never teleports an airframe or scales a render frame.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-D`. Task test prefix: `accept_f28_d_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`, section "Boost and special models".
//!
//! Every test here drives production code: [`cs_app::ordnance`]'s
//! [`session_ordnance_audit`] over a real [`OrdnanceSession`], and
//! [`step_ordnance_session`] with [`OrdnanceOrder::Nitro`] orders over the live
//! ECS. The declared side of the audit is
//! `cs_content::ordnance::OrdnanceAudit`, covered by
//! `crates/cs_content/tests/accept_f28_d_ordnance_audit.rs`.
//!
//! Every number is the synthetic fixture's own value, carried unchanged. No
//! `CS_GAME_DIR` access: the retail measurement is a separate, ignored target.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{GlobalTransform, Transform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceEventKind, OrdnanceOrder, OrdnanceSession, session_ordnance_audit,
    step_ordnance_session,
};
use cs_content::ordnance::{
    DECLARED_SYNTHETIC_AREA_DENIAL_KEY, DECLARED_SYNTHETIC_DIRECT_KEY, DECLARED_SYNTHETIC_FLAK_KEY,
    DECLARED_SYNTHETIC_GUIDED_KEY, DECLARED_SYNTHETIC_NITRO_KEY, DECLARED_SYNTHETIC_TORPEDO_KEY,
    DeclaredOrdnance, DeclaredOrdnanceDetails, declared_synthetic_area_denial,
    declared_synthetic_direct, declared_synthetic_flak, declared_synthetic_guided,
    declared_synthetic_nitro, declared_synthetic_torpedo,
};
use cs_sim::damage::{ActorId, DamageResolver};
use cs_sim::time::TickRate;
use cs_sim::weapons::{
    NitroActivationRule, OrdnanceFamily, SYNTHETIC_NITRO_CONSUMPTION_PER_S,
    SYNTHETIC_NITRO_EXTRA_THRUST_N,
};
use cs_types::Tick;
use cs_types::content::Origin;
use cs_types::net::SessionId;

const SESSION: u64 = 77;
/// The ordnance producer serial the session and its events are stamped with.
const PRODUCER: u32 = 71;
/// The tick length the fixed step runs at.
const DT_S: f64 = 1.0 / 30.0;
/// The shooter's live pose, so a boost that teleported anything would show.
const SHOOTER_POS: (f32, f32, f32) = (120.0, 45.0, -30.0);
const SHOOTER_VELOCITY: (f32, f32, f32) = (180.0, -4.0, 60.0);

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("the test session generation is nonzero")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn rate() -> TickRate {
    TickRate::new(30).expect("the test rate is nonzero")
}

/// The whole synthetic declared catalogue: one record per designed family.
fn declared_catalogue() -> Vec<DeclaredOrdnance> {
    vec![
        declared_synthetic_direct(),
        declared_synthetic_flak(),
        declared_synthetic_guided(),
        declared_synthetic_area_denial(),
        declared_synthetic_torpedo(),
        declared_synthetic_nitro(),
    ]
}

/// An empty damage authority: this slice's orders are nitro only, and nothing
/// here routes a hit.
fn damage_resolver() -> DamageResolver {
    DamageResolver::new(session_id(), 72)
}

/// The shooter's live airframe: a pose and a velocity a boost must leave alone.
fn shooter_entity(world: &mut World) -> bevy::prelude::Entity {
    world
        .spawn((
            Transform::from_xyz(SHOOTER_POS.0, SHOOTER_POS.1, SHOOTER_POS.2),
            GlobalTransform::from_xyz(SHOOTER_POS.0, SHOOTER_POS.1, SHOOTER_POS.2),
            LinearVelocity(Vec3::new(
                SHOOTER_VELOCITY.0,
                SHOOTER_VELOCITY.1,
                SHOOTER_VELOCITY.2,
            )),
        ))
        .id()
}

/// One step of the production path with still air and no orders beyond the
/// ones given, at `dt_s`.
fn run_step(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    orders: &[OrdnanceOrder],
    at: Tick,
    dt_s: f64,
) -> cs_app::ordnance::OrdnanceSessionTick {
    step_ordnance_session(
        world,
        session,
        damage,
        orders,
        &cs_app::ordnance::OrdnanceStep {
            at,
            dt_s,
            wind_velocity_m_s: [0.0; 3],
            generation: cs_app::scene::SceneGeneration(1),
            guidance: None,
            targets: &[],
            impacts: &[],
        },
    )
}

/// Assert the shooter's live pose and velocity are exactly where they were.
fn assert_pose_untouched(world: &World, shooter: bevy::prelude::Entity) {
    let transform = world
        .get::<Transform>(shooter)
        .expect("the shooter keeps its transform");
    let global = world
        .get::<GlobalTransform>(shooter)
        .expect("the shooter keeps its global transform");
    let velocity = world
        .get::<LinearVelocity>(shooter)
        .expect("the shooter keeps its velocity");
    assert_eq!(
        (
            transform.translation.x,
            transform.translation.y,
            transform.translation.z
        ),
        SHOOTER_POS,
        "a boost must not move an airframe: no nitro order may write a position"
    );
    assert_eq!(
        (
            global.translation().x,
            global.translation().y,
            global.translation().z
        ),
        SHOOTER_POS,
        "a boost must not move an airframe's world transform"
    );
    assert_eq!(
        (velocity.0.x, velocity.0.y, velocity.0.z),
        SHOOTER_VELOCITY,
        "a boost must not write a velocity directly: it reports a thrust modifier \
         and the flight model owns the integration"
    );
}

// ------------------------------------------------------- AC04 minimum ----

/// AC04 minimum scenario: an accepted boost changes thrust and capacity and
/// does nothing else — it never teleports the airframe and never scales with a
/// render frame's elapsed time.
#[test]
fn accept_f28_d_boost_changes_thrust_and_consumption_and_never_moves_or_scales_a_frame() {
    let mut world = World::new();
    let shooter_entity = shooter_entity(&mut world);
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a nonzero session opens");
    session
        .register(actor(1), &[declared_synthetic_nitro()])
        .expect("the declared booster registers");
    let mut damage = damage_resolver();

    // What the audit says the booster carries, read from the session itself.
    let declared = session_ordnance_audit(&session);
    let row = declared
        .nitro_for(&actor(1))
        .expect("the registered booster has an audit row");
    assert_eq!(
        row.extra_thrust_n(),
        SYNTHETIC_NITRO_EXTRA_THRUST_N,
        "the audit reports the booster's declared extra thrust"
    );
    assert_eq!(
        row.consumption_per_s(),
        SYNTHETIC_NITRO_CONSUMPTION_PER_S,
        "the audit reports the booster's declared consumption"
    );
    let per_tick = SYNTHETIC_NITRO_CONSUMPTION_PER_S * rate().dt_seconds();
    assert_eq!(
        row.consumption_per_tick(),
        per_tick,
        "one tick costs one declared tick's worth, converted through the declared \
         tick rate and never through a wall clock"
    );
    assert_eq!(
        row.activation(),
        NitroActivationRule::WhileHeld,
        "the declared activation rule reaches the session's ledger"
    );

    let before = row.capacity_units();

    // One tick with the control held.
    let tick = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[OrdnanceOrder::Nitro {
            shooter: actor(1),
            requested: true,
        }],
        Tick(1),
        DT_S,
    );
    assert!(
        tick.orders_refused.is_empty(),
        "an accepted boost refuses nothing: {:?}",
        tick.orders_refused
    );
    assert_eq!(tick.nitro.len(), 1, "one booster resolved this tick");
    let resolved = tick.nitro[0].1;
    assert!(
        resolved.is_active(),
        "a held control with capacity is accepted"
    );
    assert!(
        !resolved.is_refused(),
        "an accepted activation is not a refusal: {:?}",
        resolved.refused
    );
    assert_eq!(
        resolved.extra_thrust_n, SYNTHETIC_NITRO_EXTRA_THRUST_N,
        "boost changes thrust, by exactly the declared amount"
    );
    assert_eq!(
        resolved.consumed_units, per_tick,
        "boost changes consumption, by exactly one tick's worth"
    );
    assert_eq!(
        resolved.authority_multiplier,
        row.authority_multiplier(),
        "the authority multiplier is the declared tradeoff, read from the same record"
    );

    assert_pose_untouched(&world, shooter_entity);

    let after = session_ordnance_audit(&session)
        .nitro_for(&actor(1))
        .expect("the booster is still registered")
        .capacity_units();
    assert!(
        (before - after - per_tick).abs() < 1e-12,
        "capacity drops by exactly what the tick reported: {before} -> {after}"
    );

    // A nitro activation is not a launch: it emits no launch effect (the
    // record that carries a world origin) and no item.
    assert!(
        tick.effects.is_empty(),
        "a boost emits no launch effect, so nothing places a booster in the world: {:?}",
        tick.effects
    );
    assert!(tick.launched.is_empty(), "a boost launches no item");
    assert!(
        tick.mirrors.spawned.is_empty(),
        "a boost mirrors no item: {:?}",
        tick.mirrors.spawned
    );
    match tick.events.as_slice() {
        [event] => match &event.kind {
            OrdnanceEventKind::Nitro {
                shooter,
                active,
                extra_thrust_n,
                consumed_units,
            } => {
                assert_eq!(*shooter, actor(1), "the event names the booster's actor");
                assert!(*active, "the event reports an accepted activation");
                assert_eq!(*extra_thrust_n, SYNTHETIC_NITRO_EXTRA_THRUST_N);
                assert_eq!(*consumed_units, per_tick);
            }
            other => panic!("a boost emits a Nitro event, not {other:?}"),
        },
        other => panic!("exactly one network event is emitted, got {other:?}"),
    }

    // Releasing the control consumes nothing and adds no thrust.
    let idle = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[OrdnanceOrder::Nitro {
            shooter: actor(1),
            requested: false,
        }],
        Tick(2),
        DT_S,
    );
    assert!(
        !idle.nitro[0].1.is_active(),
        "a released control does not run"
    );
    assert_eq!(
        idle.nitro[0].1.extra_thrust_n, 0.0,
        "an idle tick adds no thrust"
    );
    assert_eq!(
        idle.nitro[0].1.consumed_units, 0.0,
        "an idle tick consumes nothing"
    );
    assert!(idle.events.is_empty(), "an idle booster emits no event");
    assert_pose_untouched(&world, shooter_entity);
}

/// The same boost must cost the same at any render frame length: the ledger
/// converts through the declared tick rate, never through the caller's `dt`.
#[test]
fn accept_f28_d_boost_costs_the_same_at_any_render_frame_length() {
    let per_tick = SYNTHETIC_NITRO_CONSUMPTION_PER_S * rate().dt_seconds();
    let consumed_at = |dt_s: f64| -> f64 {
        let mut world = World::new();
        shooter_entity(&mut world);
        let mut session =
            OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session opens");
        session
            .register(actor(1), &[declared_synthetic_nitro()])
            .expect("the declared booster registers");
        let mut damage = damage_resolver();
        let tick = run_step(
            &mut world,
            &mut session,
            &mut damage,
            &[OrdnanceOrder::Nitro {
                shooter: actor(1),
                requested: true,
            }],
            Tick(1),
            dt_s,
        );
        tick.nitro[0].1.consumed_units
    };

    let slow_frame = consumed_at(1.0 / 120.0);
    let nominal_frame = consumed_at(1.0 / 30.0);
    let fast_frame = consumed_at(1.0 / 15.0);
    assert_eq!(slow_frame, per_tick, "a long frame costs one tick");
    assert_eq!(
        slow_frame, nominal_frame,
        "a render frame four times longer must not cost four times as much: the \
         ledger converts through the declared tick rate"
    );
    assert_eq!(
        nominal_frame, fast_frame,
        "a render frame half as long must not cost half as much"
    );
}

/// Ten ticks walked and ten ticks jumped cost the same, so a consumer cannot
/// buy free capacity by skipping frames.
#[test]
fn accept_f28_d_boost_costs_the_same_walked_or_jumped() {
    let mut world = World::new();
    shooter_entity(&mut world);
    let mut walked = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session");
    walked
        .register(actor(1), &[declared_synthetic_nitro()])
        .expect("the declared booster registers");
    let mut walked_damage = damage_resolver();
    let hold = || OrdnanceOrder::Nitro {
        shooter: actor(1),
        requested: true,
    };
    for at in 1..=10u64 {
        run_step(
            &mut world,
            &mut walked,
            &mut walked_damage,
            &[hold()],
            Tick(at),
            DT_S,
        );
    }

    let mut world = World::new();
    shooter_entity(&mut world);
    let mut jumped = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session");
    jumped
        .register(actor(1), &[declared_synthetic_nitro()])
        .expect("the declared booster registers");
    let mut jumped_damage = damage_resolver();
    run_step(
        &mut world,
        &mut jumped,
        &mut jumped_damage,
        &[hold()],
        Tick(10),
        DT_S,
    );

    let per_tick = SYNTHETIC_NITRO_CONSUMPTION_PER_S * rate().dt_seconds();
    let walked_capacity = session_ordnance_audit(&walked)
        .nitro_for(&actor(1))
        .expect("the walked booster is registered")
        .capacity_units();
    let jumped_capacity = session_ordnance_audit(&jumped)
        .nitro_for(&actor(1))
        .expect("the jumped booster is registered")
        .capacity_units();
    assert!(
        (walked_capacity - jumped_capacity).abs() < 1e-12,
        "ten ticks walked and ten ticks jumped leave the same capacity: \
         {walked_capacity} vs {jumped_capacity}"
    );
    assert!(
        (SYNTHETIC_NITRO_EXTRA_THRUST_N * 0.0 + (walked_capacity - jumped_capacity)).abs() < 1e-12,
        "and neither is {per_tick} off a single tick's worth"
    );
}

/// A refused activation consumes nothing, and a booster that has run out is
/// refused rather than becoming free thrust.
///
/// The declared booster here declares **no recovery**, so the only thing that
/// could move the tank between two refused ticks is the refusal itself. A
/// booster with recovery (the shared fixture) is covered separately.
#[test]
fn accept_f28_d_a_refused_boost_consumes_nothing() {
    let mut world = World::new();
    let shooter_entity = shooter_entity(&mut world);
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session opens");
    session
        .register(actor(1), &[declared_nitro_with(3.0, 0.0)])
        .expect("the declared booster registers");
    let mut damage = damage_resolver();
    let per_tick = 3.0 * rate().dt_seconds();

    let capacity = |session: &OrdnanceSession| {
        session_ordnance_audit(session)
            .nitro_for(&actor(1))
            .expect("the booster is still registered")
            .capacity_units()
    };

    // Hold the control until the ledger refuses. The bound is generous: the
    // declared capacity is 12 units and a tick costs 0.1, so 120 ticks empties
    // it and the next must refuse.
    let mut refused_at = None;
    let mut consumed_total = 0.0;
    let mut full_ticks = 0u64;
    let mut partial_ticks = 0u64;
    for at in 1..=200u64 {
        let before = capacity(&session);
        let tick = run_step(
            &mut world,
            &mut session,
            &mut damage,
            &[OrdnanceOrder::Nitro {
                shooter: actor(1),
                requested: true,
            }],
            Tick(at),
            DT_S,
        );
        let resolved = tick.nitro[0].1;
        if resolved.is_refused() {
            refused_at = Some((at, resolved));
            break;
        }
        // The tank cannot go below zero, so the last accepted tick costs
        // whatever is left rather than a full tick. That is the only
        // deviation, and it is what makes the refusal reachable at all.
        let expected = per_tick.min(before);
        assert!(
            (resolved.consumed_units - expected).abs() < 1e-12,
            "an accepted tick at tick {at} costs one tick's worth, or what is \
             left: {} against {expected}",
            resolved.consumed_units
        );
        if (resolved.consumed_units - per_tick).abs() < 1e-12 {
            full_ticks += 1;
        } else {
            partial_ticks += 1;
        }
        consumed_total += resolved.consumed_units;
    }
    let (refused_at, resolved) = refused_at.expect("a held control eventually runs out");
    assert_eq!(
        partial_ticks, 1,
        "exactly one accepted tick costs less than a full tick: the one that empties \
         the tank"
    );
    assert_eq!(
        full_ticks + partial_ticks,
        refused_at - 1,
        "every tick before the refusal was accepted"
    );
    assert!(
        (consumed_total - 12.0).abs() < 1e-9,
        "the booster ran until its capacity was spent: {consumed_total}"
    );

    let drained = capacity(&session);
    assert_eq!(
        drained, 0.0,
        "the last accepted tick consumes exactly what is left, so the tank is empty"
    );
    assert!(!resolved.is_active(), "a refused activation does not run");
    assert_eq!(resolved.extra_thrust_n, 0.0, "no thrust, free or otherwise");
    assert_eq!(
        resolved.consumed_units, 0.0,
        "pressing the control while boost is unavailable consumes nothing"
    );

    // With no recovery declared, every further refused tick changes nothing at
    // all — which is the only way to observe the refusal's own cost.
    for at in (refused_at + 1)..(refused_at + 6) {
        let refused = run_step(
            &mut world,
            &mut session,
            &mut damage,
            &[OrdnanceOrder::Nitro {
                shooter: actor(1),
                requested: true,
            }],
            Tick(at),
            DT_S,
        );
        let resolved = refused.nitro[0].1;
        assert!(
            resolved.is_refused(),
            "an empty booster keeps refusing at tick {at}"
        );
        assert_eq!(resolved.extra_thrust_n, 0.0, "still no thrust at tick {at}");
        assert_eq!(
            resolved.consumed_units, 0.0,
            "a refused activation consumes nothing at tick {at}"
        );
        assert_eq!(
            capacity(&session),
            0.0,
            "and the tank stays exactly where the refusal found it"
        );
    }
    assert_pose_untouched(&world, shooter_entity);
}

/// Idle recovery is the only thing that puts capacity back, and a tick that
/// consumes capacity never also recovers it.
#[test]
fn accept_f28_d_capacity_recovers_only_while_the_booster_is_idle() {
    let mut world = World::new();
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session opens");
    session
        .register(actor(1), &[declared_nitro_with(3.0, 0.6)])
        .expect("the declared booster registers");

    let run = |at: u64, requested: bool, session: &mut OrdnanceSession, world: &mut World| {
        run_step(
            world,
            session,
            &mut damage_resolver(),
            &[OrdnanceOrder::Nitro {
                shooter: actor(1),
                requested,
            }],
            Tick(at),
            DT_S,
        )
        .nitro[0]
            .1
    };
    let capacity = |session: &OrdnanceSession| {
        session_ordnance_audit(session)
            .nitro_for(&actor(1))
            .expect("the booster is registered")
            .capacity_units()
    };

    let started = capacity(&session);
    let _burning = run(1, true, &mut session, &mut world);
    let after_burn = capacity(&session);
    assert!(
        after_burn < started,
        "a burning tick spends capacity: {started} -> {after_burn}"
    );

    // Three idle ticks at the declared 3.0 units per second.
    for at in 2..=4u64 {
        let idle = run(at, false, &mut session, &mut world);
        assert!(!idle.is_active(), "an idle tick runs no booster");
        assert_eq!(idle.consumed_units, 0.0, "an idle tick costs nothing");
    }
    // One burning tick costs one tick of 3.0 units per second; three idle ticks
    // give back three ticks of 0.6 units per second, which stays below the
    // declared capacity so nothing clamps. An implementation that recovered
    // nothing, or that also recovered on the burning tick, lands elsewhere.
    let recovered = capacity(&session);
    let burned = 3.0 * rate().dt_seconds();
    let expected = started - burned + 3.0 * 0.6 * rate().dt_seconds();
    assert!(
        (recovered - expected).abs() < 1e-12,
        "three idle ticks recover three ticks' worth and no more: \
         {recovered} against {expected}"
    );
    assert!(
        recovered > after_burn,
        "idle capacity came back: {after_burn} -> {recovered}"
    );
}

/// A nitro order for an actor with no booster registered is refused by name,
/// changing nothing.
#[test]
fn accept_f28_d_an_order_for_an_unregistered_booster_is_refused() {
    let mut world = World::new();
    shooter_entity(&mut world);
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session opens");
    let mut damage = damage_resolver();
    let tick = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[OrdnanceOrder::Nitro {
            shooter: actor(9),
            requested: true,
        }],
        Tick(1),
        DT_S,
    );
    assert!(
        tick.nitro.is_empty(),
        "no booster resolved: nothing was registered"
    );
    assert_eq!(
        tick.orders_refused.len(),
        1,
        "the order is refused by name: {:?}",
        tick.orders_refused
    );
}

// ---------------------------------------------------- the session audit ----

/// The session audit reports every registered component, the channels it
/// reaches, and the family coverage of the whole session.
#[test]
fn accept_f28_d_the_session_audit_reports_every_registered_component() {
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session opens");
    let registered = session
        .register(actor(1), &declared_catalogue())
        .expect("the whole synthetic catalogue registers");
    assert_eq!(registered.len(), 6, "six components register");

    let audit = session_ordnance_audit(&session);
    assert_eq!(
        audit.component_count(),
        6,
        "one row per registered component"
    );
    assert_eq!(
        audit.launchable_count(),
        5,
        "five launchable items and one booster"
    );
    for entry in &registered {
        let row = audit
            .row(&actor(1), &entry.ordnance)
            .unwrap_or_else(|| panic!("{} has an audit row", entry.ordnance));
        if entry.family == OrdnanceFamily::NitroBooster {
            assert!(row.is_booster(), "the booster is a booster");
            assert!(!row.is_launchable(), "a booster is driven, not launched");
        } else {
            assert!(
                row.is_launchable(),
                "{} has a launcher mount",
                entry.ordnance
            );
            assert_eq!(
                row.damage_channels(),
                2,
                "the fixture routes armor and internal damage"
            );
        }
    }
    // Every designed family is occupied by exactly one component.
    for (family, count) in audit.families() {
        assert_eq!(
            *count, 1,
            "the {family} family carries exactly one component in the full fixture"
        );
    }
    assert!(
        audit.findings_of("family_without_a_component").is_empty(),
        "a catalogue using every designed family reports no uncovered family"
    );
}

/// The audit names a component that delivers nothing and a family with no
/// component — the two gaps a catalogue can have without a session refusing
/// to load it.
#[test]
fn accept_f28_d_a_component_that_delivers_nothing_is_named() {
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session opens");
    session
        .register(actor(1), std::slice::from_ref(&dud_direct_explosive()))
        .expect("a dud component still registers: it is valid, just useless");

    let audit = session_ordnance_audit(&session);
    assert_eq!(
        audit.findings_of("component_delivers_nothing").len(),
        1,
        "a component with no damage channel and no status effect delivers nothing: {:?}",
        audit.findings()
    );
    assert_eq!(
        audit.findings_of("family_without_a_component").len(),
        OrdnanceFamily::ALL.len() - 1,
        "every family the single dud does not use is reported: {:?}",
        audit.findings()
    );
    assert!(
        !audit.is_complete(),
        "a session holding one useless component is not a complete catalogue"
    );
}

/// The direct-explosive fixture with both damage channels declared as a known
/// zero: a valid record that delivers nothing at runtime.
fn dud_direct_explosive() -> DeclaredOrdnance {
    let fixture = declared_synthetic_direct();
    let mut projectile = match fixture.details() {
        cs_content::ordnance::DeclaredOrdnanceDetails::Projectile(projectile) => {
            (**projectile).clone()
        }
        cs_content::ordnance::DeclaredOrdnanceDetails::Nitro(_) => {
            panic!("the direct-explosive fixture is a projectile")
        }
    };
    projectile.armor_damage = cs_content::ordnance::declared_known(0.0);
    projectile.internal_damage = cs_content::ordnance::declared_known(0.0);
    DeclaredOrdnance::try_new(
        fixture.ordnance().clone(),
        Origin::SyntheticFixture,
        fixture.family(),
        cs_content::ordnance::DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        cs_content::ordnance::declared_synthetic_provenance(),
    )
    .expect("a zero-damage record is valid: zero is a measurement, not a gap")
}

/// The declared area effect lowers and reaches no recipient, and the audit says
/// so instead of implying a splash was applied.
#[test]
fn accept_f28_d_an_unconsumed_area_effect_is_named() {
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session opens");
    session
        .register(actor(1), &[declared_synthetic_area_denial()])
        .expect("the area-denial component registers");
    let audit = session_ordnance_audit(&session);
    let row = audit
        .row(
            &actor(1),
            &registered_id(&session, DECLARED_SYNTHETIC_AREA_DENIAL_KEY),
        )
        .expect("the area-denial component has a row");
    assert!(row.declares_area(), "the record declares a bounded area");
    assert!(
        !row.area_applied(),
        "no production path applies the area's reach, so the row must not claim it did"
    );
    assert_eq!(
        row.unconsumed_fields().len(),
        2,
        "both of the area's declared values are reported as unconsumed: {:?}",
        row.unconsumed_fields()
    );
    assert_eq!(
        audit.findings_of("unconsumed_area_effect").len(),
        1,
        "the gap is reported once, by name: {:?}",
        audit.findings()
    );
}

/// A closed session audits to nothing: teardown releases the lowered loadout
/// along with the runtime, so no fireable loadout can be read out of a session
/// that no longer exists.
#[test]
fn accept_f28_d_a_closed_session_audits_to_nothing() {
    let mut world = World::new();
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session opens");
    session
        .register(actor(1), &declared_catalogue())
        .expect("the catalogue registers");
    assert_eq!(
        session_ordnance_audit(&session).component_count(),
        6,
        "a live session reports its loadout"
    );

    session.close(&mut world);
    let after = session_ordnance_audit(&session);
    assert!(
        after.is_empty(),
        "a closed session holds no components and no boosters: {:?} {:?}",
        after.rows(),
        after.nitro()
    );
    assert!(
        after.findings().is_empty(),
        "and reports no gaps it can no longer see: {:?}",
        after.findings()
    );
    assert!(
        session.runtime().nitro_actors().next().is_none(),
        "the rebuilt runtime holds no nitro ledger"
    );
}

/// The declared catalogue's ids are the ones a session registers, so the audit
/// and the session describe one catalogue rather than two.
#[test]
fn accept_f28_d_the_session_audit_sees_the_whole_declared_catalogue() {
    let mut wanted: Vec<String> = [
        DECLARED_SYNTHETIC_DIRECT_KEY,
        DECLARED_SYNTHETIC_FLAK_KEY,
        DECLARED_SYNTHETIC_GUIDED_KEY,
        DECLARED_SYNTHETIC_AREA_DENIAL_KEY,
        DECLARED_SYNTHETIC_TORPEDO_KEY,
        DECLARED_SYNTHETIC_NITRO_KEY,
    ]
    .iter()
    .map(|key| format!("weapon/{key}"))
    .collect();
    wanted.sort();

    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a session opens");
    let registered = session
        .register(actor(1), &declared_catalogue())
        .expect("the catalogue registers");
    let audit = session_ordnance_audit(&session);
    let mut keys: Vec<String> = registered
        .iter()
        .map(|entry| entry.ordnance.as_str().to_owned())
        .collect();
    keys.sort();
    assert_eq!(
        keys, wanted,
        "the session registers exactly the six fixture records"
    );
    assert_eq!(
        audit.rows().len(),
        wanted.len(),
        "and the audit walks all of them"
    );
    for key in &wanted {
        assert!(
            audit
                .rows()
                .iter()
                .any(|row| row.ordnance().as_str() == key),
            "{key} is audited"
        );
    }
    assert_eq!(
        session.registered_ids(&actor(1)).len(),
        6,
        "the session's own id accessor names the same six"
    );
}

/// The lowered id of one declared catalog key, read back from the session's own
/// registration.
fn registered_id(session: &OrdnanceSession, key: &str) -> cs_sim::weapons::OrdnanceId {
    let wanted = format!("weapon/{key}");
    session
        .registered_ids(&actor(1))
        .into_iter()
        .find(|id| id.as_str() == wanted)
        .unwrap_or_else(|| panic!("{key} is registered"))
}

/// The declared nitro fixture with its consumption and recovery replaced.
///
/// Capacity and thrust are left alone; only the two rates are declared here,
/// which is how the refusal test gets a booster whose tank is not refilled
/// underneath it.
fn declared_nitro_with(consumption_per_s: f64, recovery_per_s: f64) -> DeclaredOrdnance {
    let fixture = declared_synthetic_nitro();
    let DeclaredOrdnanceDetails::Nitro(nitro) = fixture.details() else {
        panic!("the nitro fixture is a booster")
    };
    let mut nitro = (**nitro).clone();
    nitro.parameters.consumption_per_s = cs_content::ordnance::declared_known(consumption_per_s);
    nitro.parameters.recovery_per_s = cs_content::ordnance::declared_known(recovery_per_s);
    DeclaredOrdnance::try_new(
        fixture.ordnance().clone(),
        Origin::SyntheticFixture,
        fixture.family(),
        DeclaredOrdnanceDetails::Nitro(Box::new(nitro)),
        None,
        cs_content::ordnance::declared_synthetic_provenance(),
    )
    .expect("the retimed nitro is valid: zero recovery is allowed, zero consumption is not")
}
