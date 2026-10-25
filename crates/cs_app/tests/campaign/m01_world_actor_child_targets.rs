//! Ground targets that are nodes of a world actor follow the actor's pose
//! (#2096). Task test prefix: `accept_m01_world_actor_child_targets_`.
//!
//! `hydrogentank1..4` are stored under `cargozep1` (`...cargozep1.move_zeppelin
//! .tilt_zeppelin.rock_zeppelin.underneath.hydrogentankN`) with a composed
//! transform in the static container that is only their offset below the
//! zeppelin (tank 1: `(15, -67, -70)`). The zeppelin is placed by the mission
//! program, so the tanks' swept boxes must stand where the zeppelin's pose puts
//! them, or no round can reach them. See
//! `docs/findings/2026-10-24-m01-world-actor-child-targets.md`.
//!
//! The retail tests need `CS_GAME_DIR`; the synthetic one runs in CI.

use bevy::prelude::{App, Vec3};
use cs_app::damage::DamageScheduleReport;
use cs_app::mission_actors::{
    MissionGroundTarget, MissionGroundTargetReport, MissionGroundTargetSweep, MissionZeppelinActor,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{build_headless, stage_for};
use cs_app::mission_targets::TargetFrame;
use cs_app::world_actors::WorldActorBinding;
use cs_content::scene::CanonicalTransform;

use crate::campaign::common::label;
use crate::campaign::m01_convoy_gun_hits::{
    MAX_FRAMES, aim_player_at, destroyed, fire, game_dir, idle,
};

const TANKS: [&str; 4] = [
    "hydrogentank1",
    "hydrogentank2",
    "hydrogentank3",
    "hydrogentank4",
];

fn retail_app() -> App {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes");
    app.update();
    idle(&mut app);
    app
}

/// Where the spawned `cargozep1` actor stands.
fn zeppelin_pose(app: &mut App) -> ([f64; 3], [f64; 4]) {
    let mut query = app
        .world_mut()
        .query::<(&MissionZeppelinActor, &WorldActorBinding)>();
    query
        .iter(app.world())
        .find(|(_, binding)| binding.subject.key().ends_with(".cargozep1"))
        .map(|(actor, _)| (actor.position_m, actor.orientation))
        .expect("cargozep1 spawned")
}

fn sweep_centre(app: &App, node: &str) -> Vec3 {
    let entity = app
        .world()
        .resource::<MissionGroundTargetReport>()
        .target(node)
        .unwrap_or_else(|| panic!("{node} spawned"))
        .entity;
    let centre = app
        .world()
        .get::<MissionGroundTargetSweep>(entity)
        .expect("sweep")
        .centre_m;
    Vec3::new(centre[0] as f32, centre[1] as f32, centre[2] as f32)
}

/// **The tanks' boxes hang under the zeppelin, not at the container offset.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_world_actor_child_targets_tanks_stand_under_the_zeppelin() {
    let mut app = retail_app();
    let (zeppelin, _) = zeppelin_pose(&mut app);
    let zeppelin = Vec3::new(zeppelin[0] as f32, zeppelin[1] as f32, zeppelin[2] as f32);
    assert!(
        zeppelin.length() > 1000.0,
        "the zeppelin is far from the origin"
    );
    for node in TANKS {
        let entity = app
            .world()
            .resource::<MissionGroundTargetReport>()
            .target(node)
            .expect("tank spawned")
            .entity;
        let target = app.world().get::<MissionGroundTarget>(entity).unwrap();
        assert_eq!(target.anchor.as_deref(), Some("cargozep1"), "{node}");
        let offset = sweep_centre(&app, node) - zeppelin;
        assert!(
            offset.length() < 200.0 && offset.y < 0.0,
            "{node} hangs below cargozep1: offset {offset:?}"
        );
    }
    // Targets that hang under no world actor keep their container pose.
    let truck = app
        .world()
        .resource::<MissionGroundTargetReport>()
        .target("t_truck01")
        .expect("truck spawned")
        .entity;
    assert_eq!(
        app.world()
            .get::<MissionGroundTarget>(truck)
            .unwrap()
            .anchor,
        None
    );
}

/// **A gun burst at `hydrogentank1` reaches its damage node.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_world_actor_child_targets_gun_burst_destroys_hydrogentank1() {
    let mut app = retail_app();
    let actor = app
        .world()
        .resource::<MissionGroundTargetReport>()
        .target("hydrogentank1")
        .expect("tank spawned")
        .actor;
    let point = sweep_centre(&app, "hydrogentank1");
    // No relation is declared here: the roster declares the tanks `Neutral`
    // toward the player and the designed M01 gun admits that (#2099).
    for _ in 0..MAX_FRAMES {
        aim_player_at(&mut app, point, Vec3::new(1.0, 0.15, 0.3));
        fire(&mut app);
        if destroyed(&app, actor) {
            return;
        }
    }
    panic!(
        "hydrogentank1 was not destroyed: {:?}",
        app.world().get_resource::<DamageScheduleReport>()
    );
}

fn transform(linear: [[f64; 3]; 3], translation: [f64; 3]) -> CanonicalTransform {
    CanonicalTransform::try_new(linear, translation).expect("finite")
}

/// **Synthetic: the re-seating composes `pose ∘ inverse(anchor) ∘ node`.**
#[test]
fn accept_m01_world_actor_child_targets_reseat_composes_through_the_anchor() {
    const IDENTITY: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    // The anchor stands at (100, 0, 0) in the container; the child 10 m below it
    // with a box 2 x 4 x 6 m.
    let frame = TargetFrame {
        world: transform(IDENTITY, [100.0, -10.0, 0.0]),
        stored_box: Some([[-1.0, -2.0, -3.0], [1.0, 2.0, 3.0]]),
        ancestors: vec![("anchor".to_owned(), transform(IDENTITY, [100.0, 0.0, 0.0]))],
    };
    let placed = frame
        .under_anchor("anchor", [1000.0, 300.0, -50.0], [0.0, 0.0, 0.0, 1.0])
        .expect("anchored");
    assert_eq!(placed.position_m, [1000.0, 290.0, -50.0]);
    assert_eq!(placed.target_point_m, [1000.0, 290.0, -50.0]);
    assert_eq!(placed.target_half_extents_m, [1.0, 2.0, 3.0]);

    // A quarter turn about +Y swaps the box's x and z extents and carries the
    // child's offset: (0, -10, 0) stays below the anchor.
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let turned = frame
        .under_anchor("anchor", [0.0, 0.0, 0.0], [0.0, half, 0.0, half])
        .expect("anchored");
    for (got, want) in turned.target_half_extents_m.iter().zip([3.0, 2.0, 1.0]) {
        assert!((got - want).abs() < 1e-9, "{got} vs {want}");
    }
    assert!((turned.position_m[1] + 10.0).abs() < 1e-9);

    assert!(
        frame
            .under_anchor("elsewhere", [0.0; 3], [0.0, 0.0, 0.0, 1.0])
            .is_none()
    );
}
