//! Task #1750 `PARITY5-FRONTEND-BRIEFING-MAP`: the briefing map screen — the
//! original `Briefing.zrd` script compiled into a timeline against the
//! voice-over's markers, and the pre-mission phase that opens on it, replays it,
//! leads to the Flight Check and comes back. Task test prefix:
//! `accept_parity5_briefing_`.
//!
//! The scripts here are built from the record's own types with invented names
//! and numbers; the retail test reads M01's real script, voice-over cue points
//! and bitmaps. The windowed frames are the owner-command smoke's
//! (`docs/findings/2026-10-21-parity5-briefing-map.md`).

use cs_app::mission_session::{
    FlightCheckPhase, PhaseResponse, PreMissionStage, RETURN_TO_CABIN_UNAVAILABLE, read_briefing,
};
use cs_app::ui::front_end::{
    BriefingPlayback, BriefingScene, ElementKind, FrontEndScreens, Screen, Timeline, TimelineError,
};
use cs_content::briefing::{
    BackgroundImage, BriefingCommand as C, BriefingDialog, BriefingScript, DialogButton,
    DialogButtonKind, ObjectivesPanel, Point, SoundCue,
};

use super::m01_lc_fc_c::{loadout, scene as flight_check_scene};

fn p(x: i32, y: i32) -> Point {
    Point { x, y }
}

fn dialog() -> BriefingDialog {
    let button = |kind, x| DialogButton {
        kind,
        at: p(x, 540),
        bitmap: "btn".into(),
        label_text: format!("MSG_{}", kind.key()),
        label_offset: p(98, 4),
        normal_font: "n".into(),
        rollover_font: "r".into(),
        activate_font: "a".into(),
    };
    BriefingDialog {
        objectives: ObjectivesPanel {
            background_bitmap: "parch".into(),
            background_at: p(0, 295),
            title_text: "MSG_OBJ".into(),
            title_font: "t".into(),
            title_at: p(35, 315),
            list_font: "l".into(),
            list_at: p(35, 335),
            wordwrap: (185, 240),
            spacing: 5,
        },
        buttons: vec![
            button(DialogButtonKind::Replay, 100),
            button(DialogButtonKind::ReturnToCabin, 300),
            button(DialogButtonKind::FlightCheck, 500),
        ],
    }
}

fn pict(id: &str, at: Point) -> C {
    C::Pict {
        id: id.into(),
        bitmap: format!("bmp_{id}"),
        at,
        center: true,
    }
}

fn script(commands: Vec<C>) -> BriefingScript {
    let cue = SoundCue {
        name: "briefing_c9m9".into(),
        volume: 1.0,
        markers: true,
        start_marker: 0,
    };
    let mut all = vec![C::PlaySound(cue.clone())];
    all.extend(commands);
    BriefingScript {
        key: "brief_c99".into(),
        image_path: String::new(),
        background: vec![BackgroundImage {
            bitmap: "map".into(),
            at: p(0, 0),
        }],
        sound: Some(cue),
        commands: all,
    }
}

/// Markers at 10, 20 and 30 s.
const MARKERS: [f64; 3] = [10.0, 20.0, 30.0];

fn sample_script() -> BriefingScript {
    script(vec![
        C::WaitForMarker(0),
        pict("A", p(3, 229)),
        C::Fade {
            id: "A".into(),
            start: 0.0,
            end: 1.0,
            duration: 0.5,
        },
        C::Spin {
            id: "A".into(),
            start_revs: Some(-0.1),
            revs: 0.1,
            duration: 1.0,
        },
        C::Move {
            id: "A".into(),
            path: [p(3, 229), p(103, 129)],
            duration: 1.0,
        },
        C::On("A".into()),
        C::WaitForMarker(1),
        C::Fade {
            id: "OBJECTIVESLIST".into(),
            start: 0.0,
            end: 1.0,
            duration: 0.5,
        },
        C::ToBack("OBJECTIVESLIST".into()),
        C::On("OBJECTIVESLIST".into()),
        C::Wait(2.0),
        C::Objective {
            id: "T1".into(),
            index: 1,
        },
        C::On("T1".into()),
        C::WaitForMarker(2),
        C::Off("A".into()),
        C::Objective {
            id: "T0".into(),
            index: 0,
        },
        C::On("T0".into()),
    ])
}

fn ids(timeline: &Timeline, time: f64) -> Vec<String> {
    timeline
        .frame(time)
        .into_iter()
        .map(|element| element.id)
        .collect()
}

#[test]
fn accept_parity5_briefing_the_timeline_waits_for_markers_and_stamps_commands_with_their_time() {
    let timeline = Timeline::compile(&sample_script(), &MARKERS).expect("compiles");
    assert!(
        ids(&timeline, 0.0).is_empty(),
        "nothing before the first marker"
    );
    assert!(ids(&timeline, 9.99).is_empty());
    assert_eq!(ids(&timeline, 10.0), ["A"], "On at marker 0");
    assert_eq!(ids(&timeline, 19.9), ["A"]);
    // Marker 1 (20 s): the panel; the second marker moves them apart.
    let at_21 = ids(&timeline, 20.1);
    assert!(at_21.contains(&"A".to_owned()) && at_21.contains(&"OBJECTIVESLIST".to_owned()));
    // The objective line arrives 2 s after marker 1.
    let kinds = |time: f64| {
        timeline
            .frame(time)
            .into_iter()
            .filter_map(|element| match element.kind {
                ElementKind::Objective { index } => Some(index),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(kinds(21.9).is_empty());
    assert_eq!(kinds(22.0), [1]);
    // Marker 2 (30 s): A goes off, objective 0 arrives.
    assert_eq!(kinds(30.0), [1, 0]);
    assert!(!ids(&timeline, 30.0).contains(&"A".to_owned()));
    assert_eq!(timeline.sound_at(), Some(0.0));
    assert!((timeline.end() - 30.0).abs() < 1e-9, "{}", timeline.end());
}

#[test]
fn accept_parity5_briefing_fade_spin_and_move_follow_the_commands_numbers() {
    let timeline = Timeline::compile(&sample_script(), &MARKERS).expect("compiles");
    let at = |time: f64| {
        timeline
            .frame(time)
            .into_iter()
            .find(|element| element.id == "A")
            .expect("A is drawn")
    };
    let start = at(10.0);
    assert!(start.alpha.abs() < 1e-6, "the fade starts at 0");
    assert!(
        (start.rotation_revs + 0.1).abs() < 1e-6,
        "the spin starts at startrevs"
    );
    assert_eq!(start.position, (3.0, 229.0));
    assert!(start.centered);
    let half = at(10.25);
    assert!((half.alpha - 0.5).abs() < 1e-6, "half of the 0.5 s fade");
    assert!((half.position.0 - 28.0).abs() < 1e-4 && (half.position.1 - 204.0).abs() < 1e-4);
    assert!((half.rotation_revs + 0.05).abs() < 1e-6);
    let done = at(12.0);
    assert!((done.alpha - 1.0).abs() < 1e-6);
    assert!(
        (done.rotation_revs - 0.1).abs() < 1e-6,
        "the spin ends at revs and stays"
    );
    assert_eq!(
        done.position,
        (103.0, 129.0),
        "the move ends at its second point and stays"
    );
}

#[test]
fn accept_parity5_briefing_to_back_and_off_change_what_is_drawn_and_in_what_order() {
    let timeline = Timeline::compile(&sample_script(), &MARKERS).expect("compiles");
    let order = ids(&timeline, 25.0);
    assert_eq!(
        order,
        ["OBJECTIVESLIST", "A", "T1"],
        "ToBack sends the panel behind A; later elements stack above"
    );
    let later = ids(&timeline, 31.0);
    assert!(!later.contains(&"A".to_owned()), "Off hides A");
}

#[test]
fn accept_parity5_briefing_a_script_the_data_cannot_back_is_refused_by_name() {
    let unknown = script(vec![C::Unknown {
        name: "Shake".into(),
    }]);
    assert_eq!(
        Timeline::compile(&unknown, &MARKERS),
        Err(TimelineError::UnknownCommand("Shake".into()))
    );
    let orphan = script(vec![C::On("NOBODY".into())]);
    assert!(matches!(
        Timeline::compile(&orphan, &MARKERS),
        Err(TimelineError::UnknownElement { command: "On", .. })
    ));
    let waits = script(vec![C::WaitForMarker(7)]);
    assert_eq!(
        Timeline::compile(&waits, &MARKERS),
        Err(TimelineError::MissingMarker { wanted: 7, have: 3 })
    );
    let twice = script(vec![pict("A", p(0, 0)), pict("A", p(1, 1))]);
    assert_eq!(
        Timeline::compile(&twice, &MARKERS),
        Err(TimelineError::DuplicateElement("A".into()))
    );
}

#[test]
fn accept_parity5_briefing_playback_runs_follows_the_voice_and_replay_restarts_it() {
    let mut clock = BriefingPlayback::new();
    clock.advance(1.5);
    clock.advance(-3.0);
    assert!(
        (clock.time() - 1.5).abs() < 1e-12,
        "the clock never runs backwards"
    );
    clock.sync_to_voice(42.0);
    assert!((clock.time() - 42.0).abs() < 1e-12);
    clock.restart();
    assert_eq!(clock.time(), 0.0);
    assert_eq!(clock.restarts(), 1);
}

fn briefing_scene() -> BriefingScene {
    let sizes = |name: &str| match name {
        "map" => Some((800, 600)),
        "btn" => Some((196, 32)),
        _ => Some((10, 10)),
    };
    let strings = |id: &str| Some(format!("text of {id}"));
    BriefingScene::build(
        &dialog(),
        &sample_script(),
        &MARKERS,
        &sizes,
        &strings,
        vec!["first".into(), "second".into()],
    )
    .expect("the scene builds")
}

#[test]
fn accept_parity5_briefing_the_scene_is_the_records_with_its_unknowns_listed() {
    let scene = briefing_scene();
    assert_eq!(scene.size, (800, 600));
    assert_eq!(scene.script_key, "brief_c99");
    assert_eq!(scene.panel_title, "text of MSG_OBJ");
    assert_eq!(scene.buttons.len(), 3);
    let replay = scene.button(DialogButtonKind::Replay).expect("replay");
    assert_eq!(replay.label, "text of MSG_REPLAY");
    assert_eq!(
        (replay.rect.x, replay.rect.y, replay.rect.width),
        (100, 540, 196)
    );
    assert!(
        scene
            .unknowns
            .iter()
            .any(|unknown| unknown.what.contains("Fade, Spin and Move")),
        "the concurrency reading is a recorded unknown"
    );
    assert!(scene.bitmaps().iter().any(|name| name == "bmp_A"));
    // Objective lines are the mission's, by index.
    let drawn = scene.timeline.frame(31.0);
    assert_eq!(
        scene.objective_lines(&drawn),
        vec![(0, "first".to_owned()), (1, "second".to_owned())]
    );

    // A bitmap the data cannot size refuses the scene; a script without a
    // voice-over has nothing to play.
    let refusing = |name: &str| (name != "map").then_some((10, 10));
    assert!(
        BriefingScene::build(
            &dialog(),
            &sample_script(),
            &MARKERS,
            &refusing,
            &|_| None,
            vec![]
        )
        .is_err()
    );
    let mut mute = sample_script();
    mute.sound = None;
    assert!(
        BriefingScene::build(
            &dialog(),
            &mute,
            &MARKERS,
            &|_| Some((1, 1)),
            &|_| None,
            vec![]
        )
        .is_err()
    );
}

fn phase() -> FlightCheckPhase {
    FlightCheckPhase::open_with_briefing(flight_check_scene("Alpha"), briefing_scene(), loadout())
        .expect("the phase opens on the briefing")
}

#[test]
fn accept_parity5_briefing_the_phase_opens_on_the_briefing_and_holds_the_mission() {
    let phase = phase();
    assert_eq!(phase.stage(), PreMissionStage::Briefing);
    assert_eq!(phase.front_end().screen(), Screen::Briefing);
    assert!(
        phase.is_pending(),
        "the mission is held behind the briefing"
    );
    assert!(phase.briefing().is_some());
}

#[test]
fn accept_parity5_briefing_replay_restarts_the_clock_and_never_leaves_the_briefing() {
    let mut phase = phase();
    phase
        .briefing_mut()
        .expect("briefing")
        .playback_mut()
        .advance(50.0);
    let response = phase.press_briefing_button(DialogButtonKind::Replay);
    assert_eq!(response, PhaseResponse::Replayed);
    let stage = phase.briefing().expect("briefing");
    assert_eq!(stage.playback().time(), 0.0);
    assert_eq!(stage.playback().restarts(), 1);
    assert_eq!(phase.stage(), PreMissionStage::Briefing);
    assert!(phase.is_pending(), "replaying never starts the mission");
}

#[test]
fn accept_parity5_briefing_go_to_flight_check_opens_it_with_the_loadout_and_return_comes_back() {
    let mut phase = phase();
    assert_eq!(
        phase.press_briefing_button(DialogButtonKind::FlightCheck),
        PhaseResponse::FlightCheckOpened
    );
    assert_eq!(phase.stage(), PreMissionStage::FlightCheck);
    assert!(
        phase.is_pending(),
        "Go to Flight Check does not start the mission"
    );
    assert!(
        !phase.front_end().loadout().is_empty(),
        "the Flight Check opens with the loadout selected"
    );
    // Return to Briefing: the briefing plays again from its start.
    phase
        .briefing_mut()
        .expect("briefing")
        .playback_mut()
        .advance(9.0);
    assert!(matches!(
        phase.press_hotspot("FC_B_RETURNBRIEF"),
        PhaseResponse::BriefingOpened
    ));
    assert_eq!(phase.stage(), PreMissionStage::Briefing);
    assert_eq!(phase.briefing().expect("briefing").playback().time(), 0.0);
    // And forward again: the selection is made again.
    assert_eq!(
        phase.press_enter(),
        PhaseResponse::FlightCheckOpened,
        "Enter is Go to Flight Check on the briefing"
    );
    assert!(!phase.front_end().loadout().is_empty());
    // Fly Mission is the Flight Check's alone.
    assert_eq!(phase.press_enter(), PhaseResponse::Launched);
    assert!(!phase.is_pending());
    assert_eq!(phase.stage(), PreMissionStage::Released);
}

#[test]
fn accept_parity5_briefing_return_to_cabin_is_unavailable_until_a_hub_exists() {
    let mut without = phase();
    let response = without.press_briefing_button(DialogButtonKind::ReturnToCabin);
    assert!(matches!(
        response,
        PhaseResponse::Unavailable { reason, .. } if reason == RETURN_TO_CABIN_UNAVAILABLE
    ));
    assert_eq!(
        without.stage(),
        PreMissionStage::Briefing,
        "nothing to land on"
    );
    assert_eq!(
        without.briefing().and_then(|stage| stage.notice()),
        Some(RETURN_TO_CABIN_UNAVAILABLE)
    );

    let mut with = phase().with_cabin_hub();
    assert_eq!(
        with.press_briefing_button(DialogButtonKind::ReturnToCabin),
        PhaseResponse::CabinOpened
    );
    assert_eq!(with.front_end().screen(), Screen::Cabin);
    assert_eq!(with.stage(), PreMissionStage::Cabin);
    assert!(
        with.is_pending(),
        "the cabin does not start the mission either"
    );
}

#[test]
fn accept_parity5_briefing_clicks_hit_the_buttons_through_the_aspect_fit() {
    let mut phase = phase();
    // A 1600x900 window fits the 800x600 map at 1200x900, offset 200 px.
    let surface = (1600, 900);
    assert_eq!(phase.click(surface, 10, 10), PhaseResponse::NoButton);
    // Replay Briefing: logical (100..296, 540..572) -> x 200 + 1.5 * 150.
    assert_eq!(phase.click(surface, 425, 815), PhaseResponse::Replayed);
    assert_eq!(phase.briefing().expect("briefing").playback().restarts(), 1);
    // Go to Flight Check: logical x 500..696.
    assert_eq!(
        phase.click(surface, 1100, 815),
        PhaseResponse::FlightCheckOpened
    );
    assert_eq!(phase.stage(), PreMissionStage::FlightCheck);
}

#[test]
fn accept_parity5_briefing_escape_is_the_way_back_of_the_screen_showing() {
    let mut phase = phase();
    assert!(matches!(
        phase.press_back(),
        PhaseResponse::Unavailable { reason, .. } if reason == RETURN_TO_CABIN_UNAVAILABLE
    ));
    phase.press_enter();
    assert_eq!(phase.stage(), PreMissionStage::FlightCheck);
    assert_eq!(phase.press_back(), PhaseResponse::BriefingOpened);
    // A phase opened on the Flight Check alone has no briefing to go back to.
    let mut alone = super::m01_lc_fc_c::phase("Alpha");
    assert!(matches!(
        alone.press_back(),
        PhaseResponse::Unavailable { .. }
    ));
    assert_eq!(alone.stage(), PreMissionStage::FlightCheck);
}

// ---------------------------------------------------------------- retail --

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the retail installation"),
    )
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_briefing_retail_m01_reads_its_script_voice_markers_and_bitmaps() {
    let root = game_dir();
    let screens = FrontEndScreens::open(&root).expect("the screens open");
    let objectives: Vec<String> = (1..=4).map(|n| format!("objective {n}")).collect();
    let source = read_briefing(
        &root,
        "zbd/c3/m01",
        &screens,
        &|id| Some(id.to_owned()),
        objectives,
    )
    .expect("M01's briefing reads");

    let scene = &source.scene;
    assert_eq!(scene.script_key, "brief_c61");
    assert_eq!(scene.sound.name, "briefing_c1m1");
    assert_eq!(scene.size, (800, 600), "the map picture");
    assert_eq!(source.voice.member, "c1-HA-m1_briefing.wav");
    assert_eq!(scene.buttons.len(), 3);
    let rects: Vec<(u32, u32)> = scene.buttons.iter().map(|b| (b.rect.x, b.rect.y)).collect();
    assert_eq!(rects, [(197, 560), (397, 560), (597, 560)]);

    // Eight cue points whose ids are not in time order; marker n is the nth
    // earliest.
    assert_eq!(source.voice.cues.len(), 8);
    assert_eq!(scene.markers.len(), 8);
    assert!(scene.markers.windows(2).all(|pair| pair[0] < pair[1]));
    let earliest = source
        .voice
        .cues
        .iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .expect("cues");
    assert_eq!(
        earliest.0, 6,
        "cue id 6 is the earliest, at {} s",
        earliest.1
    );
    assert!(
        (scene.markers[0] - 14.4).abs() < 0.01,
        "{:?}",
        scene.markers
    );
    assert!(
        (scene.markers[7] - 94.5).abs() < 0.01,
        "{:?}",
        scene.markers
    );

    // The script plays to the end of the voice-over: its last move ends with
    // the sound (4.5 s after marker 7).
    assert!(
        (scene.timeline.end() - 99.0).abs() < 0.1,
        "{}",
        scene.timeline.end()
    );
    let seconds = source.voice.pcm.duration().as_secs_f64();
    assert!(seconds > 98.0 && seconds < 99.5, "voice length {seconds}");

    // The treasure map appears at marker 0 and the four objectives by the end.
    let before = scene.timeline.frame(scene.markers[0] - 0.1);
    assert!(before.is_empty(), "{before:?}");
    let treasure = scene.timeline.frame(scene.markers[0] + 1.0);
    assert!(treasure.iter().any(|e| e.id == "TREASUREMAP"));
    let end = scene.timeline.frame(scene.timeline.end());
    assert_eq!(scene.objective_lines(&end).len(), 4);

    // Every bitmap the screen draws was read.
    for name in scene.bitmaps() {
        assert!(source.artwork.get(&name).is_some(), "{name}");
    }
}
