//! Task #1656 `PARITY-M01-OUTCOME-D`: the debrief's `Replay Mission` button
//! starts a fresh mission session through the mission host's own restart
//! path. Task test prefix: `accept_parity_m01_outcome_d_`.
//!
//! The test runs the real crash flow of the composed mission until the
//! debrief is up, presses the button by its layout row id, and reads what the
//! composed host did: the screen gone, a new session generation reached
//! `mission_host_restart` (the code the playtest's `R` also takes), the
//! settled terminal cleared and no exit written. The windowed frames are the
//! owner-command smoke's (`docs/PLAYTEST.md`).

use bevy::app::AppExit;
use bevy::ecs::message::MessageCursor;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::{Entity, KeyCode};
use cs_app::mission_session::{
    DebriefResponse, DebriefScreen, MissionHost, MissionHostRestartFailure,
    MissionHostRestartReport, MissionHostRestartRequest, MissionTally, build_headless,
    press_debrief_hotspot, teardown,
};
use cs_sim::objectives::terminal::TerminalOutcome;

use super::accept_crash_c_crash_flow::{crash_the_player, exits, step};
use super::parity_m01_outcome_c::{authored_source, key_press};
use super::vs_m01_rt_window::{Scratch, synthetic_stage};

fn report_session(served: cs_types::net::SessionId) -> u64 {
    served.get()
}

#[test]
fn accept_parity_m01_outcome_d_replay_restarts_the_mission_under_a_new_generation() {
    let scratch = Scratch::new("outcome-d-replay");
    let mut app = build_headless(&synthetic_stage(scratch.path())).expect("the stage composes");
    app.insert_resource(authored_source());
    for _ in 0..130 {
        step(&mut app);
    }
    let mut cursor = MessageCursor::<AppExit>::default();
    let _ = exits(&app, &mut cursor);
    crash_the_player(&mut app);
    for _ in 0..240 {
        step(&mut app);
        if app
            .world()
            .contains_resource::<cs_app::playtest::fatal_contact::CrashSequence>()
        {
            break;
        }
    }
    key_press(&mut app);
    for _ in 0..6 {
        step(&mut app);
    }
    let failed = app.world().resource::<MissionHost>();
    assert_eq!(
        failed.terminal().map(|terminal| terminal.outcome),
        Some(TerminalOutcome::Failure)
    );
    let first_generation = failed.generation();
    let first_session = failed.served();
    assert!(app.world().contains_resource::<DebriefScreen>());
    assert!(
        app.world().resource::<MissionTally>().session() != 0,
        "the failed run has a tally of its own generation"
    );

    // A button that is not Replay leaves the screen and the generation alone.
    let response = press_debrief_hotspot(app.world_mut(), "SB_B_RETURNPC");
    assert!(matches!(
        response,
        Some(DebriefResponse::Unavailable { .. })
    ));
    assert!(app.world().contains_resource::<DebriefScreen>());

    // The key that handed the run over is let go, so the fresh session's
    // tally starts from nothing for a reason that is not a held trigger.
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Space,
        logical_key: Key::Space,
        state: ButtonState::Released,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
    step(&mut app);
    let response = press_debrief_hotspot(app.world_mut(), "SB_B_REPLAY");
    assert_eq!(response, Some(DebriefResponse::Replay));
    assert!(
        !app.world().contains_resource::<DebriefScreen>(),
        "the press closes the debrief"
    );
    assert!(
        app.world().contains_resource::<MissionHostRestartRequest>(),
        "the press raises the host's own restart request"
    );
    let mut exit_messages = 0;
    for _ in 0..4 {
        step(&mut app);
        exit_messages += exits(&app, &mut cursor);
    }
    assert_eq!(exit_messages, 0, "a replay never ends the process");
    assert!(
        !app.world().contains_resource::<MissionHostRestartRequest>(),
        "mission_host_restart consumed the request"
    );
    assert!(
        !app.world().contains_resource::<MissionHostRestartFailure>(),
        "the restart was not refused"
    );
    let report = app.world().resource::<MissionHostRestartReport>();
    assert_eq!(report.torn_down, first_generation);
    assert_ne!(report.started, first_generation, "a new generation");
    assert_ne!(report.served, first_session, "a new session id");
    let host = app.world().resource::<MissionHost>();
    assert_eq!(host.generation(), report.started);
    assert_eq!(host.served(), report.served);
    assert!(
        host.terminal().is_none(),
        "the fresh session starts running; the failure is not carried over"
    );
    assert!(!app.world().contains_resource::<DebriefScreen>());
    let tally = app.world().get_resource::<MissionTally>();
    assert!(
        tally.is_none_or(|tally| {
            tally.session() != report_session(host.served())
                || (tally.shots == 0 && tally.hits == 0)
        }),
        "no counts carried over: {tally:?} served {:?}",
        host.served()
    );
    teardown(&mut app);
}

#[test]
fn accept_parity_m01_outcome_d_without_a_screen_a_press_does_nothing() {
    let scratch = Scratch::new("outcome-d-no-screen");
    let mut app = build_headless(&synthetic_stage(scratch.path())).expect("the stage composes");
    assert_eq!(press_debrief_hotspot(app.world_mut(), "SB_B_REPLAY"), None);
    assert!(!app.world().contains_resource::<MissionHostRestartRequest>());
    teardown(&mut app);
}
