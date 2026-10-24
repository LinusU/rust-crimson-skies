//! M01 convoy truck destruction (#2017, follow-up to BUG-M01-CONVOY-GUN-HITS
//! #1998): what the destroyed trucks do to the mission's objective
//! observation, what a restart gives back, and whether the ground target
//! boxes collide with anything else. Task test prefix:
//! `accept_m01_convoy_destruction_`.
//!
//! All three tests compose retail M01 and need `CS_GAME_DIR`.
//!
//! # What "the objective responds" means for M01
//!
//! The mission's lowered program never *counts* the trucks: its operand
//! members are `piratezep`, `player`, `shipwreck` and `t_chamber`
//! (`WorldOperands::of`), and the convoy's objective block ends on the
//! `rope1burn` animation, not on a kill count. The trucks are objective
//! *targets* (`SetTargetFlag`), so the only consumer the original's data
//! gives their destruction is the member observation the `INACTIVE` ladders
//! read. The first test therefore pins both halves: the program reads no
//! truck chain, and every truck's observation row turns `no_longer_in_play`
//! when the damage authority destroys it. See
//! `docs/findings/2026-10-24-m01-convoy-destruction.md`.

use bevy::prelude::{App, Vec3};
use cs_app::damage::DamageScheduleReport;
use cs_app::mission_actors::{
    MissionGroundTarget, MissionGroundTargetReport, MissionGroundTargetSweep,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHostRestartReport, MissionHostRestartRequest, MissionStage, MissionWorldObservation,
    build_headless, mission_world_observation, stage_for, teardown,
};
use cs_app::mission_targets::DESIGNED_TARGET_INTEGRITY;
use cs_app::scene::NodeVisualTransform;
use cs_app::weapons::{PartSweptBox, SessionDamageResolver};
use cs_app::world_facts::{WorldOperands, member_chain};
use cs_sim::damage::ActorId;

use crate::campaign::common::label;
use crate::campaign::m01_convoy_gun_hits::{
    MAX_FRAMES, aim_player_at, destroyed, fire, game_dir, idle, key,
};

const TRUCKS: [&str; 4] = ["t_truck01", "t_truck02", "t_truck03", "bridge_truck01"];

fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads")
}

fn retail_app() -> App {
    let mut app = build_headless(&retail_stage()).expect("M01 composes");
    app.update();
    idle(&mut app);
    app
}

fn truck_actor(app: &App, node: &str) -> ActorId {
    app.world()
        .resource::<MissionGroundTargetReport>()
        .target(node)
        .unwrap_or_else(|| panic!("{node} spawned"))
        .actor
}

fn truck_centre(app: &App, node: &str) -> Vec3 {
    let entity = app
        .world()
        .resource::<MissionGroundTargetReport>()
        .target(node)
        .unwrap_or_else(|| panic!("{node} spawned"))
        .entity;
    let centre = app
        .world()
        .get::<MissionGroundTargetSweep>(entity)
        .expect("sweep provenance")
        .centre_m;
    Vec3::new(centre[0] as f32, centre[1] as f32, centre[2] as f32)
}

/// Fires from beside `node` until the damage authority destroys it.
fn shoot_down(app: &mut App, node: &str) {
    let actor = truck_actor(app, node);
    let point = truck_centre(app, node);
    for _ in 0..MAX_FRAMES {
        aim_player_at(app, point, Vec3::new(1.0, 0.15, 0.3));
        fire(app);
        if destroyed(app, actor) {
            return;
        }
    }
    panic!(
        "{node} was not destroyed: {:?}",
        app.world().get_resource::<DamageScheduleReport>()
    );
}

/// The observation row of one node, read through the production writer.
fn observed_in_play(app: &mut App, node: &str) -> Option<bool> {
    mission_world_observation(app.world_mut());
    app.world()
        .resource::<MissionWorldObservation>()
        .observation
        .members()
        .get(&member_chain([node]))
        .map(|row| row.in_play)
}

/// **Destroying the convoy trucks publishes to the objective observation.**
///
/// Every truck reads `held_in_play` before the shot and `no_longer_in_play`
/// after it, the neighbours' rows do not move, and the mission program reads
/// no truck chain — so the observation row is the whole of the response the
/// original's data gives.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_convoy_destruction_publishes_to_the_objective_observation() {
    let stage = retail_stage();
    let operands = WorldOperands::of(&stage.host.control);
    for node in TRUCKS {
        assert!(
            !operands.members.iter().any(|chain| chain.join(".") == node),
            "{node} is read by the lowered program; the finding must be rewritten"
        );
    }
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    idle(&mut app);

    for node in TRUCKS {
        assert_eq!(
            observed_in_play(&mut app, node),
            Some(true),
            "{node} is in play before it is shot"
        );
    }
    for (index, node) in TRUCKS.iter().enumerate() {
        shoot_down(&mut app, node);
        assert_eq!(
            observed_in_play(&mut app, node),
            Some(false),
            "{node} leaves play when destroyed"
        );
        for later in &TRUCKS[index + 1..] {
            assert_eq!(
                observed_in_play(&mut app, later),
                Some(true),
                "{later} stays in play when {node} is destroyed"
            );
        }
    }
    teardown(&mut app);
}

/// **A restart gives the trucks back healthy, with fresh boxes, and a round
/// of the old generation hits nothing.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_convoy_destruction_restart_restores_the_trucks() {
    let mut app = retail_app();
    let old: Vec<(&str, ActorId)> = TRUCKS
        .iter()
        .map(|node| (*node, truck_actor(&app, node)))
        .collect();

    shoot_down(&mut app, "t_truck01");
    shoot_down(&mut app, "t_truck02");

    // A burst in flight at an untouched truck when the restart lands.
    let target = truck_centre(&app, "t_truck03");
    for _ in 0..4 {
        aim_player_at(&mut app, target, Vec3::new(1.0, 0.15, 0.3));
        fire(&mut app);
    }
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();
    assert!(
        app.world().contains_resource::<MissionHostRestartReport>(),
        "the composed restart ran"
    );

    for (node, old_actor) in &old {
        let bound = app
            .world()
            .resource::<MissionGroundTargetReport>()
            .target(node)
            .unwrap_or_else(|| panic!("{node} respawned"))
            .clone();
        assert_ne!(
            bound.actor, *old_actor,
            "{node}: the restart mints a new actor, not the old one"
        );
        assert_ne!(
            bound.actor.session, old_actor.session,
            "{node}: new session"
        );
        let resolver = app.world().resource::<SessionDamageResolver>().resolver();
        assert!(!resolver.is_destroyed(&bound.actor), "{node} is healthy");
        assert_eq!(
            resolver.remaining_integrity(&bound.actor, &key(node)),
            Some(DESIGNED_TARGET_INTEGRITY),
            "{node} has its full integrity again"
        );
        assert_eq!(
            resolver.remaining_integrity(old_actor, &key(node)),
            None,
            "{node}: the old generation's actor is unknown to the new resolver"
        );
        let swept = app
            .world()
            .get::<PartSweptBox>(bound.entity)
            .unwrap_or_else(|| panic!("{node} has a fresh swept box"));
        assert_eq!(swept.node, key(node));
        let stored = app
            .world()
            .get::<MissionGroundTarget>(bound.entity)
            .unwrap_or_else(|| panic!("{node} is a ground target"))
            .target_half_extents_m;
        assert_eq!(
            swept.half_extents_m, stored,
            "{node}: the fresh box takes the node's stored extents"
        );
    }

    // The old burst is gone: with the player back at the start, nothing
    // damages any truck over a long idle stretch.
    for _ in 0..180 {
        idle(&mut app);
    }
    for (node, _) in &old {
        let actor = truck_actor(&app, node);
        let resolver = app.world().resource::<SessionDamageResolver>().resolver();
        assert_eq!(
            resolver.remaining_integrity(&actor, &key(node)),
            Some(DESIGNED_TARGET_INTEGRITY),
            "{node} was damaged by a round of the old generation"
        );
    }

    // And the fresh box works: the restarted truck can be destroyed again.
    shoot_down(&mut app, "t_truck01");
    teardown(&mut app);
}

/// **The ground target boxes overlap exactly the recorded pairs.**
///
/// The zeppelin-container ground target M01 binds (`rock_zeppelin`, at the
/// container origin) and the four convoy trucks are checked against every
/// other swept box, each at its stored node box (#2048). The trucks sit on the
/// bridge, whose stored box is 17 x 97 x 563 m, so each overlaps
/// `susp_bridge`; `rock_zeppelin` overlaps the `pzhookpoint` part box. A round
/// through an overlap damages both actors (one hit per distinct actor); the
/// finding records this.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_convoy_destruction_ground_box_overlaps_are_the_recorded_ones() {
    let mut app = retail_app();
    let world = app.world_mut();
    let boxes: Vec<(bevy::prelude::Entity, [f64; 3], Vec3)> = world
        .query::<(bevy::prelude::Entity, &PartSweptBox, &NodeVisualTransform)>()
        .iter(world)
        .map(|(entity, swept, transform)| {
            (
                entity,
                swept.half_extents_m,
                transform.global().translation(),
            )
        })
        .collect();
    let grounds: Vec<(bevy::prelude::Entity, String, Vec3, [f64; 3])> = world
        .query::<(
            bevy::prelude::Entity,
            &MissionGroundTarget,
            &MissionGroundTargetSweep,
        )>()
        .iter(world)
        .map(|(entity, target, sweep)| {
            let c = sweep.centre_m;
            (
                entity,
                target.node.clone(),
                Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32),
                sweep.half_extents_m,
            )
        })
        .collect();

    let mut overlaps: Vec<(String, f32, bevy::prelude::Entity)> = Vec::new();
    for (entity, node, centre, half) in &grounds {
        if !(TRUCKS.contains(&node.as_str()) || node == "rock_zeppelin") {
            continue;
        }
        for (other, other_half, other_centre) in &boxes {
            if other == entity {
                continue;
            }
            let delta = (*centre - *other_centre).abs();
            let depth = [
                (half[0] + other_half[0]) as f32 - delta.x,
                (half[1] + other_half[1]) as f32 - delta.y,
                (half[2] + other_half[2]) as f32 - delta.z,
            ];
            if depth.iter().all(|d| *d > 0.0) {
                let least = depth.iter().copied().fold(f32::MAX, f32::min);
                let name = grounds
                    .iter()
                    .find(|(e, _, _, _)| e == other)
                    .map_or_else(|| "<part box>".to_owned(), |(_, n, _, _)| n.clone());
                overlaps.push((format!("{node} x {name}"), least, *other));
            }
        }
    }
    let mut pairs: Vec<&str> = overlaps.iter().map(|(pair, _, _)| pair.as_str()).collect();
    pairs.sort_unstable();
    assert_eq!(
        pairs,
        [
            "bridge_truck01 x susp_bridge",
            "rock_zeppelin x pzhookpoint",
            "t_truck01 x susp_bridge",
            "t_truck02 x susp_bridge",
            "t_truck03 x susp_bridge",
        ],
        "the recorded overlaps, and no others: {overlaps:?}"
    );
    teardown(&mut app);
}
