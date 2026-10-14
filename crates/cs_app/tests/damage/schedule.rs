//! Acceptance scenarios F29-C (schedule): the session's fixed tick drives
//! the damage and debris consumer passes — not only the direct callers the
//! sibling tests use.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C`, task F29-C-SCHEDULE "Drive the F29 damage and debris
//! consumer passes from the session schedule" (#668). Task test prefix:
//! `accept_f29_c_schedule_` (the stage prefix `accept_f29_c_` selects
//! these too). Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The scenarios the task names, each read off the real scheduled path:
//!
//! * **a destroyed part takes its consumers down in the live tick** — a
//!   real [`PhysicsSession`] built through `production_builder` fires real
//!   rounds at a victim's swept box, the weapon step resolves the hits into
//!   the installed [`SessionDamageResolver`], and the damage schedule turns
//!   the resolution into the designed collider decision, the authored
//!   debris instance, the cut thrust authority and the destroyed visual —
//!   all read back off the live world;
//! * **a destroyed mount disables the victim's firing** — the mount gate
//!   the schedule writes is the session's own [`WeaponSession`], so the
//!   victim really cannot fire afterward;
//! * **a lethal hit publishes the destruction** — the batch's lifecycle
//!   record becomes a `DamageActorDestroyed` event and the survivor's
//!   intact mount is grounded, in the order the findings describe;
//! * **refusals stay visible** — a foreign bound actor, an unresolved
//!   debris binding and a propulsion carrier with no flight gate are all
//!   named in the [`DamageScheduleReport`], not scheduled away;
//! * **teardown releases the debris** — a replaced session, and a binding
//!   the scene released mid-session, both run [`release_debris`] so no
//!   wreckage entity is left behind.
//!
//! Nothing here calls the consumer seams by hand: every assertion fails if
//! [`drive_damage_schedule`](cs_app::damage::drive_damage_schedule) is not
//! scheduled. The `PartDebrisBinding` / `DamageZoneBinding` /
//! `DamageActorBinding` insertions are the spawn path's job (F29-C.2's
//! finding names the stage that owns them); the test binds them by hand
//! exactly as that path will.
//!
//! One routed contact produces one [`cs_sim::damage::HitEvent`] per declared
//! channel — the fixture gun's armor 6.0 and internal 3.0 — so a single
//! accepted shot applies 9.0 to the victim's pool: the engine's 15 and the
//! mount's 10 both die on the *second* shot. The assertions read the report
//! of the tick the transition lands on, because the schedule is convergent
//! and a later tick's report is a designed no-op.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. Whether the original drove these consumers per tick is
//! unrecovered (F29 "Research boundary"); the wiring is this engine's
//! designed behavior, recorded in
//! `docs/findings/2026-10-14-f29-c-damage-consumer-schedule.md`.
//!
//! **Why this is a module and not its own test target** is the same
//! measured reason `damage/propulsion_gate.rs` gives: each Bevy/Avian
//! integration-test binary costs ~123 MB of CI runner disk, so the F29-C
//! schedule tests compile into the consumer binary's `mod`-per-file
//! layout.

use avian3d::prelude::{ColliderDisabled, LinearVelocity};
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::damage::{
    DamageActorBinding, DamageConsumerEvent, DamageConsumerRefusal, DamageScheduleReport,
    DamageTeardownRecord, DamageZoneBinding, DroppedDamageBatch, PendingDamageEvents,
    ReleasedDebris, lower_graph, lower_policy,
};
use cs_app::debris::{DamageDebrisEvent, DamageDebrisRefusal, PartDebrisBinding, SpawnedDebris};
use cs_app::physics::{
    BodyMode, BodySpec, FlightAircraft, FlightSpawnSpec, NodeColliderPresence, PhysicsSession,
    spawn_body, spawn_flight_body,
};
use cs_app::scene::{AirframeDamageState, NodeVisualTransform, SceneGeneration, SceneGenerations};
use cs_app::targeting::{TargetingSession, lower_rules, lower_selection_actions};
use cs_app::weapons::{
    MountPoseBinding, PartSweptBox, SessionDamageResolver, WeaponActorBinding, WeaponOrder,
    WeaponSession, WeaponTickInput, WeaponTickInputs,
};
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_content::scene::SceneNodeId;
use cs_content::target_rules::{
    declared_synthetic_selection_actions, declared_synthetic_target_rules,
};
use cs_content::weapons::{
    DeclaredDamageChannel, DeclaredGunDefinition, DeclaredGunMountKind, DeclaredGunRate,
    SYNTHETIC_TICKS_BETWEEN_SHOTS, declared_synthetic_gun,
};
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageEvent, DamageEventId, DamageEventKind,
    DamageGraph, DamageNode, DamageNodeKey, DamageNodeKind, DamagePolicy, DamageResolver, HitEvent,
    HitEventId, PartState, SystemKind,
};
use cs_sim::flight::{FlightModel, synthetic_fixed_wing};
use cs_sim::targeting::{TargetClass, TargetRecord};
use cs_sim::weapons::{FireIntent, FireIntentId, GunBank, SYNTHETIC_STARTING_ROUNDS};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

/// The session generation the fixture opens.
const SESSION: u64 = 41;
/// The weapon session's router producer serial.
const ROUTER_PRODUCER: u32 = 91;
/// The damage resolver's producer serial.
const DAMAGE_PRODUCER: u32 = 92;
/// The scene generation every live entity in these tests is stamped under.
const GENERATION: SceneGeneration = SceneGeneration(1);
/// The fixed rate the schedule runs at: 64 Hz is exactly representable, so
/// one pumped tick commits exactly one fixed tick.
const FIXED_HZ: u32 = 64;
/// The synthetic fixture's own nodes.
const ENGINE: &str = "engine_1";
const MOUNT: &str = "gun_mount_1";
const HULL: &str = "hull";
/// The muzzle's z, well inside the target box the round crosses.
const MUZZLE_Z: f32 = 10.0;

fn session_id(session: u64) -> SessionId {
    SessionId::new(session).expect("the test session generation is nonzero")
}

fn actor(serial: u64) -> ActorId {
    actor_in(SESSION, serial)
}

fn actor_in(session: u64, serial: u64) -> ActorId {
    ActorId {
        session: session_id(session),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn claim() -> ClaimId {
    ClaimId::new("f29c.sched-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// The authored debris object a fixture part spawns: synthetic content,
/// named like the part it belongs to — the record F29-C.2's tests use.
fn debris_id(part: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::SceneNode,
        &format!("synthetic.devastator.{part}_wreck"),
    )
    .expect("a valid debris object id")
}

/// The visual [`SceneNodeId`] the declared fixture binds the named part to.
fn scene_node(name: &str) -> SceneNodeId {
    SceneNodeId::from_content_id(
        ContentId::from_source(
            ContentKind::SceneNode,
            &format!("synthetic.devastator.{name}"),
        )
        .expect("a valid scene node id"),
    )
    .expect("the fixture's binding is a scene node")
}

/// The declared fixture gun moved onto `mount`, the same record F27-C's
/// schedule tests register.
fn declared_on(mount: &str, kind: DeclaredGunMountKind, lifetime: u64) -> DeclaredGunDefinition {
    let fixture = declared_synthetic_gun();
    DeclaredGunDefinition::try_new(
        fixture.gun().clone(),
        Origin::SyntheticFixture,
        cs_content::damage::DamageNodeKey::new(mount).expect("a valid mount key"),
        kind,
        fixture.scene_binding().cloned(),
        fixture.caliber().clone(),
        fixture.ammunition().clone(),
        known(DeclaredGunRate {
            ticks_between_shots: SYNTHETIC_TICKS_BETWEEN_SHOTS,
        }),
        fixture.muzzle_velocity_mps().clone(),
        known(lifetime),
        fixture.spread().clone(),
        fixture.damage().clone(),
        fixture.inheritance().clone(),
        fixture.effect().clone(),
        fixture.sound().clone(),
        fixture.rules().clone(),
        Provenance::designed(claim()),
    )
    .expect("a valid declared gun")
}

/// The declared internal damage of the fixture gun, read from the declared
/// record rather than restated.
fn declared_internal_damage() -> f64 {
    declared_synthetic_gun()
        .damage()
        .known_amount(DeclaredDamageChannel::Internal)
        .expect("the fixture declares an internal amount")
}

/// The shooter's live hierarchy: an actor root carrying the weapon binding
/// and the airframe velocity, with the mount node at the muzzle depth.
fn shooter_world(world: &mut World) -> Entity {
    let fixture = declared_synthetic_gun();
    let root = world
        .spawn((
            WeaponActorBinding {
                actor: actor(1),
                guns: vec![fixture.gun().clone()],
                loadout: fixture.gun().clone(),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::ZERO),
        ))
        .id();
    world.spawn((
        MountPoseBinding {
            mount: key(MOUNT),
            generation: GENERATION,
        },
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, MUZZLE_Z)),
        ChildOf(root),
    ));
    root
}

/// The static body a damage zone's collider lives on, bound as `actor`'s
/// `node` zone — the record the F29-B spawn path writes.
fn zone_node(world: &mut World, actor: ActorId, node: &DamageNodeKey) -> Entity {
    let zone = spawn_body(
        world,
        &BodySpec {
            layer: CollisionLayer::StaticWorld,
            shape: ShapeClass::Solid,
            mode: BodyMode::Static,
            mass_kg: 0.0,
            half_extents_m: [0.5; 3],
            position_m: [500.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0; 3],
        },
    )
    .expect("a valid static body spec");
    world.entity_mut(zone).insert((
        NodeColliderPresence::Live,
        DamageZoneBinding::new(actor, node.clone()),
    ));
    zone
}

/// The victim's live aircraft: a real flight body bound as the damage
/// actor, with one swept box for `node` at the origin, the zone collider
/// the part's damage decides, and the authored debris record the spawn
/// path will write.
fn victim_world(
    world: &mut World,
    victim: ActorId,
    node: &DamageNodeKey,
    debris: Resolved<ContentId>,
) -> Entity {
    let declared = declared_synthetic_airframe_damage();
    let root = spawn_flight_body(
        world,
        FlightModel::new(synthetic_fixed_wing()),
        &FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, 0.0]),
    )
    .expect("the flight body spawns");
    world.entity_mut(root).insert(DamageActorBinding {
        actor: victim,
        graph: declared.subject().clone(),
        generation: GENERATION,
    });
    world.spawn((
        PartSweptBox::new(victim, node.clone(), [3.0, 3.0, 40.0]),
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, 0.0)),
        ChildOf(root),
    ));
    zone_node(world, victim, node);
    world.spawn(PartDebrisBinding::new(
        victim,
        node.clone(),
        debris,
        GENERATION,
    ));
    root
}

/// The session's damage authority through the production declared →
/// lowered → registered path, with `actors`' synthetic airframe graphs.
fn damage_resolver(session: u64, actors: &[ActorId]) -> DamageResolver {
    let declared = declared_synthetic_airframe_damage();
    let graph = lower_graph(&declared).expect("the declared graph lowers");
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    let mut damage = DamageResolver::new(session_id(session), DAMAGE_PRODUCER);
    for actor in actors {
        damage
            .register_actor(*actor, graph.clone(), policy)
            .expect("the lowered actor registers");
    }
    damage
}

/// A weapon session with the fixture's nose gun mounted and selected for
/// every actor in `armed`.
fn weapon_session(armed: &[ActorId]) -> WeaponSession {
    let mut session = WeaponSession::new(SESSION, Tick(0), ROUTER_PRODUCER)
        .expect("a nonzero session generation opens");
    for shooter in armed {
        let nose = declared_on(MOUNT, DeclaredGunMountKind::Nose, 90);
        session
            .register(
                *shooter,
                &[nose],
                GunBank::try_new([key(MOUNT)]).expect("a valid bank"),
                SYNTHETIC_STARTING_ROUNDS,
            )
            .expect("the declared gun registers");
    }
    session
}

/// The session's declared allegiance authority: the synthetic target rules'
/// own table with the fixture roster registered, the same write a roster
/// pass commits.
fn targeting_session(session: u64, generation: SceneGeneration) -> TargetingSession {
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let bindings = lower_selection_actions(&declared_synthetic_selection_actions())
        .expect("the fixture actions lower");
    let mut targeting = TargetingSession::new(
        session_id(session),
        lowered,
        bindings,
        declared.subject().clone(),
        generation,
    );
    let roster = [
        (1, cs_sim::targeting::synthetic_player_faction()),
        (2, cs_sim::targeting::synthetic_raider_faction()),
        (3, cs_sim::targeting::synthetic_trader_faction()),
    ];
    for (serial, faction) in roster {
        targeting
            .store_mut()
            .register(TargetRecord {
                actor: actor_in(session, serial),
                faction,
                class: TargetClass::Aircraft,
                objective: false,
                revealed: true,
                phase_eligible: true,
                position: WorldPosition::try_new([0.0, 0.0, 0.0])
                    .expect("the fixture position is finite"),
            })
            .expect("the fixture actor registers");
    }
    targeting
}

/// The production fixed-tick session — `production_builder` composes the
/// animation plugin, the F27-C weapon schedule and the F29-C damage
/// schedule the same way a running session does.
fn schedule_session() -> PhysicsSession {
    PhysicsSession::production_builder()
        .fixed_hz(FIXED_HZ)
        .build()
}

fn world(session: &mut PhysicsSession) -> &mut World {
    session.world_mut().expect("the session is active")
}

/// Advances the session's world by `ticks` committed fixed ticks.
fn step(session: &mut PhysicsSession, ticks: u64) {
    session.step(ticks).expect("the session steps");
}

/// Installs one generation's session resources, the way a session's open or
/// restart does — including the visual record the consumer pass writes.
fn install_session(
    session: &mut PhysicsSession,
    weapons: WeaponSession,
    damage: DamageResolver,
    targeting: TargetingSession,
) {
    let world = world(session);
    world.insert_resource(weapons);
    world.insert_resource(SessionDamageResolver::new(damage));
    world.insert_resource(targeting);
    world.insert_resource(AirframeDamageState::new());
    let mut generations = SceneGenerations::default();
    while generations.latest() < GENERATION {
        generations.take_next();
    }
    world.insert_resource(generations);
}

/// Publishes one tick's weapon record the way the session's driver does.
fn publish(session: &mut PhysicsSession, input: WeaponTickInput) {
    world(session)
        .resource_mut::<WeaponTickInputs>()
        .push(input);
}

/// One tick's record for `at`, carrying the session's authored fire `orders`.
fn fire_at(at: Tick, shooter: ActorId, sequence: u32) -> WeaponTickInput {
    WeaponTickInput {
        session: SESSION,
        at,
        wind_velocity_m_s: Some([0.0; 3]),
        orders: vec![WeaponOrder::Fire(FireIntent {
            id: FireIntentId {
                session: SESSION,
                tick: at,
                producer: u32::try_from(shooter.serial).expect("a small actor serial"),
                sequence,
            },
            shooter,
        })],
        cockpit: None,
    }
}

/// The schedule report's snapshot after the last run.
fn schedule_report(session: &PhysicsSession) -> DamageScheduleReport {
    session
        .world()
        .expect("the session is active")
        .resource::<DamageScheduleReport>()
        .clone()
}

/// Every debris instance in the world, in entity order.
fn instances(world: &World, actor: ActorId) -> Vec<Entity> {
    world
        .iter_entities()
        .filter_map(|entity_ref| {
            let debris = entity_ref.get::<SpawnedDebris>()?;
            (debris.actor == actor).then_some(entity_ref.id())
        })
        .collect()
}

// -------------------------------------------------- the minimum scenario ---

/// The full chain the task names: the scheduled tick takes a destroyed part
/// through every consumer — the collider decision, the authored debris
/// instance, the propulsion gate and the visual record — and reports each.
#[test]
fn accept_f29_c_schedule_the_tick_drives_every_consumer() {
    let mut session = schedule_session();
    shooter_world(world(&mut session));
    let victim = victim_world(
        world(&mut session),
        actor(2),
        &key(ENGINE),
        known(debris_id(ENGINE)),
    );
    install_session(
        &mut session,
        weapon_session(&[actor(1), actor(2)]),
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    // An intact tick: the schedule ran and reported its passes, and nothing
    // changed — the convergent state needs no writes.
    step(&mut session, 1);
    let ran = schedule_report(&session);
    assert_eq!(ran.session, Some(SESSION), "the installed session served");
    assert_eq!(ran.at, Some(Tick(1)), "the committed tick is the ledger's");
    assert!(ran.missing.is_empty(), "every authority was found: {ran:?}");
    assert!(
        ran.consumers
            .as_ref()
            .expect("the consumer passes ran")
            .report
            .is_noop(),
        "an intact world needs no consumer update: {ran:?}"
    );

    // Two declared shots on the engine's swept box: each fire applies the
    // declared armor and internal hits (6.0 + 3.0), so the engine's
    // integrity of 15 is destroyed by the second shot, on tick 6.
    for (i, tick) in [2, 6].into_iter().enumerate() {
        publish(&mut session, fire_at(Tick(tick), actor(1), i as u32));
    }
    step(&mut session, 5);

    let damage = world(&mut session)
        .resource::<SessionDamageResolver>()
        .resolver()
        .clone();
    assert_eq!(
        damage.part_state(&actor(2), &key(ENGINE)),
        Some(PartState::Destroyed),
        "the declared hits destroyed the engine: {:?}",
        damage.remaining_integrity(&actor(2), &key(ENGINE)),
    );

    let ran = schedule_report(&session);
    assert_eq!(ran.at, Some(Tick(6)));

    // The collider decision the batch's part transition produced.
    let colliders = ran
        .colliders
        .as_ref()
        .expect("the tick's transitions reached the collider pass");
    assert_eq!(colliders.removed, 1, "{colliders:?}");
    let zone = world(&mut session)
        .iter_entities()
        .find(|entity_ref| {
            entity_ref
                .get::<DamageZoneBinding>()
                .is_some_and(|binding| binding.actor == actor(2))
        })
        .map(|entity_ref| entity_ref.id())
        .expect("the victim's zone is bound");
    assert_eq!(
        world(&mut session).get::<NodeColliderPresence>(zone),
        Some(&NodeColliderPresence::RemovedByDamage),
        "the destroyed zone's collider is the damage-removed record"
    );
    assert!(
        world(&mut session).get::<ColliderDisabled>(zone).is_some(),
        "Avian's own marker says the collider is out of the simulation"
    );

    // The convergent passes: thrust cut, visual destroyed, debris spawned.
    let consumers = ran.consumers.as_ref().expect("the consumer passes ran");
    assert_eq!(
        consumers.report.thrust_cut, 1,
        "the destroyed engine's propulsion carrier cut the gate: {consumers:?}"
    );
    assert_eq!(
        consumers.report.visuals_destroyed, 1,
        "the destroyed engine's visual was recorded: {consumers:?}"
    );
    let debris = ran.debris.as_ref().expect("the debris pass ran");
    assert_eq!(debris.report.spawned, 1, "{debris:?}");

    // The world evidence itself: the gate is really cut, the visual record
    // really destroyed, the debris instance really spawned.
    let flight = world(&mut session)
        .get::<FlightAircraft>(victim)
        .expect("the victim carries the flight gate");
    assert_eq!(flight.damage().thrust_authority, 0.0);
    assert!(
        world(&mut session)
            .resource::<AirframeDamageState>()
            .is_destroyed(&scene_node(ENGINE)),
        "the visual record carries the destruction"
    );
    assert_eq!(
        instances(world(&mut session), actor(2)).len(),
        1,
        "exactly one authored debris instance spawned"
    );

    // The next quiet tick reconverges to a no-op: no collider batch, and the
    // consumer and debris passes change nothing.
    step(&mut session, 1);
    let quiet = schedule_report(&session);
    assert!(quiet.colliders.is_none(), "no batch published: {quiet:?}");
    assert!(
        quiet
            .consumers
            .as_ref()
            .expect("the passes ran")
            .report
            .is_noop(),
        "the converged state is stable: {quiet:?}"
    );
    assert!(
        quiet
            .debris
            .as_ref()
            .expect("the pass ran")
            .report
            .is_noop(),
        "{quiet:?}"
    );
    assert_eq!(
        instances(world(&mut session), actor(2)).len(),
        1,
        "the same instance survives: convergence does not respawn"
    );
}

/// A destroyed mount disables the *victim's* firing through the session's
/// own weapon gate: the mount the `WeaponSession` carries is the one the
/// schedule writes.
#[test]
fn accept_f29_c_schedule_a_destroyed_mount_disables_the_victims_gun() {
    let mut session = schedule_session();
    shooter_world(world(&mut session));
    victim_world(
        world(&mut session),
        actor(2),
        &key(MOUNT),
        known(debris_id(MOUNT)),
    );
    install_session(
        &mut session,
        weapon_session(&[actor(1), actor(2)]),
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    assert!(
        !world(&mut session)
            .resource::<WeaponSession>()
            .state(&actor(2))
            .expect("the victim is armed")
            .is_disabled(&key(MOUNT)),
        "the victim's mount starts live"
    );

    // The mount's integrity is 10: one declared shot applies armor 6 +
    // internal 3 = 9, the second shot's armor hit destroys it, on tick 5.
    for (i, tick) in [1, 5].into_iter().enumerate() {
        publish(&mut session, fire_at(Tick(tick), actor(1), i as u32));
    }
    step(&mut session, 5);

    let ran = schedule_report(&session);
    let colliders = ran
        .colliders
        .as_ref()
        .expect("the mount's transition reached the collider pass");
    assert_eq!(colliders.removed, 1, "{colliders:?}");
    let consumers = ran.consumers.as_ref().expect("the consumer passes ran");
    assert_eq!(
        consumers.report.mounts_disabled, 1,
        "the destroyed mount was disabled once: {consumers:?}"
    );
    assert!(
        consumers.log.events().iter().any(|event| matches!(
            event,
            DamageConsumerEvent::MountDisabled { actor: who, mount }
                if *who == actor(2) && *mount == key(MOUNT)
        )),
        "the disable is the victim's mount: {consumers:?}"
    );
    assert!(
        world(&mut session)
            .resource::<WeaponSession>()
            .state(&actor(2))
            .expect("the victim is armed")
            .is_disabled(&key(MOUNT)),
        "the session's own weapon gate carries the disablement"
    );
    assert!(
        world(&mut session)
            .resource::<AirframeDamageState>()
            .is_destroyed(&scene_node(MOUNT)),
        "the mount's visual is destroyed"
    );
    assert_eq!(
        instances(world(&mut session), actor(2)).len(),
        1,
        "the mount's authored debris spawned"
    );
}

/// A lethal hit publishes the actor's destruction through the same batch:
/// `report.destroyed` names the victim, the `DamageActorDestroyed` event
/// is written, and the survivor's still-intact mount is grounded — the
/// order the F33-C1 finding describes.
#[test]
fn accept_f29_c_schedule_a_lethal_hit_publishes_the_destruction() {
    let mut session = schedule_session();
    shooter_world(world(&mut session));

    // A minimal lethal airframe: one `hull` with 3.0 integrity — the
    // fixture's single internal hit — plus a live mount that must ground.
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29c-lethal")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(
                key(HULL),
                DamageNodeKind::InternalStructure,
                known(declared_internal_damage()),
            )
            .with_lethal(true),
            DamageNode::new(key(MOUNT), DamageNodeKind::WeaponMount, known(10.0))
                .with_disables(SystemKind::Weapon),
        ],
    )
    .expect("the lethal graph validates");
    let mut damage = DamageResolver::new(session_id(SESSION), DAMAGE_PRODUCER);
    damage
        .register_actor(
            actor(2),
            graph.clone(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the actor registers");

    let victim = spawn_flight_body(
        world(&mut session),
        FlightModel::new(synthetic_fixed_wing()),
        &FlightSpawnSpec::level_at([0.0, 400.0, 0.0], [0.0, 0.0, 0.0]),
    )
    .expect("the flight body spawns");
    world(&mut session)
        .entity_mut(victim)
        .insert(DamageActorBinding {
            actor: actor(2),
            graph: graph.subject().clone(),
            generation: GENERATION,
        });
    world(&mut session).spawn((
        PartSweptBox::new(actor(2), key(HULL), [3.0, 3.0, 40.0]),
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, 0.0)),
        ChildOf(victim),
    ));

    install_session(
        &mut session,
        weapon_session(&[actor(1), actor(2)]),
        damage,
        targeting_session(SESSION, GENERATION),
    );

    publish(&mut session, fire_at(Tick(1), actor(1), 0));
    step(&mut session, 1);

    let ran = schedule_report(&session);
    assert_eq!(
        ran.destroyed,
        vec![actor(2)],
        "the batch's destruction reached the publication: {ran:?}"
    );
    let consumers = ran.consumers.as_ref().expect("the consumer passes ran");
    assert_eq!(
        consumers.report.mounts_grounded, 1,
        "the dead actor's intact mount was grounded, not left live: {consumers:?}"
    );
    assert!(
        world(&mut session)
            .resource::<WeaponSession>()
            .state(&actor(2))
            .expect("the victim is armed")
            .is_disabled(&key(MOUNT)),
        "the session's own gate carries the grounding"
    );
}

/// The refusals are never scheduled away: a foreign bound actor, an
/// unresolved debris binding and a propulsion carrier with no flight gate
/// are all named in the report.
#[test]
fn accept_f29_c_schedule_refusals_stay_visible() {
    let mut session = schedule_session();
    shooter_world(world(&mut session));

    // The victim: bound, but with no flight gate and an unresolved debris
    // binding on the engine — so the destruction's propulsion and debris
    // halves both name their gap. No `DamageZoneBinding` either, so the
    // collider pass surfaces the unbound zone rather than dropping it.
    let declared = declared_synthetic_airframe_damage();
    let root = world(&mut session)
        .spawn((
            DamageActorBinding {
                actor: actor(2),
                graph: declared.subject().clone(),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::ZERO),
        ))
        .id();
    world(&mut session).spawn((
        PartSweptBox::new(actor(2), key(ENGINE), [3.0, 3.0, 40.0]),
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, 0.0)),
        ChildOf(root),
    ));
    world(&mut session).spawn(PartDebrisBinding::new(
        actor(2),
        key(ENGINE),
        Resolved::Unknown {
            claim_id: claim(),
            reason: "the engine's debris object is unresolved".to_owned(),
        },
        GENERATION,
    ));

    // The foreign actor: bound under a session the resolver does not serve.
    world(&mut session).spawn(DamageActorBinding {
        actor: actor_in(SESSION + 1, 7),
        graph: declared.subject().clone(),
        generation: GENERATION,
    });

    install_session(
        &mut session,
        weapon_session(&[actor(1), actor(2)]),
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    // Two declared shots destroy the engine on the second one's tick.
    for (i, tick) in [1, 5].into_iter().enumerate() {
        publish(&mut session, fire_at(Tick(tick), actor(1), i as u32));
    }
    step(&mut session, 5);

    let ran = schedule_report(&session);
    // The destroyed engine had no bound zone, so its collider decision is
    // surfaced rather than dropped.
    let colliders = ran
        .colliders
        .as_ref()
        .expect("the tick's transitions reached the collider pass");
    assert_eq!(
        colliders.unbound, 1,
        "the unbound engine zone is reported: {colliders:?}"
    );
    let consumers = ran.consumers.as_ref().expect("the consumer passes ran");
    assert_eq!(
        consumers.report.visuals_destroyed, 1,
        "the engine's visual was still recorded: {consumers:?}"
    );
    assert!(
        consumers.log.events().iter().any(|event| matches!(
            event,
            DamageConsumerEvent::Refused(DamageConsumerRefusal::MissingFlightGate { actor: who })
                if *who == actor(2)
        )),
        "the propulsion carrier with no flight gate is named: {consumers:?}"
    );
    assert!(
        consumers.log.events().iter().any(|event| matches!(
            event,
            DamageConsumerEvent::Refused(DamageConsumerRefusal::ForeignSession {
                expected: SESSION,
                found,
            }) if *found == SESSION + 1
        )),
        "the foreign actor is refused by name: {consumers:?}"
    );
    let debris = ran.debris.as_ref().expect("the debris pass ran");
    assert!(
        debris.log.events().iter().any(|event| matches!(
            event,
            DamageDebrisEvent::Refused(DamageDebrisRefusal::UnresolvedDebris {
                actor: who,
                node,
                ..
            }) if *who == actor(2) && *node == key(ENGINE)
        )),
        "the unresolved binding is refused with its claim: {debris:?}"
    );
    assert!(
        debris.log.events().iter().any(|event| matches!(
            event,
            DamageDebrisEvent::Refused(DamageDebrisRefusal::ForeignSession {
                expected: SESSION,
                found,
            }) if *found == SESSION + 1
        )),
        "the foreign actor's debris pass is refused too: {debris:?}"
    );
    assert!(
        instances(world(&mut session), actor(2)).is_empty(),
        "nothing was guessed into the world"
    );

    // A published batch stamped by another session is dropped and named,
    // never consumed; an event no hit produced is counted `unclaimed`
    // rather than attributed to a victim by guessing.
    let mut pending = world(&mut session).resource_mut::<PendingDamageEvents>();
    pending.push(
        vec![
            HitEvent::try_new(
                HitEventId {
                    session: session_id(SESSION + 1),
                    tick: Tick(6),
                    producer: DAMAGE_PRODUCER,
                    sequence: 0,
                },
                Some(actor_in(SESSION + 1, 1)),
                actor_in(SESSION + 1, 7),
                key(ENGINE),
                DamageChannel::Internal,
                declared_internal_damage(),
            )
            .expect("a finite hit"),
        ],
        Vec::new(),
    );
    pending.push(
        Vec::new(),
        vec![DamageEvent {
            id: DamageEventId {
                session: session_id(SESSION),
                tick: Tick(6),
                producer: DAMAGE_PRODUCER,
                sequence: 0,
            },
            kind: DamageEventKind::PartTransition {
                node: key(ENGINE),
                from: PartState::Intact,
                to: PartState::Destroyed,
            },
        }],
    );
    step(&mut session, 1);
    let ran = schedule_report(&session);
    assert_eq!(
        ran.dropped,
        vec![DroppedDamageBatch {
            session: SESSION + 1,
            hits: 1,
            events: 0,
        }],
        "the foreign batch is dropped by name: {ran:?}"
    );
    assert_eq!(
        ran.unclaimed, 1,
        "the event no hit produced is counted, not attributed: {ran:?}"
    );
}

/// The teardown half: a replaced session's debris leaves with it, and an
/// actor whose bindings the scene released mid-session loses its debris
/// beside them.
#[test]
fn accept_f29_c_schedule_teardown_releases_the_debris() {
    let mut session = schedule_session();
    shooter_world(world(&mut session));
    victim_world(
        world(&mut session),
        actor(2),
        &key(ENGINE),
        known(debris_id(ENGINE)),
    );
    install_session(
        &mut session,
        weapon_session(&[actor(1), actor(2)]),
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    for (i, tick) in [1, 5].into_iter().enumerate() {
        publish(&mut session, fire_at(Tick(tick), actor(1), i as u32));
    }
    step(&mut session, 5);
    assert_eq!(
        instances(world(&mut session), actor(2)).len(),
        1,
        "the engine's debris spawned under the live session"
    );
    let ran = schedule_report(&session);
    assert_eq!(
        ran.colliders
            .as_ref()
            .expect("the destruction tick reached the collider pass")
            .removed,
        1,
        "the engine's transition was consumed, not just its state: {ran:?}"
    );

    // A batch the dead generation never got to consume.
    world(&mut session)
        .resource_mut::<PendingDamageEvents>()
        .push(
            vec![
                HitEvent::try_new(
                    HitEventId {
                        session: session_id(SESSION),
                        tick: Tick(5),
                        producer: DAMAGE_PRODUCER,
                        sequence: 99,
                    },
                    Some(actor(1)),
                    actor(2),
                    key(ENGINE),
                    DamageChannel::Internal,
                    declared_internal_damage(),
                )
                .expect("a finite hit"),
            ],
            Vec::<DamageEvent>::new(),
        );

    // The restart: a new generation's session resources replace the old ones.
    let restarted = SESSION + 1;
    world(&mut session).insert_resource(
        WeaponSession::new(restarted, Tick(0), ROUTER_PRODUCER)
            .expect("a nonzero session generation opens"),
    );
    world(&mut session).insert_resource(SessionDamageResolver::new(damage_resolver(
        restarted,
        &[actor_in(restarted, 2)],
    )));
    world(&mut session).insert_resource(targeting_session(restarted, GENERATION));

    step(&mut session, 1);
    let ran = schedule_report(&session);
    assert_eq!(
        ran.tore_down,
        Some(DamageTeardownRecord {
            session: SESSION,
            debris: 1,
            batches: 1,
        }),
        "the teardown names the superseded session, its debris and its pending batch: {ran:?}"
    );
    assert!(
        instances(world(&mut session), actor(2)).is_empty(),
        "the dead session's wreckage went with it"
    );

    // The binding-release half: an actor whose damage bindings the scene
    // released mid-session loses its debris beside them — the reload's other
    // direction.
    let mut session = schedule_session();
    shooter_world(world(&mut session));
    victim_world(
        world(&mut session),
        actor(2),
        &key(ENGINE),
        known(debris_id(ENGINE)),
    );
    install_session(
        &mut session,
        weapon_session(&[actor(1), actor(2)]),
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );
    for (i, tick) in [1, 5].into_iter().enumerate() {
        publish(&mut session, fire_at(Tick(tick), actor(1), i as u32));
    }
    step(&mut session, 5);
    assert_eq!(instances(world(&mut session), actor(2)).len(), 1);
    assert_eq!(
        schedule_report(&session)
            .colliders
            .as_ref()
            .expect("the second run's destruction came through the published batch")
            .removed,
        1
    );

    // The scene teardown: every damage binding for the actor goes away.
    let bound_entities: Vec<Entity> = world(&mut session)
        .iter_entities()
        .filter(|entity_ref| {
            [
                entity_ref.get::<DamageActorBinding>().map(|b| &b.actor),
                entity_ref.get::<DamageZoneBinding>().map(|b| &b.actor),
                entity_ref.get::<PartDebrisBinding>().map(|b| &b.actor),
            ]
            .into_iter()
            .flatten()
            .any(|bound| *bound == actor(2))
        })
        .map(|entity_ref| entity_ref.id())
        .collect();
    for entity in bound_entities {
        world(&mut session).entity_mut(entity).despawn();
    }

    step(&mut session, 1);
    let ran = schedule_report(&session);
    assert_eq!(
        ran.released,
        vec![ReleasedDebris {
            actor: actor(2),
            debris: 1,
        }],
        "the release names the actor whose bindings went away: {ran:?}"
    );
    assert!(
        instances(world(&mut session), actor(2)).is_empty(),
        "a released actor leaves no wreckage entity behind"
    );
}
