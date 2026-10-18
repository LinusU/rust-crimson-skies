//! Task #552 (F28-AE1): a declared area effect's radius and lifetime reach
//! stable recipients through the per-tick ordnance session.
//!
//! Task test prefix: `accept_f28_ae1_`. A guided item that loses its target
//! applies its declared area once: every actor inside the declared radius is a
//! recipient (by stable `ActorId`), nobody outside it is, and the area expires
//! on `opened_at + lifetime_ticks`. The area is gameplay state over the
//! `TargetPath` inputs the step already takes; it owns no pose and no Avian
//! body. Every value is newly authored synthetic fixture data. The original's
//! own area semantics are not claimed
//! (`docs/findings/2026-10-11-f28-ae1-area-effect-reach.md`).

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId, OrdnanceLauncherBinding, OrdnanceOrder,
    OrdnanceSession, OrdnanceSessionTick, OrdnanceStep, step_ordnance_session,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DECLARED_SYNTHETIC_GUIDED_KEY, DECLARED_SYNTHETIC_LAUNCHER_MOUNT, DeclaredAreaEffect,
    DeclaredOrdnance, DeclaredOrdnanceDetails, DeclaredOrdnanceFamily, DeclaredStatusEffect,
    DeclaredStatusEffectKind, declared_synthetic_guided, declared_synthetic_provenance,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver, SystemKind,
    synthetic_airframe_graph,
};
use cs_sim::time::TickRate;
use cs_sim::weapons::{OrdnanceId, StatusEffectTarget, TargetObservation, TargetPath};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

/// The supported catalogue the session is opened with: every synthetic
/// declared record, lowered, with a repeated id keeping its first lowering.
fn supported_catalogue() -> cs_sim::weapons::OrdnanceRegistry {
    let mut catalogue = cs_sim::weapons::OrdnanceRegistry::new();
    for declared in [
        cs_content::ordnance::declared_synthetic_direct(),
        cs_content::ordnance::declared_synthetic_flak(),
        cs_content::ordnance::declared_synthetic_guided(),
        cs_content::ordnance::declared_synthetic_area_denial(),
        cs_content::ordnance::declared_synthetic_torpedo(),
        cs_content::ordnance::declared_synthetic_nitro(),
    ] {
        let component = cs_app::ordnance::lower_ordnance(&declared).expect("a synthetic lowers");
        let _ = catalogue.register(component);
    }
    catalogue
}

const SESSION: u64 = 47;
/// The ordnance producer serial the session and its events are stamped with.
const PRODUCER: u32 = 97;
/// A distinct serial for the shared damage authority.
const DAMAGE_PRODUCER: u32 = 98;
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
/// The guided item's declared lifetime in these tests: long enough to be live
/// when its target is destroyed, short enough to bound the area with it.
const GUIDED_LIFETIME_TICKS: u64 = 30;
/// The declared area radius the guided blast compares against.
const GUIDED_AREA_RADIUS_M: f64 = 40.0;
/// The declared choke duration the guided blast applies.
const GUIDED_CHOKE_TICKS: u64 = 8;
/// The declared choke strength the guided blast applies.
const GUIDED_CHOKE_STRENGTH: f64 = 0.35;

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
    ClaimId::new("f28c.guidance-detonation-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn rate() -> TickRate {
    TickRate::new(30).expect("the test rate is nonzero")
}

/// A declared guided rocket with a retimed lifetime, a declared bounded area
/// effect and one declared choke status.
///
/// The guided fixture's own lost-target behavior is `Detonate`, so a destroyed
/// target ends the item. The area is declared with the item's own lifetime so
/// it can never outlive the blast, and the status is declared so the blast's
/// consumer has something to apply to the engagement's stable recipient.
fn declared_guided() -> DeclaredOrdnance {
    let fixture = declared_synthetic_guided();
    let mut projectile = match fixture.details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => (**projectile).clone(),
        DeclaredOrdnanceDetails::Nitro(_) => panic!("the guided fixture is a projectile"),
    };
    projectile.lifetime_ticks = known(GUIDED_LIFETIME_TICKS);
    projectile.area_effect = Some(DeclaredAreaEffect {
        radius_m: known(GUIDED_AREA_RADIUS_M),
        lifetime_ticks: known(GUIDED_LIFETIME_TICKS),
    });
    projectile.status = vec![DeclaredStatusEffect {
        kind: DeclaredStatusEffectKind::Choke,
        duration_ticks: known(GUIDED_CHOKE_TICKS),
        strength: known(GUIDED_CHOKE_STRENGTH),
    }];
    DeclaredOrdnance::try_new(
        ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_GUIDED_KEY)
            .expect("a weapon id"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::GuidedRocket,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("the retimed guided fixture is valid")
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

/// One guided launch request for `actor(1)`, designating `target`.
fn launch_order(tick: u64, sequence: u32, ordnance: OrdnanceId, target: ActorId) -> OrdnanceOrder {
    OrdnanceOrder::Launch(OrdnanceLaunch {
        id: OrdnanceLaunchId {
            session: SESSION,
            tick: Tick(tick),
            producer: PRODUCER,
            sequence,
        },
        shooter: actor(1),
        ordnance,
        target: Some(target),
        engagement: engagement(),
    })
}

/// The shooter's live launcher hierarchy: an actor root carrying the binding
/// and the airframe velocity, with one mount node.
fn launcher_world(world: &mut World) -> Entity {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![
                    ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_GUIDED_KEY)
                        .expect("a weapon id"),
                ],
                loadout: ContentId::from_source(ContentKind::Loadout, "synthetic.fixture_loadout")
                    .expect("a loadout id"),
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
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, MOUNT_Z)),
        ChildOf(root),
    ));
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

/// A session with the guided fixture registered for `actor(1)`.
fn session_with_guided() -> (OrdnanceSession, OrdnanceId) {
    let mut session =
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, supported_catalogue())
            .expect("a nonzero session opens");
    let registered = session
        .register(actor(1), &[declared_guided()])
        .expect("the declared guided rocket registers");
    let ordnance = registered[0].ordnance.clone();
    (session, ordnance)
}

/// One step of the production path with still air and no caller-supplied
/// contacts, taking one tick's target observation.
fn run_step(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    orders: &[OrdnanceOrder],
    at: Tick,
    guidance: Option<TargetObservation>,
    targets: &[TargetPath],
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
            guidance,
            targets,
            impacts: &[],
            blast_recipients: None,
        },
    )
}

/// A target path whose current position is `distance_m` along +X from `centre`.
fn path(serial: u64, centre: [f64; 3], distance_m: f64) -> TargetPath {
    let at = [centre[0] + distance_m, centre[1], centre[2]];
    TargetPath::try_new(actor(serial), at, at).expect("finite positions")
}

fn centre_of(session: &OrdnanceSession, projectile: &cs_sim::weapons::ProjectileId) -> [f64; 3] {
    let p = session
        .runtime()
        .get(projectile)
        .expect("the item is live")
        .current();
    [p.x(), p.y(), p.z()]
}

/// Launches the guided item and destroys its target on tick 1, with three
/// actors placed around the blast; returns everything the checks need.
fn detonate_with_actors() -> (
    World,
    OrdnanceSession,
    DamageResolver,
    cs_app::ordnance::OrdnanceGuidanceBlast,
) {
    let mut world = World::new();
    launcher_world(&mut world);
    let (mut session, ordnance) = session_with_guided();
    let mut damage = damage_resolver();
    let launched = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance, actor(2))],
        Tick(0),
        None,
        &[],
    );
    let projectile = launched.launched[0];
    let centre = centre_of(&session, &projectile);
    let targets = [
        path(2, centre, 0.0),
        path(3, centre, GUIDED_AREA_RADIUS_M - 1.0),
        path(4, centre, GUIDED_AREA_RADIUS_M),
        path(5, centre, GUIDED_AREA_RADIUS_M * 3.0),
    ];
    let destroyed = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        Tick(1),
        Some(TargetObservation::destroyed(actor(2), Tick(1))),
        &targets,
    );
    let blast = destroyed.guidance_blasts[0].clone();
    (world, session, damage, blast)
}

#[test]
fn accept_f28_ae1_a_triggered_area_reaches_exactly_the_recipients_inside_its_radius() {
    let (_world, session, _damage, blast) = detonate_with_actors();
    assert!(blast.area_refused.is_none(), "{:?}", blast.area_refused);
    let instance = blast.area.expect("the declared area was opened");
    let area = session
        .runtime()
        .areas()
        .get(&instance)
        .expect("the area is live in the ledger");
    assert_eq!(
        area.recipients,
        vec![actor(2), actor(3)],
        "inside the radius and only inside it; the radius itself is outside"
    );
    assert!(area.reached(&actor(2)) && area.reached(&actor(3)));
    assert!(!area.reached(&actor(4)), "exactly at the radius is outside");
    assert!(!area.reached(&actor(5)), "beyond the radius is outside");
    assert_eq!(area.source, blast.ordnance);
    assert_eq!(area.shooter, actor(1));
    assert_eq!(area.opened_at, Tick(1));
    assert_eq!(area.area.radius_m(), GUIDED_AREA_RADIUS_M);
    assert!(session.runtime().areas().reaches(&actor(3)));
    assert!(!session.runtime().areas().reaches(&actor(5)));
}

#[test]
fn accept_f28_ae1_the_area_expires_on_its_boundary_tick_and_a_restart_inherits_nothing() {
    let (mut world, mut session, mut damage, blast) = detonate_with_actors();
    let instance = blast.area.expect("the declared area was opened");
    let boundary = 1 + GUIDED_LIFETIME_TICKS;
    for t in 2..boundary {
        let tick = run_step(
            &mut world,
            &mut session,
            &mut damage,
            &[],
            Tick(t),
            None,
            &[],
        );
        assert!(tick.areas_expired.is_empty(), "still live at tick {t}");
        assert!(session.runtime().areas().get(&instance).is_some());
    }
    let at_boundary = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        Tick(boundary),
        None,
        &[],
    );
    assert_eq!(at_boundary.areas_expired.len(), 1, "gone on the boundary");
    assert_eq!(at_boundary.areas_expired[0].instance, instance);
    assert_eq!(at_boundary.areas_expired[0].expired_at, Tick(boundary));
    assert_eq!(
        at_boundary.areas_expired[0].recipients,
        vec![actor(2), actor(3)]
    );
    assert!(session.runtime().areas().is_empty());
    let after = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        Tick(boundary + 1),
        None,
        &[],
    );
    assert!(after.areas_expired.is_empty(), "reported exactly once");

    // A restarted session is a new generation with a new ledger.
    let restarted = OrdnanceSession::new(
        SESSION + 1,
        Tick(0),
        rate(),
        PRODUCER,
        supported_catalogue(),
    )
    .expect("a restarted session opens");
    assert!(restarted.runtime().areas().is_empty());
    assert!(!restarted.runtime().areas().reaches(&actor(2)));
}
