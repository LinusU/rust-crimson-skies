//! BUG-M01-CONVOY-GUN-HITS acceptance (#1998): the player's machine guns hit
//! and destroy M01's ground targets, the bridge convoy trucks first.
//!
//! Before the fix a `MissionGroundTarget` entity carried no `PartSweptBox`,
//! `NodeVisualTransform` or `LinearVelocity`, so `part_sweep_candidates` never
//! offered it to a fired round and the convoy's damage graph was unreachable by
//! gunfire. Task test prefix: `accept_m01_convoy_gun_hits_`.
//!
//! The synthetic tests run in CI; the retail one needs `CS_GAME_DIR` and drives
//! the composed retail M01 stage with the delivered fire edge.

use std::path::PathBuf;

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::prelude::{App, GlobalTransform, Transform, Vec3};
use cs_app::damage::{DamageActorBinding, DamageScheduleReport};
use cs_app::mission_actors::{
    DESIGNED_GROUND_TARGET_HALF_EXTENTS_M, MissionGroundTargetReport, MissionGroundTargetSweep,
    ground_target_sweep,
};
use cs_app::mission_session::{
    MissionCockpitActions, MissionComposition, MissionHost, admit_actor, build_headless, teardown,
};
use cs_app::score::SessionScoreLedger;
use cs_app::targeting::TargetingSession;
use cs_app::weapons::{PartSweptBox, SessionDamageResolver, part_sweep_candidates};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageGraph, DamageNode, DamageNodeKey, DamageNodeKind, DamagePolicy,
};
use cs_sim::targeting::{TargetClass, TargetRecord};
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::input::{Action, FlightCommand};
use cs_types::space::WorldPosition;

use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

const MAX_FRAMES: usize = 400;
const STANDOFF_M: f32 = 80.0;

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("a damage node key")
}

fn claim() -> ClaimId {
    ClaimId::new("m01-convoy-gun-hits.accept-test").expect("claim")
}

fn fire(app: &mut App) {
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: vec![Action::Flight(FlightCommand::FirePrimary)],
    });
    app.update();
}

fn idle(app: &mut App) {
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: Vec::new(),
    });
    app.update();
}

fn destroyed(app: &App, actor: ActorId) -> bool {
    app.world()
        .get_resource::<DamageScheduleReport>()
        .is_some_and(|report| report.destroyed.contains(&actor))
}

/// Parks the player `STANDOFF_M` short of `point`, nose on it, at rest.
fn aim_player_at(app: &mut App, point: Vec3, from_side: Vec3) {
    let player = app.world().resource::<MissionComposition>().player;
    let eye = point + from_side.normalize() * STANDOFF_M;
    let look = Transform::from_translation(eye).looking_at(point, Vec3::Y);
    let mut entity = app.world_mut().entity_mut(player);
    entity.insert((
        look,
        GlobalTransform::from(look),
        Position(eye),
        Rotation(look.rotation),
        LinearVelocity(Vec3::ZERO),
        AngularVelocity(Vec3::ZERO),
    ));
}

/// Spawns a synthetic damageable ground target built with the production
/// sweep bundle, in `faction`, at `centre`.
fn synthetic_ground_target(app: &mut App, serial: u64, faction: &str, centre: Vec3) -> ActorId {
    let session = app.world().resource::<MissionHost>().served();
    let generation = app
        .world()
        .resource::<cs_app::scene::SceneGenerations>()
        .latest();
    let actor = ActorId { session, serial };
    let integrity = cs_app::mission_targets::DESIGNED_TARGET_INTEGRITY;
    let node = DamageNode::new(
        key("t_truck_syn"),
        DamageNodeKind::InternalStructure,
        Resolved::Known(Known::new(integrity, Provenance::designed(claim()))),
    )
    .with_lethal(true);
    let subject = ContentId::from_source(ContentKind::Blueprint, "world-target.t_truck_syn")
        .expect("subject");
    let graph = DamageGraph::try_new(subject.clone(), vec![node]).expect("graph");
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
        .expect("registers");
    let position = [
        f64::from(centre.x),
        f64::from(centre.y),
        f64::from(centre.z),
    ];
    world
        .resource_mut::<TargetingSession>()
        .store_mut()
        .register(TargetRecord {
            actor,
            faction: ContentId::from_source(ContentKind::Faction, faction).expect("faction"),
            class: TargetClass::WorldObject,
            objective: true,
            revealed: true,
            phase_eligible: true,
            position: WorldPosition::try_new(position).expect("finite"),
        })
        .expect("rosters");
    let _ = admit_actor(&mut world.resource_mut::<SessionScoreLedger>(), actor);
    let (bundle, _) = ground_target_sweep(actor, key("t_truck_syn"), position, position);
    world.spawn((
        Transform::from_translation(centre),
        bundle,
        DamageActorBinding {
            actor,
            graph: subject,
            generation,
        },
    ));
    actor
}

fn composed(label: &str) -> (Scratch, App) {
    let scratch = Scratch::new(label);
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.update();
    (scratch, app)
}

fn player_ahead(app: &App) -> Vec3 {
    let player = app.world().resource::<MissionComposition>().player;
    let transform = app.world().get::<Transform>(player).expect("player pose");
    transform.translation + (transform.rotation * Vec3::NEG_Z) * 120.0
}

/// **A hostile ground target built with the production sweep bundle is hit by
/// the delivered fire edge and destroyed.**
#[test]
fn accept_m01_convoy_gun_hits_the_fire_key_destroys_a_hostile_ground_target() {
    let (_scratch, mut app) = composed("m01-convoy-gun-hits-hit");
    let centre = player_ahead(&app);
    let actor = synthetic_ground_target(
        &mut app,
        40,
        cs_app::mission_session::MISSION_FACTION_HOSTILE,
        centre,
    );
    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        aim_player_at(&mut app, centre, Vec3::new(1.0, 0.15, 0.3));
        fire(&mut app);
        if destroyed(&app, actor) {
            killed = true;
            break;
        }
    }
    assert!(
        killed,
        "player {:?} {:?} centre {centre:?}",
        app.world()
            .get::<Transform>(app.world().resource::<MissionComposition>().player),
        app.world()
            .get::<Position>(app.world().resource::<MissionComposition>().player)
    );
    teardown(&mut app);
}

/// **A target the gun line does not cross is not hit.**
#[test]
fn accept_m01_convoy_gun_hits_an_off_target_ground_target_is_missed() {
    let (_scratch, mut app) = composed("m01-convoy-gun-hits-miss");
    let centre = player_ahead(&app);
    let actor = synthetic_ground_target(
        &mut app,
        41,
        cs_app::mission_session::MISSION_FACTION_HOSTILE,
        centre,
    );
    // The same line of fire as the hit test, aimed 60 m to one side.
    let aim = centre + Vec3::new(60.0, 0.0, 0.0);
    for _ in 0..120 {
        aim_player_at(&mut app, aim, Vec3::new(1.0, 0.15, 0.3));
        fire(&mut app);
        assert!(!destroyed(&app, actor), "an off-line target was destroyed");
    }
    teardown(&mut app);
}

/// **A friendly ground target on the line of fire is not damaged.**
#[test]
fn accept_m01_convoy_gun_hits_a_friendly_ground_target_is_not_damaged() {
    let (_scratch, mut app) = composed("m01-convoy-gun-hits-ally");
    let centre = player_ahead(&app);
    let actor = synthetic_ground_target(
        &mut app,
        42,
        cs_app::mission_session::MISSION_FACTION_PLAYER,
        centre,
    );
    for _ in 0..MAX_FRAMES / 2 {
        aim_player_at(&mut app, centre, Vec3::new(1.0, 0.15, 0.3));
        fire(&mut app);
        assert!(!destroyed(&app, actor), "a friendly target was destroyed");
    }
    teardown(&mut app);
}

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// **Retail M01: every convoy truck carries a swept box at its measured target
/// point, and the player's fire edge destroys each one.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_convoy_gun_hits_retail_trucks_are_destroyed_by_the_fire_key() {
    use cs_app::mission_launch::plan_mission_launch;
    use cs_app::mission_session::stage_for;
    let root = game_dir();
    let plan = plan_mission_launch(
        &root,
        crate::campaign::common::label("M01"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    idle(&mut app);

    for node in ["t_truck01", "t_truck02", "t_truck03", "bridge_truck01"] {
        let bound = app
            .world()
            .resource::<MissionGroundTargetReport>()
            .target(node)
            .unwrap_or_else(|| panic!("{node} spawned"))
            .clone();
        let entity = bound.entity;
        let (box_node, half, centre) = {
            let e = app.world().entity(entity);
            let b = e
                .get::<PartSweptBox>()
                .unwrap_or_else(|| panic!("{node} has a swept box"));
            let s = e
                .get::<MissionGroundTargetSweep>()
                .expect("sweep provenance");
            (b.node.clone(), b.half_extents_m, s.centre_m)
        };
        assert_eq!(
            box_node,
            key(node),
            "{node}: the box addresses its own node"
        );
        assert_eq!(half, DESIGNED_GROUND_TARGET_HALF_EXTENTS_M);

        // The candidate exists at the measured position, as a hostile pair.
        let candidates = part_sweep_candidates(app.world(), 1.0 / 60.0, |_| None);
        assert!(
            candidates.refused.iter().all(|r| r.node() != &key(node)),
            "{node}: the box was refused: {:?}",
            candidates.refused
        );
        assert!(
            candidates.candidates.iter().any(|c| c.node() == &key(node)),
            "{node}: no sweep candidate"
        );

        let point = Vec3::new(centre[0] as f32, centre[1] as f32, centre[2] as f32);
        let mut killed = false;
        for _ in 0..MAX_FRAMES {
            aim_player_at(&mut app, point, Vec3::new(1.0, 0.15, 0.3));
            fire(&mut app);
            if destroyed(&app, bound.actor) {
                killed = true;
                break;
            }
        }
        assert!(
            killed,
            "{node} was not destroyed by the fire key: {:?}",
            app.world().get_resource::<DamageScheduleReport>()
        );
    }
    teardown(&mut app);
}
