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
use cs_sim::flight::original::lift_target;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

/// One trace row: slot error on the leader's axes, speeds, the stick words
/// and throttle as commanded, and the follower's latches.
///
/// The `#2299` columns split `vert` into absolute altitudes: the leader's
/// and Betty's own heights and vertical speeds, the slot's height above the
/// leader (the attitude part), the lift demand the port computes for Betty
/// (`lift_accel_rate * (Vt - V) + (0, g, 0)`, the `0x48c746`/`0x48c77b`
/// terms, against gravity) and her bank, so a climb of the leader and a
/// sink of Betty read as different numbers instead of one difference.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Row {
    pub(crate) t: f32,
    pub(crate) along: f32,
    pub(crate) cross: f32,
    pub(crate) vert: f32,
    pub(crate) gap: f32,
    pub(crate) own_speed: f32,
    pub(crate) leader_speed: f32,
    pub(crate) roll: f64,
    pub(crate) pitch: f64,
    pub(crate) yaw: f64,
    pub(crate) throttle: f64,
    pub(crate) engaged: bool,
    pub(crate) climbing: bool,
    /// The leader's absolute altitude and world vertical speed.
    pub(crate) leader_alt: f32,
    pub(crate) leader_vy: f32,
    /// Betty's absolute altitude and world vertical speed.
    pub(crate) betty_alt: f32,
    pub(crate) betty_vy: f32,
    /// `slot.y - leader.y`: the part of the slot's height that comes from
    /// the leader's attitude rather than its position.
    pub(crate) slot_dy: f32,
    /// The player's held stick: `true` while roll, pitch and yaw are all
    /// `0.0`, so a leader climb cannot be a commanded climb.
    leader_neutral: bool,
    /// The `0x48c746`/`0x48c77b` demand `steer` for Betty, in m/s^2:
    /// `dem_y` its world-vertical part, `dem_side`/`dem_up` its projections
    /// on her body right/up axes (the `(ax, ay)` the load command uses),
    /// `grav` the gravity term it is added to.
    dem_y: f64,
    dem_side: f64,
    dem_up: f64,
    grav: f64,
    /// `asin` of her body-right axis' world `y`, degrees — the `X.y` the
    /// image's bank coupling reads (`0x48cc6a`).
    pub(crate) bank_deg: f64,
    /// Her body-up axis' world `y`: the fraction of the lift-plane up that
    /// is still vertical, so `1 - up_y` is the share banking costs.
    pub(crate) up_y: f64,
    /// Her nose axis' world `y` as an elevation angle, degrees — the lift
    /// target's direction, `Vt.y = |v| * nose.y`, that her velocity follows.
    pub(crate) nose_deg: f64,
    /// Her world angular momentum (the `L` the rotation increments write
    /// into), its `x` and `y` parts: the bank coupling pitches about the
    /// body X and yaws about the body Y.
    pub(crate) mom_x: f64,
    pub(crate) mom_y: f64,
}

fn pose(app: &App, entity: Entity) -> (Vec3, bevy::prelude::Quat, Vec3) {
    let w = app.world();
    (
        w.get::<Position>(entity).expect("pose").0,
        w.get::<Rotation>(entity).expect("rotation").0,
        w.get::<LinearVelocity>(entity).expect("velocity").0,
    )
}

pub(crate) fn fly_trace(use_stick: bool) -> Vec<Row> {
    fly_trace_opts(use_stick, false)
}

fn fly_trace_opts(use_stick: bool, zero_betty_blow: bool) -> Vec<Row> {
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
    if zero_betty_blow {
        let world = app.world_mut();
        world
            .get_mut::<PlaytestOriginalFlight>(betty)
            .expect("flight")
            .model
            .globals
            .groundblow_mag = 0.0;
    }
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
        let (b, bq, bv) = pose(&app, betty);
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
        let flight = app
            .world()
            .get::<PlaytestOriginalFlight>(betty)
            .expect("flight");
        let cmd = flight.command;
        let player_cmd = app
            .world()
            .get::<PlaytestOriginalFlight>(player)
            .expect("player flight")
            .command;
        // The `0x48c746`/`0x48c77b` demand as the port's `aero` computes it:
        // `steer = lift_accel_rate * (Vt - V) + (0, g, 0)` on the body axes
        // the driver seeds the law's state from each tick (its own post-step
        // `state.orientation` is what the body's `Rotation` was written
        // from). For the AI seat `lift_target` is `|v|` along the nose.
        let v3 = |v: Vec3| [f64::from(v.x), f64::from(v.y), f64::from(v.z)];
        let right = v3(bq * Vec3::X);
        let up = v3(bq * Vec3::Y);
        let nose = v3(bq * Vec3::NEG_Z);
        let velocity = v3(bv);
        let target = lift_target(
            nose,
            velocity,
            flight.model.globals.lift_aoa_deg,
            flight.is_player,
        );
        let rate = flight.model.globals.lift_accel_rate;
        let grav = flight.model.airframe.gravity;
        let steer = [
            (target[0] - velocity[0]) * rate,
            (target[1] - velocity[1]) * rate + grav,
            (target[2] - velocity[2]) * rate,
        ];
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
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
            leader_alt: p.y,
            leader_vy: pv.y,
            betty_alt: b.y,
            betty_vy: bv.y,
            slot_dy: slot.y - p.y,
            leader_neutral: player_cmd.roll == 0.0
                && player_cmd.pitch == 0.0
                && player_cmd.yaw == 0.0,
            dem_y: steer[1],
            dem_side: dot(steer, right),
            dem_up: dot(steer, up),
            grav,
            bank_deg: right[1].clamp(-1.0, 1.0).asin().to_degrees(),
            up_y: up[1],
            nose_deg: nose[1].clamp(-1.0, 1.0).asin().to_degrees(),
            mom_x: flight.state.angular_momentum_world[0],
            mom_y: flight.state.angular_momentum_world[1],
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

// ------------------------------------------------- altitude authority ---
//
// M01-WINGMAN-ALTITUDE-AUTHORITY (#2296): the image windows behind
// `docs/findings/2026-11-02-m01-wingman-gains.md` §11 — the three torque
// inputs, the identity response curve, the Level-Off assist and its
// player-only flag, and the fact that no AI path gives the pitch word a
// non-zero value.

/// Every instruction in `.text` that writes a register-based `[reg + disp]`
/// through one of `forms`, with `mod = 10` and a base that is neither `esp`
/// nor `ebp` (displacements, not stack frames). For `d9` only the x87 stores
/// (`/2`, `/3`) count, so the block's `fld` reads never appear.
fn stores_into(text: &[u8], text_va: u64, disp: u32, forms: &[u8]) -> Vec<u64> {
    let want = disp.to_le_bytes();
    let mut sites = Vec::new();
    for i in 1..text.len().saturating_sub(5) {
        let modrm = text[i];
        if modrm & 0xc0 != 0x80 || matches!(modrm & 7, 4 | 5) {
            continue;
        }
        if text[i + 1..i + 5] != want[..] {
            continue;
        }
        let opcode = text[i - 1];
        if !forms.contains(&opcode) {
            continue;
        }
        if opcode == 0xd9 && !matches!((modrm >> 3) & 7, 2 | 3) {
            continue;
        }
        sites.push(text_va + i as u64 - 1);
    }
    sites
}

/// Writes of a 32-bit `[reg + 0x11c]`: `mov` (`89`), `mov imm32` (`c7`) and
/// `fst`/`fstp` (`d9 /2`, `/3`).
fn pitch_input_writers(text: &[u8], text_va: u64) -> Vec<u64> {
    stores_into(text, text_va, 0x11c, &[0x89, 0xc7, 0xd9])
}

/// Writes of the byte `[reg + 0x12c]`: `mov r/m8, r8` (`88`) and
/// `mov r/m8, imm8` (`c6`).
fn level_off_flag_writers(text: &[u8], text_va: u64) -> Vec<u64> {
    stores_into(text, text_va, 0x12c, &[0x88, 0xc6])
}

fn within(sites: &[u64], start: u64, end: u64) -> Vec<u64> {
    sites
        .iter()
        .copied()
        .filter(|site| (start..end).contains(site))
        .collect()
}

/// The image windows §11 rests on: the force block reads the three stick
/// words `+0x114/+0x11c/+0x120` as its torque inputs (the roll and yaw
/// terms are skipped while they are zero and the pitch term is a jump over
/// the whole term), the law's response curve `0x460890` is the identity, and
/// the only zero-input term is the Level-Off assist, gated on a per-vehicle
/// flag whose only writer for a non-player is the constructor.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_altitude_windows_match_the_image() {
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
    // 1. The force block: the three inputs and the tests that skip them.
    //    Roll: `fld [esi+0x114]`, the torque `+0x644`, the body Z axis.
    window(0x48ca7a, &[0xd9, 0x86, 0x14, 0x01, 0x00, 0x00]);
    window(0x48ca80, &[0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00]);
    window(0x48ca88, &[0xf6, 0xc4, 0x40]);
    window(0x48ca8b, &[0x75, 0x5d]);
    window(0x48ca8d, &[0xd9, 0x86, 0x44, 0x06, 0x00, 0x00]);
    window(0x48caaa, &[0xd8, 0x8e, 0x98, 0x01, 0x00, 0x00]);
    //    Pitch: `fld [esi+0x11c]`, `jne` over the whole term, `+0x648`, body X.
    window(0x48caea, &[0xd9, 0x86, 0x1c, 0x01, 0x00, 0x00]);
    window(0x48caf0, &[0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00]);
    window(0x48cafb, &[0x0f, 0x85, 0x9f, 0x00, 0x00, 0x00]);
    window(0x48cb07, &[0xd8, 0x8e, 0x48, 0x06, 0x00, 0x00]);
    window(0x48cb0d, &[0x8d, 0x86, 0x80, 0x01, 0x00, 0x00]);
    window(0x48cb1f, &[0xd8, 0x8e, 0x1c, 0x01, 0x00, 0x00]);
    //    Yaw: `fld [esi+0x120]`, the rudder torque `+0x64c`, body Y.
    window(0x48cba0, &[0xd9, 0x86, 0x20, 0x01, 0x00, 0x00]);
    window(0x48cbb1, &[0x0f, 0x85, 0x9f, 0x00, 0x00, 0x00]);
    window(0x48cbb7, &[0xd9, 0x86, 0x4c, 0x06, 0x00, 0x00]);
    window(0x48cbc6, &[0x8d, 0x86, 0x8c, 0x01, 0x00, 0x00]);
    //    The zero the tests compare against is a real 0.0.
    window(0x6032c8, &[0x00, 0x00, 0x00, 0x00]);

    // 2. The definition chain: each key's slot and the vehicle field it fills.
    for (va, text) in [
        (0x627fa4_u64, &b"roll_torque\0"[..]),
        (0x627f94, b"pitch_torque\0"),
        (0x627fb0, b"rudder_torque\0"),
        (0x627fc0, b"level_off_rate\0"),
    ] {
        window(va, text);
    }
    window(0x47ad3d, &[0x89, 0x8f, 0x04, 0x01, 0x00, 0x00]); // pitch_torque -> def+0x104
    window(0x47ad5b, &[0x89, 0x87, 0x00, 0x01, 0x00, 0x00]); // roll_torque -> def+0x100
    window(0x47ad79, &[0x89, 0x97, 0x08, 0x01, 0x00, 0x00]); // rudder_torque -> def+0x108
    window(0x47ad97, &[0x89, 0x8f, 0x0c, 0x01, 0x00, 0x00]); // level_off_rate -> def+0x10c
    window(0x475bdc, &[0x8b, 0x8d, 0x00, 0x01, 0x00, 0x00]);
    window(0x475be2, &[0x89, 0x8b, 0x44, 0x06, 0x00, 0x00]); // -> veh+0x644
    window(0x475be8, &[0x8b, 0x95, 0x04, 0x01, 0x00, 0x00]);
    window(0x475bee, &[0x89, 0x93, 0x48, 0x06, 0x00, 0x00]); // -> veh+0x648
    window(0x475bf4, &[0x8b, 0x85, 0x08, 0x01, 0x00, 0x00]);
    window(0x475bfa, &[0x89, 0x83, 0x4c, 0x06, 0x00, 0x00]); // -> veh+0x64c
    window(0x475c00, &[0x8b, 0x8d, 0x0c, 0x01, 0x00, 0x00]);
    window(0x475c06, &[0x89, 0x8b, 0x50, 0x06, 0x00, 0x00]); // -> veh+0x650

    // 3. The law's words reach the force block through an identity curve.
    window(0x460890, &[0xd9, 0x44, 0x24, 0x04, 0xc3]); // fld [esp+4]; ret
    window(0x41bfc6, &[0x75, 0x56]); // the emergency set skips the pre-scale
    window(0x41bfcd, &[0xd9, 0x86, 0x74, 0x09, 0x00, 0x00]);
    window(0x41bff8, &[0xd8, 0x8e, 0x00, 0x01, 0x00, 0x00]);
    window(0x41c006, &[0xd8, 0x8e, 0x08, 0x01, 0x00, 0x00]);
    window(0x41c01e, &[0x8b, 0x8e, 0x00, 0x01, 0x00, 0x00]);
    window(0x41c024, &[0x68, 0x00, 0x00, 0x80, 0x3f]);
    window(0x41c029, &[0x68, 0x00, 0x00, 0x00, 0x3f]);
    window(0x41c02f, &[0xe8, 0x5c, 0x48, 0x04, 0x00]);
    window(0x41c034, &[0xd9, 0x9e, 0x14, 0x01, 0x00, 0x00]);
    window(0x41c03a, &[0x8b, 0x96, 0x08, 0x01, 0x00, 0x00]);
    window(0x41c050, &[0xd9, 0x9e, 0x1c, 0x01, 0x00, 0x00]);
    window(0x41c056, &[0x8b, 0x86, 0x0c, 0x01, 0x00, 0x00]);
    window(0x41c06c, &[0xd9, 0x9e, 0x20, 0x01, 0x00, 0x00]);

    // 4. The Level-Off assist: flag, both zero-input gates, body Y to world up,
    //    and the rate it reads.
    window(0x48ce45, &[0x8a, 0x86, 0x2c, 0x01, 0x00, 0x00]);
    window(0x48ce4d, &[0x0f, 0x84, 0x2b, 0x01, 0x00, 0x00]);
    window(0x48ce53, &[0xd9, 0x86, 0x14, 0x01, 0x00, 0x00]);
    window(0x48ce64, &[0x0f, 0x84, 0x14, 0x01, 0x00, 0x00]);
    window(0x48ce6a, &[0xd9, 0x86, 0x1c, 0x01, 0x00, 0x00]);
    window(0x48ce7b, &[0x0f, 0x84, 0xfd, 0x00, 0x00, 0x00]);
    window(0x48ce81, &[0x8d, 0x86, 0x8c, 0x01, 0x00, 0x00]);
    window(0x6379c0, &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f]);
    window(0x48ced6, &[0xd9, 0x05, 0x6c, 0xc5, 0x71, 0x00]);
    window(0x48cedc, &[0xd8, 0x8e, 0x50, 0x06, 0x00, 0x00]);

    // 5. The flag's writers: the player input routine's clear-on-input, the
    //    player-vehicle toggle, and the constructor.
    window(0x487d4e, &[0xf6, 0xc4, 0x40]);
    window(0x487d51, &[0x74, 0x43]);
    window(0x487d64, &[0x74, 0x30]);
    window(0x487d77, &[0x74, 0x1d]);
    window(0x487d96, &[0xc6, 0x86, 0x2c, 0x01, 0x00, 0x00, 0x00]);
    window(0x488640, &[0x8b, 0x0d, 0x98, 0xc2, 0x71, 0x00]);
    window(0x488663, &[0x8a, 0x81, 0x2c, 0x01, 0x00, 0x00]);
    window(0x48866e, &[0x88, 0x81, 0x2c, 0x01, 0x00, 0x00]);
    window(0x4b0645, &[0x88, 0x9e, 0x2c, 0x01, 0x00, 0x00]);
    // The constructor zeroes the whole control block beside it (`ebx = 0`).
    window(0x4b062d, &[0x89, 0x9e, 0x1c, 0x01, 0x00, 0x00]);
    window(0x4b0633, &[0x89, 0x9e, 0x20, 0x01, 0x00, 0x00]);

    // 6. The AI's seat differences: the lift target is always `|v| * nose`,
    //    the demand is `lift_accel_rate * (Vt - V) + gravity`, and the
    //    weathervane is the player's.
    window(0x48c6e9, &[0xd9, 0x45, 0x80]);
    window(0x48c6ec, &[0xd9, 0xe0]);
    window(0x48c6f0, &[0xd8, 0x8e, 0x98, 0x01, 0x00, 0x00]);
    window(0x48c746, &[0xd9, 0x05, 0x48, 0xc4, 0x71, 0x00]);
    window(0x48c77b, &[0xd8, 0x81, 0xc4, 0x00, 0x00, 0x00]);
    window(0x48cd3e, &[0x3b, 0x35, 0x98, 0xc2, 0x71, 0x00]);
    window(0x48cd44, &[0x0f, 0x85, 0xfb, 0x00, 0x00, 0x00]);
    // The bank coupling is unconditional in both of its parts.
    window(0x48cc61, &[0xd9, 0x05, 0xf8, 0x89, 0x62, 0x00]);
    window(0x48cc6a, &[0xd8, 0x8e, 0x84, 0x01, 0x00, 0x00]);
    window(0x48ccc1, &[0xd9, 0xe1]);
    window(0x48ccc3, &[0xd8, 0x0d, 0xfc, 0x89, 0x62, 0x00]);

    // 7. The second AI law at `0x41c080` writes `+0x110` from `+0xfc` and
    //    `+0x118` from `+0x104` through the same identity curve. Neither is a
    //    control input of the force block, which reads only
    //    `+0x114`/`+0x11c`/`+0x120` (pinned above), so this law has no pitch
    //    authority either (§11.6).
    window(0x41c201, &[0x8b, 0x96, 0xfc, 0x00, 0x00, 0x00]); // mov edx,[esi+0xfc]
    window(0x41c21f, &[0xd9, 0x9e, 0x10, 0x01, 0x00, 0x00]); // fstp [esi+0x110]
    window(0x41c241, &[0x8b, 0x8e, 0x04, 0x01, 0x00, 0x00]); // mov ecx,[esi+0x104]
    window(0x41c257, &[0xd9, 0x9e, 0x18, 0x01, 0x00, 0x00]); // fstp [esi+0x118]
}

/// No AI path gives the pitch input `+0x11c` a non-zero value: the writers
/// in the AI code are the follower law, the stun zeroing and a law no mode-4
/// follower reaches; the follower and the force block contain none; and the
/// Level-Off flag `+0x12c` has exactly three writers, only one of which is
/// not a write of `0` — the player-vehicle toggle.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_altitude_has_no_ai_pitch_authority() {
    use cs_app::engine_facts::engine_facts;
    use cs_content::coordinates::load_engine_image;
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let facts = engine_facts().expect("the committed table parses");
    let at = |va: u64| usize::try_from(facts.image_offset(va).expect("va in image")).expect("off");
    let text_va = 0x401000_u64;
    let text = &loaded.bytes[at(text_va)..at(text_va) + 0x2014bd];

    let writers = pitch_input_writers(text, text_va);
    assert_eq!(
        within(&writers, 0x41b560, 0x421800),
        [0x41c050, 0x420185, 0x420ff9],
        "every writer of the pitch input in the AI code"
    );
    assert!(
        within(&writers, 0x41e760, 0x41f00a).is_empty(),
        "the follower routine never writes the pitch input itself"
    );
    assert!(
        within(&writers, 0x48c470, 0x48d2b7).is_empty(),
        "the force block only reads the pitch input"
    );
    assert_eq!(
        within(&writers, 0x487400, 0x488800),
        [0x487dcc, 0x487e3c, 0x487e5b],
        "the player input routine: the curve and its two clamps"
    );
    assert_eq!(
        within(&writers, 0x4b0500, 0x4b0700),
        [0x4b062d],
        "the vehicle reset zeroes it"
    );

    // `0x4209b0` (the only other AI routine that writes the pitch word) is
    // reached from the two bodies the mode-4 dispatch does not call, and
    // writes +0x108 from `∓0.05`, which the same zero pitch scale would zero
    // anyway.
    assert_eq!(
        within(&writers, 0x4209b0, 0x421800),
        [0x420ff9],
        "the second AI law's only pitch-input write"
    );
    let window = |va: u64, bytes: &[u8]| {
        let i = at(va);
        assert_eq!(
            &loaded.bytes[i..i + bytes.len()],
            bytes,
            "window at {va:#x}"
        );
    };
    window(0x41c2dd, &[0x83, 0xbe, 0x7c, 0x06, 0x00, 0x00, 0x04]);
    window(0x41c2e4, &[0x74, 0x72]);
    window(0x41c2fa, &[0xe8, 0xf1, 0x16, 0x00, 0x00]); // call 0x41d9f0
    window(0x41c305, &[0xe8, 0xe6, 0x0e, 0x00, 0x00]); // call 0x41d1f0
    window(0x41c359, &[0xe8, 0x02, 0x24, 0x00, 0x00]); // call 0x41e760 (mode 4)
    window(0x41d4e9, &[0xe8, 0xc2, 0x34, 0x00, 0x00]); // call 0x4209b0
    window(0x41da69, &[0xe8, 0x42, 0x2f, 0x00, 0x00]); // call 0x4209b0
    window(
        0x420d93,
        &[0xc7, 0x86, 0x08, 0x01, 0x00, 0x00, 0xcd, 0xcc, 0x4c, 0xbd],
    );
    window(
        0x420dd0,
        &[0xc7, 0x86, 0x08, 0x01, 0x00, 0x00, 0xcd, 0xcc, 0x4c, 0x3d],
    );
    // The stun latch's zeroing.
    window(0x42015a, &[0x89, 0x9e, 0x00, 0x01, 0x00, 0x00]);
    window(0x420185, &[0x89, 0x9e, 0x1c, 0x01, 0x00, 0x00]);

    // Exactly three byte stores of the Level-Off flag survive in the image,
    // and two of them write zero: the input routine's clear and the
    // constructor. The third is the player-vehicle toggle.
    assert_eq!(
        level_off_flag_writers(text, text_va),
        [0x487d96, 0x48866e, 0x4b0645],
        "writers of vehicle+0x12c"
    );
    window(0x48866b, &[0x0f, 0x94, 0xc0]); // sete al: the toggle
}

/// The port's M01 wingman flies the AI seat with the Level-Off assist clear,
/// so the force block's only zero-input term cannot run for it — the same
/// state the image gives every vehicle that is not the player's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_wingman_altitude_level_off_is_the_players_toggle() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plans");
    let stage = stage_for(&root, &plan).expect("stage reads");
    let mut app = build_headless(&stage).expect("composes");
    app.update();
    let betty = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ScriptedAircraftActor)>()
            .iter(world)
            .find(|(_, actor)| actor.record.name == "wingman_1")
            .map(|(entity, _)| entity)
            .expect("wingman_1")
    };
    let flight = app
        .world()
        .get::<PlaytestOriginalFlight>(betty)
        .expect("wingman_1 flies the original law");
    assert!(
        !flight.is_player,
        "the scripted wingman must fly the AI seat of the law"
    );
    assert!(
        !flight.state.level_off,
        "the Level-Off assist is the player's Shift+L toggle (`+0x12c`), so \
         the AI body starts with it clear and no AI path sets it"
    );
    assert!(
        flight.model.airframe.level_off_rate > 0.0,
        "`+0x650` would level the wings if the flag were ever set: {}",
        flight.model.airframe.level_off_rate
    );
    teardown(&mut app);
}

// ------------------------------------------------- vertical source ------
//
// M01-WINGMAN-VERT-SOURCE (#2299): §10 records only `vert = slot.y -
// Betty.y`, which cannot tell the leader's climb from Betty's sink. The
// extended row records both absolute altitudes and vertical speeds, the
// `0x48c746`/`0x48c77b` lift demand against gravity, and her bank, so the
// decomposition in `docs/findings/2026-11-02-m01-wingman-gains.md` §12 reads
// in numbers.

/// Prints the vertical-source columns of the 0.5 s hillside trace with
/// `original_stick` on (`CS_VERT_TRACE=1`); not a pin on the outcome.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_wingman_vert_source_trace_hillside() {
    let rows = fly_trace(true);
    assert_eq!(rows.len(), 90);
    if std::env::var_os("CS_VERT_TRACE").is_some() {
        for r in &rows {
            println!(
                "t={:5.1} L={:7.1}/{:+5.2} B={:7.1}/{:+5.2} vert={:7.1} sdy={:+5.2} \
                 dem(y,side,up)={:+6.2}/{:+6.2}/{:+6.2} g={:5.2} bank={:+6.1} up_y={:5.3} \
                 nose={:+6.2} stick(r,y)={:+5.2}/{:+5.3} L(x,y)={:+7.3}/{:+7.3} \
                 ntrl={} eng={} esc={}",
                r.t,
                r.leader_alt,
                r.leader_vy,
                r.betty_alt,
                r.betty_vy,
                r.vert,
                r.slot_dy,
                r.dem_y,
                r.dem_side,
                r.dem_up,
                r.grav,
                r.bank_deg,
                r.up_y,
                r.nose_deg,
                r.roll,
                r.yaw,
                r.mom_x,
                r.mom_y,
                u8::from(r.leader_neutral),
                u8::from(r.engaged),
                u8::from(r.climbing),
            );
        }
    }
}

/// The same trace with Betty's `groundblow_mag` forced to `0.0`
/// (`CS_VERT_TRACE=1`): whether the sink survives the removal of the only
/// attitude term the hillside finding isolated is the §12 cross-check.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_wingman_vert_source_trace_without_her_blow() {
    let rows = fly_trace_opts(true, true);
    assert_eq!(rows.len(), 90);
    if std::env::var_os("CS_VERT_TRACE").is_some() {
        for r in &rows {
            println!(
                "t={:5.1} L={:7.1}/{:+5.2} B={:7.1}/{:+5.2} vert={:7.1} sdy={:+5.2} \
                 dem(y,side,up)={:+6.2}/{:+6.2}/{:+6.2} g={:5.2} bank={:+6.1} up_y={:5.3} \
                 nose={:+6.2} stick(r,y)={:+5.2}/{:+5.3} L(x,y)={:+7.3}/{:+7.3} \
                 ntrl={} eng={} esc={}",
                r.t,
                r.leader_alt,
                r.leader_vy,
                r.betty_alt,
                r.betty_vy,
                r.vert,
                r.slot_dy,
                r.dem_y,
                r.dem_side,
                r.dem_up,
                r.grav,
                r.bank_deg,
                r.up_y,
                r.nose_deg,
                r.roll,
                r.yaw,
                r.mom_x,
                r.mom_y,
                u8::from(r.leader_neutral),
                u8::from(r.engaged),
                u8::from(r.climbing),
            );
        }
    }
}

/// `vert` splits exactly into `(leader.y - Betty.y) + (slot.y - leader.y)`:
/// the absolute-altitude split is arithmetic, not a fit, so every row must
/// close.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_wingman_vert_source_error_is_the_altitude_split() {
    for r in fly_trace(true) {
        let split = f64::from(r.leader_alt - r.betty_alt) + f64::from(r.slot_dy);
        assert!(
            (f64::from(r.vert) - split).abs() < 0.01,
            "t={}: vert={} but the altitude split is {split}",
            r.t,
            r.vert
        );
    }
}

/// The corrected sign selects: both the wings-level assist and the recovery
/// branch discriminate on `row1.y` (the up axis' vertical part — "inverted?"),
/// never on `row1.x`. With `row1.x < 0` but `row1.y > 0` (tilted, not
/// inverted) the old read picked the saturated branch the image does not.
#[test]
fn accept_m01_wingman_vert_source_sign_selects_read_row1_y() {
    use cs_app::mission_ai::{STICK_LEVEL_GAIN, wingman_stick};

    // Banked ~60 deg to the side the up row tilts toward -x: `row1.x` is
    // negative while `row1.y` is still positive (not inverted). The aim is
    // centred (bz <= 0 keeps it un-normalised), so the wings-level assist
    // runs; its upright branch is `-row0.y * 0.2`, not the saturated
    // `+/-1 * 0.2`.
    let tilted = [
        Vec3::new(1.0, -0.4, 0.0),
        Vec3::new(-0.5, 0.87, 0.0),
        Vec3::NEG_Z,
    ];
    let words = wingman_stick(Vec3::new(0.01, 0.01, -1.0), tilted, 60.0, 0.0);
    assert!(
        (words.a100 - 0.4 * STICK_LEVEL_GAIN).abs() < 1e-6,
        "row1.y > 0 keeps the proportional wing-level word: {words:?}"
    );

    // The same tilt during a slow nose-down recovery: `row1.y > 0` picks the
    // `-1` pitch word the image writes for an upright body.
    let dive = [
        Vec3::new(1.0, -0.4, 0.0),
        Vec3::new(-0.5, 0.87, 0.0),
        Vec3::new(0.0, -0.8, -0.6),
    ];
    let words = wingman_stick(Vec3::new(10.0, 0.0, 10.0), dive, 20.0, 0.0);
    assert_eq!(words.a108, -1.0, "row1.y > 0: {words:?}");
}

/// The image windows §12's demand and bank columns rest on: the AI lift
/// target is `|v|` along the nose (`0x48c6e9`..`0x48c6f0`), the demand adds
/// the gravity word (`0x48c746`) into its `y` (`0x48c77b`), and the bank
/// coupling reads the body X axis' `y` (`0x48cc6a`) with no seat gate.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_vert_source_demand_windows_match_the_image() {
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
    // The AI seat's lift target `Vt = |v| * nose`: `fld [ebp-0x80]` /
    // `fchs` / `fmul [esi+0x198]` (the body Z axis).
    window(0x48c6e9, &[0xd9, 0x45, 0x80]);
    window(0x48c6ec, &[0xd9, 0xe0]);
    window(0x48c6f0, &[0xd8, 0x8e, 0x98, 0x01, 0x00, 0x00]);
    // The demand's gravity term: `fld [0x71c448]` and `fadd [ecx+0xc4]`.
    window(0x48c746, &[0xd9, 0x05, 0x48, 0xc4, 0x71, 0x00]);
    window(0x48c77b, &[0xd8, 0x81, 0xc4, 0x00, 0x00, 0x00]);
    // The bank coupling is unconditional: `fall_off` `fmul [esi+0x184]`
    // (X.y) about body Y; `fabs` / `bank_off` about body X.
    window(0x48cc61, &[0xd9, 0x05, 0xf8, 0x89, 0x62, 0x00]);
    window(0x48cc6a, &[0xd8, 0x8e, 0x84, 0x01, 0x00, 0x00]);
    window(0x48ccc1, &[0xd9, 0xe1]);
    window(0x48ccc3, &[0xd8, 0x0d, 0xfc, 0x89, 0x62, 0x00]);
}

/// The two sign selects the fix reads: the wings-level assist's `fld
/// [esi+0x190]` and the recovery's `fld [esi+0x190]` both take the second
/// float of the `V+0x18c` row — `row1.y`, the "inverted?" test. The bank
/// coupling's own yaw part writes `scalar * [0x18c]/[0x190]/[0x194]` as the
/// up vector, which pins the layout: `+0x190` is `row1.y`, not `row1.x`
/// (`0x18c` itself). Both selects were ported as `row1.x`; §12 records the
/// correction.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_vert_source_sign_windows_match_the_image() {
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
    // The up vector as the yaw part writes it: `scalar * row1.x/.y/.z`
    // at `+0x18c`/`+0x190`/`+0x194` — `+0x190` is the row's second float.
    window(0x48cc7b, &[0xd8, 0x8e, 0x8c, 0x01, 0x00, 0x00]);
    window(0x48cc86, &[0xd8, 0x8e, 0x90, 0x01, 0x00, 0x00]);
    window(0x48cc8f, &[0xd8, 0x8e, 0x94, 0x01, 0x00, 0x00]);
    // The wings-level assist's sign select: `fld [esi+0x190]` = row1.y.
    window(0x41bd98, &[0xd9, 0x86, 0x90, 0x01, 0x00, 0x00]);
    // The recovery branch's sign select: `fld [esi+0x190]` = row1.y.
    window(0x41be15, &[0xd9, 0x86, 0x90, 0x01, 0x00, 0x00]);
}
