//! M01-WINGMAN-STICK-LAW (#2263): the original follower's stick law
//! (`0x41b560`, `0x41bc08`..`0x41be47`) flown by Betty.
//!
//! Task test prefix: `accept_m01_wingman_stick_`. The law's branches are
//! decoded in `docs/findings/2026-10-18-m01-wingman-player-leader.md` §4 and
//! re-read from the image for this task; the result of re-flying the
//! hillside with it is in
//! `docs/findings/2026-11-02-m01-wingman-stick-law.md`, and the pilot
//! gains/limits it is flown with in
//! `docs/findings/2026-11-02-m01-wingman-gains.md` (#2266).

use avian3d::prelude::Position;
use bevy::prelude::{App, Entity, Quat, Vec3, With};
use bevy::time::{Time, TimeUpdateStrategy};

use cs_app::engine_facts::engine_facts;
use cs_app::mission_actors::ScriptedAircraftActor;
use cs_app::mission_ai::MissionAircraftAi;
use cs_app::mission_ai::{
    M01_WINGMAN_PILOT_GAINS, PILOT_GAINS_AI_INPUT_KEYS, PilotInputGains, RUDDER_TOL,
    STICK_AIM_THRESHOLD, STICK_COARSE_FULL_ROLL_MIN, STICK_LEVEL_FORWARD_Y_MAX, STICK_LEVEL_GAIN,
    STICK_RECOVER_FORWARD_Y_MAX, wingman_stick, wingman_stick_command,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionPlayerBody, build_headless, stage_for, teardown};
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_content::coordinates::load_engine_image;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

const LEVEL_ROWS: [Vec3; 3] = [Vec3::X, Vec3::Y, Vec3::NEG_Z];

/// Coarse branch, goal ahead: the aim is normalised, a goal up and to the
/// right gets a right roll (scaled by the pilot's roll word) and no yaw, a
/// level goal ahead only pitch.
#[test]
fn accept_m01_wingman_stick_coarse_branch_rolls_toward_the_goal() {
    // 3 m right, 3 m above, 10 m ahead.
    let words = wingman_stick(Vec3::new(3.0, 3.0, 10.0), LEVEL_ROWS, 60.0, 0.0);
    let norm = 0.5_f32.sqrt();
    assert!((words.a100 + norm).abs() < 1e-6, "A100 = -bx: {words:?}");
    assert_eq!(words.a10c, 0.0, "the coarse branch writes no yaw word");
    let input = words.flight_input(0.7);
    let expected_roll = norm * M01_WINGMAN_PILOT_GAINS.scale[0];
    assert!(
        (input.roll - f64::from(expected_roll)).abs() < 1e-6,
        "right-wing-down by the roll word times its scale ({expected_roll}), got {}",
        input.roll
    );
    assert!(input.roll > 0.0, "toward a goal on the right");
    // A goal straight ahead is the degenerate by := 1 case.
    let ahead = wingman_stick(Vec3::new(0.0, 0.0, 10.0), LEVEL_ROWS, 60.0, 0.0);
    assert_eq!(ahead.a108, 1.0);
    assert_eq!(ahead.a100, 0.0);
}

/// A goal below the wings: the aim keeps its sign at full scale when
/// `m >= params[6]`, and is mirrored below it.
#[test]
fn accept_m01_wingman_stick_goal_below_flips_the_roll_by_magnitude() {
    // Behind (bz <= 0) so the aim is not normalised: big magnitude.
    let far = wingman_stick(Vec3::new(5.0, -5.0, -1.0), LEVEL_ROWS, 60.0, 0.0);
    assert_eq!(far.a100, -1.0, "m >= 0.35: bx := +1, A100 = -1");
    let near = wingman_stick(Vec3::new(0.1, -0.1, -1.0), LEVEL_ROWS, 60.0, 0.0);
    assert!(0.1_f32.hypot(0.1) < STICK_COARSE_FULL_ROLL_MIN);
    assert!(
        (near.a100 - 0.1).abs() < 1e-6,
        "m < 0.35: bx := -bx, A100 = +0.1"
    );
}

/// Fine branch: gated by the definition's `rudder_tol` (`def+0x260`), which
/// the constructor sets to `0.2` and the retail `wingman` record does not
/// override — so a goal ahead (normalised magnitude 1.0) never takes it.
#[test]
fn accept_m01_wingman_stick_fine_branch_needs_the_measured_rudder_tol() {
    let body = Vec3::new(-0.9, 0.02, 10.0);
    let coarse = wingman_stick(body, LEVEL_ROWS, 60.0, RUDDER_TOL);
    let fine = wingman_stick(body, LEVEL_ROWS, 60.0, 1.0);
    assert_ne!(coarse, fine);
    assert!(fine.a10c > 0.5, "A10c = -bx in the fine branch: {fine:?}");
    // The normalised magnitude of a goal ahead is exactly 1.0, above the
    // measured 0.2, so the branch is never taken there.
    let ahead = Vec3::new(-9.0, 1.0, 10.0);
    assert_eq!(
        wingman_stick(ahead, LEVEL_ROWS, 60.0, RUDDER_TOL),
        wingman_stick(ahead, LEVEL_ROWS, 60.0, 0.0)
    );
}

/// Wings-level assist and dive recovery.
#[test]
fn accept_m01_wingman_stick_level_assist_and_recovery() {
    // Dead-on, one wing low: right row points down 0.2.
    let rows = [Vec3::new(1.0, -0.2, 0.0), Vec3::Y, Vec3::NEG_Z];
    let words = wingman_stick(Vec3::new(0.01, 0.01, -1.0), rows, 60.0, 0.0);
    assert!(
        (words.a100 - 0.2 * STICK_LEVEL_GAIN).abs() < 1e-6,
        "A100 = -row0.y * 0.2: {words:?}"
    );
    assert!(
        words.flight_input(0.5).roll < 0.0,
        "rolls the low wing back up"
    );

    // Nose well down and slow: pitch word +/-1 and the throttle flag.
    let dive = [Vec3::X, Vec3::Y, Vec3::new(0.0, -0.8, -0.6)];
    let words = wingman_stick(Vec3::new(10.0, 0.0, 10.0), dive, 20.0, 0.0);
    assert!(words.recover_throttle);
    assert_eq!(words.a108, -1.0);
    let fast = wingman_stick(Vec3::new(10.0, 0.0, 10.0), dive, 40.0, 0.0);
    assert!(!fast.recover_throttle, "fast enough: no recovery");

    // The orientation wrapper builds right/up/nose rows.
    let by_rows = wingman_stick_command(Quat::IDENTITY, Vec3::new(3.0, 3.0, -10.0), 60.0, 0.0);
    assert_eq!(
        by_rows,
        wingman_stick(Vec3::new(3.0, 3.0, 10.0), LEVEL_ROWS, 60.0, 0.0)
    );
}

/// The ported thresholds are the image's words.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_stick_constants_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let facts = engine_facts().expect("the committed table parses");
    let f32_at = |va: u64| {
        let at = usize::try_from(facts.image_offset(va).expect("va in image")).expect("offset");
        f32::from_le_bytes(loaded.bytes[at..at + 4].try_into().expect("4 bytes"))
    };
    let f64_at = |va: u64| {
        let at = usize::try_from(facts.image_offset(va).expect("va in image")).expect("offset");
        f64::from_le_bytes(loaded.bytes[at..at + 8].try_into().expect("8 bytes"))
    };
    assert_eq!(f32_at(0x61fb30), STICK_AIM_THRESHOLD);
    assert_eq!(f32_at(0x61fb34), STICK_AIM_THRESHOLD);
    assert_eq!(f32_at(0x61fb40), STICK_COARSE_FULL_ROLL_MIN);
    assert_eq!(f32_at(0x6034fc), STICK_LEVEL_GAIN);
    assert_eq!(f32_at(0x6034f8), STICK_RECOVER_FORWARD_Y_MAX);
    assert_eq!(f64_at(0x603500) as f32, STICK_LEVEL_FORWARD_Y_MAX);
}

fn position_of(app: &mut App, entity: Entity) -> Vec3 {
    app.world().get::<Position>(entity).expect("a pose").0
}

/// One hillside flight of the real M01 with the ported stick law and the
/// given pilot gains: `(spawn gap, closest, worst after 30 s, final)`.
fn fly_hillside(gains: PilotInputGains) -> (f32, f32, f32, f32) {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure").expect("plans");
    let stage = stage_for(&root, &plan).expect("stage reads");
    let mut app = build_headless(&stage).expect("composes");
    app.update();
    {
        let world = app.world_mut();
        for mut ai in world.query::<&mut MissionAircraftAi>().iter_mut(world) {
            ai.original_stick = true;
            ai.pilot_gains = gains;
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
    let spawn_gap = position_of(&mut app, betty).distance(position_of(&mut app, player));
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let hz = u64::from(BASELINE_FIXED_HZ);
    let (mut closest, mut worst_after) = (f32::MAX, 0.0_f32);
    for tick in 1..=45 * hz {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        let gap = position_of(&mut app, betty).distance(position_of(&mut app, player));
        closest = closest.min(gap);
        if tick > 30 * hz {
            worst_after = worst_after.max(gap);
        }
        if tick % (hz / 2) == 0 && std::env::var_os("CS_STICK_TRACE").is_some() {
            let b = position_of(&mut app, betty);
            let p = position_of(&mut app, player);
            println!("t={tick} gap={gap:.0} dy={:.0}", b.y - p.y);
        }
    }
    let final_gap = position_of(&mut app, betty).distance(position_of(&mut app, player));
    // The run must have been flown by the ported law, with Betty actually
    // moving.
    let flown = app
        .world()
        .get::<MissionAircraftAi>(betty)
        .expect("Betty's law");
    assert!(flown.original_stick && flown.follow_engaged && flown.decisions >= 45 * hz);
    assert_eq!(flown.pilot_gains, gains);
    teardown(&mut app);
    (spawn_gap, closest, worst_after, final_gap)
}

/// Betty flies the hillside of the real M01 with the AI ground blow on and
/// the ported stick law, once with each reading of the pilot gains/limits
/// (`docs/findings/2026-11-02-m01-wingman-gains.md`). Not a pin on the
/// outcome: the law does not hold station with either reading, which is why
/// `original_stick` stays off for the M01 pins.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_wingman_stick_betty_over_the_hillside() {
    for (label, gains) in [
        (
            "merged kind_of words (0.6 / 0.0 / 0.15, 100 / 100 / 0.25)",
            M01_WINGMAN_PILOT_GAINS,
        ),
        ("ai_input_* fallback (3.5 / 1.0)", PILOT_GAINS_AI_INPUT_KEYS),
    ] {
        let (spawn_gap, closest, worst_after, final_gap) = fly_hillside(gains);
        println!(
            "stick law [{label}]: spawn gap {spawn_gap:.0} m, closest {closest:.0} m, \
             worst after 30 s {worst_after:.0} m, final {final_gap:.0} m"
        );
        assert!(worst_after.is_finite() && closest.is_finite());
    }
}

/// The escape-climb latch's constants are the image's words, and the
/// `0x41f810` windows still carry the floor/ceiling globals, the re-arm
/// timer `+0xf0` and the `+0x358 = 3` store.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_escape_latch_constants_match_the_image() {
    use cs_app::mission_ai::{ESCAPE_LOOKAHEAD_S, ESCAPE_RAND_SCALE, ESCAPE_REARM_FACTOR};
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let facts = engine_facts().expect("the committed table parses");
    let at = |va: u64| usize::try_from(facts.image_offset(va).expect("va in image")).expect("off");
    let f32_at = |va: u64| {
        let i = at(va);
        f32::from_le_bytes(loaded.bytes[i..i + 4].try_into().expect("4 bytes"))
    };
    assert_eq!(f32_at(0x603644), ESCAPE_LOOKAHEAD_S);
    assert_eq!(f32_at(0x603598), ESCAPE_RAND_SCALE);
    assert_eq!(f32_at(0x6032e0), ESCAPE_REARM_FACTOR);
    assert_eq!(f32_at(0x6032dc), 1.0);
    let window = |va: u64, bytes: &[u8]| {
        let i = at(va);
        assert_eq!(
            &loaded.bytes[i..i + bytes.len()],
            bytes,
            "window at {va:#x}"
        );
    };
    // fcomp [0x71c3f0] (floor), fcomp [0x71c3f4] (ceiling), probe scale.
    window(0x41f827, &[0xd8, 0x1d, 0xf0, 0xc3, 0x71, 0x00]);
    window(0x41f83e, &[0xd8, 0x1d, 0xf4, 0xc3, 0x71, 0x00]);
    window(0x41f8b5, &[0xd8, 0x0d, 0x44, 0x36, 0x60, 0x00]);
    // fstp [esi+0xf0] re-arm store; mov [esi+0x358], 3 latch; clear to 0.
    window(0x41f8a7, &[0xd9, 0x9e, 0xf0, 0x00, 0x00, 0x00]);
    window(
        0x41f98e,
        &[0xc7, 0x86, 0x58, 0x03, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00],
    );
    window(
        0x41f9a6,
        &[0xc7, 0x86, 0x58, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
    );
    // The call to the probe 0x4c8f70.
    window(0x41f94a, &[0xe8, 0x21, 0x96, 0x0a, 0x00]);
    // The clock the timer rides on: [0x71c470] += [0x9ad744] at 0x4897cb.
    window(0x4897cb, &[0xd9, 0x05, 0x70, 0xc4, 0x71, 0x00]);
}
