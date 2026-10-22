//! Task #1831 `PARITY5-START-FLOW.03`, mission side: in a run the start flow
//! handed over, the failure debrief's `Return to Cabin` ends the flight and
//! records how it was left, so the front end can open on the cabin. Task test
//! prefix: `accept_parity5_start_flow_03_`. The front-end half is
//! `tests/ui/parity5_start_flow_03.rs`.

use bevy::app::AppExit;
use bevy::ecs::message::MessageCursor;
use cs_app::mission_session::{
    DebriefResponse, DebriefScreen, MissionEnd, MissionHostRestartRequest, MissionLeft,
    ReturnToFrontEnd, build_headless, press_debrief_hotspot, teardown,
};

use super::accept_crash_c_crash_flow::{crash_the_player, exits, step};
use super::parity_m01_outcome_c::{authored_source, key_press};
use super::vs_m01_rt_window::{Scratch, synthetic_stage};

fn failed_run(name: &str, handed_over: bool) -> (bevy::prelude::App, Scratch) {
    let scratch = Scratch::new(name);
    let mut app = build_headless(&synthetic_stage(scratch.path())).expect("the stage composes");
    if handed_over {
        app.insert_resource(ReturnToFrontEnd);
    }
    app.insert_resource(authored_source());
    for _ in 0..130 {
        step(&mut app);
    }
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
    assert!(app.world().contains_resource::<DebriefScreen>());
    (app, scratch)
}

#[test]
fn accept_parity5_start_flow_03_return_to_cabin_ends_the_flight_for_the_front_end() {
    let (mut app, _scratch) = failed_run("start-flow-03-cabin", true);
    let mut cursor = MessageCursor::<AppExit>::default();
    let _ = exits(&app, &mut cursor);

    let response = press_debrief_hotspot(app.world_mut(), "SB_B_RETURNPC");
    assert_eq!(response, Some(DebriefResponse::ReturnToCabin));
    assert!(
        !app.world().contains_resource::<DebriefScreen>(),
        "the debrief closes"
    );
    assert_eq!(
        app.world().get_resource::<MissionLeft>(),
        Some(&MissionLeft(MissionEnd::ReturnedToCabin))
    );
    assert!(
        !app.world().contains_resource::<MissionHostRestartRequest>(),
        "leaving is not a replay"
    );
    step(&mut app);
    assert!(
        exits(&app, &mut cursor) >= 1,
        "the window ends so the front end can take the process"
    );
    teardown(&mut app);
}

/// Without the start flow (`cs --mission`) the button still has no cabin and
/// says so; the run is not left.
#[test]
fn accept_parity5_start_flow_03_without_the_start_flow_there_is_no_cabin() {
    let (mut app, _scratch) = failed_run("start-flow-03-no-cabin", false);
    let response = press_debrief_hotspot(app.world_mut(), "SB_B_RETURNPC");
    assert!(matches!(
        response,
        Some(DebriefResponse::Unavailable { .. })
    ));
    assert!(app.world().contains_resource::<DebriefScreen>());
    assert!(app.world().get_resource::<MissionLeft>().is_none());
    teardown(&mut app);
}

/// Replay Mission in a handed-over run still restarts in place: no leave.
#[test]
fn accept_parity5_start_flow_03_replay_stays_in_the_process() {
    let (mut app, _scratch) = failed_run("start-flow-03-replay", true);
    let response = press_debrief_hotspot(app.world_mut(), "SB_B_REPLAY");
    assert_eq!(response, Some(DebriefResponse::Replay));
    assert!(app.world().get_resource::<MissionLeft>().is_none());
    assert!(app.world().contains_resource::<MissionHostRestartRequest>());
    teardown(&mut app);
}
