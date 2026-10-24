//! Acceptance suite BUG-M01-CONVOY-ROCKET-DAMAGE (Rally #1999): retail M01
//! rockets must damage the convoy trucks the player actually targets.
//!
//! The blocker: `rocket_launch` aimed every rocket at the lowest-serial
//! declared-hostile actor and aimed every hit at the literal node `"hull"`,
//! which no convoy truck registers — a visibly perfect rocket therefore
//! produced only `unknown_node` refusals and never a kill. The fix:
//!
//! 1. **The engagement names the player's selection.** `rocket_launch`
//!    reads the targeting session's current selection, validates it is
//!    rostered and hostile, and resolves the victim's own registered damage
//!    node — a ground target's `targets.zrd` node (`t_truck01`), a scripted
//!    actor's `airframe`, else the first node its graph declares.
//! 2. **The detonation routes to the actor the fuse actually reached.**
//!    `OrdnanceStep::victim_nodes` maps every live damage-bound actor to its
//!    node; an `Impact`/`Proximity` trigger names its victim and that
//!    victim's own node, never the engagement's default.
//! 3. **Nothing is swallowed.** `MissionOrdnanceReport` records fired
//!    launches, the selected target and node, each detonation's victim,
//!    node, applied/refused hits, remaining integrity and destruction.
//! 4. **Destruction reaches the objective bridge.** A destroyed ground
//!    target's member observation turns `no_longer_in_play`, which the
//!    mission's `INACTIVE` ladders count.
//!
//! Synthetic tests run everywhere; the retail tests compose M01 from the
//! original installation (`CS_GAME_DIR`) and drive the production launch →
//! fuse → resolver path against `t_truck01` and the rest of the convoy.
//! Nothing here is `verified_original`: the rocket's tuning is the designed
//! `m01-play-weapons.designed-loadout` record and the original's lock
//! gesture is unmeasured.

use std::path::PathBuf;

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::prelude::{App, Entity, GlobalTransform, Quat, Transform, Vec3};
use cs_app::damage::{DamageActorBinding, DamageScheduleReport};
use cs_app::mission_actors::MissionGroundTarget;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionCockpitActions, MissionComposition, MissionHost, MissionOrdnanceDetonation,
    MissionOrdnanceLaunch, MissionOrdnanceReport, MissionStage, MissionWeaponReport,
    build_headless, stage_for, teardown,
};
use cs_app::mission_targets::DESIGNED_TARGET_INTEGRITY;
use cs_app::score::SessionScoreLedger;
use cs_app::targeting::TargetingSession;
use cs_app::weapons::SessionDamageResolver;
use cs_sim::damage::{
    ActorId, AttributionRule, DamageGraph, DamageNode, DamageNodeKey, DamageNodeKind, DamagePolicy,
};
use cs_sim::weapons::ProjectileId;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::input::{Action, FlightCommand};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

/// How far ahead of the player a truck stands, in metres.
const TRUCK_AHEAD_M: f32 = 120.0;
/// How far off the flight path the decoy truck sits, in metres.
const TRUCK_SIDE_M: f32 = 600.0;
/// The most rendered frames a test waits for a kill before failing.
const MAX_FRAMES: usize = 600;
/// The retail convoy nodes the composed M01 must accept as rocket victims,
/// in convoy order. The designed stack holds six units and each rocket
/// spends two — three rockets is all an M01 launch carries — so the retail
/// kill loop covers three trucks, one launch each.
const RETAIL_KILL_NODES: [&str; 3] = ["t_truck01", "t_truck02", "t_truck03"];

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("the test node key is valid")
}

fn claim() -> ClaimId {
    ClaimId::new("bug-m01-convoy-rocket.accept-test").expect("the claim id is valid")
}

/// A ground-target-shaped damage graph: a single lethal internal-structure
/// node named exactly like the real `targets.zrd` row — the same shape
/// `mission_targets::target_damage_node` produces for the convoy.
fn truck_graph(spelling: &str, node: &str) -> DamageGraph {
    let subject = ContentId::from_source(ContentKind::Airframe, spelling)
        .expect("the truck subject id is valid");
    let integrity = Resolved::Known(Known::new(
        DESIGNED_TARGET_INTEGRITY,
        Provenance::designed(claim()),
    ));
    DamageGraph::try_new(
        subject,
        vec![
            DamageNode::new(key(node), DamageNodeKind::InternalStructure, integrity)
                .with_lethal(true),
        ],
    )
    .expect("the truck damage graph is valid")
}

/// The composed synthetic mission, with the weapon session opened, after one
/// update.
fn composed(label: &str) -> (Scratch, App) {
    let scratch = Scratch::new(label);
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.update();
    crate::campaign::common::put_player_in_flight(&mut app, 55.0);
    (scratch, app)
}

/// The session the composed mission serves.
fn served(app: &App) -> SessionId {
    app.world().resource::<MissionHost>().served()
}

/// The player's live position and forward (its body's `-Z` nose axis).
fn player_pose(app: &App) -> (Vec3, Vec3) {
    let player = app.world().resource::<MissionComposition>().player;
    let world = app.world();
    let (translation, rotation) = if let Some(global) = world.get::<GlobalTransform>(player) {
        (global.translation(), global.rotation())
    } else if let Some(transform) = world.get::<Transform>(player) {
        (transform.translation, transform.rotation)
    } else {
        panic!("the player body has a live transform");
    };
    (translation, -(rotation * Vec3::Z))
}

/// Spawns one convoy-truck-shaped damage target: a single lethal node named
/// after the real `targets.zrd` row, registered with the shared resolver,
/// rostered hostile in the targeting store, admitted to the score ledger and
/// placed in the world at `centre`.
fn spawn_truck(app: &mut App, serial: u64, node: &str, centre: Vec3) -> (Entity, ActorId) {
    let session = served(app);
    let generation = app
        .world()
        .resource::<cs_app::scene::SceneGenerations>()
        .latest();
    let actor = ActorId { session, serial };
    let graph = truck_graph(&format!("m01-convoy-rocket.{node}"), node);
    let subject = graph.subject().clone();
    let world = app.world_mut();
    world
        .resource_mut::<SessionDamageResolver>()
        .resolver_mut()
        .register_actor(
            actor,
            graph,
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the truck graph registers");
    world
        .resource_mut::<TargetingSession>()
        .store_mut()
        .register(cs_sim::targeting::TargetRecord {
            actor,
            faction: ContentId::from_source(
                ContentKind::Faction,
                cs_app::mission_session::MISSION_FACTION_HOSTILE,
            )
            .expect("the designed hostile faction id is valid"),
            class: cs_sim::targeting::TargetClass::WorldObject,
            objective: true,
            revealed: true,
            phase_eligible: true,
            position: WorldPosition::try_new([
                f64::from(centre.x),
                f64::from(centre.y),
                f64::from(centre.z),
            ])
            .expect("the truck centre is finite"),
        })
        .expect("the truck registers in the roster");
    {
        let mut ledger = world.resource_mut::<SessionScoreLedger>();
        let _ = cs_app::mission_session::admit_actor(&mut ledger, actor);
    }
    let entity = world
        .spawn((
            Transform::from_translation(centre),
            GlobalTransform::from_translation(centre),
            LinearVelocity(Vec3::ZERO),
            DamageActorBinding {
                actor,
                graph: subject,
                generation,
            },
        ))
        .id();
    (entity, actor)
}

/// Moves a spawned truck entity (test fixture, not a production object).
fn move_truck(app: &mut App, entity: Entity, centre: Vec3) {
    let world = app.world_mut();
    if let Some(mut t) = world.get_mut::<Transform>(entity) {
        t.translation = centre;
    }
    if let Some(mut g) = world.get_mut::<GlobalTransform>(entity) {
        *g = GlobalTransform::from_translation(centre);
    }
}

/// Publishes one frame's delivered cockpit edge and runs the composed ticks.
fn fire(app: &mut App, command: FlightCommand) {
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: vec![Action::Flight(command)],
    });
    app.update();
}

/// The session's shared damage authority.
fn resolver(app: &App) -> &cs_sim::damage::DamageResolver {
    app.world().resource::<SessionDamageResolver>().resolver()
}

/// Whether the mission's damage authority/schedule reports the actor
/// destroyed.
fn destroyed(app: &App, actor: ActorId) -> bool {
    resolver(app).is_destroyed(&actor)
        || app
            .world()
            .get_resource::<DamageScheduleReport>()
            .is_some_and(|report| report.destroyed.contains(&actor))
}

/// The last ordnance tick's report, if the ordnance driver ran.
fn ordnance(app: &App) -> Option<&MissionOrdnanceReport> {
    app.world().get_resource::<MissionOrdnanceReport>()
}

/// The accumulating ordnance report's latest fold, mirrored so a test that
/// waits for a kill can read mid-loop what the session recorded so far —
/// a launch and the detonation it causes are ticks apart.
#[derive(Debug, Default)]
struct OrdnanceLog {
    launches: Vec<MissionOrdnanceLaunch>,
    orders_refused: Vec<String>,
    detonations: Vec<MissionOrdnanceDetonation>,
    routing_refused: Vec<String>,
    expired: Vec<ProjectileId>,
}

fn drain(app: &App, log: &mut OrdnanceLog) {
    if let Some(report) = ordnance(app) {
        log.launches.clone_from(&report.launches);
        log.orders_refused.clone_from(&report.orders_refused);
        log.detonations.clone_from(&report.detonations);
        log.routing_refused.clone_from(&report.routing_refused);
        log.expired.clone_from(&report.expired);
    }
}

/// **A rocket fired at a selected convoy truck hits *that* truck on *its own*
/// node — not the lowest-serial hostile, not `"hull"`.**
///
/// Spawns a decoy hostile off the flight path and a real victim ahead, then
/// selects the victim. The engagement must name the selection, the proximity
/// detonation must name the reached victim, its hits must name `t_truck02`,
/// damage must apply, the lifecycle must destroy it once, and the decoy's
/// pool must stay full.
#[test]
fn accept_m01_convoy_rocket_selected_victim_takes_the_hit() {
    let (_scratch, mut app) = composed("m01-convoy-rocket-selected");
    let (position, forward) = player_pose(&app);
    let right = forward.cross(Vec3::Y).normalize();
    let (_decoy_entity, decoy) = spawn_truck(
        &mut app,
        101,
        "t_truck01",
        position + forward * TRUCK_AHEAD_M + right * TRUCK_SIDE_M,
    );
    let (_victim_entity, victim) = spawn_truck(
        &mut app,
        102,
        "t_truck02",
        position + forward * TRUCK_AHEAD_M,
    );
    assert!(
        app.world_mut()
            .resource_mut::<TargetingSession>()
            .select_scripted(victim),
        "the truck ahead is selectable"
    );

    let mut log = OrdnanceLog::default();
    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        fire(&mut app, FlightCommand::FireSecondary);
        drain(&app, &mut log);
        if destroyed(&app, victim) {
            killed = true;
            break;
        }
    }
    assert!(
        killed,
        "the selected truck was destroyed: weapon={:?} ordnance={log:?}",
        app.world().get_resource::<MissionWeaponReport>(),
    );

    let launch = log.launches.last().expect("a rocket launch was recorded");
    assert_eq!(
        launch.target, victim,
        "the engagement names the selected truck, not a roster scan"
    );
    assert_eq!(
        launch.node,
        key("t_truck02"),
        "the engagement names the victim's own node"
    );
    let detonation = log
        .detonations
        .iter()
        .find(|d| d.victim == Some(victim) && d.destroyed)
        .expect("a detonation routed to the selected truck and destroyed it");
    assert_eq!(
        detonation.node.as_ref(),
        Some(&key("t_truck02")),
        "the hits name the struck truck's own node"
    );
    assert!(detonation.applied > 0, "at least one hit applied");
    assert_eq!(detonation.remaining_integrity, Some(0.0));
    assert!(
        detonation.refused.is_empty(),
        "no hit was refused: {:?}",
        detonation.refused
    );
    assert!(
        log.routing_refused.is_empty(),
        "no routing refusals: {:?}",
        log.routing_refused
    );

    assert!(!destroyed(&app, decoy), "the off-path decoy took no damage");
    assert_eq!(
        resolver(&app).remaining_integrity(&decoy, &key("t_truck01")),
        Some(DESIGNED_TARGET_INTEGRITY),
        "the decoy's pool stayed full"
    );
    assert!(
        app.world().resource::<SessionScoreLedger>().score() > 0,
        "the kill reached the score ledger"
    );
    teardown(&mut app);
}

/// **A second convoy target is independently selectable and hittable after
/// the first is destroyed.**
#[test]
fn accept_m01_convoy_rocket_second_target_is_independently_hittable() {
    let (_scratch, mut app) = composed("m01-convoy-rocket-two-trucks");
    let (position, forward) = player_pose(&app);
    let right = forward.cross(Vec3::Y).normalize();
    let (first_entity, first) = spawn_truck(
        &mut app,
        201,
        "t_truck01",
        position + forward * TRUCK_AHEAD_M,
    );
    let (second_entity, second) = spawn_truck(
        &mut app,
        202,
        "t_truck02",
        position + forward * TRUCK_AHEAD_M + right * TRUCK_SIDE_M,
    );

    assert!(
        app.world_mut()
            .resource_mut::<TargetingSession>()
            .select_scripted(first),
        "the first truck is selectable"
    );
    let mut log = OrdnanceLog::default();
    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        fire(&mut app, FlightCommand::FireSecondary);
        drain(&app, &mut log);
        if destroyed(&app, first) {
            killed = true;
            break;
        }
    }
    assert!(killed, "the first truck was destroyed: ordnance={log:?}");
    let score_after_first = app.world().resource::<SessionScoreLedger>().score();

    // Park the wreck far off the flight path and bring the second truck in.
    let (position, forward) = player_pose(&app);
    move_truck(&mut app, first_entity, position - forward * 5_000.0);
    move_truck(&mut app, second_entity, position + forward * TRUCK_AHEAD_M);
    app.update();

    assert!(
        app.world_mut()
            .resource_mut::<TargetingSession>()
            .select_scripted(second),
        "the second truck is selectable after the first died"
    );
    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        fire(&mut app, FlightCommand::FireSecondary);
        drain(&app, &mut log);
        if destroyed(&app, second) {
            killed = true;
            break;
        }
    }
    assert!(
        killed,
        "the second truck was independently destroyed: ordnance={log:?}"
    );
    let detonation = log
        .detonations
        .iter()
        .find(|d| d.victim == Some(second) && d.destroyed)
        .expect("a detonation routed to the second truck and destroyed it");
    assert_eq!(detonation.node.as_ref(), Some(&key("t_truck02")));
    assert!(
        app.world().resource::<SessionScoreLedger>().score() > score_after_first,
        "the second kill scored again"
    );
    teardown(&mut app);
}

/// **A rocket that reaches nobody is reported as expired — not as silent
/// damage on an arbitrary target.**
#[test]
fn accept_m01_convoy_rocket_a_miss_reports_no_phantom_damage() {
    let (_scratch, mut app) = composed("m01-convoy-rocket-miss");
    let (position, forward) = player_pose(&app);
    let right = forward.cross(Vec3::Y).normalize();
    let (_entity, truck) = spawn_truck(
        &mut app,
        301,
        "t_truck01",
        position + forward * TRUCK_AHEAD_M + right * TRUCK_SIDE_M,
    );
    assert!(
        app.world_mut()
            .resource_mut::<TargetingSession>()
            .select_scripted(truck),
        "the off-path truck is selectable"
    );

    let mut log = OrdnanceLog::default();
    for _ in 0..MAX_FRAMES {
        fire(&mut app, FlightCommand::FireSecondary);
        drain(&app, &mut log);
        if !log.expired.is_empty() {
            break;
        }
    }
    assert!(
        !log.expired.is_empty(),
        "the missed rocket expired by its fuse clock: {log:?}"
    );
    assert!(
        log.detonations.is_empty(),
        "a miss produced no detonation hits: {log:?}"
    );
    assert!(
        !destroyed(&app, truck),
        "the missed truck took no phantom damage"
    );
    assert_eq!(
        resolver(&app).remaining_integrity(&truck, &key("t_truck01")),
        Some(DESIGNED_TARGET_INTEGRITY)
    );
    assert_eq!(
        app.world().resource::<SessionScoreLedger>().score(),
        0,
        "a miss scored nothing"
    );
    teardown(&mut app);
}

/// **A non-hostile selection refuses by name — the rocket never launches.**
#[test]
fn accept_m01_convoy_rocket_a_nonhostile_selection_is_refused() {
    let (_scratch, mut app) = composed("m01-convoy-rocket-neutral");
    let (position, forward) = player_pose(&app);
    let session = served(&app);
    let neutral = ActorId {
        session,
        serial: 401,
    };
    let centre = position + forward * TRUCK_AHEAD_M;
    let world = app.world_mut();
    world
        .resource_mut::<TargetingSession>()
        .store_mut()
        .register(cs_sim::targeting::TargetRecord {
            actor: neutral,
            faction: ContentId::from_source(
                ContentKind::Faction,
                cs_app::mission_session::MISSION_FACTION_NEUTRAL,
            )
            .expect("the designed neutral faction id is valid"),
            class: cs_sim::targeting::TargetClass::WorldObject,
            objective: false,
            revealed: true,
            phase_eligible: true,
            position: WorldPosition::try_new([
                f64::from(centre.x),
                f64::from(centre.y),
                f64::from(centre.z),
            ])
            .expect("the neutral position is finite"),
        })
        .expect("the neutral actor registers");
    assert!(
        world
            .resource_mut::<TargetingSession>()
            .select_scripted(neutral),
        "a neutral actor is still selectable"
    );

    fire(&mut app, FlightCommand::FireSecondary);
    let report = app
        .world()
        .get_resource::<MissionWeaponReport>()
        .expect("the publisher ran");
    assert!(
        report
            .rocket_refusals
            .iter()
            .any(|reason| reason.contains("not hostile")),
        "the refusal names the non-hostile selection: {report:?}"
    );
    let ordnance = ordnance(&app).expect("the ordnance report was published");
    assert!(
        ordnance.projectiles.is_empty() && ordnance.detonations.is_empty(),
        "no rocket launched at the neutral target: {ordnance:?}"
    );
    teardown(&mut app);
}

/// **A rostered but damage-less selection refuses by name** — a stale actor
/// with no registered graph cannot be aimed at.
#[test]
fn accept_m01_convoy_rocket_an_unbound_selection_is_refused() {
    let (_scratch, mut app) = composed("m01-convoy-rocket-unbound");
    let (position, forward) = player_pose(&app);
    let session = served(&app);
    let stale = ActorId {
        session,
        serial: 402,
    };
    let centre = position + forward * TRUCK_AHEAD_M;
    let world = app.world_mut();
    world
        .resource_mut::<TargetingSession>()
        .store_mut()
        .register(cs_sim::targeting::TargetRecord {
            actor: stale,
            faction: ContentId::from_source(
                ContentKind::Faction,
                cs_app::mission_session::MISSION_FACTION_HOSTILE,
            )
            .expect("the designed hostile faction id is valid"),
            class: cs_sim::targeting::TargetClass::WorldObject,
            objective: true,
            revealed: true,
            phase_eligible: true,
            position: WorldPosition::try_new([
                f64::from(centre.x),
                f64::from(centre.y),
                f64::from(centre.z),
            ])
            .expect("the position is finite"),
        })
        .expect("the stale actor registers in the roster");
    assert!(
        world
            .resource_mut::<TargetingSession>()
            .select_scripted(stale),
        "the rostered stale actor is selectable"
    );

    fire(&mut app, FlightCommand::FireSecondary);
    let report = app
        .world()
        .get_resource::<MissionWeaponReport>()
        .expect("the publisher ran");
    assert!(
        report
            .rocket_refusals
            .iter()
            .any(|reason| reason.contains("registers no damage node")),
        "the refusal names the unbound actor: {report:?}"
    );
    let ordnance = ordnance(&app).expect("the ordnance report was published");
    assert!(ordnance.projectiles.is_empty());
    teardown(&mut app);
}

// ------------------------------------------------------------- retail M01 ---

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The production stage for retail M01 (`zbd/c3/m01`, #1567).
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// The live ground target for a container node.
fn ground_target(app: &mut App, node: &str) -> (Entity, ActorId, Vec3) {
    let world = app.world_mut();
    let mut query = world.query::<(Entity, &MissionGroundTarget)>();
    query
        .iter(world)
        .find(|(_, target)| target.node == node)
        .map(|(entity, target)| {
            (
                entity,
                target.damage_actor,
                Vec3::from_array(target.position_m.map(|v| v as f32)),
            )
        })
        .unwrap_or_else(|| panic!("retail M01 spawned a ground target for {node}"))
}

/// Teleports the player `range` metres from `site`, nose on it, at rest —
/// the same component set the smoke's `apply_placement` writes.
fn teleport_player_facing(app: &mut App, site: Vec3, range: f32) {
    let player = app.world().resource::<MissionComposition>().player;
    let offset = Vec3::new(range, 0.0, 0.0);
    let at = site + offset;
    // `yaw_toward` in the smoke: aim the `-Z` nose at `-offset`.
    let yaw = (offset.x).atan2(offset.z);
    let heading = Quat::from_euler(bevy::math::EulerRot::YXZ, yaw, 0.0, 0.0);
    let world = app.world_mut();
    if let Some(mut position) = world.get_mut::<Position>(player) {
        position.0 = at;
    }
    if let Some(mut rotation) = world.get_mut::<Rotation>(player) {
        rotation.0 = heading;
    }
    if let Some(mut velocity) = world.get_mut::<LinearVelocity>(player) {
        velocity.0 = Vec3::ZERO;
    }
    if let Some(mut angular) = world.get_mut::<AngularVelocity>(player) {
        angular.0 = Vec3::ZERO;
    }
    if let Some(mut transform) = world.get_mut::<Transform>(player) {
        transform.translation = at;
        transform.rotation = heading;
    }
    if let Some(mut global) = world.get_mut::<GlobalTransform>(player) {
        *global = GlobalTransform::from_translation(at) * GlobalTransform::from_rotation(heading);
    }
    app.update();
}

/// **Retail M01: a selected convoy truck takes the rocket's damage on its
/// own registered node and is destroyed — for every convoy node.**
///
/// Composes the retail stage, and for each convoy node places the player
/// within the rocket's reach facing the truck, selects the truck through the
/// production selection path and fires through the production input path.
/// The ordnance report must name that truck and node, the hit must apply,
/// the lifecycle must destroy it exactly once and the score must climb.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_convoy_rocket_retail_trucks_take_routed_damage() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("retail M01 composes headlessly");
    app.update();

    let mut score_before = app.world().resource::<SessionScoreLedger>().score();
    for node in RETAIL_KILL_NODES {
        let (_, actor, site) = ground_target(&mut app, node);
        teleport_player_facing(&mut app, site, 150.0);
        let selected = app
            .world_mut()
            .resource_mut::<TargetingSession>()
            .select_scripted(actor);
        assert!(
            selected,
            "{node} is selectable through the targeting session"
        );

        // One launch order per truck, then let the rocket fly: the designed
        // stack holds six units and each rocket spends two, so three rockets
        // is all an M01 launch carries — one per convoy node.
        fire(&mut app, FlightCommand::FireSecondary);
        let mut killed = false;
        for _ in 0..MAX_FRAMES {
            app.update();
            if destroyed(&app, actor) {
                killed = true;
                break;
            }
            let report = ordnance(&app).expect("the ordnance report was published");
            if !report.expired.is_empty()
                || report.orders_refused.iter().any(|r| r.contains("spent"))
            {
                break;
            }
        }
        let report = ordnance(&app).expect("the ordnance report was published");
        assert!(
            killed,
            "{node} was destroyed by the selected rocket: weapon={:?} ordnance={:?}",
            app.world().get_resource::<MissionWeaponReport>(),
            report,
        );
        let detonation = report
            .detonations
            .iter()
            .find(|d| d.victim == Some(actor) && d.destroyed)
            .unwrap_or_else(|| panic!("a detonation routed to {node} and destroyed it"));
        assert_eq!(
            detonation.node.as_ref(),
            Some(&key(node)),
            "{node}'s hits named its own node"
        );
        assert!(detonation.applied > 0, "{node} took at least one hit");
        let score = app.world().resource::<SessionScoreLedger>().score();
        assert!(score > score_before, "the {node} kill scored");
        score_before = score;
    }
    teardown(&mut app);
}

/// **Retail M01: firing with nothing selected is refused by name, and no
/// rocket leaves the rail.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_convoy_rocket_retail_no_selection_refuses() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("retail M01 composes headlessly");
    app.update();

    // `TargetNothing` clears the selection through the production command
    // edge, then the fire edge must refuse.
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: vec![Action::Flight(FlightCommand::TargetNothing)],
    });
    app.update();
    fire(&mut app, FlightCommand::FireSecondary);

    let report = app
        .world()
        .get_resource::<MissionWeaponReport>()
        .expect("the publisher ran");
    assert!(
        report
            .rocket_refusals
            .iter()
            .any(|reason| reason.contains("no target is selected")),
        "the refusal names the missing selection: {:?}",
        report.rocket_refusals
    );
    let ordnance = ordnance(&app).expect("the ordnance report was published");
    assert!(
        ordnance.projectiles.is_empty() && ordnance.detonations.is_empty(),
        "no rocket launched without a selection: {ordnance:?}"
    );
    teardown(&mut app);
}
