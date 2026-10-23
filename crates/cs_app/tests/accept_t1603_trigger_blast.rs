//! Task #1603 (T548/T1318 carry-over): `step_ordnance_session` routes a timed,
//! proximity or impact detonation through `route_trigger_area` over the
//! producer-supplied recipients.
//!
//! Task test prefix: `accept_t1603_`. An item that declares an area effect and
//! triggers damages every supplied recipient inside the radius, scaled by the
//! measured `1 - d^2/r^2` falloff; on the impact path the directly-hit actor is
//! excluded, on a timed trigger nobody is. Without supplied recipients, or
//! without a declared area, the single-target path is unchanged. Every value is
//! newly authored synthetic fixture data; no unit calibration is claimed
//! (`docs/findings/2026-10-18-t1318-blast-unknowns-measured.md`).

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    OrdnanceDetonation, OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId,
    OrdnanceLauncherBinding, OrdnanceOrder, OrdnanceSession, OrdnanceSessionTick, OrdnanceStep,
    step_ordnance_session,
};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DECLARED_SYNTHETIC_DIRECT_KEY, DECLARED_SYNTHETIC_LAUNCHER_MOUNT, DeclaredAreaEffect,
    DeclaredFuseRule, DeclaredOrdnance, DeclaredOrdnanceDetails, DeclaredOrdnanceFamily,
    declared_synthetic_direct, declared_synthetic_provenance,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageNodeKey, DamagePolicy, DamageResolver,
    SystemKind, synthetic_airframe_graph,
};
use cs_sim::time::TickRate;
use cs_sim::weapons::{BlastRecipient, FuseTrigger, ProjectileId, StatusEffectTarget, SweptHit};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

const SESSION: u64 = 53;
const PRODUCER: u32 = 99;
const DAMAGE_PRODUCER: u32 = 100;
const GENERATION: SceneGeneration = SceneGeneration(1);
const DT_S: f64 = 1.0 / 30.0;
const HULL: &str = "hull";
const MOUNT_Z: f32 = 10.0;
/// The declared area radius the blast compares against.
const AREA_RADIUS_M: f64 = 40.0;
/// The tick the impact (or the timed fuse) ends the item on; later than the
/// fixture's arming window.
const END_TICK: u64 = 6;
/// The declared armor channel of the direct-explosive fixture.
const ARMOR: f64 = 22.0;

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

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("t1603.blast-test").expect("a valid claim id")),
    ))
}

fn rate() -> TickRate {
    TickRate::new(30).expect("the test rate is nonzero")
}

/// The direct-explosive fixture, with or without a declared area, on its
/// impact fuse or retimed to a timed fuse that fires on [`END_TICK`].
fn declared_direct(area: bool, timed: bool) -> DeclaredOrdnance {
    let fixture = declared_synthetic_direct();
    let mut projectile = match fixture.details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => (**projectile).clone(),
        DeclaredOrdnanceDetails::Nitro(_) => panic!("the direct fixture is a projectile"),
    };
    if timed {
        projectile.fuse = DeclaredFuseRule::Timed {
            ticks: known(END_TICK),
        };
    }
    projectile.area_effect = area.then(|| DeclaredAreaEffect {
        radius_m: known(AREA_RADIUS_M),
        lifetime_ticks: known(5),
    });
    DeclaredOrdnance::try_new(
        ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_DIRECT_KEY)
            .expect("a weapon id"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::DirectExplosive,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("the retimed direct fixture is valid")
}

fn engagement() -> OrdnanceEngagement {
    OrdnanceEngagement {
        damage_target: actor(2),
        node: key(HULL),
        status_recipient: StatusEffectTarget::actor_system(actor(2), SystemKind::Propulsion),
    }
}

fn launcher_world(world: &mut World) {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(1),
                ordnance: vec![
                    ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_DIRECT_KEY)
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
            mount: key(DECLARED_SYNTHETIC_LAUNCHER_MOUNT),
            generation: GENERATION,
        },
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, MOUNT_Z)),
        ChildOf(root),
    ));
}

fn damage_resolver() -> DamageResolver {
    let mut damage = DamageResolver::new(session_id(), DAMAGE_PRODUCER);
    for serial in [2, 3, 4, 5] {
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

fn recipient(serial: u64, distance_m: f64, centre: [f64; 3]) -> BlastRecipient {
    BlastRecipient::new(
        actor(serial),
        key(HULL),
        WorldPosition::try_new([centre[0] + distance_m, centre[1], centre[2]])
            .expect("finite position"),
    )
}

/// What the end of the flight produced.
struct Ended {
    detonation: OrdnanceDetonation,
    /// Where the item was when its fuse fired.
    position: [f64; 3],
}

/// Flies one direct-explosive item from tick 0 and ends it on [`END_TICK`]:
/// by a swept contact on `actor(2)` (ignored by a timed fuse, which fires on
/// its own clock).
/// `recipients` is built from the item's position at the end tick, which a
/// probe flight (identical and deterministic) supplies.
fn fly(
    declared: &DeclaredOrdnance,
    recipients: Option<&dyn Fn([f64; 3]) -> Vec<BlastRecipient>>,
) -> Ended {
    let probe = if recipients.is_some() {
        Some(fly(declared, None).position)
    } else {
        None
    };
    let supplied = recipients.map(|build| build(probe.expect("a probe position")));
    let mut world = World::new();
    launcher_world(&mut world);
    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue(declared))
        .expect("a nonzero session opens");
    let registered = session
        .register(actor(1), std::slice::from_ref(declared))
        .expect("the declared direct rocket registers");
    let ordnance = registered[0].ordnance.clone();
    let mut damage = damage_resolver();
    let mut projectile: Option<ProjectileId> = None;
    let mut ended: Option<OrdnanceSessionTick> = None;
    let mut ended_position = None;
    for at in 0..=END_TICK {
        let orders = if at == 0 {
            vec![OrdnanceOrder::Launch(OrdnanceLaunch {
                id: OrdnanceLaunchId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: PRODUCER,
                    sequence: 0,
                },
                shooter: actor(1),
                ordnance: ordnance.clone(),
                target: None,
                engagement: engagement(),
            })]
        } else {
            Vec::new()
        };
        let impacts = match projectile {
            Some(projectile) if at == END_TICK => vec![SweptHit {
                projectile,
                target: actor(2),
                time_of_impact: 0.5,
            }],
            _ => Vec::new(),
        };
        let tick = step_ordnance_session(
            &mut world,
            &mut session,
            &mut damage,
            &orders,
            &OrdnanceStep {
                at: Tick(at),
                dt_s: DT_S,
                wind_velocity_m_s: [0.0; 3],
                generation: GENERATION,
                guidance: None,
                targets: &[],
                impacts: &impacts,
                blast_recipients: supplied.as_deref(),
                victim_nodes: None,
            },
        );
        if at == 0 {
            projectile = Some(tick.launched[0]);
        }
        assert!(
            tick.routing_refused.is_empty(),
            "{:?}",
            tick.routing_refused
        );
        if ended.is_none() && !tick.detonations.is_empty() {
            ended = Some(tick);
            ended_position = session
                .runtime()
                .get(&projectile.expect("launched"))
                .map(|item| item.current());
        }
    }
    let tick = ended.expect("the item ended");
    let detonation = tick.detonations[0].clone();
    let p = ended_position.expect("a triggered item stays live until the next advance");
    Ended {
        detonation,
        position: [p.x(), p.y(), p.z()],
    }
}

fn catalogue(declared: &DeclaredOrdnance) -> cs_sim::weapons::OrdnanceRegistry {
    let mut catalogue = cs_sim::weapons::OrdnanceRegistry::new();
    let component = cs_app::ordnance::lower_ordnance(declared).expect("a synthetic lowers");
    let _ = catalogue.register(component);
    catalogue
}

fn armor_on(ended: &Ended, serial: u64) -> Vec<f64> {
    ended
        .detonation
        .hits
        .iter()
        .filter(|hit| hit.target == actor(serial) && hit.channel == DamageChannel::Armor)
        .map(|hit| hit.damage)
        .collect()
}

/// Impact path: every in-radius supplied recipient takes the falloff damage,
/// the directly-hit actor (`actor(2)`) is excluded although it stands at the
/// centre, and the outside recipient takes nothing.
#[test]
fn accept_t1603_an_impact_blast_damages_actors_in_radius_and_skips_the_hit_one() {
    let build = |c: [f64; 3]| {
        vec![
            recipient(2, 0.0, c),
            recipient(3, 0.0, c),
            recipient(4, AREA_RADIUS_M / 2.0, c),
            recipient(5, AREA_RADIUS_M * 3.0, c),
        ]
    };
    let ended = fly(&declared_direct(true, false), Some(&build));
    assert!(matches!(
        ended.detonation.trigger,
        FuseTrigger::Impact { target, .. } if target == actor(2)
    ));
    assert!(armor_on(&ended, 2).is_empty(), "the hit actor is excluded");
    assert_eq!(armor_on(&ended, 3), vec![ARMOR], "the centre takes it all");
    assert_eq!(armor_on(&ended, 4), vec![ARMOR * 0.75], "half radius: 0.75");
    assert!(armor_on(&ended, 5).is_empty(), "outside takes nothing");
    assert!(
        ended.detonation.damage.is_some(),
        "the batch reached the authority"
    );
}

/// The area is the recipients' only source: with none supplied the blast goes
/// to the engagement's single target, as before.
#[test]
fn accept_t1603_without_supplied_recipients_the_single_target_path_is_unchanged() {
    let ended = fly(&declared_direct(true, false), None);
    assert_eq!(armor_on(&ended, 2), vec![ARMOR]);
    assert!(armor_on(&ended, 3).is_empty());
}

/// An item that declares no area never uses the recipients.
#[test]
fn accept_t1603_an_item_without_an_area_ignores_the_supplied_recipients() {
    let build = |c: [f64; 3]| vec![recipient(3, 0.0, c)];
    let ended = fly(&declared_direct(false, false), Some(&build));
    assert_eq!(armor_on(&ended, 2), vec![ARMOR]);
    assert!(armor_on(&ended, 3).is_empty());
}

/// An empty supplied slice means nobody is there: no damage at all.
#[test]
fn accept_t1603_an_empty_recipient_slice_damages_nobody() {
    let ended = fly(&declared_direct(true, false), Some(&|_| Vec::new()));
    assert!(ended.detonation.hits.is_empty());
}

/// Timed path: nobody is the directly-hit actor, so the recipient standing at
/// the centre is damaged too, with the same falloff.
#[test]
fn accept_t1603_a_timed_blast_excludes_nobody() {
    let build = |c: [f64; 3]| {
        vec![
            recipient(2, 0.0, c),
            recipient(3, AREA_RADIUS_M / 2.0, c),
            recipient(5, AREA_RADIUS_M * 3.0, c),
        ]
    };
    let ended = fly(&declared_direct(true, true), Some(&build));
    assert!(matches!(
        ended.detonation.trigger,
        FuseTrigger::Timed { ticks } if ticks == END_TICK
    ));
    assert_eq!(armor_on(&ended, 2), vec![ARMOR]);
    assert_eq!(armor_on(&ended, 3), vec![ARMOR * 0.75]);
    assert!(armor_on(&ended, 5).is_empty());
}
