//! Acceptance suite M01-EXAMINE-OBJECTIVE (Rally #1754): examining Site #1.
//!
//! The mission's own record decides when a site is examined: `OBJECTIVE2` is
//! `TRAVELERS [player, APPROACHING, t_chamber, 200.0, 1]`, a **3D** radius of
//! 200 m about the node's world origin, checked on every mission tick and
//! completing once. On completion `REMOVE_OBJECTIVE_TARGET t_chamber` clears
//! the site's objective flag, and the selected target moves on to Site #2.
//!
//! Evidence class: **static reading of the engine image plus the owner's
//! Drydock measurements** (the capture only brackets the radius: it fired at
//! 178 m 3D with the previous sample at 203 m); the radius itself is read from
//! the retail record below. Nothing here is `verified_original`. The findings
//! are in `docs/findings/2026-10-21-m01-examine-objective.md`.

use std::path::{Path, PathBuf};

use avian3d::prelude::Position;
use bevy::prelude::{App, Entity, Transform, Vec3, With};
use cs_app::cinematics::launch_attackers::{ObjectivesWakeGraph, TravelTarget};
use cs_app::mission_actors::MissionGroundTarget;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionPlayerBody, build_headless, stage_for, teardown,
};
use cs_app::targeting::TargetingSession;
use cs_script::runtime::ObjectiveLifecycle;

use super::m01_aiv_layout::member_document;
use crate::campaign::common::label;

/// `OBJECTIVE2`'s zero-based block index.
const SITE1_BLOCK: u32 = 1;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The owner's autopilot position when the hand-off fired (Drydock).
const OWNER_POSITION: [f64; 3] = [-4190.0, 273.0, -4672.0];

fn node_origin(app: &mut App, node: &str) -> [f64; 3] {
    let world = app.world_mut();
    let mut query = world.query::<&MissionGroundTarget>();
    query
        .iter(world)
        .find(|target| target.node == node)
        .unwrap_or_else(|| panic!("M01 declares the ground target `{node}`"))
        .position_m
}

fn player(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .iter(world)
        .next()
        .expect("one player body")
}

fn teleport(app: &mut App, entity: Entity, to: [f64; 3]) {
    let target = Vec3::new(to[0] as f32, to[1] as f32, to[2] as f32);
    let world = app.world_mut();
    if let Some(mut position) = world.get_mut::<Position>(entity) {
        position.0 = target;
    }
    if let Some(mut transform) = world.get_mut::<Transform>(entity) {
        transform.translation = target;
    }
}

/// Flies one frame at `to`; whether `OBJECTIVE2` is done after it.
fn fly_to(app: &mut App, body: Entity, to: [f64; 3]) -> bool {
    teleport(app, body, to);
    app.update();
    app.world()
        .resource::<MissionHost>()
        .lifecycles()
        .state(SITE1_BLOCK)
        == Some(ObjectiveLifecycle::Done)
}

fn selected_node(app: &mut App) -> Option<String> {
    let world = app.world_mut();
    let current = world.resource::<TargetingSession>().selection().current()?;
    let mut query = world.query::<&MissionGroundTarget>();
    query
        .iter(world)
        .find(|target| target.damage_actor == current)
        .map(|target| target.node.clone())
}

fn flagged(app: &mut App, node: &str) -> bool {
    let world = app.world_mut();
    let mut query = world.query::<&MissionGroundTarget>();
    query
        .iter(world)
        .find(|target| target.node == node)
        .is_some_and(|target| target.objective)
}

fn offset(origin: [f64; 3], horizontal: f64, vertical: f64) -> [f64; 3] {
    [origin[0] + horizontal, origin[1] + vertical, origin[2]]
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

fn objectives(root: &Path) -> ObjectivesWakeGraph {
    let (document, _) = member_document(root, "zbd/c3/m01/zrdr.zbd", "objectives.zrd");
    ObjectivesWakeGraph::read(&document).expect("objectives.zrd reads")
}

/// **The radius is the record's**: `OBJECTIVE2` watches `t_chamber` at 200 m
/// and `OBJECTIVE12` watches `shipwreck` at 200 m, both read from the retail
/// `objectives.zrd` with the production reader. The owner's 178 m / 203 m
/// bracket (and the 190 m / 214 m one about the node origin) sits on both
/// sides of it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_examine_objective_the_record_spells_a_200_m_radius_on_the_site_node() {
    let graph = objectives(&game_dir());
    let site1 = graph
        .get(2)
        .and_then(|o| o.travelers.clone())
        .expect("OBJECTIVE2 carries a TRAVELERS condition");
    assert_eq!(site1.target, TravelTarget::Node("t_chamber".to_owned()));
    assert_eq!(site1.radius, 200.0);
    let site3 = graph
        .get(12)
        .and_then(|o| o.travelers.clone())
        .expect("OBJECTIVE12 carries a TRAVELERS condition");
    assert_eq!(site3.target, TravelTarget::Node("shipwreck".to_owned()));
    assert_eq!(site3.radius, 200.0);
}

/// **The fly-by.** The mission opens with Site #1 selected; a pass at 250 m
/// and at 205 m (3D, about the node origin) leaves everything alone; a pass at
/// 126 m horizontal / 126 m vertical completes `OBJECTIVE2` on that very tick,
/// clears Site #1's flag and hands the selection to Site #2; flying out and in
/// again changes nothing more.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_examine_objective_the_flyby_completes_site_one_and_hands_off_to_site_two() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    app.update();
    let body = player(&mut app);
    let origin = node_origin(&mut app, "t_chamber");

    // The owner's own sample lies inside the radius about the node origin and
    // its predecessor 0.3 s earlier outside it.
    assert!(distance(OWNER_POSITION, origin) < 200.0);
    assert!(distance([-4165.0, 277.0, -4654.0], origin) > 200.0);

    // Open: Site #1 is the selected target and a flagged objective site.
    assert_eq!(selected_node(&mut app).as_deref(), Some("t_chamber"));
    assert!(flagged(&mut app, "t_chamber") && flagged(&mut app, "grasshut2"));

    // Outside: 250 m, then 205 m (3D), a pass that only brushes the radius.
    for (horizontal, vertical) in [(250.0, 0.0), (0.0, 250.0), (120.0, 166.0)] {
        let far = offset(origin, horizontal, vertical);
        assert!(distance(far, origin) > 200.0);
        for _ in 0..6 {
            assert!(!fly_to(&mut app, body, far));
        }
    }
    assert_eq!(selected_node(&mut app).as_deref(), Some("t_chamber"));
    assert!(flagged(&mut app, "t_chamber"));
    assert!(
        app.world()
            .resource::<MissionHost>()
            .target_flags()
            .changes()
            .is_empty()
    );

    // Inside: 126 m horizontal and 126 m vertical (178 m 3D).
    let near = offset(origin, 126.0, 126.0);
    assert!((distance(near, origin) - 178.2).abs() < 0.1);
    let mut completed_on = None;
    for frame in 0..6 {
        if fly_to(&mut app, body, near) {
            completed_on = Some(frame);
            break;
        }
    }
    assert_eq!(
        completed_on,
        Some(0),
        "OBJECTIVE2 completes on the first tick inside"
    );
    assert!(
        !flagged(&mut app, "t_chamber"),
        "Site #1 is no objective target any more"
    );
    assert!(flagged(&mut app, "grasshut2"), "Site #2 still is");
    assert_eq!(
        selected_node(&mut app).as_deref(),
        Some("grasshut2"),
        "the selected target moved to Site #2 by itself"
    );
    let changes = app
        .world()
        .resource::<MissionHost>()
        .target_flags()
        .changes()
        .to_vec();
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].node, "t_chamber");
    assert!(!changes[0].objective);
    assert_eq!(changes[0].handed_to.as_deref(), Some("grasshut2"));

    // One shot: out and in again stays done and moves nothing.
    for to in [offset(origin, 250.0, 0.0), near, near] {
        for _ in 0..4 {
            assert!(fly_to(&mut app, body, to), "a done objective stays done");
        }
    }
    assert_eq!(selected_node(&mut app).as_deref(), Some("grasshut2"));
    assert_eq!(
        app.world()
            .resource::<MissionHost>()
            .target_flags()
            .changes()
            .len(),
        1
    );
    teardown(&mut app);
}
