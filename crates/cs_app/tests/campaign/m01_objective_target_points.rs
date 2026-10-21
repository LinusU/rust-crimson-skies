//! Acceptance suite M01-OBJECTIVE-TARGET-POINTS (Rally #1751): the three M01
//! objective sites hold the world points the original's selected-target
//! record held, and `E` cycles them in the original's order.
//!
//! Evidence class: **original RUN by the owner's tool** (Drydock, retail 1.02
//! under the emulator, unmodified; memory reads and the `E` key only) — one
//! setup, one session. It is not `verified_original` for any other setup. The
//! measured record points are in `docs/findings/2026-10-21-m01-objective-
//! target-points.md`; the owner's sample is `[Examine] - Site #1..3` at
//! `(-4278.5, 146.6, -4760.9)`, `(-6311.5, -27.1, -4610.9)` and
//! `(-8888.5, 20.8, -4686.8)`, side 0, type 8000.

use std::path::PathBuf;

use bevy::ecs::world::World;
use cs_app::mission_actors::MissionGroundTarget;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionWeapons, build_headless, stage_for};
use cs_app::mission_targets::bind_mission_targets;
use cs_app::targeting::{TargetingSession, apply_selection_edges};
use cs_sim::damage::ActorId;
use cs_sim::targeting::SelectionFrame;
use cs_types::Tick;
use cs_types::input::{Action, FlightCommand};

use crate::campaign::common::label;

/// The owner's Drydock sample, in cycle order: node, description, point.
const MEASURED: [(&str, &str, [f64; 3]); 3] = [
    ("t_chamber", "MSG_OBJ_SITE1", [-4278.5, 146.6, -4760.9]),
    ("grasshut2", "MSG_OBJ_SITE2", [-6311.5, -27.1, -4610.9]),
    ("shipwreck", "MSG_OBJ_SITE3", [-8888.5, 20.8, -4686.8]),
];

/// The sample's resolution: it prints one decimal.
const TOLERANCE_M: f64 = 0.06;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

fn assert_point(name: &str, got: [f64; 3], want: [f64; 3]) {
    for axis in 0..3 {
        assert!(
            (got[axis] - want[axis]).abs() <= TOLERANCE_M,
            "{name}: axis {axis} is {} but the original's record held {}",
            got[axis],
            want[axis]
        );
    }
}

/// **The binding's target point equals the original's record point** for
/// each site, including `grasshut2`, whose node origin is a local offset in
/// its group and whose point sits below sea level.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_objective_target_points_the_binding_equals_the_original_record() {
    let binding = bind_mission_targets(&game_dir(), "zbd/c3/m01", "zbd/c3");
    let sites: Vec<_> = binding.declared_targets().filter(|t| t.objective).collect();
    assert_eq!(sites.len(), 3, "M01 declares three objective sites");
    for (site, (node, description, point)) in sites.iter().zip(MEASURED) {
        assert_eq!(site.node, node, "declaration order is the cycle order");
        assert_eq!(site.description.as_deref(), Some(description));
        assert_point(node, site.target_point_m, point);
    }
}

/// **The composed mission cycles the sites in the original's order**, and
/// the targeting records hold the same points.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_objective_target_points_e_cycles_the_sites_in_order() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    app.update();
    let world: &mut World = app.world_mut();

    let player = world.resource::<MissionWeapons>().player();
    let mut sites: Vec<(ActorId, &'static str)> = Vec::new();
    {
        let mut query = world.query::<&MissionGroundTarget>();
        for target in query.iter(world) {
            if let Some((node, _, point)) = MEASURED.iter().find(|m| m.0 == target.node) {
                sites.push((target.damage_actor, node));
                let store = world.resource::<TargetingSession>().store();
                let record = store
                    .record(&target.damage_actor)
                    .expect("site is rostered");
                assert_point(node, record.position.to_array(), *point);
            }
        }
    }
    assert_eq!(sites.len(), 3);

    let mut seen: Vec<&str> = Vec::new();
    for step in 0..40u64 {
        let frame = SelectionFrame::at(Tick(100 + step));
        let edges = [Action::Flight(FlightCommand::TargetNextEnemy)];
        apply_selection_edges(world, player, &edges, frame).expect("the player is registered");
        let current = world.resource::<TargetingSession>().selection().current();
        if let Some((_, node)) = sites.iter().find(|(actor, _)| Some(*actor) == current) {
            seen.push(node);
        }
    }
    let order: Vec<&str> = MEASURED.iter().map(|m| m.0).collect();
    assert!(seen.len() >= 4, "E wrapped past the third site: {seen:?}");
    for pair in seen.windows(2) {
        let at = order.iter().position(|node| *node == pair[0]).unwrap();
        assert_eq!(pair[1], order[(at + 1) % 3], "cycle order: {seen:?}");
    }
}
