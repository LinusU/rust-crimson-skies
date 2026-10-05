//! Acceptance scenarios F29-C.1: the propulsion → flight-authority consumer.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C` (follow-up F29-C.1). Task test prefix:
//! `accept_f29_c_propulsion_`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! A destroyed engine takes the flight authority down; a scratch does not; a
//! repair brings it back; an unresolved pool and an unregistered actor are
//! refused by name. These tests drive the production path end to end: the
//! declared graph is lowered by [`lower_graph`] / [`lower_policy`], the real
//! [`DamageResolver`] resolves the hit, [`apply_propulsion_state`] reflects the
//! authoritative [`SystemKind::Propulsion`] state onto the spawned body's
//! [`FlightAircraft`], and the production [`FlightForcesPlugin`] ticks it. The
//! thrust is observed off the *flight tick's own output*, so a pass that
//! changed nothing the equations read fails at the thrust, not merely at a
//! record comparison.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. Whether the original cut thrust on a destroyed engine is
//! unrecovered (F29 "Research boundary"); see
//! `docs/findings/2026-10-05-f29-c1-propulsion-consumer.md`.
//!
//! **Why this is a module and not its own test target.** It was originally
//! `tests/accept_f29_c_propulsion_gate.rs`, its own integration-test target,
//! and the CI `cargo test` step then died of the runner's disk on two
//! consecutive runs of this branch — `No space left on device`, with no test
//! failure in the step at all, against a runner measured at 1.91 GiB free in
//! `docs/findings/2026-09-30-t432-ci-disk-verification.md`. This target alone
//! was a **123.5 MB** binary (measured with the committed
//! `[profile.dev] debug = "line-tables-only"`), and Bevy/Avian is already
//! linked by `tests/flight/`, so every byte of it was budget the runner did
//! not have. It is therefore compiled into the F29-C damage-consumer binary by
//! `tests/accept_f29_c_damage_consumers.rs`, which already covers this stage's
//! other consumer — the same `mod`-per-file shape `tests/world/`,
//! `tests/physics/` and `tests/campaign/` use. Nothing is lost: the seven
//! tests keep their names, their `accept_f29_c_propulsion_` prefix and their
//! production path; the target list is one binary shorter.

use bevy::prelude::Entity;
use cs_app::damage::{
    DamageConsumerEvent, DamageConsumerRefusal, DamageConsumerReport, apply_propulsion_state,
    lower_graph, lower_policy,
};
use cs_app::physics::{
    FixtureBodySpec, FlightAircraft, FlightForcesPlugin, FlightSpawnSpec, PhysicsFixture,
    spawn_flight_body,
};
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageGraph, DamageNode, DamageNodeKey,
    DamageNodeKind, DamagePolicy, DamageResolver, HitEvent, HitEventId, PartState,
    SYNTHETIC_ENGINE_INTEGRITY, SYNTHETIC_ENGINE_NODE, SYNTHETIC_MOUNT_NODE, SystemKind,
    SystemState,
};
use cs_sim::flight::{DamageState, EngineState, FlightInput, FlightModel, synthetic_fixed_wing};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
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

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn engine() -> DamageNodeKey {
    key(SYNTHETIC_ENGINE_NODE)
}

fn claim() -> ClaimId {
    ClaimId::new("f29c1.test").expect("a valid claim id")
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

/// Registers a one-node graph whose only part is `node`.
fn registered_with(node: DamageNode) -> DamageResolver {
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29c1-single-part")
            .expect("a valid airframe id"),
        vec![node],
    )
    .expect("the one-node graph validates");
    let mut resolver = DamageResolver::new(session(SESSION), PRODUCER);
    resolver
        .register_actor(
            actor(1),
            graph,
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the actor registers");
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

// -------------------------------------------------- the minimum scenario ---

/// A destroyed engine takes the flight authority down: the next flight tick
/// produces no thrust although the engine is running at full throttle.
#[test]
fn accept_f29_c_propulsion_a_destroyed_engine_cuts_thrust_in_the_flight_tick() {
    let mut resolver = registered();
    let mut world = fixture();
    let plane = full_throttle(&mut world);

    let baseline = apply(&mut world, plane, &resolver, actor(1));
    assert!(
        baseline.report.is_noop() && baseline.log.is_empty(),
        "an intact engine needs no gate update: {baseline:?}"
    );
    let full = thrust_after_tick(&mut world, plane);
    assert!(
        full > 0.0,
        "an intact engine at full throttle thrusts: {full}"
    );

    resolve_hit(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    assert_eq!(
        resolver.system_state(&actor(1), SystemKind::Propulsion),
        Some(SystemState::Disabled)
    );

    let outcome = apply(&mut world, plane, &resolver, actor(1));
    assert_eq!(
        outcome.report,
        DamageConsumerReport {
            thrust_cut: 1,
            ..Default::default()
        }
    );
    assert_eq!(
        outcome.log.events(),
        &[DamageConsumerEvent::ThrustCut { actor: actor(1) }]
    );
    let damage = record(&world, plane).damage();
    assert_eq!(damage.thrust_authority, 0.0);
    assert_eq!(
        damage.control_authority,
        DamageState::PRISTINE.control_authority,
        "the propulsion gate never writes control authority"
    );
    assert_eq!(damage.lift_scale, DamageState::PRISTINE.lift_scale);

    assert!(record(&world, plane).engine().running);
    assert_eq!(
        thrust_after_tick(&mut world, plane),
        0.0,
        "the flight tick produces no thrust from a destroyed engine"
    );

    // Convergent: the same state applied again changes nothing.
    let again = apply(&mut world, plane, &resolver, actor(1));
    assert!(again.report.is_noop() && again.log.is_empty(), "{again:?}");
}

/// A scratched engine is `Damaged`, not `Destroyed`: the gate stays open.
#[test]
fn accept_f29_c_propulsion_a_scratched_engine_keeps_full_thrust() {
    let mut resolver = registered();
    let mut world = fixture();
    let plane = full_throttle(&mut world);
    let full = thrust_after_tick(&mut world, plane);

    resolve_hit(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY / 2.0);
    assert_eq!(
        resolver.part_state(&actor(1), &engine()),
        Some(PartState::Damaged)
    );

    let outcome = apply(&mut world, plane, &resolver, actor(1));
    assert!(
        outcome.report.is_noop() && outcome.log.is_empty(),
        "{outcome:?}"
    );
    assert_eq!(record(&world, plane).damage(), DamageState::PRISTINE);
    let scratched = thrust_after_tick(&mut world, plane);
    assert!(
        (scratched - full).abs() < 1e-9,
        "a scratch keeps full thrust: {scratched} vs {full}"
    );
}

/// A repair — the resolver's state for the actor back to intact — lifts the
/// gate's cut, and the next tick thrusts again.
#[test]
fn accept_f29_c_propulsion_a_repair_restores_thrust() {
    let mut destroyed = registered();
    let mut world = fixture();
    let plane = full_throttle(&mut world);

    resolve_hit(&mut destroyed, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let cut = apply(&mut world, plane, &destroyed, actor(1));
    assert_eq!(cut.report.thrust_cut, 1);
    assert_eq!(thrust_after_tick(&mut world, plane), 0.0);

    // The repaired state: the same actor's engine is intact again. The
    // resolver has no repair entry yet, so the repaired authoritative state is
    // a freshly registered actor, as F29-C's convergence test models it.
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
    assert_eq!(record(&world, plane).damage(), DamageState::PRISTINE);
    let thrust = thrust_after_tick(&mut world, plane);
    assert!(thrust > 0.0, "a repaired engine thrusts again: {thrust}");
}

/// An enabled propulsion system lifts only the gate's own cut: a partial
/// thrust authority another producer wrote is left alone.
#[test]
fn accept_f29_c_propulsion_an_enabled_engine_leaves_a_producers_partial_authority() {
    let resolver = registered();
    let mut world = fixture();
    let plane = full_throttle(&mut world);
    let partial = DamageState {
        thrust_authority: 0.5,
        ..DamageState::PRISTINE
    };
    world
        .world_mut()
        .get_mut::<FlightAircraft>(plane)
        .expect("a flight body")
        .set_damage(partial)
        .expect("a valid damage state");

    let outcome = apply(&mut world, plane, &resolver, actor(1));
    assert!(outcome.report.is_noop(), "{outcome:?}");
    assert_eq!(record(&world, plane).damage(), partial);
}

// --------------------------------------------------------- the refusals ---

/// An unresolved engine pool asserts neither direction: it is refused by name
/// and the gate — even a cut one — is left exactly as it is.
#[test]
fn accept_f29_c_propulsion_an_unresolved_engine_pool_is_refused_and_leaves_the_gate() {
    let reason = "the engine's integrity is unmeasured";
    let resolver = registered_with(
        DamageNode::new(
            engine(),
            DamageNodeKind::Engine,
            Resolved::Unknown {
                claim_id: claim(),
                reason: reason.to_owned(),
            },
        )
        .with_disables(SystemKind::Propulsion),
    );
    assert_eq!(
        resolver.system_state(&actor(1), SystemKind::Propulsion),
        Some(SystemState::Unknown)
    );
    let refusal = DamageConsumerEvent::Refused(DamageConsumerRefusal::UnresolvedIntegrity {
        actor: actor(1),
        node: engine(),
        claim_id: claim(),
        reason: reason.to_owned(),
    });

    let mut world = fixture();
    let plane = full_throttle(&mut world);
    let outcome = apply(&mut world, plane, &resolver, actor(1));
    assert_eq!(
        outcome.report,
        DamageConsumerReport {
            refused: 1,
            ..Default::default()
        }
    );
    assert_eq!(outcome.log.events(), std::slice::from_ref(&refusal));
    assert_eq!(record(&world, plane).damage(), DamageState::PRISTINE);
    assert!(thrust_after_tick(&mut world, plane) > 0.0);

    // A cut the unknown state cannot confirm is not lifted either.
    let cut = DamageState {
        thrust_authority: 0.0,
        ..DamageState::PRISTINE
    };
    world
        .world_mut()
        .get_mut::<FlightAircraft>(plane)
        .expect("a flight body")
        .set_damage(cut)
        .expect("a valid damage state");
    let outcome = apply(&mut world, plane, &resolver, actor(1));
    assert_eq!(outcome.log.events(), &[refusal]);
    assert_eq!(record(&world, plane).damage(), cut);
}

/// A foreign-session or unregistered actor is refused and the gate is left.
#[test]
fn accept_f29_c_propulsion_a_foreign_or_unregistered_actor_is_refused() {
    let mut resolver = registered();
    resolve_hit(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let mut world = fixture();
    let plane = full_throttle(&mut world);

    let foreign = ActorId {
        session: session(SESSION + 1),
        serial: 1,
    };
    let outcome = apply(&mut world, plane, &resolver, foreign);
    assert_eq!(
        outcome.log.events(),
        &[DamageConsumerEvent::Refused(
            DamageConsumerRefusal::ForeignSession {
                expected: SESSION,
                found: SESSION + 1,
            }
        )]
    );
    assert_eq!(outcome.report.refused, 1);
    assert_eq!(record(&world, plane).damage(), DamageState::PRISTINE);

    let unknown = apply(&mut world, plane, &resolver, actor(99));
    assert_eq!(
        unknown.log.events(),
        &[DamageConsumerEvent::Refused(
            DamageConsumerRefusal::UnknownActor { actor: actor(99) }
        )]
    );
    assert_eq!(unknown.report.refused, 1);
    assert_eq!(record(&world, plane).damage(), DamageState::PRISTINE);
    assert!(thrust_after_tick(&mut world, plane) > 0.0);
}

/// A graph that declares no propulsion carrier has nothing to gate: even a
/// destroyed weapon mount leaves the thrust authority alone.
#[test]
fn accept_f29_c_propulsion_a_graph_without_an_engine_leaves_the_gate() {
    let mount = key(SYNTHETIC_MOUNT_NODE);
    let mut resolver = registered_with(
        DamageNode::new(
            mount.clone(),
            DamageNodeKind::WeaponMount,
            Resolved::Known(Known::new(10.0, Provenance::designed(claim()))),
        )
        .with_disables(SystemKind::Weapon),
    );
    resolve_hit(&mut resolver, &mount, 10.0);
    assert_eq!(
        resolver.system_state(&actor(1), SystemKind::Propulsion),
        None
    );

    let mut world = fixture();
    let plane = full_throttle(&mut world);
    let outcome = apply(&mut world, plane, &resolver, actor(1));
    assert!(
        outcome.report.is_noop() && outcome.log.is_empty(),
        "{outcome:?}"
    );
    assert_eq!(record(&world, plane).damage(), DamageState::PRISTINE);
}
