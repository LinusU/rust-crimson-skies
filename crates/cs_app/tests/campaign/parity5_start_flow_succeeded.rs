//! Task #1972: in a run the start flow handed over, a won mission shows the
//! Mission Succeeded debrief and its `Return to Cabin` hands back to the cabin
//! (it used to end the process). Task test prefix:
//! `accept_parity5_start_flow_succeeded_`.

use bevy::app::AppExit;
use bevy::ecs::message::MessageCursor;
use cs_app::mission_session::{
    DebriefResponse, DebriefScreen, MissionEnd, MissionLeft, ReturnToFrontEnd, build_headless,
    press_debrief_hotspot, teardown,
};
use cs_script::ir::Outcome;

use super::accept_crash_c_crash_flow::{exits, step};
use super::parity_m01_outcome_c::authored_source;
use super::vs_m01_rt_host_02::settling_stage;
use super::vs_m01_rt_window::Scratch;

fn won_run(name: &str, handed_over: bool, outcome: Outcome) -> (bevy::prelude::App, Scratch) {
    let scratch = Scratch::new(name);
    let mut app =
        build_headless(&settling_stage(scratch.path(), outcome)).expect("the stage composes");
    if handed_over {
        app.insert_resource(ReturnToFrontEnd);
    }
    app.insert_resource(authored_source());
    for _ in 0..6 {
        step(&mut app);
    }
    (app, scratch)
}

#[test]
fn accept_parity5_start_flow_succeeded_a_won_mission_shows_the_debrief_and_returns_to_the_cabin() {
    let (mut app, _scratch) = won_run("succeeded-cabin", true, Outcome::Succeeded);
    let screen = app
        .world()
        .get_resource::<DebriefScreen>()
        .expect("the won mission shows the debrief instead of ending");
    let title = screen
        .scene()
        .texts
        .iter()
        .find(|text| text.id == "SB_T_COMPLETETITLE")
        .expect("the scene draws the caption");
    assert_eq!(title.text, "Mission Completed");
    let cash = screen
        .scene()
        .texts
        .iter()
        .find(|text| text.id == "SB_T_CASH")
        .expect("the cash row");
    assert!(cash.unknown, "Cash Earned stays unknown");
    assert!(
        screen
            .scene()
            .unknowns
            .iter()
            .any(|unknown| unknown.row == "SB_T_COMPLETETITLE"),
        "the unphotographed variant is recorded"
    );

    let mut cursor = MessageCursor::<AppExit>::default();
    let _ = exits(&app, &mut cursor);
    let response = press_debrief_hotspot(app.world_mut(), "SB_B_RETURNPC");
    assert_eq!(response, Some(DebriefResponse::ReturnToCabin));
    assert_eq!(
        app.world().get_resource::<MissionLeft>(),
        Some(&MissionLeft(MissionEnd::ReturnedToCabin))
    );
    step(&mut app);
    assert!(
        exits(&app, &mut cursor) >= 1,
        "the window ends for the cabin"
    );
    teardown(&mut app);
}

/// Outside the start flow a win keeps the run's exit, as before.
#[test]
fn accept_parity5_start_flow_succeeded_without_the_start_flow_the_exit_is_unchanged() {
    let (mut app, _scratch) = won_run("succeeded-plain", false, Outcome::Succeeded);
    assert!(!app.world().contains_resource::<DebriefScreen>());
    teardown(&mut app);
}
