//! BUG-M01-INTRO-RADIO-OVERLAP (#2010): the opening crew radio of the real
//! first mission (ZBD/C3/M01, "The Lost Treasure") starts after the launch
//! cinematic hands the player control, never over the cinematic.
//!
//! The control program's `OBJECTIVE1 BEGIN_DORMANT 2.0` wakes `snd_HI1Start`
//! (`docs/findings/2026-10-19-m01-launch-attackers.md`). The host's gameplay
//! clock (objective timers and the lifecycle clock) is held while
//! `LaunchCinematic` plays, so the two seconds are counted from the hand-over.
//! These members drive the real composed C3 M01 host and read the radio
//! outcomes of its own sound step. What is **not** claimed: the original's
//! exact offset between hand-over and first word (0 s or the authored 2 s) is
//! unmeasured; the owner's observation fixes only the ordering.

use bevy::app::App;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHostReport, RadioLineOutcome, build_headless, install_launch_cinematic,
    launch_cinematic_playing, stage_for, teardown,
};
use cs_app::physics::BASELINE_FIXED_HZ;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

/// The group C3 M01's `OBJECTIVE1` wake cues (not C2's `snd_NW1Start`).
const OPENING_GROUP: &str = "snd_HI1Start";

/// What one pumped run saw.
struct Run {
    /// The fixed tick on which the cinematic ended (0 without one).
    handover: u64,
    /// The first tick that queued an opening line, and the members in order.
    first_queued: Option<u64>,
    members: Vec<String>,
}

fn run(cinematic_ticks: Option<u64>) -> Run {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app: App = build_headless(&stage).expect("M01's stage composes headlessly");
    if let Some(ticks) = cinematic_ticks {
        install_launch_cinematic(&mut app, ticks).expect("the phase installs");
    }
    let timestep = app
        .world()
        .resource::<bevy::time::Time<bevy::time::Fixed>>()
        .timestep();
    let mut handover = 0;
    let mut first_queued = None;
    let mut members = Vec::new();
    let mut seen = 0;
    for _ in 0..20_000 {
        *app.world_mut()
            .resource_mut::<bevy::time::TimeUpdateStrategy>() =
            bevy::time::TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        let playing = launch_cinematic_playing(app.world());
        let Some(report) = app.world().get_resource::<MissionHostReport>() else {
            continue;
        };
        let tick = report.tick.0;
        if tick == seen {
            continue;
        }
        seen = tick;
        if cinematic_ticks.is_some() && !playing && handover == 0 {
            handover = tick;
        }
        let produced = report.answer.as_ref().expect("the composed step runs");
        for outcome in &produced.radio {
            if let RadioLineOutcome::Queued { group, member, .. } = outcome
                && group == OPENING_GROUP
            {
                first_queued.get_or_insert(tick);
                members.push(member.clone());
            }
        }
        if first_queued.is_some() && tick > first_queued.unwrap_or(0) + 4 {
            break;
        }
    }
    teardown(&mut app);
    Run {
        handover,
        first_queued,
        members,
    }
}

/// Without a cinematic the opening speech starts at `OBJECTIVE1`'s own two
/// seconds, the path the skip/bypass case takes.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_radio_handover_a_without_a_cinematic_gameplay_begins_at_once() {
    let run = run(None);
    let at = run.first_queued.expect("the opening group queues lines");
    let wake = 2 * u64::from(BASELINE_FIXED_HZ);
    assert!(
        (wake..wake + 8).contains(&at),
        "the speech follows the authored 2.0 s wake: tick {at}"
    );
    assert!(
        run.members.iter().all(|m| m.starts_with("VO_c1-HA-m1_")),
        "C3 M01's voices are the c1-HA-m1 set: {:?}",
        run.members
    );
}

/// A cinematic longer than the wake: nothing is queued while it plays, and the
/// speech starts the authored two seconds after the hand-over. Fails on the
/// old behaviour, which queued the lines at tick ~240 inside the cinematic.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_radio_handover_b_the_speech_waits_for_the_handover() {
    let run = run(Some(1_000));
    let at = run.first_queued.expect("the opening group queues lines");
    assert!(run.handover > 0, "the cinematic ended");
    assert!(
        at >= run.handover,
        "no opening line before the hand-over: queued {at}, handover {}",
        run.handover
    );
    let wake = 2 * u64::from(BASELINE_FIXED_HZ);
    assert!(
        (run.handover + wake..run.handover + wake + 8).contains(&at),
        "the authored 2.0 s counts from the hand-over: queued {at}, handover {}",
        run.handover
    );
    let mut sorted = run.members.clone();
    sorted.dedup();
    assert_eq!(sorted, run.members, "no line is queued twice");
}

/// The measured 40.15 s cinematic.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_radio_handover_c_the_measured_cinematic_holds_the_speech() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let ticks = stage
        .launch_cinematic
        .clone()
        .expect("the measured length")
        .ticks(BASELINE_FIXED_HZ);
    drop(plan);
    drop(stage);
    let run = run(Some(ticks));
    assert_eq!(run.handover, ticks, "the hand-over is the measured length");
    let at = run.first_queued.expect("the opening group queues lines");
    assert!(
        at >= ticks,
        "no opening line over the cinematic: queued {at}"
    );
}
