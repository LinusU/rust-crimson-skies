//! #1746: the composed M01 hand-over pose and the no-input flight from it,
//! against the trajectory the owner measured from one run of the original
//! (retail 1.02 under the Drydock emulator, memory reads only; the times are
//! virtual and read as +-1 s). It is an original RUN of that one
//! configuration, never `verified_original` of any other.
//!
//! The measured table is a committed fact of the task description; the
//! divergences it exposes are recorded in
//! `docs/findings/2026-10-21-m01-hand-over-pose.md` with one task per cause.
use std::path::PathBuf;

use cs_app::mission_actors::MissionAircraftRoster;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::stage_for;
use cs_sim::flight::original::{OriginalInput, OriginalState};
use cs_types::space::{Quaternion, Radians, UnitVec3};

use crate::campaign::common::label;

/// Whole seconds after the hand-over and the measured world position, metres.
const MEASURED: [(u32, [f64; 3]); 11] = [
    (0, [-1426.0, 150.0, -1813.0]),
    (1, [-1428.1, 149.8, -1815.5]),
    (2, [-1459.0, 146.3, -1852.3]),
    (3, [-1513.3, 141.6, -1917.1]),
    (4, [-1579.9, 136.3, -1996.4]),
    (5, [-1651.0, 130.8, -2081.1]),
    (6, [-1723.4, 125.2, -2167.4]),
    (10, [-2014.8, 102.6, -2514.7]),
    (15, [-2379.3, 74.5, -2949.1]),
    (20, [-2743.8, 46.3, -3383.5]),
    (22, [-2889.6, 35.0, -3557.3]),
];

/// The throttle the intro placement leaves (`launch_body::INTRO_THROTTLE`,
/// static): the hand-over setting is not itself measured.
const HAND_OVER_THROTTLE: f64 = 0.4;
/// The +-1 s the virtual clock is read to.
const TIME_UNCERTAINTY_S: f64 = 1.0;
const DT: f64 = 1.0 / 60.0;

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
}

/// Our trajectory from the composed hand-over pose, one entry per tick.
fn fly_from_hand_over(root: &std::path::Path) -> (Vec<OriginalState>, [f32; 3]) {
    let plan = plan_mission_launch(root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(root, &plan).expect("stage");
    let orientation =
        Quaternion::from_axis_angle(UnitVec3::UP, Radians(f64::from(stage.start.heading)))
            .expect("a yaw is a unit quaternion");
    // The original's body is almost at rest at the hand-over (3 m in the
    // first second), so no start speed is declared here.
    let mut state = OriginalState {
        position_m: stage.start.position.map(f64::from),
        velocity_mps: [0.0; 3],
        orientation,
        angular_momentum_world: [0.0; 3],
        throttle: HAND_OVER_THROTTLE,
        fuel: stage.flight.fuel,
        level_off: false,
    };
    let input = OriginalInput {
        throttle: HAND_OVER_THROTTLE,
        ..OriginalInput::default()
    };
    let mut ticks = vec![state.clone()];
    for _ in 0..22 * 60 {
        stage
            .flight
            .model
            .step(&mut state, input, DT)
            .expect("the law steps");
        ticks.push(state.clone());
    }
    (ticks, stage.start.position)
}

/// The hand-over pose is the measured one, altitude included.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_m01_hand_over_pose_is_the_measured_one() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    assert_eq!(stage.start.position, [-1426.0, 150.0, -1813.0]);
    assert!((stage.start.heading.to_degrees() - 40.0).abs() < 1e-4);
}

/// The no-input flight keeps the original's heading, and the divergences the
/// finding records are pinned so that fixing a cause shows up here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_m01_free_flight_against_the_measured_trajectory() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let (ticks, start) = fly_from_hand_over(&root);
    assert_eq!(ticks[0].position_m, start.map(f64::from));

    let mut report = String::new();
    let mut matched = 0;
    for (t, measured) in MEASURED.iter() {
        let centre = (*t as f64 * 60.0) as i64;
        let reach = (TIME_UNCERTAINTY_S * 60.0) as i64;
        let best = (centre - reach).max(0)..=(centre + reach).min(22 * 60);
        let nearest = best
            .map(|i| distance(ticks[i as usize].position_m, *measured))
            .fold(f64::INFINITY, f64::min);
        let at = ticks[(*t * 60) as usize].position_m;
        report += &format!(
            "t={t:>2} ours=({:.1}, {:.1}, {:.1}) measured=({:.1}, {:.1}, {:.1}) nearest-within-1s={nearest:.1} m\n",
            at[0], at[1], at[2], measured[0], measured[1], measured[2]
        );
        if nearest <= 3.0 {
            matched += 1;
        }
    }
    println!("{report}");

    // Heading: the measured displacement bearing is atan2(dx, dz) of
    // (-72.9, -86.9); ours must head the same way once under way.
    let measured_bearing = (-72.9_f64).atan2(-86.9).to_degrees();
    let last = &ticks[22 * 60];
    let from = &ticks[10 * 60];
    let ours = (last.position_m[0] - from.position_m[0])
        .atan2(last.position_m[2] - from.position_m[2])
        .to_degrees();
    assert!(
        (ours - measured_bearing).abs() < 1.0,
        "bearing {ours} vs {measured_bearing}"
    );

    // Recorded divergence (finding 2026-10-21-m01-hand-over-pose.md): from
    // rest the law has no lift, so the airframe falls instead of sinking
    // 5.6 m/s; only the first two samples agree today. When a cause task
    // lands this count rises and the pin moves with it.
    assert_eq!(matched, 2, "{report}");
    assert!(
        last.position_m[1] < 0.0,
        "ours falls below the sea, the original is at 35 m"
    );
}

/// The other airplane objects at the hand-over, read from the original run by
/// the owner's tool (#1753): the wingman, then the three Kestrels (names are
/// inferred from the roster; the run read positions only).
const WINGMAN_AT_HAND_OVER: [f64; 3] = [-1378.0, 150.0, -1706.0];
const KESTRELS_AT_HAND_OVER: [[f64; 3]; 3] = [
    [-5615.0, 300.0, -8134.0],
    [-5548.0, 300.0, -8296.0],
    [-5680.0, 300.0, -8246.0],
];
/// The measured positions are whole metres.
const POSITION_TOLERANCE_M: f64 = 1.0;

/// At hand-over the composed roster is exactly the four airplanes the original
/// run lists besides the player, at the measured positions: the wingman 117 m
/// from the player, the three Kestrels 7.6-7.7 km away and asleep.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_m01_actor_placement_at_hand_over_is_the_measured_one() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let player = stage.start.position.map(f64::from);
    let roster = MissionAircraftRoster::from_records(&stage.aircraft).0;
    assert_eq!(
        roster.len(),
        4,
        "the original run lists four other airplanes"
    );
    let position_of = |record: &cs_app::mission_start::ScriptedAircraft| {
        record
            .stored_pose
            .expect("a stored pose")
            .position_metres()
            .map(f64::from)
    };
    let wingman = roster
        .iter()
        .find(|record| record.name == "wingman_1")
        .expect("wingman_1");
    assert!(distance(position_of(wingman), WINGMAN_AT_HAND_OVER) < POSITION_TOLERANCE_M);
    let gap = distance(position_of(wingman), player);
    assert!(
        (gap - 117.0).abs() < 1.0,
        "wingman is {gap} m from the player"
    );
    assert_ne!(wingman.starts_asleep, Some(true));

    let kestrels: Vec<_> = roster
        .iter()
        .filter(|record| record.name.starts_with("medkestrel_"))
        .collect();
    assert_eq!(kestrels.len(), 3);
    for measured in KESTRELS_AT_HAND_OVER {
        assert!(
            kestrels
                .iter()
                .any(|record| distance(position_of(record), measured) < POSITION_TOLERANCE_M),
            "no Kestrel record at {measured:?}"
        );
    }
    for record in kestrels {
        let gap = distance(position_of(record), player);
        assert!(
            (7500.0..7800.0).contains(&gap),
            "{} is {gap} m away",
            record.name
        );
        assert_eq!(
            record.starts_asleep,
            Some(true),
            "{} is dormant",
            record.name
        );
    }
}

/// The sea is a contact source (#1850): a body just above M01's water at the
/// measured cruise sink angle (nose 2.8 deg down, 113.5 m/s) is held by the sea
/// surface and neither passes through it nor is destroyed by it.
///
/// The water tiles are the world's own (`wtr00000`, y = 0), colliding because
/// their stored node flags carry the bits the original's contact sweep tests
/// (`docs/findings/2026-10-22-m01-sea-contact.md`).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_m01_sea_contact_a_low_body_skims_the_sea() {
    use bevy::prelude::*;
    use cs_app::mission_session::{MissionPlayerBody, MissionWorldParts, build_headless, teardown};
    use cs_app::playtest::PlaytestState;
    use cs_app::playtest::scene::PlaytestSea;

    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    for _ in 0..3 {
        app.update();
    }
    let seas = app
        .world_mut()
        .query_filtered::<&Transform, With<PlaytestSea>>()
        .iter(app.world())
        .count();
    assert!(seas > 0, "the world's water tiles are colliders");

    // The widest flat tile at y = 0: the point the body is aimed over.
    let parts = app.world().resource::<MissionWorldParts>();
    let scenery = parts.scenery().expect("scenery");
    let mut best: Option<(f32, Vec3)> = None;
    for placement in &scenery.placements {
        if placement.node_flags & cs_app::playtest_retail::CONTACT_SWEEP_NODE_FLAGS
            != cs_app::playtest_retail::CONTACT_SWEEP_NODE_FLAGS
        {
            continue;
        }
        let Some(collision) = scenery.colliders.get(&placement.mesh) else {
            continue;
        };
        let world: Vec<Vec3> = collision
            .vertices()
            .iter()
            .map(|v| placement.global.transform_point(Vec3::from(*v)))
            .collect();
        if world.is_empty() || world.iter().any(|v| v.y.abs() > 0.01) {
            continue;
        }
        let (mut lo, mut hi) = (world[0], world[0]);
        for v in &world {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        let size = (hi - lo).min_element().max(0.0) + (hi - lo).x.min((hi - lo).z);
        if best.is_none_or(|(s, _)| size > s) && (hi - lo).x > 200.0 && (hi - lo).z > 200.0 {
            best = Some((size, (lo + hi) * 0.5));
        }
    }
    let (_, centre) = best.expect("a flat water tile at least 200 m across");

    let angle = 2.8_f32.to_radians();
    let speed = 113.5_f32;
    let start = Vec3::new(centre.x - 10.0, 1.0, centre.z);
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(app.world())
        .expect("the player body");
    {
        let mut entity = app.world_mut().entity_mut(player);
        *entity.get_mut::<avian3d::prelude::Position>().unwrap() =
            avian3d::prelude::Position(start);
        *entity.get_mut::<Transform>().unwrap() =
            Transform::from_translation(start).with_rotation(Quat::from_rotation_z(-angle));
        *entity.get_mut::<avian3d::prelude::Rotation>().unwrap() =
            avian3d::prelude::Rotation(Quat::from_rotation_z(-angle));
        *entity
            .get_mut::<avian3d::prelude::LinearVelocity>()
            .unwrap() = avian3d::prelude::LinearVelocity(Vec3::new(
            speed * angle.cos(),
            -speed * angle.sin(),
            0.0,
        ));
    }
    let mut lowest = f32::INFINITY;
    for _ in 0..30 {
        app.update();
        let y = app
            .world()
            .entity(player)
            .get::<avian3d::prelude::Position>()
            .expect("the player body survives the contact")
            .0
            .y;
        lowest = lowest.min(y);
    }
    let state = app.world().resource::<PlaytestState>();
    assert!(
        state.ground_contacts > 0,
        "the sea reported no contact (lowest y {lowest})"
    );
    assert!(
        lowest > -3.0,
        "the body passed through the sea (lowest y {lowest})"
    );
    assert_eq!(state.resets, 0, "the contact did not reset the flight");
    teardown(&mut app);
}
