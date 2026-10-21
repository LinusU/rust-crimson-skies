//! Task #1671 `M01-LC-FC-C`: the windowed pre-mission phase. The Flight Check
//! is built from the original layout's own format and the mission's view, and
//! the mission does not tick until Fly Mission is delivered through the
//! front-end machine. Task test prefix: `accept_m01_lc_fc_c_`.
//!
//! The synthetic layout is in the original's own row format with invented
//! names and numbers; the retail test reads the installation's real
//! `[FlightCheck]` section. The GPU test starts the real `cs --mission M01`
//! window and checks that it holds on the Flight Check.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use cs_app::mission_session::{
    CHANGE_AMMO_UNAVAILABLE, FlightCheckPhase, FlightCheckScene, PhaseResponse,
    RETURN_TO_BRIEFING_UNAVAILABLE, build_headless, install_flight_check, mission_ammunition_id,
};
use cs_app::mission_start::AIRFRAME_TABLE;
use cs_app::playtest::fixed_ticks;
use cs_app::ui::front_end::{
    Entry, FlightCheckView, FrontEndScreens, Loadout, OriginalLayout, Screen,
};
use cs_content::ui_layout::Rect;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// A flight check in the original's row format: a 640x480 background, a
/// fly-mission and a return button, two change-ammo buttons, one change-plane
/// button, text rows and the four lists. Names and numbers are invented.
const LAYOUT: &str = "\
[@FlightCheck@]
V1=GX,400
V4=V4,Paper.Png
V5=V5,0xFF112233

FC_BACKGROUND\t=P,Back.jpg,0,0,1,1,0,0,0
FC_B_FLYMISSION\t=B,Fly.png,500,<GX>,0,0,,,,
FC_B_RETURNBRIEF=B,Ret.png,300,<GX>,0,0,,0x1000,1
FC_B_CHANGEPLANE=B,<V4>,100,100,0,IDS_FC_B_CHANGEPLANE,PlaneSelection,0x1000,1
FC_B_CHANGEAMMO =B,<V4>,200,100,0,IDS_FC_B_CHANGEAMMO,OrdinanceLayout,0x1000,1
FC_B_CHANGEAMMOW=B,<V4>,200,300,0,IDS_FC_B_CHANGEAMMO,OrdinanceLayout,0x1000,1
FC_T_TITLE\t=T,IDS_FC_TITLE,10,10,0,190,24,<V5>,1
FC_T_MISSION    =T,,10,40,0,400,0,0xFF2D3843,0
FC_T_PILOTPLANE =T,,20,60,0,300,<STDTEXTH>,<V5>,0
FC_T_WINGMANPLANE=T,,20,260,0,300,<STDTEXTH>,<V5>,0
FC_T_OBJECTIVES =T,,400,60,0,200,360000,0xFF2D3843,0
FC_T_GUNLISTP   =A,20,80,0,150,16,0xFF121315,0,1
FC_T_ROCKETLISTW=A,20,280,0,150,16,0xFF121315,0,1
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art {
        "Back.jpg" => Some((640, 480)),
        "Fly.png" | "Ret.png" => Some((80, 80)),
        "Paper.Png" => Some((60, 120)),
        _ => None,
    }
}

fn strings(id: &str) -> Option<String> {
    Some(
        match id {
            "IDS_FC_TITLE" => "Flight Check",
            "IDS_FC_B_CHANGEAMMO" => "Change Ammo",
            "IDS_FC_B_CHANGEPLANE" => "Change Plane",
            _ => return None,
        }
        .to_owned(),
    )
}

fn airframe(scene_root: &str) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Airframe, scene_root).expect("valid id"),
        Provenance::designed(ClaimId::new("m01lcfcc.test").expect("valid claim")),
    ))
}

fn view(title: &str) -> FlightCheckView {
    let row = &AIRFRAME_TABLE[0];
    FlightCheckView::build(
        Entry::Known(title.to_owned()),
        &["MSG_ONE".to_owned(), "MSG_TWO".to_owned()],
        |key| match key {
            "MSG_ONE" => Ok("first objective".to_owned()),
            _ => Err("no such row".to_owned()),
        },
        &airframe(row.scene_root),
        &[],
    )
}

pub(crate) fn scene(title: &str) -> FlightCheckScene {
    let layout = OriginalLayout::parse(LAYOUT).expect("the layout parses");
    FlightCheckScene::build(&layout, &view(title), &extents, &strings).expect("the scene builds")
}

pub(crate) fn loadout() -> Loadout {
    Loadout {
        player: Some(
            ContentId::from_source(ContentKind::Airframe, AIRFRAME_TABLE[0].scene_root)
                .expect("valid id"),
        ),
        wingmate: None,
        ammunition: Some(mission_ammunition_id()),
    }
}

pub(crate) fn phase(title: &str) -> FlightCheckPhase {
    FlightCheckPhase::open(scene(title), loadout()).expect("the phase opens")
}

fn text<'a>(scene: &'a FlightCheckScene, id: &str) -> &'a cs_app::mission_session::SceneText {
    scene
        .texts
        .iter()
        .find(|text| text.id == id)
        .unwrap_or_else(|| panic!("{id} is drawn"))
}

#[test]
fn accept_m01_lc_fc_c_the_scene_is_the_layouts_and_the_views_with_unknowns_named() {
    let one = scene("Alpha - One");
    let two = scene("Beta - Two");
    assert_eq!(text(&one, "FC_T_MISSION").text, "Alpha - One");
    assert_eq!(text(&two, "FC_T_MISSION").text, "Beta - Two");
    assert_eq!(text(&one, "FC_T_TITLE").text, "Flight Check");
    assert_eq!(text(&one, "FC_T_TITLE").color, 0xFF11_2233);
    assert_eq!(
        text(&one, "FC_T_OBJECTIVES").text,
        "first objective\nunknown (MSG_TWO)"
    );
    // The name row draws the record's name line; this fixture view was built
    // without the measured defaults, so it is drawn as unknown.
    assert!(text(&one, "FC_T_PILOTPLANE").unknown);

    // The buttons sit where the layout states, one state frame tall.
    let fly = one
        .buttons
        .iter()
        .find(|button| button.hotspot == "FC_B_FLYMISSION")
        .expect("fly mission is drawn");
    assert_eq!(
        fly.rect,
        Rect {
            x: 500,
            y: 400,
            width: 80,
            height: 20
        }
    );
    // Four drawn buttons: the Change Plane pill is hidden while the
    // plane-choice count (native 2018's JQ) is unmeasured — the script's own
    // `3 > JQ` gate keeps it off the M01 Flight Check.
    assert_eq!(
        one.buttons.len(),
        4,
        "the layout's buttons minus Change Plane"
    );
    assert_eq!(
        one.buttons
            .iter()
            .find(|button| button.hotspot == "FC_B_CHANGEAMMO")
            .and_then(|button| button.label.as_deref()),
        Some("Change Ammo")
    );

    // What the data does not state is shown as unknown and listed: the hidden
    // Change Plane pill, both name lines and every gun and rocket list.
    // Nothing designed stands in.
    let rows: Vec<_> = one
        .unknowns
        .iter()
        .map(|unknown| unknown.row.as_str())
        .collect();
    assert_eq!(
        rows,
        [
            "FC_B_CHANGEPLANE",
            "FC_T_PILOTPLANE",
            "FC_T_WINGMANPLANE",
            "FC_T_GUNLISTP",
            "FC_T_ROCKETLISTW"
        ]
    );
    assert!(text(&one, "FC_T_GUNLISTP").unknown);
    assert_eq!(text(&one, "FC_T_GUNLISTP").text, "unknown");
}

#[test]
fn accept_m01_lc_fc_c_a_section_without_fly_mission_is_refused() {
    let layout =
        OriginalLayout::parse(&LAYOUT.replace("FC_B_FLYMISSION", "FC_B_NOTHING")).expect("parses");
    let error = FlightCheckScene::build(&layout, &view("T"), &extents, &strings)
        .expect_err("a screen that could never start the mission is refused");
    assert_eq!(
        error,
        cs_app::mission_session::FlightCheckSceneError::NoFlyMission
    );
}

#[test]
fn accept_m01_lc_fc_c_the_mission_does_not_tick_before_fly_mission_and_ticks_after() {
    let scratch = Scratch::new("fc_c_gate");
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    install_flight_check(&mut app, phase("Alpha"));

    for _ in 0..90 {
        app.update();
    }
    assert_eq!(
        fixed_ticks(&app),
        0,
        "the simulation must not advance while the Flight Check is up"
    );
    assert!(app.world().resource::<FlightCheckPhase>().is_pending());

    // Buttons that are drawn but do nothing yet release nothing, and say why.
    {
        let mut phase = app.world_mut().resource_mut::<FlightCheckPhase>();
        assert_eq!(
            phase.press_hotspot("FC_B_RETURNBRIEF"),
            PhaseResponse::Unavailable {
                hotspot: "FC_B_RETURNBRIEF".to_owned(),
                reason: RETURN_TO_BRIEFING_UNAVAILABLE
            }
        );
        assert_eq!(
            phase.press_hotspot("FC_B_CHANGEAMMOW"),
            PhaseResponse::Unavailable {
                hotspot: "FC_B_CHANGEAMMOW".to_owned(),
                reason: CHANGE_AMMO_UNAVAILABLE
            }
        );
        assert_eq!(phase.notice(), Some(CHANGE_AMMO_UNAVAILABLE));
        // A point on no button, and a hotspot the screen does not have.
        assert_eq!(phase.click((640, 480), 1, 1), PhaseResponse::NoButton);
        assert_eq!(phase.press_hotspot("FC_B_MISSING"), PhaseResponse::NoButton);
    }
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(fixed_ticks(&app), 0, "an unavailable button starts nothing");

    // Fly Mission, by a click on its hotspot in a window twice the logical
    // size, goes through the machine's Launch and LoadSucceeded rows.
    let response = {
        let mut phase = app.world_mut().resource_mut::<FlightCheckPhase>();
        phase.click((1280, 960), 2 * 520, 2 * 405)
    };
    assert_eq!(response, PhaseResponse::Launched);
    let phase = app.world().resource::<FlightCheckPhase>();
    assert_eq!(phase.front_end().screen(), Screen::Flight);
    assert!(!phase.is_pending());

    for _ in 0..30 {
        app.update();
    }
    assert!(
        fixed_ticks(&app) >= 1,
        "the mission must tick once Fly Mission is delivered, got {}",
        fixed_ticks(&app)
    );
    // The phase is over: a second press is not a second launch.
    let mut phase = app.world_mut().resource_mut::<FlightCheckPhase>();
    assert_eq!(phase.press_enter(), PhaseResponse::Over);
}

#[test]
fn accept_m01_lc_fc_c_enter_delivers_fly_mission() {
    let mut phase = phase("Alpha");
    assert!(phase.is_pending());
    assert_eq!(phase.press_enter(), PhaseResponse::Launched);
    assert_eq!(phase.front_end().screen(), Screen::Flight);
}

#[test]
fn accept_m01_lc_fc_c_an_invalid_loadout_is_refused_by_the_machine_and_holds_the_mission() {
    let mut empty = loadout();
    empty.ammunition = None;
    let mut phase = FlightCheckPhase::open(scene("Alpha"), empty).expect("opens");
    let response = phase.press_enter();
    assert!(
        matches!(response, PhaseResponse::Refused(_)),
        "the machine's own guard decides: {response:?}"
    );
    assert!(phase.is_pending(), "a refused launch holds the mission");
    assert!(phase.notice().is_some());
}

/// **The retail half.** The installation's own `[FlightCheck]` section and
/// art build the screen M01 shows.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_fc_c_retail_m01_screen_is_built_from_the_original_layout() {
    let root = game_dir();
    let view = FlightCheckView::from_retail(&root, "zbd/c3/m01").expect("M01 reads");
    let screens = FrontEndScreens::open(&root).expect("the screens open");
    let scene = FlightCheckScene::from_retail(&screens, &view, &|_| None)
        .expect("the retail section builds a scene");
    assert_eq!(scene.size, (800, 600), "the background is the 800x600 pane");
    let fly = scene
        .buttons
        .iter()
        .find(|button| button.hotspot == "FC_B_FLYMISSION")
        .expect("fly mission is drawn");
    assert_eq!((fly.rect.x, fly.rect.y), (551, 553));
    for id in [
        "FC_B_FLYMISSION",
        "FC_B_RETURNBRIEF",
        "FC_B_CHANGEAMMO",
        "FC_B_CHANGEAMMOW",
    ] {
        assert!(
            scene.buttons.iter().any(|button| button.hotspot == id),
            "{id} is drawn"
        );
    }
    // The script's `3 > JQ` gate: with the plane-choice count unmeasured the
    // Change Plane pills stay hidden, as on the owner capture, and are named
    // as unknowns instead.
    for id in ["FC_B_CHANGEPLANE", "FC_B_CHANGEPLANEW"] {
        assert!(
            !scene.buttons.iter().any(|button| button.hotspot == id),
            "{id} is hidden while the choice count is unmeasured"
        );
        assert!(
            scene.unknowns.iter().any(|unknown| unknown.row == id),
            "{id} is listed among the scene's unknowns"
        );
    }
    // The title the view reads is drawn in the mission-name row.
    let mission = text(&scene, "FC_T_MISSION");
    assert_eq!(Some(&mission.text), view.title.known());
    // The lists are the measured defaults, drawn as text, not as unknowns.
    for id in [
        "FC_T_GUNLISTP",
        "FC_T_ROCKETLISTP",
        "FC_T_GUNLISTW",
        "FC_T_ROCKETLISTW",
    ] {
        assert!(
            !scene.unknowns.iter().any(|unknown| unknown.row == id),
            "{:?}",
            scene.unknowns
        );
        assert!(!text(&scene, id).text.is_empty(), "{id} carries its lines");
    }
    // Every art file the scene draws decodes from the container.
    let artwork = cs_app::mission_session::FlightCheckArtwork::read(&screens, &scene)
        .expect("the art decodes");
    for name in scene.art_names() {
        assert!(artwork.get(name).is_some(), "{name} is decoded");
    }
}

/// **The GPU half.** The real `cs --mission M01` window opens on the Flight
/// Check and stays there: it prints the held-phase report, and a run that is
/// not given Fly Mission is still alive after the wait.
#[test]
#[ignore = "requires CS_GAME_DIR and a GPU (gpu capability)"]
fn accept_m01_lc_fc_c_gpu_the_window_holds_the_mission_on_the_flight_check() {
    let root = game_dir();
    let binary = PathBuf::from(env!("CARGO_BIN_EXE_cs"));
    let mut child = Command::new(binary)
        .args(["--cs-path"])
        .arg(&root)
        .args(["--mission", "M01"])
        .env("CS_CAPABILITIES", "retail,gpu,audio")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("cs starts");
    // A debug build takes a while to read the mission before the window
    // opens; the report line is printed once the phase is built.
    let (sender, lines) = std::sync::mpsc::channel();
    let stdout = child.stdout.take().expect("piped");
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    let started = Instant::now();
    let mut out = String::new();
    let mut held = false;
    while started.elapsed() < Duration::from_secs(420) && !held {
        if let Ok(line) = lines.recv_timeout(Duration::from_millis(500)) {
            held = line.contains("held until Fly Mission");
            out.push_str(&line);
            out.push('\n');
        }
    }
    // With the phase up and nobody delivering Fly Mission, the window must
    // stay open.
    let mut alive = held;
    let settle = Instant::now();
    while alive && settle.elapsed() < Duration::from_secs(8) {
        if child.try_wait().expect("polls").is_some() {
            alive = false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    // Only this test's own child is stopped, by its handle.
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
    while let Ok(line) = lines.try_recv() {
        out.push_str(&line);
        out.push('\n');
    }
    assert!(
        alive,
        "the window must still be up on the Flight Check, got:\n{out}"
    );
    assert!(
        out.contains("mission flight check:") && out.contains("held until Fly Mission"),
        "the held phase is reported:\n{out}"
    );
}
