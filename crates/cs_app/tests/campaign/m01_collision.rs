//! #1851: the no-input M01 flight with contact, run from the composed hand-over
//! pose and compared with the owner's measured trajectory of the original
//! (#1752: one run of retail 1.02 under the Drydock emulator, memory reads only,
//! times virtual and read as +-1 s; never `verified_original` of anything else).
//!
//! The test pins **what main does**, not what the original does. Nothing here
//! is tuned to the measured table; each divergence is recorded in
//! `docs/findings/2026-10-23-m01-collision-comparison.md` with the task that
//! would close it. A cause task that lands moves the pins with it.
use std::path::PathBuf;

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, RigidBody, Rotation};
use bevy::prelude::*;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionPlayerBody, build_headless, stage_for, teardown};
use cs_app::playtest::PlaytestState;
use cs_app::playtest::fatal_contact::{
    CrashSequence, FailureCause, FatalContactRule, PlayerCrashed, TerrainContactJudged,
};
use cs_app::world::WorldColliderInstance;
use cs_content::world::WorldCollisionRole;

use crate::campaign::common::label;

/// Seconds after the hand-over, measured position (m) and speed (m/s, where the
/// capture gave one), from the #1752 table.
const MEASURED: [(usize, [f64; 3], Option<f64>); 11] = [
    (0, [-1426.0, 150.0, -1813.0], Some(111.8)),
    (10, [-2014.8, 102.6, -2514.7], Some(113.6)),
    (20, [-2743.8, 46.3, -3383.5], Some(113.6)),
    (28, [-3327.1, 1.2, -4078.6], Some(113.6)),
    (30, [-3470.5, 7.2, -4250.0], Some(111.0)),
    (34, [-3749.3, 27.5, -4593.4], Some(110.4)),
    (36, [-3839.8, 105.3, -4718.8], Some(78.5)),
    (40, [-3883.0, 373.9, -4851.8], Some(75.8)),
    (60, [-3993.7, 1608.7, -5940.9], Some(94.6)),
    (70, [-3899.5, 1925.0, -6902.7], Some(111.2)),
    (80, [-3634.7, 1707.9, -8014.6], Some(123.2)),
];

const SECONDS: usize = 80;

/// One sample per simulated second: position, speed, ground and obstacle
/// contact counts, resets, and the contacts the fatal rule judged so far.
struct Sample {
    p: Vec3,
    speed: f32,
    ground: u64,
    obstacle: u64,
    resets: u32,
    judged: u64,
    hull_damage: f64,
    crashed: bool,
}

fn fly() -> Vec<Sample> {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    let mut samples = Vec::new();
    let mut judged = 0_u64;
    let mut hull_damage = 0.0_f64;
    for f in 0..=SECONDS * 60 {
        app.update();
        for message in app
            .world()
            .resource::<Messages<TerrainContactJudged>>()
            .iter_current_update_messages()
        {
            judged += 1;
            hull_damage += message.hull_damage;
        }
        if f % 60 == 0 {
            let e = app
                .world_mut()
                .query_filtered::<Entity, With<MissionPlayerBody>>()
                .single(app.world())
                .expect("the player body survives the whole flight");
            let entity = app.world().entity(e);
            let p = entity.get::<avian3d::prelude::Position>().unwrap().0;
            let v = entity.get::<avian3d::prelude::LinearVelocity>().unwrap().0;
            let state = app.world().resource::<PlaytestState>();
            samples.push(Sample {
                p,
                speed: v.length(),
                ground: state.ground_contacts,
                obstacle: state.obstacle_contacts,
                resets: state.resets,
                judged,
                hull_damage,
                crashed: app.world().contains_resource::<CrashSequence>(),
            });
        }
    }
    teardown(&mut app);
    samples
}

/// The no-input flight from the hand-over pose, second by second, against the
/// measured table, and the pins of what main does today.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_m01_collision_no_input_trajectory_is_pinned() {
    let s = fly();
    let mut report = String::new();
    for (t, m, speed) in MEASURED {
        let o = &s[t];
        let d = ((o.p.x as f64 - m[0]).powi(2)
            + (o.p.y as f64 - m[1]).powi(2)
            + (o.p.z as f64 - m[2]).powi(2))
        .sqrt();
        report += &format!(
            "t={t:>2} ours=({:.1}, {:.1}, {:.1}) {:.1} m/s  measured=({:.1}, {:.1}, {:.1}) {} m/s  off={d:.0} m  ground={} obstacle={} judged={} hull={:.0} crashed={}\n",
            o.p.x,
            o.p.y,
            o.p.z,
            o.speed,
            m[0],
            m[1],
            m[2],
            speed.map_or("-".into(), |v| format!("{v:.1}")),
            o.ground,
            o.obstacle,
            o.judged,
            o.hull_damage,
            o.crashed
        );
    }
    println!("{report}");
    let at = |t: usize| &s[t];

    // The hand-over is the measured pose (within the first tick's motion), but
    // not the measured speed: main starts at the declared 55 m/s development
    // value, the original at 111.8 m/s (#1746 spawn speed; unmet).
    assert!((at(0).p.y - 150.0).abs() < 0.1, "{report}");
    assert!((at(0).speed - 55.5).abs() < 1.5, "{report}");
    assert!(
        (at(0).speed - 111.8).abs() > 40.0,
        "spawn speed now matches the original: move this pin and the finding"
    );
    // Free flight: main flies level at 150 m and 100.5 m/s for 22 s; the
    // original sinks 5.6 m/s to 46.3 m at t=20 at 113.6 m/s (#1746 sink and
    // thrust; unmet). Same heading though: x and z are in the same direction.
    for t in 5..=20 {
        assert!((at(t).p.y - 150.0).abs() < 0.5, "t={t}: {report}");
        assert!((at(t).speed - 100.5).abs() < 1.5, "t={t}: {report}");
    }
    assert!(at(20).p.y - 46.3 > 100.0, "{report}");
    // The original is on the sea at t=28; main is 148 m up. Main flies into
    // the island's hillside at t=40..42 instead (#1814: the scenery below the
    // world's grouping nodes is solid now; before, nothing there collided and
    // the flight went on to dive onto the sea at t=55). The original has its
    // own beach impact at t=34..36 (110 -> 78 m/s).
    assert!(at(28).p.y > 140.0, "{report}");
    assert_eq!(
        (at(40).judged, at(41).judged, at(42).judged),
        (0, 1, 2),
        "{report}"
    );
    assert!(at(40).speed > 99.0 && at(42).speed < 70.0, "{report}");
    // The bounce throws main up and it keeps climbing; it is never reset.
    assert!(at(SECONDS).resets == 0 && at(41).resets == 0, "{report}");
    assert!(
        at(60).p.y > at(42).p.y && at(SECONDS).p.y > at(60).p.y,
        "{report}"
    );
    assert!(at(SECONDS).judged > 0, "no judged contact at all: {report}");
    // After the hillside the original reaches 1925 m at t=70 (the 60 degree
    // throw is unmet: the response is a normal impulse); main climbs to 567 m.
    let ceiling = s.iter().map(|x| x.p.y).fold(f32::MIN, f32::max);
    assert!(ceiling < 1000.0, "ceiling {ceiling}: {report}");
    assert!(at(70).speed > 80.0, "{report}");
    for (t, m, _) in MEASURED.iter().filter(|(t, ..)| *t >= 60) {
        assert!(
            (at(*t).p.y as f64 - m[1]).abs() > 900.0,
            "t={t} now agrees with the measured altitude: {report}"
        );
    }
    // PARITY4-CRASH-E.01 (#1983): the contacts are judged and do damage. Two
    // episodes at the hillside (t=41, 42) spend 455 of the designed 1200 hull
    // (the designed crash-to-hull curve, #2005) and nothing after; the pool
    // holds, so the flight is never crashed.
    assert_eq!(
        (at(60).judged, at(70).judged, at(SECONDS).judged),
        (2, 2, 2),
        "{report}"
    );
    assert!((at(SECONDS).hull_damage - 455.0).abs() < 1.0, "{report}");
    assert!(s.iter().all(|x| !x.crashed), "{report}");
}

/// PARITY4-CRASH-E.01 (#1983): in the composed mission the aircraft's terrain
/// is the imported world's own colliders — one `WorldColliderInstance`
/// `Solid` mesh body at the origin carries the island, the plateau and the
/// sea the no-input flight skims. A dive into it is judged like any other
/// terrain contact and, once the designed hull pool is spent, ends in the
/// crash sequence; before the terrain filter named world colliders, the same
/// contacts were silent bounces (the #1857 logging, finding
/// `2026-10-23-crash-e-01-world-collider-is-terrain.md`).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_crash_e1_a_dive_into_the_world_collider_is_judged_and_fatal() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    for _ in 0..10 {
        app.update();
    }
    let player = app
        .world_mut()
        .query_filtered::<Entity, With<MissionPlayerBody>>()
        .single(app.world())
        .expect("the player body");

    // The placement the #1857 crash smoke makes, steeper: same point lifted
    // 400 m over the sea, same heading, the nose put straight down and the
    // original's nose-down key (ArrowUp, flight command 1) held so the law
    // keeps the dive instead of levelling the stickless aircraft off.
    let mut entity = app.world_mut().entity_mut(player);
    let from = entity.get::<Position>().unwrap().0;
    let (yaw, _, _) = entity.get::<Rotation>().unwrap().0.to_euler(EulerRot::YXZ);
    let dive = Quat::from_euler(EulerRot::YXZ, yaw, -std::f32::consts::FRAC_PI_2, 0.0);
    entity.get_mut::<Position>().unwrap().0 = Vec3::new(from.x, 400.0, from.z);
    entity.get_mut::<Rotation>().unwrap().0 = dive;
    entity.get_mut::<Transform>().unwrap().rotation = dive;
    entity.get_mut::<LinearVelocity>().unwrap().0 = dive * Vec3::NEG_Z * 110.0;
    entity.get_mut::<AngularVelocity>().unwrap().0 = Vec3::ZERO;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowUp);

    let mut judged = Vec::new();
    let mut crashed_msgs = Vec::new();
    let mut frames = 0;
    while frames < 3600 && !app.world().contains_resource::<CrashSequence>() {
        app.update();
        frames += 1;
        judged.extend(
            app.world()
                .resource::<Messages<TerrainContactJudged>>()
                .iter_current_update_messages()
                .copied(),
        );
        crashed_msgs.extend(
            app.world()
                .resource::<Messages<PlayerCrashed>>()
                .iter_current_update_messages()
                .copied(),
        );
    }

    assert!(
        !judged.is_empty(),
        "a dive into the world's own collider was never judged a terrain \
         contact after {frames} frames"
    );
    assert!(
        judged.iter().all(|j| j.aircraft == player),
        "a judged contact names another aircraft: {judged:?}"
    );
    let last = judged.last().unwrap();
    assert!(
        last.fatal,
        "the contacts did not empty the hull pool: {judged:?}"
    );
    assert!(
        app.world().contains_resource::<CrashSequence>(),
        "no crash sequence after {frames} frames ({} contacts judged)",
        judged.len()
    );
    assert_eq!(crashed_msgs.len(), 1, "one crash message: {crashed_msgs:?}");

    // The fatal contact names the world collider itself, not a playtest slab,
    // and the message's record is the one stamped on the aircraft.
    let crash = crashed_msgs[0].crash;
    assert_eq!(
        app.world().get::<FatalContactRule>(player).unwrap().crash,
        Some(crash)
    );
    let FailureCause::TerrainContact {
        terrain, severity, ..
    } = crash.cause;
    assert!(severity > 0.0);
    let collider = app
        .world()
        .get::<WorldColliderInstance>(terrain)
        .expect("the fatal terrain is an imported world collider");
    assert_eq!(collider.role(), WorldCollisionRole::Solid);
    assert_eq!(
        *app.world().get::<RigidBody>(terrain).unwrap(),
        RigidBody::Static
    );
    teardown(&mut app);
}
