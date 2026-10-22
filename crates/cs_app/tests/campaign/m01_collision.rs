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

use bevy::prelude::*;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionPlayerBody, build_headless, stage_for, teardown};
use cs_app::playtest::PlaytestState;

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
/// contact counts, resets.
struct Sample {
    p: Vec3,
    speed: f32,
    ground: u64,
    obstacle: u64,
    resets: u32,
}

fn fly() -> Vec<Sample> {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    let mut samples = Vec::new();
    for f in 0..=SECONDS * 60 {
        app.update();
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
            "t={t:>2} ours=({:.1}, {:.1}, {:.1}) {:.1} m/s  measured=({:.1}, {:.1}, {:.1}) {} m/s  off={d:.0} m  ground={} obstacle={}\n",
            o.p.x,
            o.p.y,
            o.p.z,
            o.speed,
            m[0],
            m[1],
            m[2],
            speed.map_or("-".into(), |v| format!("{v:.1}")),
            o.ground,
            o.obstacle
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
    // The original is on the sea at t=28; main is 148 m up and does not touch
    // the sea until t=55 (the dive after the hill at t=50..54).
    assert!(at(28).p.y > 140.0, "{report}");
    assert!(at(54).p.y > 20.0 && at(55).p.y < 5.0, "{report}");
    // From then on the sea holds the body: it skims at ~1 m (the sea contact
    // of #1850), never through it, and it is never reset.
    for t in 55..=SECONDS {
        assert!(at(t).p.y > -3.0, "t={t} below the sea: {report}");
    }
    assert!((at(60).p.y - 1.1).abs() < 0.5, "{report}");
    assert!(at(SECONDS).resets == 0 && at(55).resets == 0, "{report}");
    // Contacts: main reports some, so the contact source is live.
    assert!(
        at(SECONDS).ground + at(SECONDS).obstacle > 0,
        "no contact at all: {report}"
    );
    // Main never climbs: the original reaches 1925 m at t=70 after the beach
    // impact (the response is a normal impulse, no 60 degree throw; unmet).
    let ceiling = s.iter().map(|x| x.p.y).fold(f32::MIN, f32::max);
    assert!(ceiling < 200.0, "ceiling {ceiling}: {report}");
    // It is stopped by an obstacle at t=62 and ends the window almost at rest
    // at t=70 on the way to the island, 3 km from the original's t=70 position.
    assert!(at(70).speed < 5.0, "{report}");
    for (t, m, _) in MEASURED.iter().filter(|(t, ..)| *t >= 60) {
        assert!(
            (at(*t).p.y as f64 - m[1]).abs() > 1000.0,
            "t={t} now agrees with the measured altitude: {report}"
        );
    }
}
