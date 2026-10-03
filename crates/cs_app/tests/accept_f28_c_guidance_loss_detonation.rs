//! Acceptance scenario F28-C.1 through the per-tick ordnance session: a guided
//! item that loses its target with `Detonate` applies its declared blast —
//! the same damage channels and status effects a triggered item applies — at
//! the position the runtime last recorded, bounded by the item's declared
//! lifetime and its declared area effect.
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-C` (follow-up of task #122). Task test prefix:
//! `accept_f28_c_`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! These tests drive production code only: [`cs_app::ordnance`]'s
//! [`OrdnanceSession`] and [`step_ordnance_session`], over the `cs_sim`
//! runtime it owns. Every value is newly authored synthetic fixture data,
//! never original game data. No `CS_GAME_DIR` access.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceEngagement, OrdnanceEventKind, OrdnanceLaunch, OrdnanceLaunchId,
    OrdnanceLauncherBinding, OrdnanceOrder, OrdnanceSession, OrdnanceSessionTick, OrdnanceStep,
    step_ordnance_session,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DECLARED_SYNTHETIC_GUIDED_KEY, DECLARED_SYNTHETIC_LAUNCHER_MOUNT, DeclaredAreaEffect,
    DeclaredOrdnance, DeclaredOrdnanceDetails, DeclaredOrdnanceFamily, DeclaredStatusEffect,
    DeclaredStatusEffectKind, declared_synthetic_guided, declared_synthetic_provenance,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageEventKind, DamageNodeKey, DamagePolicy, DamageResolver,
    SystemKind, synthetic_airframe_graph,
};
use cs_sim::time::TickRate;
use cs_sim::weapons::{OrdnanceId, StatusEffectKind, StatusEffectTarget, TargetObservation};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

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
        OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER).expect("a nonzero session opens");
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
        },
    )
}

/// A guided item whose designated target is destroyed applies its declared
/// damage and statuses once, at the position the runtime last recorded, bounded
/// by the item's declared lifetime and area effect.
#[test]
fn accept_f28_c_a_guidance_loss_detonation_applies_its_declared_blast_at_the_last_position() {
    let mut world = World::new();
    launcher_world(&mut world);
    let (mut session, ordnance) = session_with_guided();
    let mut damage = damage_resolver();
    let recipient = StatusEffectTarget::actor_system(actor(2), SystemKind::Propulsion);

    // Tick 0: the launch is accepted from the live launcher pose, with the
    // hostile target designated. No observation yet, so the tracker holds it.
    let launched = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[launch_order(0, 0, ordnance.clone(), actor(2))],
        Tick(0),
        None,
    );
    assert_eq!(launched.launched.len(), 1, "one guided item is in flight");
    let projectile = launched.launched[0];
    let last_position = session
        .runtime()
        .get(&projectile)
        .expect("the item is live after its launch tick")
        .current();
    let integrity_before = damage
        .remaining_integrity(&actor(2), &key(HULL))
        .expect("the target's hull is registered");

    // Tick 1: the designated target is destroyed. The item detonates at the
    // position the runtime last recorded, before this tick's motion advance.
    let destroyed = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        Tick(1),
        Some(TargetObservation::destroyed(actor(2), Tick(1))),
    );

    let guidance = destroyed
        .guidance
        .as_ref()
        .expect("an observation was supplied");
    assert_eq!(
        guidance.detonated.len(),
        1,
        "the lost target detonated the guided item"
    );
    let detonation = &guidance.detonated[0];
    assert_eq!(detonation.projectile(), projectile);
    assert_eq!(detonation.shooter(), actor(1));
    assert_eq!(detonation.ordnance(), &ordnance);
    assert_eq!(
        detonation.position(),
        last_position,
        "the blast is at the position the runtime last recorded"
    );
    assert_eq!(
        detonation.channels().armor,
        22.0,
        "the declared armor channel travels with the blast"
    );
    assert_eq!(
        detonation.status().len(),
        1,
        "the declared status travels with it"
    );
    let area = detonation
        .area_effect()
        .expect("the declared area effect travels with the blast");
    assert_eq!(
        area.radius_m(),
        GUIDED_AREA_RADIUS_M,
        "the declared radius is carried unchanged, never fabricated"
    );
    assert!(area.radius_m() > 0.0, "no zero-radius effect is fabricated");
    assert!(
        area.lifetime_ticks() <= detonation.definition().lifetime_ticks(),
        "the declared area is bounded by the item's own lifetime"
    );
    assert_eq!(
        session.runtime().len(),
        0,
        "the detonated item is no longer live"
    );

    // The step applied the blast's declared damage and statuses to the
    // engagement.
    assert_eq!(destroyed.guidance_blasts.len(), 1, "the blast was applied");
    let blast = &destroyed.guidance_blasts[0];
    assert_eq!(blast.projectile, projectile);
    assert_eq!(blast.shooter, actor(1));
    assert_eq!(blast.ordnance, ordnance);
    assert_eq!(
        blast.position, last_position,
        "the applied blast names the last recorded position"
    );
    assert_eq!(blast.hits.len(), 2, "one hit per non-zero declared channel");
    let resolution = blast
        .damage
        .as_ref()
        .expect("the damage authority accepted the blast");
    assert!(
        resolution
            .events
            .iter()
            .any(|event| matches!(event.kind, DamageEventKind::HitApplied { .. })),
        "the authority applied the blast's routed hits"
    );
    assert!(blast.damage_refused.is_none());
    assert!(blast.status_refused.is_none());
    assert_eq!(
        blast.status_applied.len(),
        1,
        "the one declared status was applied to the stable recipient"
    );
    assert!(
        destroyed.events.iter().any(|event| matches!(
            event.kind,
            OrdnanceEventKind::GuidanceDetonated { projectile: p, .. } if p == projectile
        )),
        "the detonation was appended to the network log"
    );

    let integrity_after = damage
        .remaining_integrity(&actor(2), &key(HULL))
        .expect("the target's hull is registered");
    assert!(
        integrity_after < integrity_before,
        "the blast reached the shared damage authority: {integrity_after} < {integrity_before}"
    );
    assert_eq!(
        session.engine_status(&recipient).choke,
        GUIDED_CHOKE_STRENGTH,
        "the declared status effect reached the stable recipient"
    );

    // Exactly once: the item is gone, so a later observation applies no second
    // blast and changes no integrity.
    let later = run_step(
        &mut world,
        &mut session,
        &mut damage,
        &[],
        Tick(2),
        Some(TargetObservation::destroyed(actor(2), Tick(2))),
    );
    assert!(
        later.guidance_blasts.is_empty(),
        "a detonation is never applied twice"
    );
    assert_eq!(
        later
            .guidance
            .as_ref()
            .map(|guidance| guidance.detonated.len()),
        Some(0),
        "the ended item cannot detonate again"
    );
    assert_eq!(
        damage
            .remaining_integrity(&actor(2), &key(HULL))
            .expect("the target's hull is registered"),
        integrity_after,
        "the later tick changed no integrity"
    );
    assert!(
        !session.is_under(&recipient, StatusEffectKind::Stall),
        "the blast applied only the declared kind"
    );
}
