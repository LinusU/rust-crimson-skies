//! #2246: the M01 no-input flight and a gentle pull-up, per tick from the
//! hand-over to t = 10 s, with the ground blow's amount and the probe's hit.
//!
//! A measurement: it pins nothing about the spawn. It runs the composed
//! mission on whatever spawn the checkout has and prints one row per tick; the
//! finding `docs/findings/2026-11-01-m01-release-spawn-with-groundblow.md`
//! holds the two traces (main's declared start, and #1845's release state).
//! The ground blow's amount is recomputed from the probe's hit with the
//! law's own formula (`(elev - chord) / elev * sqrt(d)`), because the law
//! does not report it.
use std::path::PathBuf;

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::prelude::*;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionPlayerBody, build_headless, stage_for, teardown};
use cs_app::playtest::PlaytestState;
use cs_app::playtest::fatal_contact::TerrainContactJudged;
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_app::playtest::world_sweep::GroundBlowProbes;

use crate::campaign::common::label;

const TICKS: usize = 10 * 60;

/// One tick's row.
struct Row {
    tick: usize,
    altitude_m: f32,
    speed_m_s: f32,
    vertical_m_s: f32,
    pitch_deg: f32,
    throttle: f64,
    hit: bool,
    chord_m: f32,
    blow_amount: f64,
    ground: u64,
    obstacle: u64,
    judged: u64,
}

/// Flies `TICKS` ticks from the hand-over with the player's pitch command
/// `pitch` (and the declared throttle command untouched).
fn fly(pitch: f64) -> Vec<Row> {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plan");
    let stage = stage_for(&root, &plan).expect("stage");
    let mut app = build_headless(&stage).expect("M01 composes");
    // The session's held command is copied onto the record before every
    // frame's fixed loop (`apply_flight_command`); a fixed tick's first system
    // runs after it, so the pitch it states is the one the law flies.
    app.add_systems(
        FixedFirst,
        move |mut flights: Query<&mut PlaytestOriginalFlight, With<MissionPlayerBody>>| {
            for mut flight in &mut flights {
                flight.command.pitch = pitch;
            }
        },
    );
    let mut rows = Vec::new();
    let mut judged = 0_u64;
    for tick in 0..=TICKS {
        app.update();
        judged += app
            .world()
            .resource::<Messages<TerrainContactJudged>>()
            .iter_current_update_messages()
            .count() as u64;
        let entity = app
            .world_mut()
            .query_filtered::<Entity, With<MissionPlayerBody>>()
            .single(app.world())
            .expect("the player body survives");
        let world = app.world();
        let e = world.entity(entity);
        let p = e.get::<Position>().unwrap().0;
        let v = e.get::<LinearVelocity>().unwrap().0;
        let rotation = e.get::<Rotation>().unwrap().0;
        let flight = e.get::<PlaytestOriginalFlight>().unwrap();
        let forward = rotation * Vec3::NEG_Z;
        let back = rotation * Vec3::Z;
        let hit = world.resource::<GroundBlowProbes>().hit(entity);
        let elev = flight.model.globals.groundblow_elev_m;
        let (chord_m, blow_amount) = hit.map_or((0.0, 0.0), |h| {
            let d = f64::from(back.x) * h.normal[0]
                + f64::from(back.y) * h.normal[1]
                + f64::from(back.z) * h.normal[2];
            let amount = if d > 0.0 {
                (elev - h.distance_m) / elev * d.sqrt()
            } else {
                0.0
            };
            (h.distance_m as f32, amount)
        });
        let state = world.resource::<PlaytestState>();
        rows.push(Row {
            tick,
            altitude_m: p.y,
            speed_m_s: v.length(),
            vertical_m_s: v.y,
            pitch_deg: forward.y.clamp(-1.0, 1.0).asin().to_degrees(),
            throttle: flight.state.throttle,
            hit: hit.is_some(),
            chord_m,
            blow_amount,
            ground: state.ground_contacts,
            obstacle: state.obstacle_contacts,
            judged,
        });
    }
    teardown(&mut app);
    rows
}

fn report(name: &str, rows: &[Row]) -> String {
    let mut out = format!("== {name} ==\n");
    out += "tick  alt_m   speed  vert   pitch  thr  hit chord  blow    ground obst judged\n";
    for r in rows {
        out += &format!(
            "{:>4} {:>7.1} {:>6.1} {:>6.1} {:>6.1} {:>4.2}  {}  {:>6.1} {:>6.3} {:>5} {:>5} {:>5}\n",
            r.tick,
            r.altitude_m,
            r.speed_m_s,
            r.vertical_m_s,
            r.pitch_deg,
            r.throttle,
            u8::from(r.hit),
            r.chord_m,
            r.blow_amount,
            r.ground,
            r.obstacle,
            r.judged
        );
    }
    let first = rows
        .iter()
        .find(|r| r.ground + r.obstacle > 0 || r.judged > 0);
    out += &match first {
        Some(r) => format!("first contact: tick {} at {:.1} m\n", r.tick, r.altitude_m),
        None => "no terrain or sea contact within 10 s\n".to_owned(),
    };
    out
}

/// The per-tick trace of the idle player and of a gentle pull-up (pitch
/// command 0.2) from the hand-over to t = 10 s.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_release_groundblow_trace_from_hand_over_to_ten_seconds() {
    let idle = fly(0.0);
    let pull = fly(0.2);
    println!("{}", report("no input", &idle));
    println!("{}", report("pitch command 0.2", &pull));
    for rows in [&idle, &pull] {
        assert_eq!(rows.len(), TICKS + 1);
        assert!(rows.iter().all(|r| r.altitude_m.is_finite()));
    }
    // The probe is cast every tick for the player (#2214).
    assert!(idle.iter().any(|r| r.hit || r.altitude_m < 400.0));
}
