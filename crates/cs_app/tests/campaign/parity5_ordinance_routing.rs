//! Task #2070 (`F2066-C`): the Flight Check's Change Ammo buttons open the
//! `OrdinanceLayout` screen. Task test prefix:
//! `accept_parity5_ordinance_routing_`.
//!
//! * The synthetic tests run the production phase over layout text in the
//!   original's *format* (invented strings and art sizes) and the committed
//!   engine facts' fresh-campaign plane records. They prove the routing and
//!   the stores, not retail data; they run in CI.
//! * The `retail` test reads M01's real Flight Check and `OrdinanceLayout`
//!   rows from `$CS_GAME_DIR`; it is `#[ignore]`d because CI has no
//!   installation. The windowed frames are the owner-command smoke's.

use cs_app::engine_facts::engine_facts;
use cs_app::mission_session::{
    CHANGE_AMMO_UNAVAILABLE, FlightCheckPhase, OrdinanceSetup, PhaseResponse, PreMissionStage,
};
use cs_app::ui::front_end::ordinance_feed::RocketTable;
use cs_app::ui::front_end::ordinance_layout::{OrdinanceList, OrdinanceSide};
use cs_app::ui::front_end::{OriginalLayout, Screen};

use super::m01_lc_fc_c::phase as bare_phase;
use super::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

const ROWS: &str = "\
[GLOBALVARS]
G1=STDTEXTH,15
G2=STDITEMH,15
G3=FC_SLIDER,s.png
G4=FC_UP,u.png
G5=FC_DOWN,d.png
G6=GN_DROPUP,up.png
G7=GN_DROPDOWN,down.png

[@OrdinanceLayout@]
V1=Y,553
V2=V2,0xFF354560
V3=V3,136
V4=V4,410
V5=DROPWIDTH,148
V6=GUNS,5
V7=ROCKETS,12

OL_BACKGROUND		=P,bg.jpg,0,0,1,1,0,0,0
OL_B_ACCEPT=B,accept.png,341,<Y>,0,0,FlightCheck,0x1000,1,,,,,,
OL_B_CANCEL=B,cancel.png,551,<Y>,0,0,FlightCheck,0x1000,1,,,,,,
OL_T_TITLE			=T,IDS_OL_TITLE,132,36,0,190,24,<V2>,1
OL_T_AMMOTITLE		=T,IDS_OL_AMMO_TITLE,<V3>,74,0,150,<STDTEXTH>,<V2>,0
OL_T_AMMOCAPTION    =T,IDS_OL_AMMO_CAPTION,246,75,0,200,<STDTEXTH>,<V2>,0
OL_T_ROCKETTITLE	=T,IDS_OL_ROCKET_TITLE,<V3>,290,11,200,<STDTEXTH>,<V2>,0
OL_T_ROCKETCAPTION	=T,IDS_OL_ROCKET_CAPTION,214,291,11,200,<STDTEXTH>,<V2>,0
OL_T_AMMODESCTITLE	=T,IDS_OL_AMMODESC_TITLE,566,76,0,200,<STDTEXTH>,<V2>,0
OL_T_ROCKETDESCTITLE=T,IDS_OL_ROCKETDESC_TITLE,566,312,0,200,<STDTEXTH>,<V2>,0
OL_T_PLANEINFO		=T,,400,50,0,400,<STDTEXTH>,<V2>,0
OL_T_GunName0		=T,,142,105,11,200,<STDTEXTH>,0xFF121315,0
OL_T_GunName1		=T,,142,147,11,200,<STDTEXTH>,0xFF121315,0
OL_T_GunName2		=T,,142,189,11,200,<STDTEXTH>,0xFF121315,0
OL_T_GunName3		=T,,142,231,11,200,<STDTEXTH>,0xFF121315,0
OL_S_AMMODESC       =S,0x00000000,,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,566,96,0,172,190,,0xFF121315
OL_S_ROCKETDESC     =S,0x00000000,,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,566,332,0,172,199,,0xFF121315
OL_D_AMMO0      =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,120,155,<DROPWIDTH>,<STDITEMH>,<GUNS>
OL_D_AMMO1      =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,162,150,<DROPWIDTH>,<STDITEMH>,<GUNS>
OL_D_AMMO2      =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,204,145,<DROPWIDTH>,<STDITEMH>,<GUNS>
OL_D_AMMO3      =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,246,140,<DROPWIDTH>,<STDITEMH>,<GUNS>
OL_D_ROCKETS0   =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,320,120,<DROPWIDTH>,<STDITEMH>,<ROCKETS>
OL_D_ROCKETS1   =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,348,115,<DROPWIDTH>,<STDITEMH>,<ROCKETS>
OL_D_ROCKETS2   =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,376,110,<DROPWIDTH>,<STDITEMH>,<ROCKETS>
OL_D_ROCKETS3   =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V3>,404,105,<DROPWIDTH>,<STDITEMH>,<ROCKETS>
OL_D_ROCKETS4   =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V4>,320,120,<DROPWIDTH>,<STDITEMH>,<ROCKETS>
OL_D_ROCKETS5   =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V4>,348,115,<DROPWIDTH>,<STDITEMH>,<ROCKETS>
OL_D_ROCKETS6   =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V4>,376,110,<DROPWIDTH>,<STDITEMH>,<ROCKETS>
OL_D_ROCKETS7   =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V4>,404,105,<DROPWIDTH>,<STDITEMH>,<ROCKETS>
";

fn extent(art: &str) -> Option<(u32, u32)> {
    match art {
        "accept.png" | "cancel.png" => Some((100, 60)),
        "bg.jpg" => Some((800, 600)),
        _ => None,
    }
}

fn name(id: &str) -> Option<String> {
    Some(format!("[AB12]{id}"))
}

fn text(id: u32) -> Option<String> {
    Some(format!("s{id}"))
}

fn setup() -> OrdinanceSetup {
    let layout = OriginalLayout::parse(ROWS).expect("the fixture parses");
    OrdinanceSetup::build(&layout, &name, &text, &extent).expect("the setup builds")
}

fn phase() -> FlightCheckPhase {
    bare_phase("Alpha").with_ordinance(setup())
}

const SURFACE: (u32, u32) = (800, 600);

fn centre(rect: cs_app::ui::front_end::ShellRect) -> (u32, u32) {
    (
        (rect.x + rect.width as i32 / 2) as u32,
        (rect.y + rect.height as i32 / 2) as u32,
    )
}

/// The centre of the accept (0) or cancel (1) button's drawn frame.
fn button_centre(phase: &FlightCheckPhase, which: usize) -> (u32, u32) {
    let rect = phase.ordinance().expect("the screen").buttons()[which].rect;
    (rect.x + rect.width / 2, rect.y + rect.height / 2)
}

fn click_at(phase: &mut FlightCheckPhase, at: (u32, u32)) -> PhaseResponse {
    phase.click(SURFACE, at.0, at.1)
}

#[test]
fn accept_parity5_ordinance_routing_the_change_ammo_buttons_open_the_screen_per_side() {
    for (hotspot, side, first_gun) in [
        ("FC_B_CHANGEAMMO", OrdinanceSide::Pilot, "s3322"),
        ("FC_B_CHANGEAMMOW", OrdinanceSide::Wingmate, ""),
    ] {
        let mut phase = phase();
        assert_eq!(phase.stage(), PreMissionStage::FlightCheck);
        assert_eq!(
            phase.press_hotspot(hotspot),
            PhaseResponse::OrdinanceOpened(side)
        );
        assert_eq!(phase.stage(), PreMissionStage::Ordinance);
        assert!(phase.is_pending(), "the mission stays held");
        let stage = phase.ordinance_stage().expect("the screen is open");
        assert_eq!(stage.side(), side);
        assert_eq!(stage.screen().side(), side);
        assert_eq!(stage.screen().entered(), 0, "the Flight Check sets no XQ");
        if !first_gun.is_empty() {
            let names: Vec<_> = stage.screen().texts().into_iter().map(|t| t.text).collect();
            assert!(names.iter().any(|n| n == first_gun), "{names:?}");
        }
        // The machine stays on the Flight Check: no Launch is delivered.
        assert_eq!(phase.front_end().screen(), Screen::FlightCheck);
        // The Flight Check's own buttons are not live under the screen.
        assert_eq!(phase.press_hotspot("FC_B_FLYMISSION"), PhaseResponse::Over);
    }
}

#[test]
fn accept_parity5_ordinance_routing_without_the_screen_the_buttons_say_why() {
    let mut phase = bare_phase("Alpha");
    let response = phase.press_hotspot("FC_B_CHANGEAMMO");
    assert_eq!(
        response,
        PhaseResponse::Unavailable {
            hotspot: "FC_B_CHANGEAMMO".to_owned(),
            reason: CHANGE_AMMO_UNAVAILABLE
        }
    );
    assert_eq!(phase.stage(), PreMissionStage::FlightCheck);
}

#[test]
fn accept_parity5_ordinance_routing_escape_closes_a_list_then_leaves_without_committing() {
    let mut phase = phase();
    phase.press_hotspot("FC_B_CHANGEAMMO");
    let before = phase.ordinance_record(OrdinanceSide::Pilot).cloned();
    let open_at = {
        let stage = phase.ordinance_stage().expect("open");
        centre(stage.screen().ammo_box(0).expect("box").rect)
    };
    assert!(matches!(
        click_at(&mut phase, open_at),
        PhaseResponse::OrdinanceEdited(_)
    ));
    assert!(
        phase
            .ordinance_stage()
            .unwrap()
            .screen()
            .open_list()
            .is_some()
    );
    // Pick row 2 of the open list.
    let row_at = {
        let screen = phase.ordinance_stage().unwrap().screen();
        centre(screen.item_rect(OrdinanceList::Ammo, 0, 2).expect("row"))
    };
    assert!(matches!(
        click_at(&mut phase, row_at),
        PhaseResponse::OrdinanceEdited(_)
    ));
    assert_eq!(
        phase.ordinance_stage().unwrap().draft().working().ammo[0],
        2
    );
    // Open again, Esc closes the list only.
    click_at(&mut phase, open_at);
    assert!(matches!(
        phase.press_back(),
        PhaseResponse::OrdinanceEdited(_)
    ));
    assert_eq!(phase.stage(), PreMissionStage::Ordinance);
    // The second Esc leaves and drops the draft.
    assert_eq!(
        phase.press_back(),
        PhaseResponse::OrdinanceClosed { committed: false }
    );
    assert_eq!(phase.stage(), PreMissionStage::FlightCheck);
    assert_eq!(
        phase.ordinance_record(OrdinanceSide::Pilot).cloned(),
        before
    );
}

#[test]
fn accept_parity5_ordinance_routing_accept_applies_the_stores_and_the_next_visit_shows_them() {
    let mut phase = phase();
    phase.press_hotspot("FC_B_CHANGEAMMO");
    // Ammunition box 0: row 2. Rocket box 0: row 2 of the gated table.
    let (ammo_box, ammo_row, rocket_box, rocket_row, accept) = {
        let stage = phase.ordinance_stage().unwrap();
        let screen = stage.screen();
        (
            centre(screen.ammo_box(0).unwrap().rect),
            centre(screen.item_rect(OrdinanceList::Ammo, 0, 2).unwrap()),
            centre(screen.rocket_box(0).unwrap().rect),
            centre(screen.item_rect(OrdinanceList::Rocket, 0, 2).unwrap()),
            button_centre(&phase, 0),
        )
    };
    for at in [ammo_box, ammo_row, rocket_box, rocket_row] {
        assert!(matches!(
            click_at(&mut phase, at),
            PhaseResponse::OrdinanceEdited(_)
        ));
    }
    // Nothing is committed before accept.
    let facts = engine_facts().expect("committed table");
    let fresh = phase
        .ordinance_record(OrdinanceSide::Pilot)
        .unwrap()
        .clone();
    assert_ne!(phase.ordinance_stage().unwrap().draft().working(), &fresh);
    assert_eq!(
        click_at(&mut phase, accept),
        PhaseResponse::OrdinanceClosed { committed: true }
    );
    assert_eq!(phase.stage(), PreMissionStage::FlightCheck);
    let committed = phase.ordinance_record(OrdinanceSide::Pilot).unwrap();
    assert_eq!(committed.ammo[0], 2);
    // Row 2 of the table under the default gate shows the record `nth(2)`.
    let table = RocketTable::new(
        facts.ordinance(),
        cs_app::ui::front_end::ordinance_feed::ProgressGate::default(),
    );
    assert_eq!(committed.rockets[0], table.nth(2) as u32);
    // The wingmate's record is untouched.
    assert_ne!(
        phase
            .ordinance_record(OrdinanceSide::Wingmate)
            .unwrap()
            .ammo[0],
        99
    );
    // The next visit starts on the committed values.
    phase.press_hotspot("FC_B_CHANGEAMMO");
    let stage = phase.ordinance_stage().unwrap();
    assert_eq!(stage.screen().ammo_choice(0), Some(2));
    assert_eq!(stage.screen().rocket_choice(0), Some(2));
}

#[test]
fn accept_parity5_ordinance_routing_cancel_does_nothing_without_xq() {
    let mut phase = phase();
    phase.press_hotspot("FC_B_CHANGEAMMOW");
    let cancel = button_centre(&phase, 1);
    assert_eq!(click_at(&mut phase, cancel), PhaseResponse::NoButton);
    assert_eq!(phase.stage(), PreMissionStage::Ordinance);
}

#[test]
fn accept_parity5_ordinance_routing_the_screen_is_hit_tested_through_the_window_fit() {
    let mut phase = phase();
    phase.press_hotspot("FC_B_CHANGEAMMO");
    let at = {
        let stage = phase.ordinance_stage().unwrap();
        centre(stage.screen().ammo_box(0).unwrap().rect)
    };
    // A window twice the logical size: the same box is at twice the point.
    assert!(matches!(
        phase.click((1600, 1200), at.0 * 2, at.1 * 2),
        PhaseResponse::OrdinanceEdited(_)
    ));
    assert!(
        phase
            .ordinance_stage()
            .unwrap()
            .screen()
            .open_list()
            .is_some()
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_ordinance_routing_retail_m01_flight_check_opens_the_retail_screen() {
    let root = game_dir();
    let scratch = Scratch::new("ordinance_routing");
    let stage = synthetic_stage(scratch.path());
    let (mut phase, artwork) =
        FlightCheckPhase::from_retail(&root, "zbd/c3/m01", &stage).expect("the phase opens");
    let setup = phase
        .ordinance()
        .expect("the retail phase holds the screen");
    for name in setup.art_names() {
        assert!(artwork.get(name).is_some(), "{name} is decoded");
    }
    assert_eq!(
        phase.press_hotspot("FC_B_CHANGEAMMO"),
        PhaseResponse::OrdinanceOpened(OrdinanceSide::Pilot)
    );
    let stage = phase.ordinance_stage().unwrap();
    let texts = stage.screen().texts();
    assert!(
        texts
            .iter()
            .all(|line| !line.text.is_empty() || line.id.contains("PLANEINFO")),
        "every title and gun name resolves to the installation's text: {texts:?}"
    );
    assert!(stage.screen().row_texts(OrdinanceList::Ammo).len() == 5);
}
