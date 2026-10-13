//! Acceptance scenarios F29-C.1a: the propulsion gate composes with the
//! equipment producer instead of being overwritten by it.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C` (follow-up F29-C.1a). Task test prefix:
//! `accept_f29_c_propulsion_equipment_`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! F29-C.1 wired the propulsion consumer's thrust cut into
//! [`FlightAircraft`]'s `DamageState`, but `FlightAircraft::bind_equipment`
//! replaced that whole state whenever the producer's [`FlightEquipment`]
//! record changed — a fuel burn or ordnance release silently lifted a
//! destroyed engine's cut until the next consumer pass. These tests drive
//! the production path end to end — the declared graph lowered by
//! [`lower_graph`] / [`lower_policy`], the real [`DamageResolver`],
//! [`apply_propulsion_state`] on the spawned body and the production
//! [`FlightForcesPlugin`] tick — and assert the two producers of the damage
//! state compose: the equipment rebind still lands (its loadout moves the
//! integrator's mass) while the damage system's cut stands.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data; see
//! `docs/findings/2026-10-05-f29-c1-propulsion-consumer.md` ("Known limits",
//! the producer overlap this stage resolves).
//!
//! **Module, not target.** Same reason as `propulsion_gate.rs`: every
//! Bevy/Avian-linked integration-test binary costs the CI runner's disk
//! ~100 MB it does not have, so this file is compiled into the F29-C
//! damage-consumer binary by `tests/accept_f29_c_damage_consumers.rs`.

use avian3d::prelude::Mass;
use bevy::prelude::Entity;
use cs_app::damage::{
    DamageConsumerEvent, DamageConsumerReport, apply_propulsion_state, lower_graph, lower_policy,
};
use cs_app::physics::flight::{FlightEquipment, PropulsionGate};
use cs_app::physics::{
    FixtureBodySpec, FlightAircraft, FlightForcesPlugin, FlightSpawnSpec, PhysicsFixture,
    spawn_flight_body,
};
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_sim::damage::{
    ActorId, DamageChannel, DamageNodeKey, DamageResolver, HitEvent, HitEventId,
    SYNTHETIC_ENGINE_INTEGRITY, SYNTHETIC_ENGINE_NODE, SystemKind, SystemState,
};
use cs_sim::flight::{
    DamageState, EngineState, FlightInput, FlightModel, LoadoutMass, synthetic_fixed_wing,
};
use cs_types::Tick;
use cs_types::net::SessionId;

const SESSION: u64 = 43;
const PRODUCER: u32 = 1;

// ----------------------------------------------------------------- helpers ---

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

fn engine() -> DamageNodeKey {
    DamageNodeKey::new(SYNTHETIC_ENGINE_NODE).expect("test node keys are valid")
}

/// The production declared → lowered → resolver path for one actor.
fn registered() -> DamageResolver {
    let declared = declared_synthetic_airframe_damage();
    let graph = lower_graph(&declared).expect("the declared graph lowers");
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    let mut resolver = DamageResolver::new(session(SESSION), PRODUCER);
    resolver
        .register_actor(actor(1), graph, policy)
        .expect("the lowered actor registers");
    resolver
}

/// Resolves exactly one internal hit of `damage` on `node`.
fn resolve_hit(resolver: &mut DamageResolver, node: &DamageNodeKey, damage: f64) {
    let hit = HitEvent::try_new(
        HitEventId {
            session: session(SESSION),
            tick: Tick(0),
            producer: PRODUCER,
            sequence: 0,
        },
        Some(actor(2)),
        actor(1),
        node.clone(),
        DamageChannel::Internal,
        damage,
    )
    .expect("a finite, non-negative hit");
    resolver
        .resolve(Tick(0), &[hit])
        .expect("the resolver accepts the batch");
}

/// A flight world with the production [`FlightForcesPlugin`]; the fixture's
/// own body is parked far away.
fn fixture() -> PhysicsFixture {
    PhysicsFixture::builder(FixtureBodySpec {
        mass_kg: 1.0,
        half_extents_m: [0.05, 0.05, 0.05],
        position_m: [-10_000.0, 0.0, 0.0],
        linear_velocity_m_s: [0.0; 3],
    })
    .configure(|app| {
        app.add_plugins(FlightForcesPlugin);
    })
    .build()
    .expect("the fixture spec is valid")
}

/// Spawns the synthetic fixed-wing aircraft at full throttle with a running
/// engine, through the production spawn path.
fn full_throttle(fixture: &mut PhysicsFixture) -> Entity {
    spawn_flight_body(
        fixture.world_mut(),
        FlightModel::new(synthetic_fixed_wing()),
        &FlightSpawnSpec {
            engine: EngineState::direct(1.0),
            command: FlightInput::try_new(0.0, 0.0, 0.0, 1.0, false).expect("valid input"),
            ..FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, -60.0])
        },
    )
    .expect("the spawn spec is valid")
}

/// Runs the consumer pass on the spawned body's live [`FlightAircraft`].
fn apply(
    fixture: &mut PhysicsFixture,
    plane: Entity,
    resolver: &DamageResolver,
    actor: ActorId,
) -> cs_app::damage::DamageConsumerOutcome {
    let mut world = fixture.world_mut().entity_mut(plane);
    let mut flight = world
        .get_mut::<FlightAircraft>()
        .expect("a spawned flight body carries FlightAircraft");
    apply_propulsion_state(resolver, actor, &mut flight)
}

fn record(fixture: &PhysicsFixture, plane: Entity) -> FlightAircraft {
    fixture
        .world()
        .get::<FlightAircraft>(plane)
        .expect("a spawned flight body carries FlightAircraft")
        .clone()
}

/// Steps one fixed tick and returns the thrust the flight equations produced.
fn thrust_after_tick(fixture: &mut PhysicsFixture, plane: Entity) -> f64 {
    fixture.step(1);
    record(fixture, plane)
        .last_output()
        .expect("a driven tick leaves an output")
        .instrument_state
        .thrust_n
}

/// The integrator's mass of the spawned body.
fn mass(fixture: &PhysicsFixture, plane: Entity) -> f32 {
    fixture
        .world()
        .get::<Mass>(plane)
        .expect("a spawned flight body has a mass")
        .0
}

// -------------------------------------------------- the minimum scenario ---

/// A destroyed engine's cut survives a producer's equipment change on the
/// same body: the rebind lands — its loadout moves the integrator's mass —
/// and the next production tick still produces 0 N of thrust.
#[test]
fn accept_f29_c_propulsion_equipment_a_destroyed_engine_stays_cut_across_an_equipment_change() {
    let mut resolver = registered();
    let mut world = fixture();
    let plane = full_throttle(&mut world);

    // The producer declares a loaded aircraft; the tick binds it (the
    // airframe is 1200 kg, so 550 kg of loadout reads as 1750 kg).
    world.world_mut().entity_mut(plane).insert(FlightEquipment {
        loadout: LoadoutMass {
            fuel_kg: 300.0,
            ordnance_kg: 200.0,
            armor_kg: 50.0,
        },
        ..FlightEquipment::EMPTY
    });
    let full = thrust_after_tick(&mut world, plane);
    assert!(
        full > 0.0,
        "an intact engine at full throttle thrusts: {full}"
    );
    assert_eq!(mass(&world, plane), 1750.0);

    resolve_hit(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    assert_eq!(
        resolver.system_state(&actor(1), SystemKind::Propulsion),
        Some(SystemState::Disabled)
    );
    let cut = apply(&mut world, plane, &resolver, actor(1));
    assert_eq!(cut.report.thrust_cut, 1);
    assert_eq!(thrust_after_tick(&mut world, plane), 0.0);

    // The producer's record changes mid-flight — an ordnance release and
    // spent armor — and the next production tick rebinds it. The declared
    // loadout must land while the damage system's cut stands.
    world.world_mut().entity_mut(plane).insert(FlightEquipment {
        loadout: LoadoutMass {
            fuel_kg: 300.0,
            ..LoadoutMass::EMPTY
        },
        ..FlightEquipment::EMPTY
    });
    assert_eq!(
        thrust_after_tick(&mut world, plane),
        0.0,
        "an equipment rebind must not lift the destroyed engine's cut"
    );
    let aircraft = record(&world, plane);
    assert_eq!(
        aircraft.loadout(),
        LoadoutMass {
            fuel_kg: 300.0,
            ..LoadoutMass::EMPTY
        },
        "the changed equipment record still bound"
    );
    assert_eq!(mass(&world, plane), 1500.0);
    assert_eq!(aircraft.propulsion_gate(), PropulsionGate::Cut);
    assert_eq!(aircraft.damage().thrust_authority, 0.0);
    assert_eq!(
        aircraft.damage().lift_scale,
        DamageState::PRISTINE.lift_scale,
        "the gate writes thrust only"
    );

    // Convergent: the same resolver state applied after the rebind changes
    // nothing, and the tick after still produces no thrust.
    let again = apply(&mut world, plane, &resolver, actor(1));
    assert!(again.report.is_noop() && again.log.is_empty(), "{again:?}");
    assert_eq!(thrust_after_tick(&mut world, plane), 0.0);
}

/// A repair lifts the gate's own cut across an equipment rebind — and the
/// authority the thrust returns to is the producer's declared one, not a
/// pristine `1` the consumer invented.
#[test]
fn accept_f29_c_propulsion_equipment_a_repair_restores_the_declared_authority() {
    let mut destroyed = registered();
    let mut world = fixture();
    let plane = full_throttle(&mut world);
    let full = thrust_after_tick(&mut world, plane);
    assert!(full > 0.0);

    resolve_hit(&mut destroyed, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let cut = apply(&mut world, plane, &destroyed, actor(1));
    assert_eq!(cut.report.thrust_cut, 1);
    assert_eq!(thrust_after_tick(&mut world, plane), 0.0);

    // While the engine is down the producer declares a half-thrust aircraft;
    // the rebind lands and the cut stands.
    world.world_mut().entity_mut(plane).insert(FlightEquipment {
        damage: DamageState {
            thrust_authority: 0.5,
            ..DamageState::PRISTINE
        },
        ..FlightEquipment::EMPTY
    });
    assert_eq!(thrust_after_tick(&mut world, plane), 0.0);
    assert_eq!(
        record(&world, plane).damage().thrust_authority,
        0.0,
        "the declared 0.5 is composed under the cut, not surfaced"
    );

    // The repaired state: the same actor's engine intact again (the resolver
    // has no repair entry yet, so a freshly registered actor is the intact
    // authoritative state, as F29-C.1's repair test models it).
    let repaired = registered();
    let outcome = apply(&mut world, plane, &repaired, actor(1));
    assert_eq!(
        outcome.report,
        DamageConsumerReport {
            thrust_restored: 1,
            ..Default::default()
        }
    );
    assert_eq!(
        outcome.log.events(),
        &[DamageConsumerEvent::ThrustRestored { actor: actor(1) }]
    );

    let aircraft = record(&world, plane);
    assert_eq!(aircraft.propulsion_gate(), PropulsionGate::Open);
    assert_eq!(
        aircraft.damage().thrust_authority,
        0.5,
        "the repair restores the equipment's declared authority"
    );
    let thrust = thrust_after_tick(&mut world, plane);
    assert!(
        (thrust - full / 2.0).abs() < 1.0,
        "a repaired engine thrusts at the declared authority: {thrust} vs {}",
        full / 2.0
    );
}

/// An equipment record that declares zero thrust is the producer's own
/// authority, not the damage system's cut: an enabled propulsion state lifts
/// nothing, and the declared `0` keeps producing 0 N.
#[test]
fn accept_f29_c_propulsion_equipment_an_enabled_engine_keeps_an_equipment_declared_zero() {
    let resolver = registered();
    let mut world = fixture();
    let plane = full_throttle(&mut world);

    world.world_mut().entity_mut(plane).insert(FlightEquipment {
        damage: DamageState {
            thrust_authority: 0.0,
            ..DamageState::PRISTINE
        },
        ..FlightEquipment::EMPTY
    });
    assert_eq!(
        thrust_after_tick(&mut world, plane),
        0.0,
        "the producer's declared zero reaches the equations"
    );

    let outcome = apply(&mut world, plane, &resolver, actor(1));
    assert!(
        outcome.report.is_noop() && outcome.log.is_empty(),
        "an enabled propulsion system lifts only the gate's own cut: {outcome:?}"
    );
    let aircraft = record(&world, plane);
    assert_eq!(aircraft.propulsion_gate(), PropulsionGate::Open);
    assert_eq!(aircraft.damage().thrust_authority, 0.0);
    assert_eq!(
        thrust_after_tick(&mut world, plane),
        0.0,
        "a producer's declared zero is never lifted to a cut it did not make"
    );
}
