//! Acceptance suite PARITY-M01-OUTCOME-A (#1653): the player as a damage
//! victim, and what the mission's own hostile aircraft do to a player who
//! gives no input.
//!
//! Three things this suite proves:
//!
//! 1. **The player can be hurt.** The composed mission registers the player
//!    with the shared damage resolver and hangs a swept box under the body, so
//!    the rounds a hostile aircraft fires through the production gun step can
//!    land (`mission_session::player_victim`).
//! 2. **The path works end to end on retail AI data.** In the `ZBD/C1C/M01`
//!    archive (work order M06, whose `rusdevastator` records are awake at the
//!    start) the designed pursuit-and-fire law closes on an idle player and
//!    its rounds lower the hull.
//! 3. **Retail M01 itself does not reach that path for an idle player.** Its
//!    Kestrels are woken only by the program's own `WAKEUP_ENEMIES`, at the end
//!    of a chain the player drives, so an idle player is never attacked
//!    (`docs/findings/2026-10-18-m01-hostile-engagement.md`).
//!
//! The hull pool, its box and the pursuit-and-fire law are **designed**, not
//! original measurements, and nothing here is fitted to the retail capture's
//! first damage at 1:37.

use bevy::prelude::{App, Time, With};
use bevy::time::TimeUpdateStrategy;

use cs_app::cli::{CliRequest, parse};
use cs_app::mission_actors::ScriptedAircraftActor;
use cs_app::mission_ai::MissionAircraftAi;
use cs_app::mission_launch::{declared_missions, plan_mission_launch};
use cs_app::mission_session::{
    Check, EngagementMetrics, MISSION_PLAYER_SERIAL, MISSION_TARGET_NODE, MissionHost,
    MissionPlayerBody, MissionStage, PlayerHullBox, PlayerVictim, SmokeMeasurements,
    build_headless, evaluate_mission_smoke, stage_for, teardown,
};
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::scene::NodeVisualTransform;
use cs_app::weapons::SessionDamageResolver;
use cs_app::weapons::part_sweep_candidates;
use cs_sim::damage::DamageNodeKey;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// How long the idle player is left alone, in simulated seconds. Longer than
/// the retail capture's 97 s so the run is judged on what happens, not on a
/// time fitted to that capture.
const IDLE_SECONDS: u64 = 120;

fn retail_stage() -> MissionStage {
    let root = game_dir();
    let (mission, title) = declared_missions()[0];
    let plan =
        plan_mission_launch(&root, label(mission), title).expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// The `ZBD/C1C/M01` stage (work order M06), whose hostile records start awake.
fn awake_hostiles_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M06's launch closure plans");
    stage_for(&root, &plan).expect("M06's stage reads through the production readers")
}

fn hull_integrity(app: &App) -> Option<f64> {
    let victim = app.world().get_resource::<PlayerVictim>()?;
    let hull = DamageNodeKey::new(MISSION_TARGET_NODE).expect("the hull key is valid");
    app.world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .remaining_integrity(&victim.actor, &hull)
}

/// **What an idle player meets in retail M01: the Kestrels stay asleep.**
///
/// The mission's own program wakes `medkestrel_1..3` only through
/// `WAKEUP_ENEMIES` in `OBJECTIVE23` (and `OBJECTIVE39`), at the end of a chain
/// the player drives (`OBJECTIVE2` is a `Travelers` approach to `t_chamber`
/// within 200 m, …). With no input the chain never reaches it, so no hostile
/// aircraft fires and the player's hull — a real damage victim, see
/// `accept_m01_outcome_a_the_player_is_a_damage_victim_with_a_live_hull_box` —
/// is untouched. This pins that measurement; it is **not** a claim about the
/// original, whose idle player took damage at 1:37 from a source this
/// repository has not identified
/// (`docs/findings/2026-10-18-m01-hostile-engagement.md`).
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_outcome_a_an_idle_player_never_wakes_the_kestrels() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let start = hull_integrity(&app).expect("the player's hull has a designed pool");
    let kestrels = |app: &mut App| -> Vec<(String, bool)> {
        let world = app.world_mut();
        world
            .query::<(&ScriptedAircraftActor, Option<&MissionAircraftAi>)>()
            .iter(world)
            .filter(|(actor, _)| actor.record.name.starts_with("medkestrel"))
            .map(|(actor, ai)| (actor.record.name.clone(), ai.is_some()))
            .collect()
    };
    let before = kestrels(&mut app);
    assert_eq!(before.len(), 3, "M01 spawns three Kestrels: {before:?}");
    assert!(
        before.iter().all(|(_, flown)| !flown),
        "the Kestrels start asleep, with no law attached: {before:?}"
    );

    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let steps = IDLE_SECONDS * u64::from(BASELINE_FIXED_HZ);
    for _ in 0..steps {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
    }

    let host = app.world().resource::<MissionHost>();
    assert_eq!(
        host.wake_enemies().applied(),
        0,
        "no WAKEUP_ENEMIES site fired for a player who gave no input"
    );
    let after = kestrels(&mut app);
    assert!(
        after.iter().all(|(_, flown)| !flown),
        "the Kestrels are still asleep after {IDLE_SECONDS} s: {after:?}"
    );
    assert_eq!(
        hull_integrity(&app),
        Some(start),
        "nothing damaged the idle player's hull in {IDLE_SECONDS} s"
    );
    teardown(&mut app);
}

/// **Awake hostile aircraft from the mission's own AI records damage an idle
/// player.** Their designed law closes on the player and the rounds they fire
/// lower the hull; the player sends no input and is never moved by the test.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_outcome_a_awake_hostiles_from_ai_data_damage_an_idle_player() {
    let stage = awake_hostiles_stage();
    let mut app = build_headless(&stage).expect("the stage composes headlessly");
    app.update();
    let start = hull_integrity(&app).expect("the player's hull has a designed pool");

    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    let hz = u64::from(BASELINE_FIXED_HZ);
    let mut first_damage_tick = None;
    for tick in 1..=IDLE_SECONDS * hz {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        if first_damage_tick.is_none() && hull_integrity(&app).is_some_and(|now| now < start) {
            first_damage_tick = Some(tick);
        }
        // A few seconds past the first hit is all the run needs.
        if first_damage_tick.is_some_and(|first| tick >= first + 5 * hz) {
            break;
        }
    }
    let end = hull_integrity(&app).expect("the pool is still readable");
    println!(
        "first damage at tick {first_damage_tick:?} ({:.1} s), hull {start} -> {end}",
        first_damage_tick.map_or(f64::NAN, |tick| tick as f64 / hz as f64)
    );
    assert!(
        first_damage_tick.is_some(),
        "an awake hostile's rounds damaged the idle player within {IDLE_SECONDS} s: hull \
         {start} -> {end}"
    );
    teardown(&mut app);
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

fn engaged(measured: EngagementMetrics) -> Vec<Check> {
    evaluate_mission_smoke(&SmokeMeasurements {
        no_teleport: true,
        engagement: Some(measured),
        ..SmokeMeasurements::default()
    })
}

/// **The player is a damage victim with a box a hostile round can sweep, and
/// the box follows the body.**
#[test]
fn accept_m01_outcome_a_the_player_is_a_damage_victim_with_a_live_hull_box() {
    let scratch = Scratch::new("m01-outcome-a-victim");
    let mut app = build_headless(&synthetic_stage(scratch.path())).expect("the stage composes");
    app.update();
    let victim = app
        .world()
        .get_resource::<PlayerVictim>()
        .expect("the composed mission registers the player as a victim")
        .clone();
    assert_eq!(victim.actor.serial, MISSION_PLAYER_SERIAL);
    assert!(hull_integrity(&app).is_some_and(|pool| pool > 0.0));

    // Fly a few ticks: the hull box must track the body, not stay at spawn.
    let timestep = app.world().resource::<Time<bevy::time::Fixed>>().timestep();
    for _ in 0..30 {
        *app.world_mut().resource_mut::<TimeUpdateStrategy>() =
            TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
    }
    let body = app
        .world_mut()
        .query_filtered::<&bevy::prelude::GlobalTransform, With<MissionPlayerBody>>()
        .single(app.world())
        .expect("one player body")
        .translation();
    let hull = app
        .world_mut()
        .query_filtered::<&NodeVisualTransform, With<PlayerHullBox>>()
        .single(app.world())
        .expect("one hull box")
        .global()
        .translation();
    assert!(
        body.distance(hull) < 1.0,
        "the hull box follows the body: {body} vs {hull}"
    );
    let swept = part_sweep_candidates(app.world(), 1.0 / 60.0, |_| None);
    assert!(
        swept
            .candidates
            .iter()
            .any(|candidate| candidate.target.actor == victim.actor),
        "a round sweeps the player's box: refused {:?}",
        swept.refused
    );
    teardown(&mut app);
}

/// **The engagement judge passes only a hull that fell, and says why.**
#[test]
fn accept_m01_outcome_a_the_engagement_check_judges_the_hull() {
    let hit = engaged(EngagementMetrics {
        watch_frames: 6_000,
        start_hull: Some(1_200.0),
        min_hull: Some(900.0),
        first_damage_frame: Some(3_000),
        samples: vec![(10, 1_200.0), (60, 900.0)],
    });
    let verdict = hit
        .iter()
        .find(|check| check.id == "hostiles_damage_idle_player")
        .expect("the engagement check is reported");
    assert!(verdict.passed, "{verdict:?}");
    assert!(
        verdict.detail.contains("first hit at t+50.0s"),
        "{verdict:?}"
    );

    let untouched = engaged(EngagementMetrics {
        watch_frames: 6_000,
        start_hull: Some(1_200.0),
        min_hull: Some(1_200.0),
        ..EngagementMetrics::default()
    });
    assert!(!untouched[0].passed, "{:?}", untouched[0]);

    let unreadable = engaged(EngagementMetrics::default());
    assert!(!unreadable[0].passed, "{:?}", unreadable[0]);
    assert!(unreadable[0].detail.contains("never readable"));
}

/// **`--smoke-engagement` belongs to a mission smoke and not to the
/// no-teleport mode.**
#[test]
fn accept_m01_outcome_a_the_cli_takes_the_engagement_flag_only_with_a_mission_smoke() {
    let ok = parse(words(&[
        "--cs-path",
        "/i",
        "--mission",
        "M01",
        "--smoke-seconds",
        "120",
        "--smoke-engagement",
    ]));
    let CliRequest::Mission(request) = ok else {
        panic!("parses as a mission request: {ok:?}");
    };
    assert!(request.smoke_engagement && !request.smoke_no_teleport);
    for bad in [
        words(&["--cs-path", "/i", "--mission", "M01", "--smoke-engagement"]),
        words(&[
            "--cs-path",
            "/i",
            "--mission",
            "M01",
            "--smoke-seconds",
            "120",
            "--smoke-engagement",
            "--smoke-no-teleport",
        ]),
    ] {
        assert!(
            matches!(parse(bad.clone()), CliRequest::Invalid { .. }),
            "{bad:?}"
        );
    }
}

/// **The windowed owner-command smoke in engagement mode reports the idle
/// player's hull honestly.** No input and no teleport; the report names the
/// mode, carries the hull samples and agrees with the exit status. Whether the
/// hull fell is the run's finding, not this test's assumption: retail M01's
/// Kestrels stay asleep for an idle player, so the check fails there and the
/// binary exits non-zero.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_m01_outcome_a_the_windowed_engagement_smoke_reports_the_hull() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the installation");
    let capabilities = std::env::var("CS_CAPABILITIES").expect("CS_CAPABILITIES lists gpu,audio");
    let engine_image = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE is set");
    let dir = std::env::var("CS_M01_ENGAGEMENT_OUT").map_or_else(
        |_| std::env::temp_dir().join("m01-smoke-engagement"),
        std::path::PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_cs"))
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .args([
            "--smoke-seconds",
            "60",
            "--smoke-engagement",
            "--capture-dir",
        ])
        .arg(&dir)
        .env("CS_CAPABILITIES", capabilities)
        .env("CS_ENGINE_IMAGE", engine_image)
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report = std::fs::read_to_string(dir.join(cs_app::mission_session::SMOKE_REPORT_FILE))
        .unwrap_or_else(|error| panic!("report.json is written: {error}\n{stderr}"));
    println!("{report}");
    for needle in [
        "\"mode\":\"engagement\"",
        "\"player_teleported\":false",
        "\"id\":\"hostiles_damage_idle_player\"",
        "\"human_play\":false",
        "\"placements\":[]",
        "\"start_hull\":1200",
    ] {
        assert!(report.contains(needle), "{needle} in {report}");
    }
    let passed = report.trim_end().ends_with("\"passed\":true}");
    assert_eq!(
        output.status.success(),
        passed,
        "the exit status agrees with the report: {output:?}\n{stderr}"
    );
}
