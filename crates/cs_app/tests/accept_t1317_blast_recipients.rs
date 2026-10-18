//! Task #1317 (T548 follow-up): `step_ordnance_session` routes a guidance-loss
//! blast through `route_detonation_area` over the producer-supplied recipients.
//!
//! Task test prefix: `accept_t1317_`. An item that declares an area effect and
//! loses its target damages every supplied recipient inside the radius, scaled
//! by the measured `1 - d^2/r^2` falloff, and not the engagement's single
//! target as such; without supplied recipients, or without a declared area,
//! the single-target path of F28-C.1 is unchanged. Every value is newly
//! authored synthetic fixture data; no unit calibration is claimed
//! (`docs/findings/2026-10-11-f28-c-2-lost-target-blast-measured.md`).

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
    ActorId, AttributionRule, DamageChannel, DamageNodeKey, DamagePolicy, DamageResolver,
    SystemKind, synthetic_airframe_graph,
};
use cs_sim::time::TickRate;
use cs_sim::weapons::{BlastRecipient, OrdnanceId, StatusEffectTarget, TargetObservation};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

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
    for serial in [3, 4] {
        damage
            .register_actor(
                actor(serial),
                synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("the fixture graph registers");
    }
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
    blast_recipients: Option<&[BlastRecipient]>,
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
            targets: &[],
            impacts: &[],
            blast_recipients,
        },
    )
}

fn recipient(serial: u64, distance_m: f64, centre: [f64; 3]) -> BlastRecipient {
    BlastRecipient::new(
        actor(serial),
        key(HULL),
        WorldPosition::try_new([centre[0] + distance_m, centre[1], centre[2]])
            .expect("finite position"),
    )
}

/// Launches the guided item on tick 0 and destroys its designated target
/// (`actor(2)`) on tick 1, giving the blast `recipients` built from the live
/// item's position. Returns the blast and the damage authority.
fn detonate(
    recipients: impl FnOnce([f64; 3]) -> Option<Vec<BlastRecipient>>,
) -> (cs_app::ordnance::OrdnanceGuidanceBlast, DamageResolver) {
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
        None,
    );
    let p = session
        .runtime()
        .get(&launched.launched[0])
        .expect("the item is live")
        .current();
    let supplied = recipients([p.x(), p.y(), p.z()]);
    let destroyed = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        Tick(1),
        Some(TargetObservation::destroyed(actor(2), Tick(1))),
        supplied.as_deref(),
    );
    assert!(destroyed.routing_refused.is_empty());
    (destroyed.guidance_blasts[0].clone(), damage)
}

fn armor_on(blast: &cs_app::ordnance::OrdnanceGuidanceBlast, serial: u64) -> Vec<f64> {
    blast
        .hits
        .iter()
        .filter(|hit| hit.target == actor(serial) && hit.channel == DamageChannel::Armor)
        .map(|hit| hit.damage)
        .collect()
}

/// Three recipients at different distances: the one at the centre takes the
/// full declared armor channel, the one at half the radius takes 0.75 of it,
/// the one outside takes nothing. The engagement's own target (`actor(2)`)
/// is only a recipient by position, so the one at the centre is `actor(3)`.
#[test]
fn accept_t1317_a_blast_damages_each_supplied_recipient_by_its_own_distance() {
    let (blast, _damage) = detonate(|c| {
        Some(vec![
            recipient(2, GUIDED_AREA_RADIUS_M / 2.0, c),
            recipient(3, 0.0, c),
            recipient(4, GUIDED_AREA_RADIUS_M * 3.0, c),
        ])
    });
    assert_eq!(armor_on(&blast, 3), vec![22.0], "the centre takes it all");
    assert_eq!(armor_on(&blast, 2), vec![22.0 * 0.75], "half radius: 0.75");
    assert!(armor_on(&blast, 4).is_empty(), "outside takes nothing");
    assert!(blast.damage.is_some(), "the batch reached the authority");
}

/// With no supplied recipients the blast keeps going to the engagement's
/// single target, whatever else is around (F28-C.1, unchanged).
#[test]
fn accept_t1317_without_supplied_recipients_the_single_target_path_is_unchanged() {
    let (blast, _damage) = detonate(|_| None);
    assert_eq!(armor_on(&blast, 2), vec![22.0]);
    assert!(armor_on(&blast, 3).is_empty() && armor_on(&blast, 4).is_empty());
}

/// An empty supplied slice means nobody is in the blast: no damage at all,
/// not a fallback to the engagement target.
#[test]
fn accept_t1317_an_empty_recipient_slice_damages_nobody() {
    let (blast, _damage) = detonate(|_| Some(Vec::new()));
    assert!(blast.hits.is_empty());
}
