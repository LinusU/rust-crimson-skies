//! Acceptance scenario F27-C (schedule): the session's fixed-tick schedule
//! drives the production per-tick weapon step — selection, the existing fire
//! input, accepted-shot effects, swept damage into the shared resolver, round
//! retirement and ECS mirrors — and tears a superseded session down.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-C`. Task test prefix: `accept_f27_c_sched_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! These tests drive production code only: a real
//! [`cs_app::physics::PhysicsSession`] built through `production_builder` —
//! the composition a running session uses, including the weapon schedule
//! plugin. Every observation is read from [`WeaponScheduleReport`] and the
//! session resources themselves: **no test here calls
//! `step_weapon_session`** — the schedule is its production caller. The fire
//! input is produced by the real [`InputSession`], not a fabricated action.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::environment::{EnvironmentSession, RunSeeds};
use cs_app::input::{DeviceEvent, FrameInput, InputSession, SessionMode};
use cs_app::physics::{PhysicsSession, PhysicsTickLedger};
use cs_app::scene::{NodeVisualTransform, SceneGeneration, SceneGenerations};
use cs_app::session_weapons::{
    SessionCockpitActions, SessionInstallRefusal, SessionLocalSeat, SessionWeaponHost,
    SessionWeaponOrders, SessionWeaponPublishReport, install_session_weapons,
};
use cs_app::targeting::{TargetingSession, lower_rules, lower_selection_actions};
use cs_app::weapons::{
    CockpitInput, DroppedTickInput, DroppedTickInputReason, MountPoseBinding, PartSweptBox,
    RegisteredWeapon, SessionDamageResolver, WeaponActorBinding, WeaponOrder, WeaponRoundMirror,
    WeaponScheduleAuthority, WeaponScheduleMissing, WeaponScheduleReport, WeaponSession,
    WeaponTeardownRecord, WeaponTickInput, WeaponTickInputs,
};
use cs_content::target_rules::{
    declared_synthetic_selection_actions, declared_synthetic_target_rules,
};
use cs_content::weapons::{
    DeclaredDamageChannel, DeclaredGunDefinition, DeclaredGunMountKind, DeclaredGunRate,
    SYNTHETIC_TICKS_BETWEEN_SHOTS, declared_synthetic_gun,
};
use cs_sim::control::LocalSeatId;
use cs_sim::damage::{
    ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver, synthetic_airframe_graph,
};
use cs_sim::targeting::{TargetClass, TargetRecord};
use cs_sim::weapons::{FireIntent, FireIntentId, GunBank, SYNTHETIC_STARTING_ROUNDS};
use cs_types::Tick;
use cs_types::content::{Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::input::{Action, DeviceClass, DeviceId, FlightCommand, Key};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

/// The session generation the fixture opens.
const SESSION: u64 = 41;
/// The weapon session's router producer serial.
const ROUTER_PRODUCER: u32 = 91;
/// The damage resolver's producer serial.
const DAMAGE_PRODUCER: u32 = 92;
/// The producer serial the session allocated to the local cockpit channel.
const COCKPIT_PRODUCER: u32 = 93;
/// The scene generation every live entity in these tests is stamped under.
const GENERATION: SceneGeneration = SceneGeneration(1);
/// The fixed rate the schedule runs at: 64 Hz is exactly representable, so
/// one pumped tick commits exactly one fixed tick.
const FIXED_HZ: u32 = 64;
/// The synthetic fixture's own mount, used here as the nose gun.
const NOSE_MOUNT: &str = "gun_mount_1";
const WING_MOUNT: &str = "wing_mount_1";
const HULL: &str = "hull";
/// The muzzle's z, well behind the target box the round crosses.
const MUZZLE_Z: f32 = 10.0;

fn session_id(session: u64) -> SessionId {
    SessionId::new(session).expect("the test session generation is nonzero")
}

/// The fixture's shooter: serial 1 of the default session.
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
    ClaimId::new("f27c.sched-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// The declared fixture gun moved onto `mount` and declared as a `kind` gun,
/// the same record F27-C's step tests register.
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

/// The shooter's live hierarchy: an actor root carrying the binding and the
/// airframe velocity, with one mount node per mount at the muzzle depth these
/// tests fire from.
fn shooter_world(world: &mut World, mounts: &[&str]) -> Entity {
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
    for (index, mount) in mounts.iter().enumerate() {
        let x = 2.0 * index as f32;
        world.spawn((
            MountPoseBinding {
                mount: key(mount),
                generation: GENERATION,
            },
            NodeVisualTransform(GlobalTransform::from_xyz(x, 0.0, MUZZLE_Z)),
            ChildOf(root),
        ));
    }
    root
}

/// One aircraft's part box: a root carrying the live velocity the previous
/// centre is reconstructed from, and one swept box at the origin.
fn part_box(world: &mut World, owner: ActorId, half_extents_m: [f64; 3]) {
    let root = world.spawn(LinearVelocity(Vec3::ZERO)).id();
    world.spawn((
        PartSweptBox::new(owner, key(HULL), half_extents_m),
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, 0.0)),
        ChildOf(root),
    ));
}

/// The session's damage authority, with the named actors' synthetic airframe
/// graphs registered.
fn damage_resolver(session: u64, actors: &[ActorId]) -> DamageResolver {
    let mut damage = DamageResolver::new(session_id(session), DAMAGE_PRODUCER);
    for actor in actors {
        damage
            .register_actor(
                *actor,
                synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("the fixture graph registers");
    }
    damage
}

/// A weapon session with only the fixture's nose gun mounted and selected.
fn session_with_one_gun(lifetime: u64) -> (WeaponSession, Vec<RegisteredWeapon>) {
    let nose = declared_on(NOSE_MOUNT, DeclaredGunMountKind::Nose, lifetime);
    let mut session = WeaponSession::new(SESSION, Tick(0), ROUTER_PRODUCER)
        .expect("a nonzero session generation opens");
    let registered = session
        .register(
            actor(1),
            &[nose],
            GunBank::try_new([key(NOSE_MOUNT)]).expect("a valid bank"),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("the declared gun registers");
    (session, registered)
}

/// A weapon session with the fixture's two declared guns mounted on the nose
/// and the left wing, both selected.
fn session_with_two_guns(lifetime: u64) -> (WeaponSession, Vec<RegisteredWeapon>) {
    let nose = declared_on(NOSE_MOUNT, DeclaredGunMountKind::Nose, lifetime);
    let wing = declared_on(WING_MOUNT, DeclaredGunMountKind::WingLeft, lifetime);
    let mut session = WeaponSession::new(SESSION, Tick(0), ROUTER_PRODUCER)
        .expect("a nonzero session generation opens");
    let registered = session
        .register(
            actor(1),
            &[nose, wing],
            GunBank::try_new([key(NOSE_MOUNT), key(WING_MOUNT)]).expect("a valid bank"),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("the declared guns register");
    (session, registered)
}

/// The session's declared allegiance authority: the synthetic target rules'
/// own table (player is hostile to raiders, neutral to traders) with the
/// fixture roster registered into the store, the same write a roster pass
/// commits.
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
/// animation plugin and the F27-C weapon schedule the same way a running
/// session does.
fn schedule_session() -> PhysicsSession {
    PhysicsSession::production_builder()
        .fixed_hz(FIXED_HZ)
        .build()
}

/// The session's world, for the spawns and resources the typed API does not
/// cover.
fn world(session: &mut PhysicsSession) -> &mut World {
    session.world_mut().expect("the session is active")
}

/// Advances the session's world by `ticks` committed fixed ticks through
/// `PhysicsSession::step`, the same call a session driver pumps.
fn step(session: &mut PhysicsSession, ticks: u64) {
    session.step(ticks).expect("the session steps");
}

/// Installs one generation's session resources, the way a session's open or
/// restart does.
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
    let mut generations = SceneGenerations::default();
    while generations.latest() < GENERATION {
        generations.take_next();
    }
    world.insert_resource(generations);
}

/// Publishes one tick's record the way the session's driver does.
fn publish(session: &mut PhysicsSession, input: WeaponTickInput) {
    world(session)
        .resource_mut::<WeaponTickInputs>()
        .push(input);
}

/// One tick's record for `at`, carrying the session's authored `orders` and
/// no cockpit.
fn tick_input(at: Tick, orders: Vec<WeaponOrder>) -> WeaponTickInput {
    WeaponTickInput {
        session: SESSION,
        at,
        wind_velocity_m_s: Some([0.0; 3]),
        orders,
        cockpit: None,
    }
}

/// The committed tick the world is sitting on.
fn committed(session: &PhysicsSession) -> Tick {
    Tick(
        session
            .world()
            .expect("the session is active")
            .resource::<PhysicsTickLedger>()
            .ticks,
    )
}

/// The report's snapshot after the last run.
fn report(session: &PhysicsSession) -> WeaponScheduleReport {
    session
        .world()
        .expect("the session is active")
        .resource::<WeaponScheduleReport>()
        .clone()
}

/// The live mirror entities in the world.
fn mirror_count(session: &mut PhysicsSession) -> usize {
    world(session)
        .iter_entities()
        .filter(|entity_ref| entity_ref.contains::<WeaponRoundMirror>())
        .count()
}

/// One shooter's fire intent on `tick`, stamped the way the session's own
/// producers stamp theirs: the actor's serial as the producer.
fn intent(tick: u64, sequence: u32) -> FireIntent {
    FireIntent {
        id: FireIntentId {
            session: SESSION,
            tick: Tick(tick),
            producer: u32::try_from(actor(1).serial).expect("a small actor serial"),
            sequence,
        },
        shooter: actor(1),
    }
}

fn keyboard() -> DeviceId {
    DeviceId::stable(DeviceClass::Keyboard, "kbd.f27c-sched/0")
        .expect("the fixture identity is valid")
}

/// The actions the real input boundary delivers for one frame's held keys,
/// exactly as a session driver reads them from `InputSession::pump_frame`.
fn delivered_actions(down: &[Key]) -> Vec<Action> {
    let mut session =
        InputSession::designed_default(LocalSeatId(0), SessionMode::SinglePlayer, Tick(0));
    session
        .collector_mut()
        .connect_device(keyboard())
        .expect("the fixture keyboard connects");
    let events = vec![DeviceEvent::KeyboardFrame {
        device: keyboard(),
        keys: down.to_vec(),
    }];
    session
        .pump_frame(FrameInput::Devices(&events), 1)
        .expect("the frame pumps")
        .delivered
}

/// The schedule steps the installed session once per committed fixed tick: a
/// tick with no published record resolves nothing and is named, a published
/// fire order is accepted by the production step, and the session's own tick
/// advances with the committed count — never ahead of it.
#[test]
fn accept_f27_c_sched_the_schedule_steps_the_session_once_per_committed_tick() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    let (weapons, _) = session_with_one_gun(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    // Tick 1 commits with nothing published: the schedule ran — and said so —
    // without stepping.
    step(&mut session, 1);
    let ran = report(&session);
    assert_eq!(ran.ticks, 1, "the schedule ran the committed tick");
    assert_eq!(ran.at, Some(Tick(1)), "the committed tick is the ledger's");
    assert_eq!(committed(&session), Tick(1));
    assert_eq!(ran.session, Some(SESSION));
    assert!(ran.missing_inputs, "no publication was served: {ran:?}");
    assert!(ran.stepped.is_none(), "the step did not run: {ran:?}");
    assert_eq!(
        session
            .world()
            .expect("the session is active")
            .resource::<WeaponSession>()
            .tick(),
        Tick(0),
        "an unserved tick does not advance the cadence"
    );

    // Tick 2: the session publishes a fire order; the step accepts it.
    publish(
        &mut session,
        tick_input(Tick(2), vec![WeaponOrder::Fire(intent(2, 0))]),
    );
    step(&mut session, 1);
    let fired = report(&session);
    assert_eq!(fired.at, Some(Tick(2)));
    assert!(!fired.missing_inputs);
    assert_eq!(
        fired.orders,
        vec![WeaponOrder::Fire(intent(2, 0))],
        "the step was called with the session's published order"
    );
    let stepped = fired.stepped.as_ref().expect("the step ran");
    assert_eq!(stepped.at, Tick(2), "the step resolved the committed tick");
    assert!(stepped.refused.is_none(), "{stepped:?}");
    assert_eq!(
        stepped.accepted.len(),
        1,
        "the published fire was accepted: {stepped:?}"
    );
    assert_eq!(
        stepped.mirrors.spawned.len(),
        1,
        "the accepted round got its ECS mirror through the schedule"
    );
    assert_eq!(
        session
            .world()
            .expect("the session is active")
            .resource::<WeaponSession>()
            .tick(),
        Tick(2),
        "the cadence advanced with the committed tick"
    );
}

/// The existing input boundary is the fire input: a frame's real delivered
/// `FirePrimary` edge becomes the step's fire order — stamped with the
/// session, the committed tick, the channel's producer and a fresh sequence —
/// while a `CycleWeapon` edge mints nothing and is named unmapped rather than
/// silently inventing a bank cycle.
#[test]
fn accept_f27_c_sched_the_delivered_fire_edge_is_the_fire_order() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    let (weapons, _) = session_with_one_gun(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    // A real frame of held keys through the production input boundary: Space
    // delivers FirePrimary, Tab delivers CycleWeapon.
    let actions = delivered_actions(&[Key::Space, Key::Tab]);
    assert_eq!(
        actions,
        vec![
            Action::Flight(FlightCommand::FirePrimary),
            Action::Flight(FlightCommand::CycleWeapon),
        ],
        "the boundary delivered what the designed map binds"
    );
    publish(
        &mut session,
        WeaponTickInput {
            session: SESSION,
            at: Tick(1),
            wind_velocity_m_s: Some([0.0; 3]),
            orders: Vec::new(),
            cockpit: Some(CockpitInput {
                shooter: actor(1),
                producer: COCKPIT_PRODUCER,
                actions,
            }),
        },
    );
    step(&mut session, 1);
    let ran = report(&session);
    let minted = WeaponOrder::Fire(FireIntent {
        id: FireIntentId {
            session: SESSION,
            tick: Tick(1),
            producer: COCKPIT_PRODUCER,
            sequence: 0,
        },
        shooter: actor(1),
    });
    assert_eq!(
        ran.orders,
        vec![minted],
        "the committed tick's fire edge became the step's order"
    );
    assert_eq!(
        ran.unmapped_actions,
        vec![Action::Flight(FlightCommand::CycleWeapon)],
        "an unmeasured control mints nothing and is named"
    );
    assert_eq!(
        ran.stepped.as_ref().expect("the step ran").accepted.len(),
        1,
        "the minted order fired the selected bank"
    );

    // The next press mints the next sequence in the same channel, so the
    // once-only intent id is never reused.
    let actions = delivered_actions(&[Key::Space]);
    publish(
        &mut session,
        WeaponTickInput {
            session: SESSION,
            at: Tick(2),
            wind_velocity_m_s: Some([0.0; 3]),
            orders: Vec::new(),
            cockpit: Some(CockpitInput {
                shooter: actor(1),
                producer: COCKPIT_PRODUCER,
                actions,
            }),
        },
    );
    step(&mut session, 1);
    let second = report(&session);
    let WeaponOrder::Fire(intent) = &second.orders[0] else {
        panic!("the second press minted a fire order: {second:?}")
    };
    assert_eq!(
        intent.id,
        FireIntentId {
            session: SESSION,
            tick: Tick(2),
            producer: COCKPIT_PRODUCER,
            sequence: 1,
        },
        "the channel's sequence orders its own intents"
    );
}

/// A session-authored `SelectBank` order reaches the step verbatim: the
/// published bank becomes the selected one, and the next tick's fire order
/// fires the gun on that bank — the schedule does not interpret selection.
#[test]
fn accept_f27_c_sched_a_selection_order_reaches_the_step_verbatim() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT, WING_MOUNT]);
    let (weapons, _) = session_with_two_guns(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    // Tick 1: the session narrows the selection to the wing bank.
    let select = WeaponOrder::SelectBank {
        shooter: actor(1),
        bank: GunBank::try_new([key(WING_MOUNT)]).expect("a valid bank"),
    };
    publish(&mut session, tick_input(Tick(1), vec![select.clone()]));
    step(&mut session, 1);
    let ran = report(&session);
    assert_eq!(ran.orders, vec![select], "the order reached the step");
    let stepped = ran.stepped.as_ref().expect("the step ran");
    assert!(stepped.refused.is_none(), "{stepped:?}");

    // Tick 2: the fire order fires only the gun on the selected bank — the
    // nose gun's mount is silent.
    publish(
        &mut session,
        tick_input(Tick(2), vec![WeaponOrder::Fire(intent(2, 0))]),
    );
    step(&mut session, 1);
    let fired = report(&session);
    let stepped = fired.stepped.as_ref().expect("the step ran");
    assert_eq!(stepped.accepted.len(), 1, "one bank fired: {stepped:?}");
    assert_eq!(
        stepped.accepted[0].mount,
        key(WING_MOUNT),
        "the selected bank's gun is the one that fired"
    );
}

/// Swept hits route into the one resolver the whole session shares: the
/// resource installed as the session's damage authority — the same instance a
/// collision or crash producer borrows — is the one that loses integrity.
#[test]
fn accept_f27_c_sched_swept_damage_reaches_the_shared_resolver() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    // Deep enough along the flight axis that the first step's segment crosses
    // the box.
    part_box(world(&mut session), actor(2), [3.0, 3.0, 40.0]);
    let (weapons, _) = session_with_one_gun(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    publish(
        &mut session,
        tick_input(Tick(1), vec![WeaponOrder::Fire(intent(1, 0))]),
    );
    step(&mut session, 1);
    let fired = report(&session);
    let stepped = fired.stepped.as_ref().expect("the step ran");
    assert_eq!(stepped.routed.len(), 1, "the round was swept");
    assert_eq!(
        stepped.routed[0].outcome.sweep.hits.len(),
        1,
        "the round crossed the target's hull box: {stepped:?}"
    );
    // The authority that took the damage is the *installed resource* itself —
    // the same `DamageResolver` every other producer in this tick sees.
    assert_eq!(
        session
            .world()
            .expect("the session is active")
            .resource::<SessionDamageResolver>()
            .resolver()
            .remaining_integrity(&actor(2), &key(HULL)),
        Some(40.0 - declared_internal_damage()),
        "the shared resolver lost exactly the declared internal amount"
    );
}

/// The relation is the targeting session's declared allegiance table — not a
/// table the weapon schedule keeps: a raider on the flight path takes the
/// hit, the declared-neutral trader on the same path does not, and an actor
/// nobody rostered answers `None`, which `HostileOnly` also excludes.
#[test]
fn accept_f27_c_sched_the_declared_allegiance_table_admits_or_excludes() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    // A hostile box and a declared-neutral box on exactly the same flight
    // path, plus one unrostered owner: only the declared relation differs.
    part_box(world(&mut session), actor(2), [3.0, 3.0, 40.0]);
    part_box(world(&mut session), actor(3), [3.0, 3.0, 40.0]);
    part_box(world(&mut session), actor(4), [3.0, 3.0, 40.0]);
    let (weapons, _) = session_with_one_gun(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2), actor(3), actor(4)]),
        targeting_session(SESSION, GENERATION),
    );

    publish(
        &mut session,
        tick_input(Tick(1), vec![WeaponOrder::Fire(intent(1, 0))]),
    );
    step(&mut session, 1);
    let fired = report(&session);
    let stepped = fired.stepped.as_ref().expect("the step ran");
    assert_eq!(stepped.routed.len(), 1, "the round was swept");
    assert_eq!(
        stepped.routed[0].outcome.sweep.hits.len(),
        1,
        "exactly one candidate was admitted: {stepped:?}"
    );
    assert_eq!(
        stepped.routed[0].outcome.sweep.admitted[0].target.actor,
        actor(2),
        "the declared-hostile raider was the admitted candidate"
    );
    let damage = session
        .world()
        .expect("the session is active")
        .resource::<SessionDamageResolver>()
        .resolver()
        .clone();
    assert_eq!(
        damage.remaining_integrity(&actor(2), &key(HULL)),
        Some(40.0 - declared_internal_damage()),
        "the hostile raider took the declared amount"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(3), &key(HULL)),
        Some(40.0),
        "the declared-neutral trader was excluded by the declared rules"
    );
    assert_eq!(
        damage.remaining_integrity(&actor(4), &key(HULL)),
        Some(40.0),
        "an unrostered actor is an undeclared relation, not a hostile one"
    );
}

/// A replaced session is a teardown the schedule performs itself: the old
/// generation's rounds and mirrors are released, its pending inputs are
/// dropped with it, the next committed tick serves the new generation — and
/// the report names what went away.
#[test]
fn accept_f27_c_sched_a_replaced_session_is_torn_down() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    let (weapons, _) = session_with_one_gun(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    // Tick 1: a live round, a live mirror, and one more record still pending
    // for tick 2.
    publish(
        &mut session,
        tick_input(Tick(1), vec![WeaponOrder::Fire(intent(1, 0))]),
    );
    publish(&mut session, tick_input(Tick(2), Vec::new()));
    step(&mut session, 1);
    assert_eq!(
        mirror_count(&mut session),
        1,
        "the fired round left its mirror"
    );
    assert_eq!(
        session
            .world()
            .expect("the session is active")
            .resource::<WeaponSession>()
            .round_ids()
            .len(),
        1,
        "the round is live in the session about to be replaced"
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
    // The new generation's own first record, published ahead of the teardown:
    // it is not the dead session's to take with it.
    publish(
        &mut session,
        WeaponTickInput {
            session: restarted,
            at: Tick(2),
            wind_velocity_m_s: Some([0.0; 3]),
            orders: Vec::new(),
            cockpit: None,
        },
    );

    // Tick 2 commits: the schedule tears the old generation down before
    // anything can step under the new one.
    step(&mut session, 1);
    let ran = report(&session);
    assert_eq!(
        ran.tore_down,
        Some(WeaponTeardownRecord {
            session: SESSION,
            mirrors: 1,
            inputs: 1,
        }),
        "the teardown names the superseded session, its mirrors and only its pending inputs: {ran:?}"
    );
    assert_eq!(
        mirror_count(&mut session),
        0,
        "the old session's mirrors went with it"
    );
    assert_eq!(
        ran.session,
        Some(restarted),
        "the new generation serves now"
    );
    // The record the new generation published ahead of the teardown kept its
    // tick: the dead session's own pending record — the tick-2 one the count
    // above names — is the only one that went with it.
    assert!(
        !ran.missing_inputs,
        "the new generation's record survived the teardown: {ran:?}"
    );
    let stepped = ran.stepped.as_ref().expect("the kept record stepped");
    assert_eq!(stepped.at, Tick(2), "{stepped:?}");
    assert!(stepped.refused.is_none(), "{stepped:?}");

    // Tick 3: the new generation's own publication is served under its own
    // session.
    publish(
        &mut session,
        WeaponTickInput {
            session: restarted,
            at: Tick(3),
            wind_velocity_m_s: Some([0.0; 3]),
            orders: Vec::new(),
            cockpit: None,
        },
    );
    step(&mut session, 1);
    let served = report(&session);
    assert!(served.tore_down.is_none(), "the teardown happened once");
    let stepped = served.stepped.as_ref().expect("the new session stepped");
    assert_eq!(stepped.at, Tick(3));
    assert!(stepped.refused.is_none(), "{stepped:?}");
}

/// A removed session is the same teardown: the round's mirror does not
/// outlive the resource, and a session-less tick reports itself rather than
/// looking like a stepped one.
#[test]
fn accept_f27_c_sched_a_removed_session_releases_its_mirrors() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    let (weapons, _) = session_with_one_gun(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );
    publish(
        &mut session,
        tick_input(Tick(1), vec![WeaponOrder::Fire(intent(1, 0))]),
    );
    step(&mut session, 1);
    assert_eq!(mirror_count(&mut session), 1);

    world(&mut session).remove_resource::<WeaponSession>();
    step(&mut session, 1);
    let ran = report(&session);
    assert_eq!(
        ran.tore_down.map(|tore_down| tore_down.session),
        Some(SESSION),
        "the teardown names the session that went away"
    );
    assert_eq!(
        mirror_count(&mut session),
        0,
        "no mirror outlives its session"
    );
    assert_eq!(ran.session, None);
    assert!(ran.stepped.is_none());
}

/// Records the world cannot honestly serve are named, not stepped: a foreign
/// generation's record, a record whose tick already passed, a tick the
/// session measured no wind for, and a publication queue that never had this
/// tick at all.
#[test]
fn accept_f27_c_sched_unservable_ticks_are_named_not_invented() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    let (weapons, _) = session_with_one_gun(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    // Pushed deliberately out of order: the queue serves by the tick a record
    // commits, not by push order — the stale record drops first, the foreign
    // one next, and the tick-3 record waits its turn.
    publish(
        &mut session,
        WeaponTickInput {
            session: SESSION + 9,
            at: Tick(1),
            wind_velocity_m_s: Some([0.0; 3]),
            orders: Vec::new(),
            cockpit: None,
        },
    );
    publish(
        &mut session,
        WeaponTickInput {
            session: SESSION,
            at: Tick(3),
            wind_velocity_m_s: Some([0.0; 3]),
            orders: Vec::new(),
            cockpit: None,
        },
    );
    publish(&mut session, tick_input(Tick(0), Vec::new()));
    step(&mut session, 1);
    let ran = report(&session);
    assert_eq!(
        ran.dropped_inputs,
        vec![
            DroppedTickInput {
                at: Tick(0),
                reason: DroppedTickInputReason::StaleTick,
            },
            DroppedTickInput {
                at: Tick(1),
                reason: DroppedTickInputReason::ForeignSession {
                    expected: SESSION,
                    found: SESSION + 9,
                },
            },
        ],
        "the stale and foreign records are named in served order: {ran:?}"
    );
    assert_eq!(
        ran.inputs_ahead,
        Some(Tick(3)),
        "the earliest pending record waits for its own tick"
    );
    assert!(ran.missing_inputs);
    assert!(ran.stepped.is_none());

    // Tick 2: the session publishes a record with no measured wind — an
    // honest unmeasured, so the step cannot invent still air.
    publish(
        &mut session,
        WeaponTickInput {
            session: SESSION,
            at: Tick(2),
            wind_velocity_m_s: None,
            orders: vec![WeaponOrder::Fire(intent(2, 0))],
            cockpit: None,
        },
    );
    step(&mut session, 1);
    let unwound = report(&session);
    assert!(
        unwound.wind_unavailable,
        "unmeasured wind is named: {unwound:?}"
    );
    assert!(
        unwound.stepped.is_none(),
        "no wind was invented: {unwound:?}"
    );

    // Tick 3's record — still pending — is served on its own tick.
    step(&mut session, 1);
    let served = report(&session);
    assert_eq!(served.at, Some(Tick(3)));
    assert!(!served.missing_inputs);
    assert!(
        served.stepped.is_some(),
        "the kept record stepped on its tick"
    );
}

/// A missing or foreign authority is named, not worked around: the step
/// cannot route damage without the session's one resolver, cannot read a
/// declared relation without the targeting session, and cannot serve a
/// resolver that still belongs to another generation.
#[test]
fn accept_f27_c_sched_a_missing_or_foreign_authority_is_named() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    let (weapons, _) = session_with_one_gun(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );
    publish(
        &mut session,
        tick_input(Tick(1), vec![WeaponOrder::Fire(intent(1, 0))]),
    );

    // Without the shared resolver the step cannot apply damage; the
    // publication stays unserved and the miss is named.
    world(&mut session).remove_resource::<SessionDamageResolver>();
    step(&mut session, 1);
    let ran = report(&session);
    assert_eq!(
        ran.missing,
        vec![WeaponScheduleMissing::DamageResolver],
        "the absent authority is named: {ran:?}"
    );
    assert!(ran.stepped.is_none(), "the step did not run: {ran:?}");

    // A resolver that still serves another generation is foreign, not a
    // substitute — and so is a foreign targeting table.
    world(&mut session).insert_resource(SessionDamageResolver::new(DamageResolver::new(
        session_id(SESSION + 7),
        DAMAGE_PRODUCER,
    )));
    step(&mut session, 1);
    let foreign = report(&session);
    assert_eq!(
        foreign.missing,
        vec![WeaponScheduleMissing::ForeignSession {
            authority: WeaponScheduleAuthority::DamageResolver,
            found: SESSION + 7,
        }],
        "the foreign resolver is named by its own generation: {foreign:?}"
    );
    assert!(foreign.stepped.is_none());

    world(&mut session).insert_resource(SessionDamageResolver::new(damage_resolver(
        SESSION,
        &[actor(2)],
    )));
    world(&mut session).insert_resource(targeting_session(SESSION + 7, GENERATION));
    step(&mut session, 1);
    let foreign_targeting = report(&session);
    assert_eq!(
        foreign_targeting.missing,
        vec![WeaponScheduleMissing::ForeignSession {
            authority: WeaponScheduleAuthority::Targeting,
            found: SESSION + 7,
        }],
        "the foreign targeting table is named too: {foreign_targeting:?}"
    );
    assert!(foreign_targeting.stepped.is_none());
}

/// The step's own gates still apply through the schedule: a published fire
/// order whose intent names a foreign session reaches the step — and is
/// refused by name there, not filtered out earlier by the schedule.
#[test]
fn accept_f27_c_sched_the_steps_own_refusals_still_apply() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    let (weapons, _) = session_with_one_gun(90);
    install_session(
        &mut session,
        weapons,
        damage_resolver(SESSION, &[actor(2)]),
        targeting_session(SESSION, GENERATION),
    );

    // A session-authored fire intent for a session that is not this one: the
    // schedule serves the record it was given and the step refuses the order.
    let foreign_shooter = actor_in(SESSION + 1, 1);
    publish(
        &mut session,
        tick_input(
            Tick(1),
            vec![WeaponOrder::Fire(FireIntent {
                id: FireIntentId {
                    session: SESSION,
                    tick: Tick(1),
                    producer: u32::try_from(foreign_shooter.serial).expect("a small actor serial"),
                    sequence: 0,
                },
                shooter: foreign_shooter,
            })],
        ),
    );
    step(&mut session, 1);
    let ran = report(&session);
    let stepped = ran.stepped.as_ref().expect("the step ran");
    assert!(
        !stepped.fire_refused.is_empty(),
        "the foreign shooter is refused by the step itself, not pre-filtered: {stepped:?}"
    );
    assert!(stepped.accepted.is_empty(), "nothing fired: {stepped:?}");
    assert_eq!(
        mirror_count(&mut session),
        0,
        "no round and no mirror exist"
    );
}

// ----------------------------------------------- F27-C-PUBLISH (task #1299) --

fn host_environment() -> EnvironmentSession {
    EnvironmentSession::new(
        crate::environment::common::storm(),
        crate::environment::common::tick_rate(),
        RunSeeds::from_root(7),
    )
    .expect("the storm fixture's schedule is ordered")
}

fn unmeasured_wind_environment() -> EnvironmentSession {
    use crate::environment::common;
    use cs_content::environment::{
        EnvironmentDefinition, EnvironmentId, EnvironmentProfile, EnvironmentState,
        EnvironmentTimeline, FogDefinition, LightingDefinition, PrecipitationDefinition,
        PrecipitationKind,
    };
    let definition = EnvironmentDefinition::try_new(
        EnvironmentId::new("fixture.publish-unknown-wind").expect("a valid key"),
        Origin::SyntheticFixture,
        EnvironmentProfile::SyntheticDeveloper,
        common::authored_sky(),
        common::default_orientation(),
        LightingDefinition::try_new(
            common::unknown("f27c.publish.sun", "no sun"),
            common::unknown("f27c.publish.ambient", "no ambient"),
        )
        .expect("lighting"),
        FogDefinition::designed_default(1e-3, [0.7, 0.8, 0.95], "f27c.publish.fog").expect("fog"),
        Vec::new(),
        EnvironmentState::new(
            common::unknown("f27c.publish.wind", "nobody measured the wind"),
            PrecipitationDefinition::new(common::known(
                PrecipitationKind::Clear,
                "f27c.publish.precipitation",
            )),
            common::unknown("f27c.publish.visibility", "no sight range"),
        ),
        EnvironmentTimeline::default(),
        Provenance::designed(common::claim("f27c.publish.record")),
    )
    .expect("a valid definition");
    EnvironmentSession::new(definition, common::tick_rate(), RunSeeds::from_root(7))
        .expect("an ordered schedule")
}

fn publishing_session(environment: EnvironmentSession) -> PhysicsSession {
    let mut session = schedule_session();
    shooter_world(world(&mut session), &[NOSE_MOUNT]);
    let (weapons, _) = session_with_one_gun(90);
    let w = world(&mut session);
    let mut generations = SceneGenerations::default();
    while generations.latest() < GENERATION {
        generations.take_next();
    }
    w.insert_resource(generations);
    install_session_weapons(
        w,
        weapons,
        SessionDamageResolver::new(damage_resolver(SESSION, &[actor(2)])),
        targeting_session(SESSION, GENERATION),
    )
    .expect("one generation installs");
    w.insert_resource(SessionWeaponHost {
        environment,
        local_seat: Some(SessionLocalSeat {
            shooter: actor(1),
            producer: COCKPIT_PRODUCER,
        }),
    });
    session
}

/// The host publishes every committed tick on its own: the schedule serves
/// the measured wind, the queued authored order and the delivered edge
/// without a test calling `publish`.
#[test]
fn accept_f27_c_publish_the_host_serves_wind_orders_and_the_cockpit_edge() {
    let mut session = publishing_session(host_environment());

    // Tick 1: nothing queued, a measured wind: the step runs with no orders.
    step(&mut session, 1);
    let first = report(&session);
    assert!(!first.missing_inputs, "the host published: {first:?}");
    assert!(!first.wind_unavailable);
    assert!(first.stepped.is_some(), "the step ran: {first:?}");
    let published = session
        .world()
        .expect("active")
        .resource::<SessionWeaponPublishReport>()
        .clone();
    assert_eq!(published.at, Some(Tick(1)));
    assert!(published.wind_measured);

    // Tick 2: an authored order and a real delivered FirePrimary edge.
    let select = WeaponOrder::SelectBank {
        shooter: actor(1),
        bank: GunBank::try_new([key(NOSE_MOUNT)]).expect("a valid bank"),
    };
    let w = world(&mut session);
    w.resource_mut::<SessionWeaponOrders>().push(select.clone());
    w.resource_mut::<SessionCockpitActions>()
        .extend(delivered_actions(&[Key::Space]));
    step(&mut session, 1);
    let second = report(&session);
    assert_eq!(second.at, Some(Tick(2)));
    assert_eq!(second.orders.len(), 2, "authored then minted: {second:?}");
    assert_eq!(second.orders[0], select, "authored orders come first");
    let WeaponOrder::Fire(minted) = &second.orders[1] else {
        panic!("the edge minted a fire order: {second:?}")
    };
    assert_eq!(minted.id.producer, COCKPIT_PRODUCER);
    assert_eq!(minted.id.tick, Tick(2));
    assert_eq!(minted.shooter, actor(1));
    assert_eq!(
        second.stepped.as_ref().expect("stepped").accepted.len(),
        1,
        "the delivered edge fired the gun"
    );

    // Tick 3: the edge was taken once; the press does not repeat.
    step(&mut session, 1);
    let third = report(&session);
    assert!(third.orders.is_empty(), "no repeat: {third:?}");
}

/// An environment with no measured wind publishes `None`, and the schedule
/// reports it instead of stepping on still air.
#[test]
fn accept_f27_c_publish_an_unmeasured_wind_is_reported_not_invented() {
    let mut session = publishing_session(unmeasured_wind_environment());
    step(&mut session, 1);
    let ran = report(&session);
    assert!(ran.wind_unavailable, "{ran:?}");
    assert!(ran.stepped.is_none(), "no step on invented air: {ran:?}");
    assert!(
        !session
            .world()
            .expect("active")
            .resource::<SessionWeaponPublishReport>()
            .wind_measured
    );
}

/// A restart installs a new generation: the old rounds and pending records go,
/// the new session is served under its own generation, and a mixed set is
/// refused before anything is replaced.
#[test]
fn accept_f27_c_publish_a_restart_replaces_the_generation() {
    let mut session = publishing_session(host_environment());
    step(&mut session, 1);
    assert_eq!(report(&session).session, Some(SESSION));

    // A mixed-generation set is refused and leaves the session untouched.
    let (stale, _) = session_with_one_gun(90);
    let refused = install_session_weapons(
        world(&mut session),
        stale,
        SessionDamageResolver::new(damage_resolver(SESSION + 1, &[])),
        targeting_session(SESSION, GENERATION),
    );
    assert!(
        matches!(
            refused,
            Err(SessionInstallRefusal::GenerationMismatch { .. })
        ),
        "{refused:?}"
    );
    step(&mut session, 1);
    assert_eq!(report(&session).session, Some(SESSION));

    // The restart: generation SESSION + 1, installed whole.
    let next = SESSION + 1;
    let fresh = WeaponSession::new(next, Tick(0), ROUTER_PRODUCER).expect("opens");
    install_session_weapons(
        world(&mut session),
        fresh,
        SessionDamageResolver::new(damage_resolver(next, &[])),
        targeting_session(next, GENERATION),
    )
    .expect("one generation installs");
    step(&mut session, 1);
    let after = report(&session);
    assert_eq!(after.session, Some(next));
    assert_eq!(
        after.tore_down.as_ref().map(|t| t.session),
        Some(SESSION),
        "the old generation was torn down: {after:?}"
    );
    assert!(
        !after.missing_inputs,
        "the new generation is served: {after:?}"
    );
    let published = session
        .world()
        .expect("active")
        .resource::<SessionWeaponPublishReport>()
        .clone();
    assert_eq!(published.at, Some(Tick(3)));
}
