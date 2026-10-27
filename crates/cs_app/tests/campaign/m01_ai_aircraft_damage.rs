//! M01 scripted AI aircraft damage (#2100): the Kestrels (`medkestrel_1..3`,
//! group 1) carry a designed integrity pool and a designed swept hit box, so
//! the player's gun rounds destroy them and the group count the objective
//! blocks read drops. Task test prefix: `accept_m01_ai_aircraft_damage_`.
//!
//! The pool and the box are design choices labelled `designed`
//! (`m01-ai-aircraft.designed-pool`, `.designed-box`); see
//! `docs/findings/2026-10-25-m01-ai-aircraft-damage.md`. The tests compose
//! retail M01 and need `CS_GAME_DIR`.

use bevy::prelude::{App, Entity, Vec3};
use cs_app::damage::DamageActorBinding;
use cs_app::mission_actors::{
    AI_AIRCRAFT_BOX_CLAIM, DESIGNED_AI_AIRCRAFT_HALF_EXTENTS_M, ScriptedAircraftActor,
    ScriptedAircraftHullBox,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionWorldObservation, build_headless, mission_world_observation, stage_for, teardown,
};
use cs_app::scene::NodeVisualTransform;
use cs_app::weapons::PartSweptBox;

use cs_script::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MissionProgram, Objective, SymbolId, Value,
};
use cs_types::content::{ContentKind, Provenance};
use cs_types::evidence::ClaimId;

use crate::campaign::common::{cid, label};
use crate::campaign::m01_convoy_gun_hits::{
    MAX_FRAMES, aim_player_at, destroyed, fire, game_dir, idle,
};

/// A one-objective program that spells the wake M01's OBJECTIVE23 spells
/// (`WAKEUP_ENEMIES medkestrel_1..3`) and completes on its first tick. The
/// real site is reached only after a landing, 15+ s into a flown route
/// (`docs/findings/2026-10-24-m01-c3-win-route-census.md`); this stands in for
/// it so the retail Kestrels are released by the production wake consumer.
fn kestrel_wake_program() -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "test-m01-ai-aircraft-damage"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(ContentKind::Objective, "test-m01-kestrel-wake"),
            condition: Condition::Const(true),
            actions: vec![Action::Directive {
                operation: DirectiveOperation::WakeEnemies,
                args: ["medkestrel_1", "medkestrel_2", "medkestrel_3"]
                    .map(|name| Value::Str(name.to_owned()))
                    .to_vec(),
            }],
            span: None,
        }],
    }
}

fn retail_app() -> App {
    retail_app_with(false)
}

fn retail_app_with(wake: bool) -> App {
    retail_app_with_burst(wake, true)
}

/// `burst` false drops the muzzle burst's art from the stage, so the run
/// differs from the shipped one in presentation only (#2142).
fn retail_app_with_burst(wake: bool, burst: bool) -> App {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let mut stage = stage_for(&root, &plan).expect("M01's stage reads");
    if !burst {
        stage.muzzle_burst = None;
    }
    if wake {
        stage.host.control = kestrel_wake_program();
    }
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    idle(&mut app);
    app
}

fn kestrel(app: &mut App, name: &str) -> (Entity, cs_sim::damage::ActorId) {
    let world = app.world_mut();
    world
        .query::<(Entity, &ScriptedAircraftActor, &DamageActorBinding)>()
        .iter(world)
        .find(|(_, actor, _)| actor.record.name == name)
        .map(|(entity, actor, _)| (entity, actor.damage_actor))
        .unwrap_or_else(|| panic!("{name} spawned"))
}

fn group_one(app: &mut App) -> Option<u32> {
    mission_world_observation(app.world_mut());
    app.world()
        .resource::<MissionWorldObservation>()
        .observation
        .groups()
        .get(&1)
        .copied()
}

/// **Every Kestrel carries the labelled designed pool and box.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_ai_aircraft_damage_retail_kestrels_carry_the_designed_box() {
    let mut app = retail_app();
    for name in ["medkestrel_1", "medkestrel_2", "medkestrel_3"] {
        let (entity, _) = kestrel(&mut app, name);
        let world = app.world_mut();
        let boxes: Vec<_> = world
            .query::<(
                &bevy::prelude::ChildOf,
                &PartSweptBox,
                &ScriptedAircraftHullBox,
            )>()
            .iter(world)
            .filter(|(parent, ..)| parent.parent() == entity)
            .map(|(_, swept, hull)| (swept.half_extents_m, hull.provenance.clone()))
            .collect();
        assert_eq!(boxes.len(), 1, "{name}: one hull box");
        assert_eq!(boxes[0].0, DESIGNED_AI_AIRCRAFT_HALF_EXTENTS_M);
        assert_eq!(boxes[0].1.claim_id.as_str(), AI_AIRCRAFT_BOX_CLAIM);
    }
    teardown(&mut app);
}

/// The frame budget of the fire-key kill. Aimed at the hull box centre (the
/// box the rounds sweep) the Kestrel dies on frame 453 (measured, retail,
/// 2026-10-27), past the shared `MAX_FRAMES`: the Kestrel flies on and the
/// aimed rounds meet it only along its path.
const KILL_FRAMES: usize = MAX_FRAMES * 3;

/// The centre of the swept hull box the rounds are tested against, a child of
/// `kestrel`. The Kestrel has other visual children (the model's nodes); which
/// one a query meets first depends on entity and archetype order, which the
/// muzzle burst's presentation entities shift, so the aim must name the box.
fn hull_box_centre(world: &mut bevy::prelude::World, kestrel: Entity) -> Option<Vec3> {
    world
        .query::<(
            &bevy::prelude::ChildOf,
            &NodeVisualTransform,
            &ScriptedAircraftHullBox,
        )>()
        .iter(world)
        .find(|(parent, ..)| parent.parent() == kestrel)
        .map(|(_, visual, _)| visual.global().translation())
}

/// Flies the fire-key attack on `medkestrel_1` and returns the frame it dies
/// on. Each frame re-aims at the hull box centre, which follows the body.
fn fire_key_kill_frame(burst: bool) -> usize {
    let mut app = retail_app_with_burst(true, burst);
    let (entity, actor) = kestrel(&mut app, "medkestrel_1");
    assert_eq!(group_one(&mut app), Some(3), "group 1 starts with three");
    // Let the production wake release and arm the group.
    for _ in 0..30 {
        idle(&mut app);
    }
    assert!(
        !app.world()
            .get::<ScriptedAircraftActor>(entity)
            .expect("actor")
            .asleep,
        "the wake released medkestrel_1"
    );
    assert!(!destroyed(&app, actor));
    let mut killed = None;
    for frame in 0..KILL_FRAMES {
        let centre = hull_box_centre(app.world_mut(), entity).unwrap_or(Vec3::ZERO);
        aim_player_at(&mut app, centre, Vec3::new(1.0, 0.15, 0.3));
        fire(&mut app);
        if destroyed(&app, actor) {
            killed = Some(frame);
            break;
        }
    }
    let killed = killed.expect("medkestrel_1 was not destroyed by the fire key");
    assert_eq!(group_one(&mut app), Some(2), "group 1's count dropped");
    let (_, other) = kestrel(&mut app, "medkestrel_2");
    assert!(!destroyed(&app, other), "the neighbours are untouched");
    teardown(&mut app);
    killed
}

/// **The player's guns destroy `medkestrel_1` through the production fire
/// edge, and group 1's count drops.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_ai_aircraft_damage_retail_the_fire_key_destroys_medkestrel_1() {
    fire_key_kill_frame(true);
}

/// **The muzzle burst is presentation only (#2142).** The kill frame is the
/// same with the burst's art installed and without it. The 381 -> 453 shift
/// #2106 showed was the test aiming at whichever model node its query met
/// first, not the simulation.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_muzzle_sim_retail_kill_frame_ignores_the_burst() {
    let with_burst = fire_key_kill_frame(true);
    assert_eq!(with_burst, fire_key_kill_frame(false));
    assert_eq!(
        with_burst, 452,
        "the hull-box-aimed kill frame (index of 453)"
    );
}

/// **The aim names the hull box whatever order its siblings come in
/// (#2142)** — the synthetic, CI-visible half.
#[test]
fn accept_m01_muzzle_sim_hull_box_centre_ignores_sibling_order() {
    use bevy::prelude::{ChildOf, GlobalTransform, Transform, World};
    let mut centres = Vec::new();
    for box_first in [true, false] {
        let mut world = World::new();
        let kestrel = world.spawn_empty().id();
        let visual =
            |x: f32| NodeVisualTransform(GlobalTransform::from(Transform::from_xyz(x, 0.0, 0.0)));
        let spawn_node = |world: &mut World, x: f32| {
            world.spawn((ChildOf(kestrel), visual(x)));
        };
        let spawn_box = |world: &mut World| {
            world.spawn((
                ChildOf(kestrel),
                visual(1.0),
                ScriptedAircraftHullBox {
                    provenance: Provenance::designed(
                        ClaimId::new(AI_AIRCRAFT_BOX_CLAIM).expect("a declared claim id"),
                    ),
                },
            ));
        };
        if box_first {
            spawn_box(&mut world);
            spawn_node(&mut world, -4.0);
        } else {
            spawn_node(&mut world, -4.0);
            spawn_box(&mut world);
        }
        centres.push(hull_box_centre(&mut world, kestrel));
    }
    assert_eq!(centres[0], Some(Vec3::new(1.0, 0.0, 0.0)));
    assert_eq!(centres[0], centres[1]);
}
