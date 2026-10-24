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
use cs_types::content::ContentKind;

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
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let mut stage = stage_for(&root, &plan).expect("M01's stage reads");
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

/// **The player's guns destroy `medkestrel_1` through the production fire
/// edge, and group 1's count drops.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_ai_aircraft_damage_retail_the_fire_key_destroys_medkestrel_1() {
    let mut app = retail_app_with(true);
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
    // Each frame re-aims at the box centre, which follows the body.
    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        let centre = {
            let world = app.world_mut();
            world
                .query::<(&bevy::prelude::ChildOf, &NodeVisualTransform)>()
                .iter(world)
                .find(|(parent, _)| parent.parent() == entity)
                .map(|(_, visual)| visual.global().translation())
                .unwrap_or(Vec3::ZERO)
        };
        aim_player_at(&mut app, centre, Vec3::new(1.0, 0.15, 0.3));
        fire(&mut app);
        if destroyed(&app, actor) {
            killed = true;
            break;
        }
    }
    assert!(killed, "medkestrel_1 was not destroyed by the fire key");
    assert_eq!(group_one(&mut app), Some(2), "group 1's count dropped");
    let (_, other) = kestrel(&mut app, "medkestrel_2");
    assert!(!destroyed(&app, other), "the neighbours are untouched");
    teardown(&mut app);
}
