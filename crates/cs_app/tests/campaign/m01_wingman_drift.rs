//! M01-WINGMAN-DRIFT-DECOMPOSITION (#2295): where the `original_stick`
//! hillside drift comes from.
//!
//! Task test prefix: `accept_m01_wingman_drift_`. The trace is test-side only
//! (it reads the committed poses and the flight command); the analysis is in
//! `docs/findings/2026-11-02-m01-wingman-gains.md` §10.

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::prelude::{App, Entity, Vec3, With};
use bevy::time::{Time, TimeUpdateStrategy};

use cs_app::mission_actors::ScriptedAircraftActor;
use cs_app::mission_ai::{
    M01_WINGMAN_PILOT_GAINS, MissionAircraftAi, STATION_OFFSET_PLAYER_M, station_point,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionPlayerBody, build_headless, stage_for, teardown};
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::playtest::scene::PlaytestOriginalFlight;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

/// One trace row: slot error on the leader's axes, speeds, the stick words
/// and throttle as commanded, and the follower's latches.
#[derive(Debug, Clone, Copy)]
struct Row {
    t: f32,
    along: f32,
    cross: f32,
    vert: f32,
    gap: f32,
    own_speed: f32,
    leader_speed: f32,
    roll: f64,
    pitch: f64,
    yaw: f64,
    throttle: f64,
    engaged: bool,
    climbing: bool,
}

fn pose(app: &App, entity: Entity) -> (Vec3, bevy::prelude::Quat, Vec3) {
    let w = app.world();
    (
        w.get::<Position>(entity).expect("pose").0,
        w.get::<Rotation>(entity).expect("rotation").0,
        w.get::<LinearVelocity>(entity).expect("velocity").0,
    )
}

fn fly_trace(use_stick: bool) -> Vec<Row> {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plans");
    let stage = stage_for(&root, &plan).expect("stage reads");
    let mut app = build_headless(&stage).expect("composes");
    app.update();
    {
        let world = app.world_mut();
        for mut ai in world.query::<&mut MissionAircraftAi>().iter_mut(world) {
            ai.original_stick = use_stick;
            ai.pilot_gains = M01_WINGMAN_PILOT_GAINS;
        }
    }
    let player = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<MissionPlayerBody>>()
            .iter(world)
            .next()
            .expect("player")
    };
    let betty = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ScriptedAircraftActor)>()
            .iter(world)
            .find(|(_, actor)| actor.record.name == "wingman_1")
            .map(|(entity, _)| entity)
            .expect("wingman_1")
    };
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let hz = u64::from(BASELINE_FIXED_HZ);
    let mut rows = Vec::new();
    for tick in 1..=45 * hz {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        if tick % (hz / 2) != 0 {
            continue;
        }
        let (b, _, bv) = pose(&app, betty);
        let (p, po, pv) = pose(&app, player);
        let slot = station_point(p, po, STATION_OFFSET_PLAYER_M);
        let err = slot - b;
        let forward = if pv.length() > 0.0 {
            pv / pv.length()
        } else {
            po * Vec3::NEG_Z
        };
        let along = err.dot(forward);
        let vert = err.y;
        let cross = (err.length_squared() - along * along - vert * vert)
            .max(0.0)
            .sqrt();
        let ai = app.world().get::<MissionAircraftAi>(betty).expect("law");
        let cmd = app
            .world()
            .get::<PlaytestOriginalFlight>(betty)
            .expect("flight")
            .command;
        rows.push(Row {
            t: tick as f32 / hz as f32,
            along,
            cross,
            vert,
            gap: (b - p).length(),
            own_speed: bv.length(),
            leader_speed: pv.length(),
            roll: cmd.roll,
            pitch: cmd.pitch,
            yaw: cmd.yaw,
            throttle: cmd.throttle,
            engaged: ai.follow_engaged,
            climbing: ai.escape.climbing,
        });
    }
    teardown(&mut app);
    rows
}

/// Prints the 0.5 s trace of the hillside flight with `original_stick` on
/// (`CS_DRIFT_TRACE=1`); not a pin on the outcome.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_wingman_drift_trace_hillside() {
    let rows = fly_trace(true);
    assert_eq!(rows.len(), 90);
    if std::env::var_os("CS_DRIFT_TRACE").is_some() {
        for r in &rows {
            let sat = |v: f64| if v.abs() >= 0.999 { "S" } else { "." };
            println!(
                "t={:5.1} along={:7.1} cross={:7.1} vert={:7.1} gap={:7.1} v={:6.1}/{:6.1} \
                 r={:+.2}{} p={:+.2}{} y={:+.2}{} thr={:.2} eng={} esc={}",
                r.t,
                r.along,
                r.cross,
                r.vert,
                r.gap,
                r.own_speed,
                r.leader_speed,
                r.roll,
                sat(r.roll),
                r.pitch,
                sat(r.pitch),
                r.yaw,
                sat(r.yaw),
                r.throttle,
                u8::from(r.engaged),
                u8::from(r.climbing),
            );
        }
    }
}

/// With the M01 wingman's merged gains the follower's pitch command is zero
/// for any vertical error, and the escape climb's emergency words are the
/// only gains that give it a pitch (`wingman_stick`'s `+0x108` word goes
/// through the pitch scale `+0x8c8`, which `mode_alt` = 0.0 sets).
#[test]
fn accept_m01_wingman_drift_pitch_is_dead_without_the_escape_words() {
    use bevy::prelude::Quat;
    use cs_app::mission_ai::{PILOT_GAINS_AI_INPUT_KEYS, wingman_stick_command};
    let level = Quat::IDENTITY;
    // The goal 85 m straight above and 200 m ahead: the vertical error the
    // trace shows at 22 s.
    let above = Vec3::new(0.0, 85.0, -200.0);
    let words = wingman_stick_command(level, above, 105.0, 0.2);
    assert!(
        words.a108 > 0.0,
        "the law itself asks for nose-up: {words:?}"
    );
    let merged = words.flight_input_with(1.0, M01_WINGMAN_PILOT_GAINS);
    assert_eq!(merged.pitch, 0.0, "the merged pitch scale zeroes it");
    let escape = words.flight_input_with(1.0, PILOT_GAINS_AI_INPUT_KEYS);
    assert!(
        escape.pitch > 0.5,
        "the emergency words pass it: {escape:?}"
    );
}

/// The image windows the dead-pitch conclusion rests on
/// (`docs/findings/2026-11-02-m01-wingman-gains.md` §10).
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_drift_pitch_scale_windows_match_the_image() {
    use cs_app::engine_facts::engine_facts;
    use cs_content::coordinates::load_engine_image;
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let facts = engine_facts().expect("the committed table parses");
    let at = |va: u64| usize::try_from(facts.image_offset(va).expect("va in image")).expect("off");
    let window = |va: u64, bytes: &[u8]| {
        let i = at(va);
        assert_eq!(
            &loaded.bytes[i..i + bytes.len()],
            bytes,
            "window at {va:#x}"
        );
    };
    // The key text and the parser's store: `mode_alt` -> def+0xa8.
    window(0x6280b8, b"mode_alt\0");
    window(0x47b08e, &[0x68, 0xb8, 0x80, 0x62, 0x00]);
    window(0x47b0a6, &[0x89, 0x87, 0xa8, 0x00, 0x00, 0x00]);
    // The constructor's default for the same word is 2.0 (overwritten above).
    window(0x478b10, &[0xbf, 0x00, 0x00, 0x00, 0x40]);
    // The spawn copy: first source def+0xa8 unless -1.0, store vehicle+0x8c8.
    window(0x47d1e9, &[0x8b, 0x83, 0xa8, 0x00, 0x00, 0x00]);
    window(0x47d1ef, &[0xb9, 0x00, 0x00, 0x80, 0xbf]);
    window(0x47d1fb, &[0x89, 0x86, 0xc8, 0x08, 0x00, 0x00]);
    // Step 7 of the law: the pitch word +0x108 is multiplied by +0x8c8.
    window(0x41be80, &[0xd9, 0x86, 0xc8, 0x08, 0x00, 0x00]);
    window(0x41be97, &[0xd8, 0x8e, 0x08, 0x01, 0x00, 0x00]);
    // No other store of `+0x8c8` in `.text`: the only two are the spawn copy's.
    let text = &loaded.bytes[at(0x401000)..at(0x401000) + 0x2014bd];
    let mut stores = Vec::new();
    for i in 3..text.len().saturating_sub(4) {
        if text[i..i + 4] == [0xc8, 0x08, 0x00, 0x00]
            && (matches!(text[i - 2], 0x89 | 0xc7 | 0xd9) && text[i - 1] & 0xc0 == 0x80
                || matches!(text[i - 3], 0x89 | 0xc7 | 0xd9) && text[i - 2] & 0xc7 == 0x84)
        {
            let is_load = text[i - 2] == 0xd9 && text[i - 1] & 0x38 == 0x00
                || text[i - 3] == 0xd9 && text[i - 2] & 0x38 == 0x00;
            if !is_load {
                stores.push(0x401000 + i as u64 - 2);
            }
        }
    }
    assert_eq!(stores, [0x47d1fb, 0x47d20f], "writers of vehicle+0x8c8");
}

/// The trace's rows are the ones the finding quotes: the pitch command is
/// exactly zero on every row outside the escape climb.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_wingman_drift_pitch_command_is_zero_outside_the_climb() {
    let rows = fly_trace(true);
    let outside: Vec<&Row> = rows.iter().filter(|r| !r.climbing).collect();
    assert!(outside.len() > 80);
    assert!(
        outside
            .iter()
            .filter(|r| r.t < 35.0)
            .all(|r| r.pitch == 0.0),
        "the dead pitch axis never moves before the first escape latch"
    );
    assert!(rows.iter().any(|r| r.climbing && r.pitch > 0.5));
}
