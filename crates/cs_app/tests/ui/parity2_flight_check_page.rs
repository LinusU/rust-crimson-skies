//! Task #1728 `PARITY2-M01-FLIGHT-CHECK-PAGE`: the Flight Check page shows
//! the pilot and wingman names (`<nickname> (<long plane name>)`), the plane
//! silhouettes (the `FC_PlaneIcons.Png` frame the record's `plane_id` picks)
//! and the original button captions. Task test prefix:
//! `accept_parity2_m01_flight_check_page_`.
//!
//! The fixture tests drive `FlightCheckView::read_default_loadout` and
//! `FlightCheckScene::build` over invented strings and an invented layout in
//! the original's row format; the retail test reads M01 (`zbd/c3/m01`) and
//! pins the measured names, frame and captions the finding of
//! `docs/findings/2026-10-21-flight-check-page.md` records.

use std::path::PathBuf;

use cs_app::mission_session::{FlightCheckArtwork, FlightCheckScene, FlightCheckStrings};
use cs_app::ui::front_end::{Entry, FlightCheckView, FrontEndScreens, OriginalLayout};
use cs_content::ui_layout::Rect;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// A flight check in the original's row format with both plane panes, all six
/// buttons and the name rows. Names and numbers are invented.
const LAYOUT: &str = "\
[@FlightCheck@]
V1=GX,560
V2=V2,132
V3=V3,350
V4=V4,Paper.Png
V5=V5,0xFF112233

FC_BACKGROUND\t=P,Back.jpg,0,0,1,1,0,0,0
FC_P_PILOTPLANE =P,Icons.Png,144,190,0,12,0,1,1
FC_P_WINGPLANE  =P,Icons.Png,144,408,0,12,0,1,1
FC_B_FLYMISSION\t=B,Fly.png,551,<GX>,0,0,,,,
FC_B_RETURNBRIEF=B,Ret.png,341,<GX>,0,0,,0x1000,1
FC_B_CHANGEPLANE=B,<V4>,128,<V2>,0,IDS_FC_B_CHANGEPLANE,PlaneSelection,0x1000,1
FC_B_CHANGEAMMO =B,<V4>,274,<V2>,0,IDS_FC_B_CHANGEAMMO,OrdinanceLayout,0x1000,1
FC_B_CHANGEPLANEW=B,<V4>,128,<V3>,0,IDS_FC_B_CHANGEPLANE,PlaneSelection,0x1000,1
FC_B_CHANGEAMMOW=B,<V4>,274,<V3>,0,IDS_FC_B_CHANGEAMMO,OrdinanceLayout,0x1000,1
FC_T_TITLE\t=T,IDS_FC_TITLE,132,36,0,190,24,<V5>,1
FC_T_PILOT\t=T,IDS_FC_PILOT,138,102,0,94,20,<V5>,0
FC_T_PILOTPLANE =T,,236,106,0,300,16,<V5>,0
FC_T_WINGMAN\t=T,IDS_FC_WINGMAN,138,320,0,94,20,<V5>,0
FC_T_WINGMANPLANE=T,,236,323,0,300,16,<V5>,0
FC_T_MISSION    =T,,136,70,0,400,0,0xFF2D3843,0
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art {
        "Back.jpg" => Some((800, 600)),
        "Fly.png" | "Ret.png" => Some((180, 240)),
        "Paper.Png" => Some((130, 360)),
        "Icons.Png" => Some((80, 960)),
        _ => None,
    }
}

fn strings(id: &str) -> Option<String> {
    Some(
        match id {
            "IDS_FC_TITLE" => "Flight Check",
            "IDS_FC_PILOT" => "Pilot",
            "IDS_FC_WINGMAN" => "Wingman",
            "IDS_FC_B_CHANGEAMMO" => "Change Ammo",
            "IDS_FC_B_CHANGEPLANE" => "Change Plane",
            _ => return None,
        }
        .to_owned(),
    )
}

/// The string rows `read_default_loadout` reads, invented: the two nickname
/// ids, the item-number template and every label block.
fn fixture_strings(id: u32) -> Option<String> {
    Some(match id {
        511 => "]Ace One".to_owned(),
        512 => "]Wing Two".to_owned(),
        519 => "%1!d!)".to_owned(),
        3000..=3010 => format!("Fixture Plane {id}"),
        3320..=3325 | 3365..=3369 | 3395..=3406 => format!("Label {id}"),
        _ => return None,
    })
}

fn airframe(scene_root: &str) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Airframe, scene_root).expect("valid id"),
        Provenance::designed(ClaimId::new("p2fcp.test").expect("valid claim")),
    ))
}

fn view() -> FlightCheckView {
    let mut view = FlightCheckView::build(
        Entry::Known("Fixture Mission".to_owned()),
        &[],
        |_| Err("no such row".to_owned()),
        &airframe("player_pfighter"),
        &[airframe("player_pfighter")],
    );
    view.read_default_loadout(&fixture_strings);
    view
}

fn scene(view: &FlightCheckView) -> FlightCheckScene {
    let layout = OriginalLayout::parse(LAYOUT).expect("the layout parses");
    FlightCheckScene::build(&layout, view, &extents, &strings).expect("the scene builds")
}

/// The view's name lines compose `<nickname> (<long name>)` from the string
/// rows — the nickname's `]` marker and font tags stripped — and the icon
/// frame is the record's plane id.
#[test]
fn accept_parity2_m01_flight_check_page_the_view_composes_names_and_frames() {
    let view = view();
    let facts = cs_app::engine_facts::engine_facts()
        .expect("the committed facts read")
        .flight_check()
        .layout()
        .value("name_label_base");
    assert_eq!(facts, 3000, "the measured long-name base");
    assert_eq!(
        view.pilot.name.known().map(String::as_str),
        Some("Ace One (Fixture Plane 3005)"),
        "nickname row 511 + long-name row 3005"
    );
    assert_eq!(
        view.wingman.name.known().map(String::as_str),
        Some("Wing Two (Fixture Plane 3005)"),
        "nickname row 512 + long-name row 3005"
    );
    assert_eq!(view.pilot.icon_frame.known(), Some(&5));
    assert_eq!(view.wingman.icon_frame.known(), Some(&5));
}

/// The scene draws both plane panes at their layout positions with the
/// measured strip frame, the name rows carry the composed lines, and the
/// Change Ammo pills carry their original captions.
#[test]
fn accept_parity2_m01_flight_check_page_the_scene_draws_panes_names_and_captions() {
    let scene = scene(&view());
    assert_eq!(scene.panes.len(), 2, "both plane panes are drawn");
    let pilot = &scene.panes[0];
    assert_eq!(pilot.id, "FC_P_PILOTPLANE");
    assert_eq!(pilot.art, "Icons.Png");
    assert_eq!(
        pilot.rect,
        Rect {
            x: 144,
            y: 190,
            width: 80,
            height: 80
        },
        "one 80px frame of the 80x960 strip"
    );
    assert_eq!(pilot.frames, 12);
    assert_eq!(pilot.frame, 5, "the record's plane id is the frame");
    assert_eq!(scene.panes[1].id, "FC_P_WINGPLANE");
    assert_eq!(scene.panes[1].rect.y, 408);

    let text = |id: &str| {
        scene
            .texts
            .iter()
            .find(|text| text.id == id)
            .unwrap_or_else(|| panic!("{id} is drawn"))
    };
    assert_eq!(text("FC_T_PILOTPLANE").text, "Ace One (Fixture Plane 3005)");
    assert_eq!(
        text("FC_T_WINGMANPLANE").text,
        "Wing Two (Fixture Plane 3005)"
    );
    for (id, caption) in [
        ("FC_T_TITLE", "Flight Check"),
        ("FC_T_PILOT", "Pilot"),
        ("FC_T_WINGMAN", "Wingman"),
    ] {
        assert_eq!(text(id).text, caption);
    }
    for hotspot in ["FC_B_CHANGEAMMO", "FC_B_CHANGEAMMOW"] {
        let button = scene
            .buttons
            .iter()
            .find(|button| button.hotspot == hotspot)
            .expect("the pill is drawn");
        assert_eq!(button.label.as_deref(), Some("Change Ammo"));
    }
    // The strip is among the art the screen needs.
    assert!(
        scene
            .art_names()
            .iter()
            .any(|name| name.eq_ignore_ascii_case("Icons.Png")),
        "the icon strip is loaded: {:?}",
        scene.art_names()
    );
}

/// The script's `3 > JQ` gate: Change Plane pills are hidden while the choice
/// count is unmeasured, named as unknowns, and drawn with their caption once
/// the count reaches three.
#[test]
fn accept_parity2_m01_flight_check_page_change_plane_follows_the_choice_count() {
    let mut view = view();
    let hidden = scene(&view);
    for id in ["FC_B_CHANGEPLANE", "FC_B_CHANGEPLANEW"] {
        assert!(
            !hidden.buttons.iter().any(|button| button.hotspot == id),
            "{id} hidden while the count is unmeasured"
        );
        assert!(
            hidden.unknowns.iter().any(|unknown| unknown.row == id),
            "{id} is named among the unknowns"
        );
    }

    view.plane_choice_count = Some(2);
    let under = scene(&view);
    assert!(
        !under
            .buttons
            .iter()
            .any(|button| button.hotspot == "FC_B_CHANGEPLANE"),
        "two choices still hide the pill"
    );

    view.plane_choice_count = Some(3);
    let shown = scene(&view);
    let pill = shown
        .buttons
        .iter()
        .find(|button| button.hotspot == "FC_B_CHANGEPLANE")
        .expect("three choices draw the pill");
    assert_eq!(pill.label.as_deref(), Some("Change Plane"));
    assert!(
        !shown
            .unknowns
            .iter()
            .any(|unknown| unknown.row == "FC_B_CHANGEPLANE")
    );
}

/// A slot whose frame does not read is named unknown instead of guessing a
/// silhouette.
#[test]
fn accept_parity2_m01_flight_check_page_an_unread_frame_is_named_unknown() {
    let mut view = view();
    view.pilot.icon_frame = Entry::Unknown {
        reason: "the record's plane id did not read".to_owned(),
        resolving_task: "the roster record".to_owned(),
    };
    let scene = scene(&view);
    assert_eq!(scene.panes.len(), 1, "only the wingman pane draws");
    assert_eq!(scene.panes[0].id, "FC_P_WINGPLANE");
    assert!(
        scene
            .unknowns
            .iter()
            .any(|unknown| unknown.row == "FC_P_PILOTPLANE")
    );
}

/// **The retail half.** M01's own data: the measured names, the Devastator
/// frame on both panes and the original captions.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity2_m01_flight_check_page_retail_m01_names_silhouettes_captions() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let view = FlightCheckView::from_retail(&root, "zbd/c3/m01").expect("M01 reads");
    assert_eq!(
        view.pilot.name.known().map(String::as_str),
        Some("Gypsy Magic (Hughes P21-J MKIII Devastator)")
    );
    assert_eq!(
        view.wingman.name.known().map(String::as_str),
        Some("The Knave (Hughes P21-J MKIII Devastator)")
    );

    let screens = FrontEndScreens::open(&root).expect("the screen container opens");
    // The production `IDS_*` resolution (`RetailParts::read`): the shipped
    // resource headers' `#define`s into `langui.dll`'s rows.
    let ui = FlightCheckStrings::read(&root, &screens).expect("the UI strings read");
    let strings = |id: &str| ui.resolve(id);
    let scene = FlightCheckScene::from_retail(&screens, &view, &strings)
        .expect("the retail section builds a scene");
    assert_eq!(scene.panes.len(), 2);
    for (pane, id, y) in [
        (&scene.panes[0], "FC_P_PILOTPLANE", 190),
        (&scene.panes[1], "FC_P_WINGPLANE", 408),
    ] {
        assert_eq!(pane.id, id);
        assert_eq!(pane.art, "FC_PlaneIcons.Png");
        assert_eq!(pane.frames, 12);
        assert_eq!(pane.frame, 5, "the Devastator silhouette");
        assert_eq!(
            pane.rect,
            Rect {
                x: 144,
                y,
                width: 80,
                height: 80
            }
        );
    }
    let text = |id: &str| {
        scene
            .texts
            .iter()
            .find(|text| text.id == id)
            .unwrap_or_else(|| panic!("{id} is drawn"))
            .text
            .as_str()
    };
    assert_eq!(
        text("FC_T_PILOTPLANE"),
        "Gypsy Magic (Hughes P21-J MKIII Devastator)"
    );
    assert_eq!(
        text("FC_T_WINGMANPLANE"),
        "The Knave (Hughes P21-J MKIII Devastator)"
    );
    assert_eq!(text("FC_T_TITLE"), "FLIGHT CHECK");
    assert_eq!(text("FC_T_PILOT"), "PILOT");
    assert_eq!(text("FC_T_WINGMAN"), "WINGMAN");
    assert_eq!(text("FC_T_OBJTITLE"), "Objectives\n\n");
    assert_eq!(text("FC_T_GUNTITLEP"), "GUNS");
    assert_eq!(text("FC_T_GUNTITLEW"), "GUNS");
    assert_eq!(text("FC_T_ROCKETTITLEP"), "ROCKETS");
    assert_eq!(text("FC_T_ROCKETTITLEW"), "ROCKETS");
    for hotspot in ["FC_B_CHANGEAMMO", "FC_B_CHANGEAMMOW"] {
        let button = scene
            .buttons
            .iter()
            .find(|button| button.hotspot == hotspot)
            .unwrap_or_else(|| panic!("{hotspot} is drawn"));
        assert_eq!(button.label.as_deref(), Some("Change Ammo"));
    }

    // The strip decodes from the screen container.
    let artwork = FlightCheckArtwork::read(&screens, &scene).expect("the art decodes");
    assert!(artwork.get("FC_PlaneIcons.Png").is_some());
}
