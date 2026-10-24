//! Acceptance task #2062: the Instant Action `OrdinanceLayout` screen. Task
//! test prefix: `accept_parity5_ordinance_layout_`.
//!
//! * The synthetic tests run over layout text in the original's *format* (the
//!   real `[@OrdinanceLayout@]` columns, invented strings). They prove the
//!   screen's rules; they run in CI and prove nothing about retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows and strings.
//!   `#[ignore]`d because CI has no installation.

use cs_app::ui::front_end::OriginalLayout;
use cs_app::ui::front_end::construction_slots::SlotError;
use cs_app::ui::front_end::ordinance_layout::{
    AMMO_BOXES, FALLBACK_DESCRIPTION, GREYED, GunPosition, NO_GUN_KIND, OrdinanceFeed,
    OrdinanceLayoutScreen, OrdinanceList, OrdinancePress, OrdinanceSide, ROCKET_BOXES,
};

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

fn names(name: &str) -> Option<String> {
    Some(format!("[AB12]{name}"))
}

fn screen(text: &str, entered: i32) -> Result<OrdinanceLayoutScreen, SlotError> {
    let layout = OriginalLayout::parse(text).expect("the fixture parses");
    OrdinanceLayoutScreen::from_layout(&layout, OrdinanceSide::Pilot, entered, &names)
}

fn feed() -> OrdinanceFeed {
    let gun = |kind, name: &str, choice| GunPosition {
        kind,
        name: name.to_owned(),
        choice,
    };
    OrdinanceFeed {
        plane_info: Some("Plane".to_owned()),
        // Guns 0, 2 and 3 are fitted; position 1 is empty (kind 5).
        guns: vec![
            gun(0, "Gun A", 1),
            gun(NO_GUN_KIND, "None", 0),
            gun(2, "Gun C", 3),
            gun(3, "Gun D", 0),
        ],
        ammo_rows: (0..5).map(|i| format!("ammo {i}")).collect(),
        rocket_rows: (0..12).map(|i| format!("rocket {i}")).collect(),
        // Boxes 0, 1 and 4 are live.
        rockets: vec![Some(2), Some(7), None, None, Some(0), None, None, None],
    }
}

fn point_in_row(
    screen: &OrdinanceLayoutScreen,
    list: OrdinanceList,
    slot: usize,
    row: usize,
) -> (i32, i32) {
    let r = screen.item_rect(list, slot, row).expect("row rect");
    (r.x + 2, r.y + 2)
}

#[test]
fn accept_parity5_ordinance_layout_a_the_rows_give_four_ammo_and_eight_rocket_boxes() {
    let s = screen(ROWS, 0).expect("the rows read");
    for slot in 0..AMMO_BOXES {
        let b = s.ammo_box(slot).expect("four ammunition boxes");
        assert_eq!((b.displayed, b.rect.width, b.rect.x), (5, 148, 136));
    }
    assert!(s.ammo_box(AMMO_BOXES).is_none());
    for slot in 0..ROCKET_BOXES {
        let b = s.rocket_box(slot).expect("eight rocket boxes");
        assert_eq!(b.displayed, 12);
        // Two columns of four: 136 and 410.
        assert_eq!(b.rect.x, if slot < 4 { 136 } else { 410 });
    }
    assert!(s.rocket_box(ROCKET_BOXES).is_none());
    assert_eq!(s.ammo_box(2).map(|b| b.rect.y), Some(204));
    assert_eq!(s.rocket_box(7).map(|b| b.rect.y), Some(404));
    assert_eq!(s.ammo_description_rect().x, 566);
    assert_eq!(s.rocket_description_rect().y, 332);
    assert_eq!((s.accept_button().x, s.accept_button().y), (341, 553));
    assert_eq!(s.accept_button().art, "accept.png");
    assert_eq!(s.cancel_button().x, 551);
    assert_eq!(
        s.texts().len(),
        7,
        "seven titles until the feed names the rest"
    );
}

#[test]
fn accept_parity5_ordinance_layout_b_a_missing_row_is_refused() {
    let text = ROWS.replace("OL_D_ROCKETS7", "OL_D_ROCKETSX");
    assert_eq!(
        screen(&text, 0).err(),
        Some(SlotError::RowMissing("OL_D_ROCKETS7".to_owned()))
    );
    let text = ROWS.replace("[@OrdinanceLayout@]", "[@Elsewhere@]");
    assert!(matches!(screen(&text, 0), Err(SlotError::NoSection(_))));
}

#[test]
fn accept_parity5_ordinance_layout_c_init_greys_missing_guns_and_deactivates_boxes() {
    let mut s = screen(ROWS, 0).expect("rows");
    assert_eq!(s.ammo_description_entry(), FALLBACK_DESCRIPTION);
    s.init(feed());
    assert!(s.ammo_active(0) && !s.ammo_active(1) && s.ammo_active(2));
    assert!(s.rocket_active(0) && s.rocket_active(4) && !s.rocket_active(2));
    let texts = s.texts();
    let line = |text: &str| {
        texts
            .iter()
            .find(|t| t.text == text)
            .expect("name line")
            .clone()
    };
    assert_eq!(line("None").color, GREYED);
    assert_eq!(line("Gun A").color, 0xFF12_1315);
    assert_eq!(line("Gun A").rect.y, 105);
    assert_eq!(line("Plane").rect.x, 400);
    // The descriptions start on the first live box's choice.
    assert_eq!(s.ammo_description_entry(), 1);
    assert_eq!(s.rocket_description_entry(), 2);
    assert_eq!(s.box_text(OrdinanceList::Ammo, 2), Some("ammo 3"));
    assert_eq!(s.box_text(OrdinanceList::Rocket, 1), Some("rocket 7"));
    assert_eq!(s.box_text(OrdinanceList::Rocket, 2), None);
    // No live box at all: both descriptions start on 4.
    let mut none = screen(ROWS, 0).expect("rows");
    none.init(OrdinanceFeed {
        guns: vec![GunPosition {
            kind: NO_GUN_KIND,
            name: String::new(),
            choice: 0,
        }],
        ..OrdinanceFeed::default()
    });
    assert_eq!(
        (
            none.ammo_description_entry(),
            none.rocket_description_entry()
        ),
        (4, 4)
    );
}

#[test]
fn accept_parity5_ordinance_layout_d_lists_open_roll_over_and_choose() {
    let mut s = screen(ROWS, 0).expect("rows");
    s.init(feed());
    // A deactivated box does not open.
    let b = s.ammo_box(1).unwrap().rect;
    assert_eq!(s.press_at(b.x + 2, b.y + 2), OrdinancePress::Nothing);
    let b = s.ammo_box(0).unwrap().rect;
    assert_eq!(s.press_at(b.x + 2, b.y + 2), OrdinancePress::Toggled);
    assert_eq!(s.open_list(), Some((OrdinanceList::Ammo, 0)));
    // Rolling over a row shows its description (10013).
    let (x, y) = point_in_row(&s, OrdinanceList::Ammo, 0, 3);
    s.hover_at(Some((x, y)));
    assert_eq!((s.rolled(), s.ammo_description_entry()), (Some(3), 3));
    // Choosing stores it (10015 / E 2036).
    assert_eq!(
        s.press_at(x, y),
        OrdinancePress::Ammo { slot: 0, choice: 3 }
    );
    assert_eq!(s.ammo_choice(0), Some(3));
    assert_eq!(s.open_list(), None);
    // A rocket box: row 11 is the last of twelve, row 12 is past the list.
    let b = s.rocket_box(4).unwrap().rect;
    assert_eq!(s.press_at(b.x + 2, b.y + 2), OrdinancePress::Toggled);
    let (x, y) = point_in_row(&s, OrdinanceList::Rocket, 4, 12);
    assert_eq!(s.press_at(x, y), OrdinancePress::Nothing);
    s.press_at(b.x + 2, b.y + 2);
    let (x, y) = point_in_row(&s, OrdinanceList::Rocket, 4, 11);
    assert_eq!(
        s.press_at(x, y),
        OrdinancePress::Rocket {
            slot: 4,
            choice: 11
        }
    );
    assert_eq!(s.rocket_choice(4), Some(11));
    assert_eq!(s.rocket_description_entry(), 11);
    // Esc closes an open list.
    s.press_at(b.x + 2, b.y + 2);
    assert!(s.close_list());
    assert!(!s.close_list());
}

#[test]
fn accept_parity5_ordinance_layout_e_accept_and_cancel_follow_xq() {
    let from_flight_check = screen(ROWS, 1).expect("rows");
    assert_eq!(
        from_flight_check.press_button("ol_b_accept"),
        OrdinancePress::Accept { leave: false }
    );
    assert_eq!(
        from_flight_check.press_button("OL_B_CANCEL"),
        OrdinancePress::Cancel
    );
    let entry = screen(ROWS, 0).expect("rows");
    assert_eq!(
        entry.press_button("OL_B_ACCEPT"),
        OrdinancePress::Accept { leave: true }
    );
    assert_eq!(entry.press_button("OL_B_CANCEL"), OrdinancePress::Nothing);
    assert_eq!(OrdinanceSide::Pilot.zq(), -1);
    assert_eq!(OrdinanceSide::Wingmate.zq(), -2);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_ordinance_layout_retail_the_installations_rows_and_strings_build_the_screen() {
    use cs_app::ui::front_end::FrontEndScreens;
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let root = std::path::PathBuf::from(game_dir);
    let screens = FrontEndScreens::open(&root).expect("the installation's screens open");
    let layout = screens.original_layout().expect("the real layout reads");
    let strings = cs_app::mission_session::FlightCheckStrings::read(&root, &screens)
        .expect("the string table reads");
    let name = |name: &str| strings.resolve_tagged(name);
    let s = OrdinanceLayoutScreen::from_layout(&layout, OrdinanceSide::Wingmate, 1, &name)
        .expect("the real OrdinanceLayout rows read");
    for slot in 0..AMMO_BOXES {
        let b = s.ammo_box(slot).expect("four ammunition boxes");
        assert_eq!((b.displayed, b.rect.width, b.rect.x), (5, 148, 136));
    }
    for slot in 0..ROCKET_BOXES {
        assert_eq!(s.rocket_box(slot).map(|b| b.displayed), Some(12));
    }
    assert_eq!(s.ammo_box(0).map(|b| b.rect.y), Some(120));
    assert_eq!(s.rocket_box(7).map(|b| b.rect.y), Some(404));
    let texts = s.texts();
    assert_eq!(texts.len(), 7);
    assert!(texts.iter().all(|t| !t.text.is_empty()));
    // The ammunition rows (3360 + 0..5), the gun kinds (3320 + kind) and the
    // rocket rows (3395 + 0..12) are in the string table.
    for id in (3360..3365).chain(3320..3326).chain(3395..3407) {
        let raw = strings
            .resolve_id_tagged(id)
            .unwrap_or_else(|| panic!("string {id} is in the table"));
        assert!(!raw.is_empty());
    }
}
