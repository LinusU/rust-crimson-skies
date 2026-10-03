//! Acceptance scenarios F28-C through the per-tick ordnance session: the
//! launch producer reading live launcher poses, the routed damage and applied
//! status effects, the ECS mirror, the network events and the teardown.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-C`. Task test prefix: `accept_f28_c_`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! These tests drive production code only: [`cs_app::ordnance`]'s
//! [`OrdnanceSession`], [`step_ordnance_session`] and [`sync_ordnance_mirrors`],
//! over the live ECS read ([`live_launcher_transforms`]) and the `cs_sim`
//! runtime it owns. The minimum acceptance scenario (AC03) runs through the
//! step: a timed engine-status effect is applied on the tick its item triggers,
//! is live up to its boundary, expires exactly on its `expires_at` tick, and a
//! restarted session inherits nothing.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Transform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceEngagement, OrdnanceEventKind, OrdnanceItemMirror, OrdnanceLaunch, OrdnanceLaunchId,
    OrdnanceLauncherBinding, OrdnanceOrder, OrdnanceOrderRefusal, OrdnanceSession,
    OrdnanceSessionTick, OrdnanceStep, OrdnanceStepRefusal, step_ordnance_session,
    sync_ordnance_mirrors,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::{MountPoseBinding, MountPoseRefusal, SessionRefusal};
use cs_content::ordnance::{
    DECLARED_SYNTHETIC_AREA_DENIAL_KEY, DECLARED_SYNTHETIC_LAUNCHER_MOUNT, DeclaredAreaEffect,
    DeclaredFuseRule, DeclaredOrdnance, DeclaredOrdnanceDetails, DeclaredOrdnanceFamily,
    declared_synthetic_area_denial, declared_synthetic_nitro, declared_synthetic_provenance,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageEventKind, DamageNodeKey, DamagePolicy, DamageResolver,
    SystemKind, synthetic_airframe_graph,
};
use cs_sim::time::TickRate;
use cs_sim::weapons::{
    OrdnanceId, OrdnanceRuntimeError, SYNTHETIC_CHOKE_STRENGTH, SYNTHETIC_NITRO_EXTRA_THRUST_N,
    StatusEffectKind, StatusEffectTarget,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

const SESSION: u64 = 41;
/// The ordnance producer serial the session and its events are stamped with.
const PRODUCER: u32 = 91;
/// A distinct serial for the shared damage authority.
const DAMAGE_PRODUCER: u32 = 92;
/// The scene generation every live entity in these tests is stamped under.
const GENERATION: SceneGeneration = SceneGeneration(1);
/// The tick length the fixed step runs at.
const DT_S: f64 = 1.0 / 30.0;
/// The fixture's own launcher mount.
const MOUNT: &str = DECLARED_SYNTHETIC_LAUNCHER_MOUNT;
/// The target's damage node the routed hits name.
const HULL: &str = "hull";
/// The mount node's depth, so a launch reads a non-origin pose.
const MOUNT_Z: f32 = 10.0;

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("the test session generation is nonzero")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn claim() -> ClaimId {
    ClaimId::new("f28c.session-step-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn rate() -> TickRate {
    TickRate::new(30).expect("the test rate is nonzero")
}

/// The declared area-denial fixture retimed so its timed fuse fires before its
/// own lifetime: the F28-B runtime removes an expired item *before* the fuse
/// decision, so a fuse equal to the lifetime can never trigger. The declared
/// area's own bounded lifetime is retimed with it, because the runtime refuses
/// an area that outlives the item that carries it.
fn declared_area_denial(fuse_ticks: u64, lifetime_ticks: u64) -> DeclaredOrdnance {
    let fixture = declared_synthetic_area_denial();
    let mut projectile = match fixture.details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => (**projectile).clone(),
        DeclaredOrdnanceDetails::Nitro(_) => panic!("the area-denial fixture is a projectile"),
    };
    projectile.fuse = DeclaredFuseRule::Timed {
        ticks: known(fuse_ticks),
    };
    projectile.lifetime_ticks = known(lifetime_ticks);
    projectile.area_effect = Some(DeclaredAreaEffect {
        radius_m: known(55.0),
        lifetime_ticks: known(lifetime_ticks),
    });
    DeclaredOrdnance::try_new(
        ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_AREA_DENIAL_KEY)
            .expect("a weapon id"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::AreaDenialEngine,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("the retimed area denial is valid")
}

/// Where a launched item's effects land: the hostile target's hull and its
/// propulsion system.
fn engagement() -> OrdnanceEngagement {
    OrdnanceEngagement {
        damage_target: actor(2),
        node: key(HULL),
        status_recipient: StatusEffectTarget::actor_system(actor(2), SystemKind::Propulsion),
    }
}

/// One launch request for `actor(1)` on `tick`.
fn launch_order(tick: u64, sequence: u32, ordnance: OrdnanceId) -> OrdnanceOrder {
    OrdnanceOrder::Launch(OrdnanceLaunch {
        id: OrdnanceLaunchId {
            session: SESSION,
            tick: Tick(tick),
            producer: PRODUCER,
            sequence,
        },
        shooter: actor(1),
        ordnance,
        target: None,
        engagement: engagement(),
    })
}

/// The shooter's live launcher hierarchy: an actor root carrying the binding
/// and the airframe velocity, with one mount node per mount.
fn launcher_world(world: &mut World, mounts: &[&str]) -> Entity {
    launcher_root(world, actor(1), mounts)
}

/// One shooter's live launcher hierarchy. Each mount sits at its own x, well in
/// front of the origin, so a launch that read a fixed center origin would be
/// distinguishable.
fn launcher_root(world: &mut World, shooter: ActorId, mounts: &[&str]) -> Entity {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: shooter,
                ordnance: vec![
                    ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_AREA_DENIAL_KEY)
                        .expect("a weapon id"),
                ],
                loadout: ContentId::from_source(ContentKind::Loadout, "synthetic.fixture_loadout")
                    .expect("a loadout id"),
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
            NodeVisualTransform(GlobalTransform::from_xyz(x, 0.0, MOUNT_Z)),
            ChildOf(root),
        ));
    }
    root
}

/// The shared damage authority with the target's synthetic airframe graph
/// registered.
fn damage_resolver() -> DamageResolver {
    let mut damage = DamageResolver::new(session_id(), DAMAGE_PRODUCER);
    damage
        .register_actor(
            actor(2),
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the fixture graph registers");
    damage
}

/// A session with the retimed area-denial fixture registered for `actor(1)`.
fn session_with_area_denial(fuse_ticks: u64, lifetime_ticks: u64) -> (OrdnanceSession, OrdnanceId) {
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a nonzero session opens");
    let registered = session
        .register(
            actor(1),
            &[declared_area_denial(fuse_ticks, lifetime_ticks)],
        )
        .expect("the declared area denial registers");
    let ordnance = registered[0].ordnance.clone();
    (session, ordnance)
}

/// One step of the production path with still air, no observation and no
/// caller-supplied contacts.
fn run_step(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    orders: &[OrdnanceOrder],
    at: Tick,
) -> OrdnanceSessionTick {
    step_ordnance_session(
        world,
        session,
        damage,
        orders,
        &OrdnanceStep {
            at,
            dt_s: DT_S,
            wind_velocity_m_s: [0.0; 3],
            generation: GENERATION,
            guidance: None,
            targets: &[],
            impacts: &[],
        },
    )
}

/// AC03 minimum scenario: the timed engine-status effect is applied on the tick
/// its item triggers, is live right up to its boundary, expires exactly on its
/// `expires_at` tick (once), and a restarted session inherits nothing.
#[test]
fn accept_f28_c_a_timed_engine_status_expires_on_its_tick_and_resets_on_restart() {
    let mut world = World::new();
    launcher_world(&mut world, &[MOUNT]);
    let (mut session, ordnance) = session_with_area_denial(5, 20);
    let mut damage = damage_resolver();
    let recipient = StatusEffectTarget::actor_system(actor(2), SystemKind::Propulsion);

    // Tick 0: the launch is accepted from the live launcher pose.
    let launched = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance)],
        Tick(0),
    );
    assert_eq!(launched.launched.len(), 1, "one item is in flight");

    // Drive until the timed fuse fires. The status is applied on that tick.
    let mut applied_at = None;
    let mut expires_at = None;
    for at in 1..=20u64 {
        let tick = run_step(&mut world, &mut session, &mut damage, &[], Tick(at));
        if let Some(detonation) = tick.detonations.first() {
            assert_eq!(
                detonation.status_applied.len(),
                1,
                "the area effect applied its one declared status"
            );
            let effect = session
                .runtime()
                .status()
                .get(&detonation.status_applied[0])
                .expect("the applied effect is live");
            applied_at = Some(Tick(at));
            expires_at = Some(effect.expires_at);
            break;
        }
    }
    let applied_at = applied_at.expect("the timed fuse triggered within its lifetime");
    let expires_at = expires_at.expect("the applied effect has a boundary tick");
    assert!(
        expires_at.0 > applied_at.0,
        "the declared duration bounds the effect: {expires_at:?} after {applied_at:?}"
    );
    assert!(
        session.is_under(&recipient, StatusEffectKind::Choke),
        "the engine is choked on the tick its item applied the effect"
    );

    // Every tick between the application and the boundary keeps the choke
    // live and expires nothing.
    for at in (applied_at.0 + 1)..expires_at.0 {
        let tick = run_step(&mut world, &mut session, &mut damage, &[], Tick(at));
        assert!(
            tick.status_expired.is_empty(),
            "nothing expires before the boundary at tick {at}"
        );
    }
    let live = session.engine_status(&recipient);
    assert_eq!(
        live.choke, SYNTHETIC_CHOKE_STRENGTH,
        "the engine is still choked on the tick before its boundary"
    );
    assert!(live.is_degraded(), "a choke degrades the engine");

    // The boundary tick expires it exactly once.
    let boundary = run_step(&mut world, &mut session, &mut damage, &[], expires_at);
    assert_eq!(
        boundary.status_expired.len(),
        1,
        "the effect expires exactly once, on its own tick"
    );
    assert_eq!(
        boundary.status_expired[0].expired_at, expires_at,
        "the expiry names the declared boundary, not the tick after it"
    );
    assert_eq!(
        boundary.status_expired[0].kind,
        StatusEffectKind::Choke,
        "the expired effect is the declared kind"
    );
    let after = session.engine_status(&recipient);
    assert_eq!(after.choke, 0.0, "the engine is clear after the boundary");
    assert!(
        !session.is_under(&recipient, StatusEffectKind::Choke),
        "the expired effect is no longer live"
    );

    // A restarted session has a new ledger: it inherits nothing.
    session.close(&mut world);
    assert!(session.is_closed());
    let restarted =
        OrdnanceSession::new(SESSION + 1, Tick(0), rate(), PRODUCER).expect("a new session opens");
    let restarted_recipient = StatusEffectTarget::actor_system(actor(2), SystemKind::Propulsion);
    assert!(
        !restarted.is_under(&restarted_recipient, StatusEffectKind::Choke),
        "a restarted session starts with an empty status ledger"
    );
    assert_eq!(
        restarted.engine_status(&restarted_recipient).choke,
        0.0,
        "the restarted engine is unchoked"
    );
}

/// A choke that is still live when the session is torn down does not survive
/// the restart: the closed session is under nothing and a new session starts
/// empty.
#[test]
fn accept_f28_c_a_live_status_does_not_survive_a_restart() {
    let mut world = World::new();
    launcher_world(&mut world, &[MOUNT]);
    let (mut session, ordnance) = session_with_area_denial(5, 20);
    let mut damage = damage_resolver();
    let recipient = StatusEffectTarget::actor_system(actor(2), SystemKind::Propulsion);

    run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance)],
        Tick(0),
    );
    let mut applied = false;
    for at in 1..=20u64 {
        let tick = run_step(&mut world, &mut session, &mut damage, &[], Tick(at));
        if !tick.detonations.is_empty() {
            applied = true;
            break;
        }
    }
    assert!(applied, "the item triggered and applied its choke");
    assert!(
        session.is_under(&recipient, StatusEffectKind::Choke),
        "the choke is live before the teardown"
    );
    assert!(
        session
            .runtime()
            .status()
            .effects_on(&recipient)
            .iter()
            .any(|effect| effect.is_live_at(session.tick())),
        "the ledger holds the live effect"
    );

    session.close(&mut world);
    assert!(
        !session.is_under(&recipient, StatusEffectKind::Choke),
        "a closed session is under nothing"
    );
    assert!(
        session.runtime().status().is_empty(),
        "the teardown released the live ledger rather than leaving it reachable"
    );
    let restarted =
        OrdnanceSession::new(SESSION + 1, Tick(0), rate(), PRODUCER).expect("a new session opens");
    assert!(
        !restarted.is_under(&recipient, StatusEffectKind::Choke),
        "a restart inherits no live effect"
    );
}

/// The producer wiring: an accepted launch reads the component's live launcher
/// pose from the ECS hierarchy, emits the declared media effect at that pose,
/// appends a stamped network event and mirrors the item from the authoritative
/// position.
#[test]
fn accept_f28_c_a_launch_reads_the_live_pose_and_mirrors_the_item() {
    let mut world = World::new();
    launcher_world(&mut world, &[MOUNT]);
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a nonzero session opens");
    let registered = session
        .register(actor(1), &[declared_area_denial(5, 20)])
        .expect("the declared area denial registers");
    let ordnance = registered[0].ordnance.clone();
    let mut damage = damage_resolver();

    let launched = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance)],
        Tick(0),
    );
    assert_eq!(launched.launched.len(), 1);
    assert_eq!(launched.effects.len(), 1, "one effect per accepted launch");
    let effect = &launched.effects[0];
    assert_eq!(
        (effect.shooter, effect.at),
        (actor(1), Tick(0)),
        "the effect names the shooter and the tick of release"
    );
    assert_eq!(
        effect.origin.to_array(),
        [0.0, 0.0, f64::from(MOUNT_Z)],
        "the effect is at the live mount pose, not the world origin"
    );
    assert_eq!(
        (effect.visual.as_str(), effect.sound.as_str()),
        (registered[0].visual.as_str(), registered[0].sound.as_str()),
        "the effect carries the lowered component's declared media"
    );
    assert_eq!(
        session.effects().len(),
        1,
        "the session's effect log holds it"
    );

    // The network event is stamped with the session, producer and tick.
    let event = launched
        .events
        .iter()
        .find(|event| matches!(event.kind, OrdnanceEventKind::Launched { .. }))
        .expect("the launch emitted a network event");
    assert_eq!(
        (event.id.session, event.id.producer, event.id.tick),
        (session_id(), PRODUCER, Tick(0)),
        "the event carries the session, producer and tick"
    );
    assert_eq!(
        session.events().first().map(|event| event.id.sequence),
        Some(0),
        "the first event of the session has sequence zero"
    );

    // The ECS mirror is written from the authoritative position.
    let projectile = launched.launched[0];
    let authoritative = session
        .runtime()
        .get(&projectile)
        .expect("the item is live")
        .current()
        .to_array();
    let mirror = world
        .iter_entities()
        .find_map(|entity_ref| {
            let mirror = entity_ref.get::<OrdnanceItemMirror>()?;
            (mirror.projectile == projectile).then(|| entity_ref.id())
        })
        .expect("the item has a mirror");
    let translation = world
        .get::<Transform>(mirror)
        .expect("the mirror carries a transform")
        .translation;
    assert_eq!(
        translation.z, authoritative[2] as f32,
        "the mirror's position is the authoritative one"
    );
    assert_eq!(
        world
            .get::<OrdnanceItemMirror>(mirror)
            .expect("the mirror component")
            .shooter,
        actor(1)
    );

    // The next pass writes it forward and spawns nothing twice.
    let next = run_step(&mut world, &mut session, &mut damage, &[], Tick(1));
    assert_eq!(next.mirrors.moved, 1, "the mirror was written forward");
    assert!(
        next.mirrors.spawned.is_empty(),
        "no mirror was spawned twice"
    );
    assert!(next.mirrors.despawned.is_empty());
}

/// The consumer wiring: a triggered item routes its declared channels into the
/// shared damage authority exactly once, and its retirement leaves the
/// authority untouched.
#[test]
fn accept_f28_c_a_triggered_item_routes_its_damage_once_into_the_authority() {
    let mut world = World::new();
    launcher_world(&mut world, &[MOUNT]);
    let (mut session, ordnance) = session_with_area_denial(5, 20);
    let mut damage = damage_resolver();
    let integrity_before = damage
        .remaining_integrity(&actor(2), &key(HULL))
        .expect("the target's hull is registered");

    run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance)],
        Tick(0),
    );
    let mut trigger_tick = None;
    for at in 1..=20u64 {
        let tick = run_step(&mut world, &mut session, &mut damage, &[], Tick(at));
        if let Some(detonation) = tick.detonations.first() {
            assert_eq!(
                detonation.hits.len(),
                2,
                "one hit per non-zero declared channel"
            );
            let resolution = detonation
                .damage
                .as_ref()
                .expect("the authority accepted the routed batch");
            assert!(
                resolution
                    .events
                    .iter()
                    .any(|event| matches!(event.kind, DamageEventKind::HitApplied { .. })),
                "the authority applied the routed hits"
            );
            assert!(detonation.damage_refused.is_none());
            assert!(detonation.status_refused.is_none());
            trigger_tick = Some(Tick(at));
            break;
        }
    }
    let trigger_tick = trigger_tick.expect("the timed fuse triggered");
    let integrity_after = damage
        .remaining_integrity(&actor(2), &key(HULL))
        .expect("the target's hull is registered");
    assert!(
        integrity_after < integrity_before,
        "the routed damage reached the authority: {integrity_after} < {integrity_before}"
    );

    // The item retires on the next tick; it must not route a second time.
    let retired = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        Tick(trigger_tick.0 + 1),
    );
    assert_eq!(retired.retired.len(), 1, "the triggered item retired");
    assert!(
        retired.detonations.is_empty(),
        "a retired item routes nothing again"
    );
    assert_eq!(
        damage
            .remaining_integrity(&actor(2), &key(HULL))
            .expect("the target's hull is registered"),
        integrity_after,
        "the retirement changed no integrity"
    );
}

/// Retry safety: a launch request that names an already-resolved id is refused
/// by name and launches nothing, whether it is replayed in the same step or in
/// a later one.
#[test]
fn accept_f28_c_a_replayed_launch_request_neither_duplicates_nor_consumes() {
    let mut world = World::new();
    launcher_world(&mut world, &[MOUNT]);
    let (mut session, ordnance) = session_with_area_denial(5, 20);
    let mut damage = damage_resolver();

    let first = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance.clone())],
        Tick(0),
    );
    assert_eq!(first.launched.len(), 1);

    // The same id twice in one step: the second is refused, not a second item.
    let same_step = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance.clone())],
        Tick(1),
    );
    assert_eq!(
        same_step.orders_refused,
        vec![OrdnanceOrderRefusal::DuplicateLaunch {
            id: OrdnanceLaunchId {
                session: SESSION,
                tick: Tick(0),
                producer: PRODUCER,
                sequence: 0,
            },
        }],
        "the replayed id is refused by name"
    );
    assert!(
        same_step.launched.is_empty() && same_step.effects.is_empty(),
        "a refused replay launches nothing and emits no effect"
    );
    assert_eq!(session.runtime().len(), 1, "no second item exists");
}

/// An unreadable launcher is reported by name and the launch that needed it is
/// refused: nothing fires from the world origin.
#[test]
fn accept_f28_c_an_unreadable_launcher_is_named_and_nothing_launches_from_the_origin() {
    let mut world = World::new();
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![
                    ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_AREA_DENIAL_KEY)
                        .expect("a weapon id"),
                ],
                loadout: ContentId::from_source(ContentKind::Loadout, "synthetic.fixture_loadout")
                    .expect("a loadout id"),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::ZERO),
        ))
        .id();
    // The mount node exists but its pose belongs to a hierarchy that was
    // replaced.
    world.spawn((
        MountPoseBinding {
            mount: key(MOUNT),
            generation: SceneGeneration(7),
        },
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, MOUNT_Z)),
        ChildOf(root),
    ));
    let (mut session, ordnance) = session_with_area_denial(5, 20);
    let mut damage = damage_resolver();

    let refused = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance)],
        Tick(0),
    );
    assert_eq!(
        refused.unreadable_launchers,
        vec![cs_app::ordnance::UnreadableLauncher {
            shooter: actor(1),
            refusal: MountPoseRefusal::StaleGeneration {
                mount: key(MOUNT),
                found: SceneGeneration(7),
                expected: GENERATION,
            },
        }],
        "the stale launcher pose is reported by name"
    );
    assert_eq!(
        refused.orders_refused,
        vec![OrdnanceOrderRefusal::MissingMountTransform {
            shooter: actor(1),
            mount: key(MOUNT),
        }],
        "the launch is refused instead of firing from the world origin"
    );
    assert!(
        refused.launched.is_empty() && refused.effects.is_empty(),
        "no item and no effect came from the unreadable launcher"
    );
    assert!(session.runtime().is_empty());
}

/// A nitro order reaches its registered booster through the step: an accepted
/// activation reports its declared thrust and consumption, and an actor with no
/// booster is refused by name.
#[test]
fn accept_f28_c_a_nitro_order_changes_thrust_and_consumes_capacity() {
    let mut world = World::new();
    launcher_world(&mut world, &[MOUNT]);
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a nonzero session opens");
    session
        .register(actor(1), &[declared_synthetic_nitro()])
        .expect("the declared booster registers");
    let mut damage = damage_resolver();

    let boosted = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[OrdnanceOrder::Nitro {
            shooter: actor(1),
            requested: true,
        }],
        Tick(1),
    );
    assert_eq!(boosted.nitro.len(), 1, "the booster was driven this tick");
    let (shooter, nitro) = &boosted.nitro[0];
    assert_eq!(*shooter, actor(1));
    assert!(nitro.is_active(), "the activation was accepted");
    assert_eq!(
        nitro.extra_thrust_n, SYNTHETIC_NITRO_EXTRA_THRUST_N,
        "the accepted boost adds its declared extra thrust"
    );
    assert!(nitro.consumed_units > 0.0, "the accepted boost consumes");
    assert!(
        boosted
            .events
            .iter()
            .any(|event| matches!(event.kind, OrdnanceEventKind::Nitro { active: true, .. })),
        "the activation emitted a network event"
    );

    // An actor with no registered booster is refused by name.
    let unknown = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[OrdnanceOrder::Nitro {
            shooter: actor(3),
            requested: true,
        }],
        Tick(2),
    );
    assert_eq!(
        unknown.orders_refused,
        vec![OrdnanceOrderRefusal::Runtime(
            OrdnanceRuntimeError::UnknownBooster { shooter: actor(3) }
        )],
        "an unregistered booster is refused by name"
    );
    assert!(unknown.nitro.is_empty());
}

/// Teardown releases every live item and mirror and the closed session refuses
/// every later order, step and registration.
#[test]
fn accept_f28_c_teardown_releases_the_items_and_refuses_later_orders() {
    let mut world = World::new();
    launcher_world(&mut world, &[MOUNT]);
    let (mut session, ordnance) = session_with_area_denial(5, 20);
    let mut damage = damage_resolver();

    run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance.clone())],
        Tick(0),
    );
    assert_eq!(session.runtime().len(), 1);

    let teardown = session.close(&mut world);
    assert_eq!(teardown.projectiles.len(), 1, "the live item was released");
    assert_eq!(teardown.mirrors, 1, "its mirror was despawned");
    assert!(session.is_closed());
    assert!(
        session.runtime().is_empty(),
        "no live item survives the teardown"
    );
    assert_eq!(
        world
            .iter_entities()
            .filter(|entity_ref| entity_ref.contains::<OrdnanceItemMirror>())
            .count(),
        0,
        "no mirror survives the teardown"
    );

    let later = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(1, 1, ordnance)],
        Tick(1),
    );
    assert_eq!(later.refused, Some(OrdnanceStepRefusal::Closed));
    assert!(
        later.orders_refused.is_empty(),
        "a closed session reads no order at all"
    );
    assert_eq!(
        session.register(actor(1), &[declared_area_denial(5, 20)]),
        Err(cs_app::ordnance::OrdnanceRegistrationError::Closed),
        "a closed session registers nothing"
    );
}

/// A repeated tick is refused whole: nothing advances, launches or expires a
/// second time.
#[test]
fn accept_f28_c_a_repeated_tick_is_refused_whole() {
    let mut world = World::new();
    launcher_world(&mut world, &[MOUNT]);
    let (mut session, ordnance) = session_with_area_denial(5, 20);
    let mut damage = damage_resolver();

    let first = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance.clone())],
        Tick(0),
    );
    assert_eq!(first.launched.len(), 1);

    let repeated = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 1, ordnance)],
        Tick(0),
    );
    assert_eq!(
        repeated.refused,
        Some(OrdnanceStepRefusal::StaleTick {
            resolved_through: Tick(0),
            at: Tick(0),
        }),
        "the repeat of a resolved tick is refused whole"
    );
    assert!(
        repeated.launched.is_empty() && repeated.effects.is_empty(),
        "a refused step launches nothing"
    );
    assert_eq!(session.runtime().len(), 1, "no second item was spawned");
}

/// A session generation of zero cannot open: it is refused at the door rather
/// than opened to refuse everything later.
#[test]
fn accept_f28_c_a_session_on_generation_zero_refuses_at_the_door() {
    assert_eq!(
        OrdnanceSession::new(0, Tick(0), rate(), PRODUCER).err(),
        Some(SessionRefusal::NoSession),
        "generation zero is not a session"
    );
}

/// A registration from another session generation is refused by name and
/// registers nothing.
#[test]
fn accept_f28_c_a_foreign_session_registration_is_refused() {
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a nonzero session opens");
    let foreign = ActorId {
        session: SessionId::new(SESSION + 1).expect("a nonzero generation"),
        serial: 1,
    };
    assert_eq!(
        session.register(foreign, &[declared_area_denial(5, 20)]),
        Err(
            cs_app::ordnance::OrdnanceRegistrationError::ForeignSession {
                expected: SESSION,
                found: SESSION + 1,
            }
        ),
        "an actor from another generation cannot register here"
    );
    assert!(
        session.runtime().is_empty(),
        "a refused registration registered nothing"
    );
}

/// The standalone mirror reconciliation keeps an orphan out of a reloaded
/// scene: a mirror under a previous generation is despawned and respawned under
/// the live one.
#[test]
fn accept_f28_c_a_mirror_from_another_generation_is_reconciled() {
    let mut world = World::new();
    launcher_world(&mut world, &[MOUNT]);
    let (mut session, ordnance) = session_with_area_denial(5, 20);
    let mut damage = damage_resolver();
    let launched = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance)],
        Tick(0),
    );
    let projectile = launched.launched[0];

    // A reconciliation under a newer generation must not keep the old mirror.
    let newer = SceneGeneration(2);
    let reconciled = sync_ordnance_mirrors(&mut world, &session, newer);
    assert_eq!(
        reconciled.despawned,
        vec![projectile],
        "the stale-generation mirror is despawned"
    );
    assert_eq!(
        reconciled.spawned,
        vec![projectile],
        "a mirror under the live generation replaces it"
    );
    assert!(
        world.iter_entities().all(|entity_ref| {
            entity_ref
                .get::<OrdnanceItemMirror>()
                .is_none_or(|mirror| mirror.generation == newer)
        }),
        "no mirror under the old generation survives"
    );
}
