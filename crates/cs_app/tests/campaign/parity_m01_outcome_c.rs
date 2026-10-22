//! Task #1655 `PARITY-M01-OUTCOME-C`: the campaign journal's debrief screen
//! after a failed run (`Mission Failed` / `Mission Results`, the four stat rows,
//! the `Best to Date` / `Most Recent` tabs and the `Replay Mission`, `View All
//! Missions` and `Return to Cabin` buttons). Task test prefix:
//! `accept_parity_m01_outcome_c_`.
//!
//! The headless tests run the real crash flow of the composed mission and read
//! what the mission host does with the settled failure; the retail tests read
//! the installation's own `[ScrapBook]` layout and string image
//! (`$CS_GAME_DIR`, ignored in CI). The windowed frames are the owner-command
//! smoke's (`docs/findings/2026-10-22-parity-m01-outcome-c-debrief-screen.md`).

use std::collections::BTreeMap;

use bevy::app::AppExit;
use bevy::ecs::message::MessageCursor;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::{Entity, KeyCode};
use cs_app::mission_session::{
    Arg, DebriefResponse, DebriefRole, DebriefScreen, DebriefSource, M01_CASH_EARNED_UNKNOWN_CLAIM,
    MissionDebrief, MissionHost, build_headless, format_message, teardown,
};
use cs_app::ui::front_end::OriginalLayout;
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_types::content::Resolved;
use cs_types::evidence::ClaimId;

use super::accept_crash_c_crash_flow::{crash_the_player, exits, step};
use super::vs_m01_rt_window::{Scratch, synthetic_stage};

/// A `[ScrapBook]` section in the original's shape with authored numbers.
const LAYOUT: &str = "\
[@ScrapBook@]
V1=STATX,642
V2=TITLEX,417
SB_BACKGROUND=P,SB_BackGround.jpg,0,0,1,1,0,0,0
SB_STATCARD=P,SB_P_Card.png,403,297,13,2,0,2,1
SB_B_REPLAY =B,SB_B_PaperButton.png,594,505,0,IDS_SBTOC_B_REPLAY,,0x1000,,,,,,,,0xFF000000
SB_B_RETURNPC=B,GN_B_ReturntoCabin.png,593,561,0,0,PassengerCabin,0x1000,1,,,,,,
SB_T_NAMEANDAREA =T,,58,53,0,0,0,0xFF000000,0
SB_T_COMPLETETITLE =T,IDS_SB_MISSION_TITLE,<TITLEX>,339,14,0,0,0xFF000000,0
SB_T_TIMETITLE =T,IDS_SB_TIME_TITLE,<TITLEX>,412,14,0,0,0xFF000000,0
SB_T_TIME =T,,<STATX>,412,14,0,0,0xFF000000,0
SB_T_HIT =T,,<STATX>,436,14,0,0,0xFF000000,0
SB_T_CASH =T,,<STATX>,460,14,0,0,0xFF000000,0
SB_T_PLANES =T,,<STATX>,484,14,0,0,0xFF000000,0
";

pub(super) fn authored_source() -> DebriefSource {
    let extents = [
        ("sb_background.jpg", (800, 600)),
        ("sb_p_card.png", (354, 480)),
        ("sb_b_paperbutton.png", (130, 112)),
        ("gn_b_returntocabin.png", (200, 128)),
    ]
    .into_iter()
    .map(|(name, extent)| (name.to_owned(), extent))
    .collect();
    let strings: BTreeMap<String, String> = [
        ("IDS_SB_FAILURE", "Mission Failed"),
        ("IDS_SB_NAMEANDAREA", "%1!s! - %2!s!"),
        ("IDS_SB_PERCENTAGE_VAL", "%1!d!%%"),
        ("IDS_SB_CASH_VAL", "$%1!d!"),
        ("IDS_SB_PLANES_VAL", "%1!d!"),
        ("IDS_SB_TIME_TITLE", "Run Time"),
        ("IDS_SBTOC_B_REPLAY", "Replay Mission"),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_owned(), text.to_owned()))
    .collect();
    DebriefSource::from_parts(
        OriginalLayout::parse(LAYOUT).expect("the authored layout parses"),
        extents,
        strings,
        Some("Pilot".to_owned()),
        Some("The Mission".to_owned()),
    )
}

pub(super) fn key_press(app: &mut bevy::prelude::App) {
    app.world_mut().write_message(KeyboardInput {
        key_code: KeyCode::Space,
        logical_key: Key::Space,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

#[test]
fn accept_parity_m01_outcome_c_message_formats_follow_the_string_image() {
    let time = format_message("%1!02d!:%2!02d!", &[Arg::Number(1), Arg::Number(37)]);
    assert_eq!(time.as_deref(), Some("01:37"));
    let percent = format_message("%1!d!%%", &[Arg::Number(0)]);
    assert_eq!(percent.as_deref(), Some("0%"));
    let cash = format_message("$%1!d!", &[Arg::Number(0)]);
    assert_eq!(cash.as_deref(), Some("$0"));
    let title = format_message("%1!s! - %2!s!", &[Arg::Text("A"), Arg::Text("B")]);
    assert_eq!(title.as_deref(), Some("A - B"));
    // A missing argument, a text given to a number and a malformed format are
    // refused rather than printed.
    assert_eq!(format_message("%2!d!", &[Arg::Number(1)]), None);
    assert_eq!(format_message("%1!d!", &[Arg::Text("x")]), None);
    assert_eq!(format_message("%1d", &[Arg::Number(1)]), None);
}

#[test]
fn accept_parity_m01_outcome_c_a_failed_window_run_shows_the_debrief_instead_of_exiting() {
    let scratch = Scratch::new("outcome-c-debrief");
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
    assert!(
        !app.world().contains_resource::<DebriefScreen>(),
        "the crash sequence holds the run open: no debrief yet"
    );
    key_press(&mut app);
    let mut exit_messages = 0;
    for _ in 0..6 {
        step(&mut app);
        exit_messages += exits(&app, &mut cursor);
    }
    let terminal = app
        .world()
        .resource::<MissionHost>()
        .terminal()
        .expect("the key handed the run over")
        .clone();
    assert_eq!(terminal.outcome, TerminalOutcome::Failure);
    assert_eq!(
        exit_messages, 0,
        "a failed run in a window stays open on its debrief"
    );
    let debrief = terminal.debrief.expect("the debrief");
    let screen = app
        .world()
        .get_resource::<DebriefScreen>()
        .expect("the debrief screen replaced the exit");
    let text = |id: &str| {
        screen
            .scene()
            .texts
            .iter()
            .find(|text| text.id == id)
            .unwrap_or_else(|| panic!("the scene draws {id}"))
            .clone()
    };
    assert_eq!(text("SB_T_COMPLETETITLE").text, "Mission Failed");
    assert_eq!(text("SB_T_NAMEANDAREA").text, "Pilot - The Mission");
    assert_eq!(text("SB_T_TIME").text, debrief.run_time());
    assert_eq!(text("SB_T_HIT").text, "0%");
    assert_eq!(text("SB_T_PLANES").text, debrief.planes_downed.to_string());
    let cash = text("SB_T_CASH");
    assert!(
        cash.unknown && cash.text == "unknown",
        "no cash rule is measured, so the cash row says so"
    );
    assert!(
        screen
            .scene()
            .unknowns
            .iter()
            .any(|unknown| unknown.row == "SB_T_CASH")
    );
    teardown(&mut app);
}

#[test]
fn accept_parity_m01_outcome_c_a_run_without_the_screen_still_exits() {
    // The headless composition installs no source: the failure keeps its exit.
    let scratch = Scratch::new("outcome-c-no-screen");
    let mut app = build_headless(&synthetic_stage(scratch.path())).expect("the stage composes");
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
    let mut exit_messages = 0;
    for _ in 0..6 {
        step(&mut app);
        exit_messages += exits(&app, &mut cursor);
    }
    assert_eq!(exit_messages, 1);
    assert!(!app.world().contains_resource::<DebriefScreen>());
    teardown(&mut app);
}

#[test]
fn accept_parity_m01_outcome_c_buttons_report_instead_of_doing_nothing_silently() {
    let source = authored_source();
    let debrief = MissionDebrief {
        run_ticks: 71 * 60,
        ticks_per_second: 60,
        shots: 0,
        hits: 0,
        planes_downed: 0,
        cash_earned: Resolved::<u64>::unknown(
            ClaimId::new(M01_CASH_EARNED_UNKNOWN_CLAIM).expect("a claim id"),
            "no cash rule is measured",
        )
        .expect("the reason is not empty"),
    };
    let mut screen = source.screen_for(&debrief).expect("the screen builds");
    assert_eq!(
        screen
            .scene()
            .texts
            .iter()
            .find(|t| t.id == "SB_T_TIME")
            .unwrap()
            .text,
        "1:11"
    );
    let response = screen.press_hotspot("SB_B_RETURNPC");
    assert!(matches!(
        response,
        DebriefResponse::Unavailable {
            role: DebriefRole::ReturnToCabin,
            ..
        }
    ));
    assert!(
        screen
            .notice()
            .is_some_and(|notice| notice.contains("cabin"))
    );
    assert_eq!(
        screen.press_hotspot("NOT_A_BUTTON"),
        DebriefResponse::NoButton
    );
    // A click on the Replay button's region (800x600 shown at 1:1) hits it.
    let rect = screen
        .scene()
        .buttons
        .iter()
        .find(|button| button.role == DebriefRole::ReplayMission)
        .expect("the replay button")
        .rect;
    let response = screen.click((800, 600), rect.x + 4, rect.y + 4);
    assert_eq!(
        response,
        DebriefResponse::Replay,
        "Replay Mission asks the caller to restart the mission"
    );
    assert!(
        screen
            .notice()
            .is_none_or(|notice| !notice.contains("Replay"))
    );
}

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The retail screen of a run like the owner's second frame: 71 s, no shot
/// fired, nothing downed.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_outcome_c_retail_screen_states_the_owners_frame() {
    let (source, artwork) =
        DebriefSource::read(&game_dir(), "zbd/c3/m01").expect("the retail source reads");
    let debrief = MissionDebrief {
        run_ticks: 71 * 60,
        ticks_per_second: 60,
        shots: 0,
        hits: 0,
        planes_downed: 0,
        cash_earned: Resolved::<u64>::unknown(
            ClaimId::new(M01_CASH_EARNED_UNKNOWN_CLAIM).expect("a claim id"),
            "no cash rule is measured",
        )
        .expect("the reason is not empty"),
    };
    let screen = source
        .screen_for(&debrief)
        .expect("the retail screen builds");
    let scene = screen.scene();
    assert_eq!(scene.size, (800, 600));
    let text = |id: &str| {
        scene
            .texts
            .iter()
            .find(|text| text.id == id)
            .unwrap_or_else(|| panic!("the scene draws {id}"))
    };
    // The captions are the installation's own rows.
    assert_eq!(text("SB_T_COMPLETETITLE").text, "Mission Failed");
    assert_eq!(text("SB_T_RESULTSTITLE").text, "Mission Results");
    assert_eq!(text("SB_T_TIMETITLE").text, "Run Time");
    assert_eq!(text("SB_T_HITTITLE").text, "Gun Hit Ratio");
    assert_eq!(text("SB_T_CASHTITLE").text, "Cash Earned");
    assert_eq!(text("SB_T_PLANESTITLE").text, "Overall Planes Downed");
    // The title is `<player> - <mission>`.
    let title = &text("SB_T_NAMEANDAREA").text;
    assert!(
        title == "Nathan Zachary - The Lost Treasure",
        "the title line is {title:?}"
    );
    // The values.
    assert_eq!(text("SB_T_TIME").text, "1:11");
    assert_eq!(text("SB_T_HIT").text, "0%");
    assert_eq!(text("SB_T_PLANES").text, "0");
    assert!(text("SB_T_CASH").unknown);
    // The Rockets Expended row is left out, as the owner's frame leaves it out.
    assert!(scene.texts.iter().all(|text| !text.id.contains("ROCKETS")));
    // The rows sit where the layout states: the stat column at X 642, the
    // value rows 24 px apart from the Run Time row at 412.
    assert_eq!(text("SB_T_TIME").rect.x, 642);
    assert_eq!(text("SB_T_TIME").rect.y, 412);
    assert_eq!(text("SB_T_HIT").rect.y, 436);
    assert_eq!(text("SB_T_PLANES").rect.y, 484);
    // The buttons: labels from the string image, places from the layout.
    let button = |role: DebriefRole| {
        scene
            .buttons
            .iter()
            .find(|button| button.role == role)
            .unwrap_or_else(|| panic!("the scene draws {role:?}"))
    };
    let replay = button(DebriefRole::ReplayMission);
    assert_eq!(replay.label.as_deref(), Some("Replay Mission"));
    assert_eq!((replay.rect.x, replay.rect.y), (594, 505));
    assert_eq!(
        button(DebriefRole::BestToDate).label.as_deref(),
        Some("Best to Date")
    );
    assert_eq!(
        button(DebriefRole::MostRecent).label.as_deref(),
        Some("Most Recent")
    );
    let cabin = button(DebriefRole::ReturnToCabin);
    assert_eq!((cabin.rect.x, cabin.rect.y), (593, 561));
    let all = button(DebriefRole::ViewAllMissions);
    assert_eq!((all.rect.x, all.rect.y), (375, 560));
    // Every art the scene draws was decoded.
    for name in scene.art_names() {
        assert!(artwork.get(name).is_some(), "{name} is decoded");
    }
    // What the frames cannot show is listed, not drawn.
    for row in ["SB_T_COMPLETETITLE", "SB_B_BEST / SB_B_MOST", "SB_T_CASH"] {
        assert!(
            scene.unknowns.iter().any(|unknown| unknown.row == row),
            "{row} is listed as unknown"
        );
    }
    assert!(
        scene
            .unknowns
            .iter()
            .any(|unknown| unknown.row.contains("ROCKETS"))
    );
}
