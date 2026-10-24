//! Acceptance task PARITY5 P5-CONS-E (#2046): the Plane Construction hub's
//! Sell hangar (`HANGAR.SCRIPT`), the four saved-plane picture slots and the
//! Plane Name dialog's *Default loadout* box. Task test prefix:
//! `accept_parity5_cabin_plane_construction_hangar_`.
//!
//! * The synthetic tests run over layout text in the original's *format* (the
//!   real `[@Hangar@]` columns, invented strings). They prove the page's rules
//!   and the route from the hub's saved-plane count to the sale; they run in
//!   CI and prove nothing about retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows and strings.
//!   `#[ignore]`d because CI has no installation.

use cs_app::ui::front_end::construction_hangar::{
    DONE_BUTTON, EXPORT_BUTTON, HangarOutcome, HangarPage, SAVED_SLOTS, SELL_BUTTON, SavedPlane,
    SlotIcons, WORKING_SLOT, YES_ANSWER, blueprint_art, slot_icon_art,
};
use cs_app::ui::front_end::{
    HUB_SECTION, HubOutcome, HubPage, OriginalLayout, SELL_NOT_BUILT, StartingLoadout,
};

use super::parity5_cabin_plane_construction_hub::{
    LAYOUT, TestInput, extents, hub, ids, inject, name_page, names, open_hub, press_button,
    section, state, tap, type_text,
};

const GLOBALS: &str = "\
[GLOBALVARS]
G5=FC_UP,FC_B_ScrollUp.png
G6=FC_DOWN,FC_B_ScrollDown.png
G7=FC_SLIDER,FC_B_ScrollBar.png
G8=STDTEXTH,16
G17=STDITEMH,15
G27=GN_DROPUP,GN_B_ListboxarrowSMALLup.png
G28=GN_DROPDOWN,GN_B_ListboxarrowSMALLdown.png
";

/// The real `[@Hangar@]` rows.
const HANGAR_ROWS: &str = "\
[@Hangar@]
V1=GX,553
V2=V2,0xFF354560
V3=V3,444
V4=V4,FC_B_PaperButton.Png
V5=INFOX,430
HA_BACKGROUND   =P,PS_BackGround.jpg,0,0,1,1,0,0,0
HA_P_PILOTPLANE =P,FC_PlaneIcons.Png,<V3>,138,0,12,0,1,1
HA_B_DONE   =B,MS_B_Done.png,551,<GX>,0,0,,,,,,,,,
HA_B_SELLP  =B,<V4>,560,158,0,IDS_PS_B_SELL,,0x1000,0,,,,,,,0x76767600,0x1D110C92,0x374D6B00,0x00000000
HA_B_EXPORTP=B,<V4>,560,188,0,IDS_PS_B_EXPORT,,0x1000,0,,,,,,,0x76767600,0x1D110C92,0x374D6B00,0x00000000
HA_T_PROMPT     =T,IDS_HA_SELLEXPORT,136,70,0,500,0,0xFF2D3843,0
HA_T_TITLE      =T,IDS_HA_TITLE,136,37,0,180,30,<V2>,0
HA_T_PLANE      =T,,138,108,0,0,0,<V2>,0
HA_T_PILOTPLANE =T,,236,110,0,400,0,<V2>,0
HA_T_VALUEP     =T,,<INFOX>,290,0,0,0,0xFF121315,0
HA_D_PILOTPLANE =D,<FC_SLIDER>,<FC_UP>,<FC_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,138,132,0,271,<STDITEMH>,25
";

fn layout_text() -> String {
    let hub = LAYOUT.replace(
        "PX_P_PLANE1=P,,<PLANEX>,<PLANEY>,2,1,0,1,0\n",
        "PX_P_PLANE1=P,,<PLANEX>,<PLANEY>,2,1,0,1,0\nPX_P_PLANE2=P,,<PLANEX>,<PLANEY>,3,1,0,1,0\nPX_P_PLANE3=P,,<PLANEX>,<PLANEY>,4,1,0,1,0\n",
    );
    assert!(hub.contains("PX_P_PLANE3"), "the fixture was extended");
    format!("{GLOBALS}{hub}\n{HANGAR_ROWS}")
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(&layout_text()).expect("the fixture parses")
}

fn hangar_names(name: &str) -> Option<String> {
    match name {
        "IDS_HA_TITLE" => Some("[AB14I]Hangar".to_owned()),
        "IDS_HA_SELLEXPORT" => Some("[TREB9B]Sell or export".to_owned()),
        other => names(other),
    }
}

fn hangar_ids(id: u32) -> Option<String> {
    let text = match id {
        207 => "[COUR9]%1!s! (%2!s!)".to_owned(),
        700 => "[COUR9]Sell this %1!s! for $%2!d!?".to_owned(),
        702 => "[COUR9]Export this %1!s!.".to_owned(),
        704 => "[COUR9]The %1!s! %2!s! cannot be sold.".to_owned(),
        1258 => "[COUR9]VALUE: $%1!d!".to_owned(),
        3000..=3010 => format!("[COUR9]Airframe {}", id - 3000),
        3020..=3030 => format!("[COUR9]AF{}", id - 3020),
        other => return ids(other),
    };
    Some(text)
}

fn page() -> HangarPage {
    HangarPage::from_layout(&layout(), &hangar_names, &hangar_ids)
        .expect("the fixture has the rows")
}

fn plane(name: &str, airframe: u32, value: u64) -> Option<SavedPlane> {
    Some(SavedPlane {
        kind: 1,
        name: name.to_owned(),
        airframe,
        value,
    })
}

/// Four saved planes in records 0, 1, 3 and 5; record 2 is empty.
fn four() -> Vec<Option<SavedPlane>> {
    vec![
        plane("Alpha", 0, 1_000),
        plane("Bravo", 1, 2_000),
        None,
        plane("Charlie", 2, 3_000),
        None,
        plane("Delta", 3, 4_000),
    ]
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_the_page_reads_the_sections_rows() {
    let page = page();
    assert_eq!(page.title().text, "Hangar");
    assert_eq!(page.prompt().text, "Sell or export");
    assert_eq!(
        (
            page.list().rect.x,
            page.list().rect.y,
            page.list().rect.width
        ),
        (138, 132, 271)
    );
    assert_eq!(page.list().displayed, SAVED_SLOTS as u32);
    let none = OriginalLayout::parse("[@Other@]\nA=P,a.png,0,0,0,1,0,0,0\n").expect("parses");
    assert!(HangarPage::from_layout(&none, &hangar_names, &hangar_ids).is_err());
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_the_count_is_the_occupied_records_less_the_working_plane()
 {
    let mut page = page();
    page.set_planes(four(), false, 0, 0);
    assert_eq!(page.saved_count(), 4);
    page.set_planes(four(), true, 5, 0);
    assert_eq!(
        page.saved_count(),
        3,
        "the construction screen keeps a record"
    );
    assert!(page.sell_enabled());
    let mut two = four();
    two[3] = None;
    two[5] = None;
    page.set_planes(two, true, 0, 0);
    assert_eq!(page.saved_count(), 1);
    assert!(!page.sell_enabled());
    page.set_planes(Vec::new(), true, 0, 0);
    assert_eq!(page.saved_count(), 0, "an empty store does not underflow");
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_rows_and_records_map_as_the_engine_walks_them() {
    let mut page = page();
    page.set_planes(four(), false, 0, 0);
    // Not constructing: the rows are the occupied records in order.
    let slots: Vec<usize> = (0..4).map(|row| page.slot_for_index(row)).collect();
    assert_eq!(slots, [0, 1, 3, 5]);
    assert_eq!(page.slot_for_index(4), WORKING_SLOT, "running off the end");
    assert_eq!(page.index_for_slot(3), Some(2));
    assert_eq!(
        page.index_for_slot(2),
        Some(1),
        "an empty record counts up to it"
    );
    // Constructing with the working plane in record 5: the walk stops there
    // and the row count skips it.
    page.set_planes(four(), true, 5, 0);
    assert_eq!(page.slot_for_index(2), 3);
    assert_eq!(page.slot_for_index(3), 5);
    assert_eq!(page.index_for_slot(5), Some(2));
    assert_eq!(page.selected_index(), Some(2));
    // Selecting a row stores its record.
    page.select_index(1);
    assert_eq!(page.selected_slot(), 1);
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_the_texts_follow_the_selected_plane() {
    let mut page = page();
    assert!(page.plane_text().is_none(), "no plane, no text");
    page.set_planes(four(), false, 1, 0);
    let (row, text) = page.plane_text().expect("a plane is selected");
    assert_eq!(row.id, "HA_T_PLANE");
    assert_eq!(text, "Bravo (Airframe 1)");
    let (row, text) = page.value_text().expect("a plane is selected");
    assert_eq!(row.id, "HA_T_VALUEP");
    assert_eq!(text, "VALUE: $2000");
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_selling_asks_then_pays_and_empties_the_record() {
    let mut page = page();
    page.set_planes(four(), false, 3, 500);
    let HangarOutcome::ConfirmSell { text } = page.press(SELL_BUTTON) else {
        panic!("a sellable plane asks");
    };
    assert_eq!(text, "Sell this AF2 for $3000?");
    assert_eq!(page.asking(), Some(text.as_str()));
    // Nothing else is pressable under the box.
    assert_eq!(page.press(DONE_BUTTON), HangarOutcome::Nothing);
    // A no keeps the plane and the cash.
    assert_eq!(page.answer_sell(0), HangarOutcome::Kept);
    assert_eq!(page.cash(), 500);
    assert_eq!(page.saved_count(), 4);
    // A yes pays the value and empties the record; row 0 is selected.
    let _ = page.press(SELL_BUTTON);
    assert_eq!(
        page.answer_sell(YES_ANSWER),
        HangarOutcome::Sold {
            slot: 3,
            value: 3_000
        }
    );
    assert_eq!(page.cash(), 3_500);
    assert_eq!(page.saved_count(), 3);
    assert_eq!(page.selected_slot(), 0);
    assert_eq!(page.answer_sell(YES_ANSWER), HangarOutcome::Nothing);
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_selling_needs_three_and_refuses_a_kept_plane() {
    let mut page = page();
    let mut few = four();
    few[3] = None;
    few[5] = None;
    page.set_planes(few, false, 0, 0);
    assert_eq!(page.saved_count(), 2);
    assert_eq!(page.press(SELL_BUTTON), HangarOutcome::TooFew);
    // A record whose first word is 2 is not sold: an OK-only box.
    let mut kept = four();
    if let Some(plane) = kept[0].as_mut() {
        plane.kind = 2;
    }
    page.set_planes(kept, false, 0, 0);
    let HangarOutcome::NotSold { text } = page.press(SELL_BUTTON) else {
        panic!("a kept plane is refused");
    };
    assert_eq!(text, "The AF0 Alpha cannot be sold.");
    assert!(page.asking().is_none());
    assert_eq!(page.saved_count(), 4, "nothing was sold");
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_done_selects_the_first_row_and_export_leaves_the_plane()
 {
    let mut page = page();
    page.set_planes(four(), false, 5, 0);
    let HangarOutcome::Export { text } = page.press(EXPORT_BUTTON) else {
        panic!("export answers");
    };
    assert_eq!(text.as_deref(), Some("Export this AF3."));
    assert_eq!(page.saved_count(), 4, "nothing was exported or sold");
    assert_eq!(page.press(DONE_BUTTON), HangarOutcome::Done);
    assert_eq!(page.selected_slot(), 0);
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_the_hub_takes_its_sell_count_from_the_saved_planes()
 {
    let mut hub = HubPage::from_layout(&layout(), &hangar_names, &hangar_ids).expect("the hub");
    assert!(hub.hangar().is_some(), "the layout has the Hangar rows");
    assert!(!hub.sell_enabled());
    assert_eq!(hub.press("PX_B_SELL"), HubOutcome::Nothing);
    // Four saved records, one of them the plane being built: three to sell.
    hub.set_saved_planes(four(), 5, 100);
    assert!(hub.sell_enabled());
    assert_eq!(hub.press("PX_B_SELL"), HubOutcome::Sell);
    // The hangar the press opens is the hub's own, and a sale lowers the count.
    let hangar = hub.hangar_mut().expect("the hangar");
    let _ = hangar.press(SELL_BUTTON);
    assert!(matches!(
        hangar.answer_sell(YES_ANSWER),
        HangarOutcome::Sold { .. }
    ));
    assert_eq!(hangar.saved_count(), 2);
    assert_eq!(SELL_NOT_BUILT.section, HUB_SECTION);
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_the_four_picture_panes_draw_in_z_order_with_the_engines_names()
 {
    let layout = layout();
    let section = layout.section(HUB_SECTION).expect("the hub section");
    let icons = SlotIcons::from_section(section).expect("four panes");
    let layers: Vec<u32> = icons.icons().iter().map(|icon| icon.layer).collect();
    assert_eq!(layers, [0, 1, 2, 3]);
    // PX_P_PLANE0..3 carry Z 5, 2, 3, 4: the picture of slot 1 is the back one.
    let order: Vec<u32> = icons.drawing_order().iter().map(|i| i.layer).collect();
    assert_eq!(order, [1, 2, 3, 0]);
    let pictures = icons.pictures(4, 13);
    assert_eq!(pictures[0].0, "PX_Icon_4_11_1.tga", "paint 13 reads as 11");
    assert_eq!(pictures[3].0, "PX_Icon_4_11_0.tga");
    assert!(pictures.iter().all(|(_, origin)| *origin == (16, 44)));
    assert_eq!(slot_icon_art(2, 5, 3), "PX_Icon_2_5_3.tga");
    assert_eq!(blueprint_art(7), "PX_7_Blueprint.tga");
    // The hub page carries them; a layout with fewer panes carries none.
    let hub = HubPage::from_layout(&layout, &hangar_names, &hangar_ids).expect("the hub");
    assert_eq!(hub.slot_icons(), Some(&icons));
    let plain = OriginalLayout::parse(&format!("{GLOBALS}{LAYOUT}")).expect("parses");
    let hub = HubPage::from_layout(&plain, &hangar_names, &hangar_ids).expect("the hub");
    assert!(hub.slot_icons().is_none());
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_the_default_loadout_box_picks_the_starting_loadout()
 {
    let mut hub = HubPage::from_layout(&layout(), &hangar_names, &hangar_ids).expect("the hub");
    // The box starts checked (`PN_B_DEFAULT`, `1,1`).
    assert_eq!(hub.starting_loadout(), StartingLoadout::Stock);
    hub.set_default_loadout(false);
    assert_eq!(hub.starting_loadout(), StartingLoadout::Empty);
    hub.set_default_loadout(true);
    assert_eq!(hub.starting_loadout(), StartingLoadout::Stock);
}

/// The hub fixture's art plus the hangar's buttons (four 32-row frames) and
/// the end-of-screen Done strip.
fn hangar_extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "fc_b_paperbutton.png" | "ms_b_done.png" => Some((150, 128)),
        "ps_background.jpg" => Some((800, 600)),
        _ => extents(art),
    }
}

/// The cabin → the Plane Name dialog → the hub, over the layout with the
/// `[@Hangar@]` rows, with four saved planes (one is the plane being built).
fn hangar_app() -> bevy::app::App {
    use bevy::app::App;
    use bevy::input::InputSystems;
    use bevy::prelude::{IntoScheduleConfigs, PreUpdate};
    use cs_app::ui::front_end::{MenuShell, MissionEnd, StartFlow, headless_app_construction};

    let layout = layout();
    let mut shell = MenuShell::build(&layout, &hangar_extents, &|_: &str| None);
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    let hub = HubPage::from_layout(&layout, &hangar_names, &hangar_ids).expect("the hub");
    let mut app: App = headless_app_construction(shell, Some(flow), name_page(), hub);
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    open_hub(&mut app, "Ace");
    let mut planes = four();
    planes[0].as_mut().expect("a plane").kind = 2;
    assert!(
        app.world_mut()
            .resource_mut::<cs_app::ui::front_end::ShellResource>()
            .set_hub_saved_planes(planes, 5, 100)
    );
    app.update();
    app
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_sell_opens_the_hangar_and_done_returns_to_the_hub()
 {
    use cs_app::ui::front_end::construction_hangar::{EXPORT_NOT_BUILT, HANGAR_SECTION};
    let mut app = hangar_app();
    assert_eq!(section(&app), HUB_SECTION);
    press_button(&mut app, "PX_B_SELL");
    assert_eq!(section(&app), HANGAR_SECTION);
    let flow = state(&app).flow().expect("a flow");
    assert!(flow.hangar_open());
    assert_eq!(flow.section(), Some(HANGAR_SECTION));
    assert!(flow.notice().is_none(), "the hangar is built: no notice");
    // The hub's selected record stands in the hangar: record 5 is the plane
    // being built, so the first row is selected through Done's reset later.
    // Export is marked (A 22 unread) and leaves the planes alone.
    press_button(&mut app, EXPORT_BUTTON);
    assert_eq!(
        state(&app).flow().expect("a flow").notice(),
        Some(&EXPORT_NOT_BUILT)
    );
    tap(&mut app, bevy::input::keyboard::KeyCode::Enter);
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    // Row 0 is record 0, a plane that is not sold: Sell answers with the
    // OK-only box (its text is carried by the page's flow state).
    let list = hub(&app).hangar().expect("the hangar").list().rect;
    super::parity5_cabin_plane_construction_hub::click_logical(&mut app, list.x + 10, list.y + 5);
    let row = hub(&app).hangar().expect("the hangar").item_rect(0);
    super::parity5_cabin_plane_construction_hub::click_logical(&mut app, row.x + 5, row.y + 5);
    assert_eq!(hub(&app).hangar().map(HangarPage::selected_slot), Some(0));
    press_button(&mut app, SELL_BUTTON);
    assert!(hub(&app).hangar().is_some_and(|h| h.asking().is_none()));
    assert_eq!(section(&app), HANGAR_SECTION);
    assert_eq!(hub(&app).hangar().map(HangarPage::saved_count), Some(3));
    tap(&mut app, bevy::input::keyboard::KeyCode::Enter);
    // Done: the hub again, with its tab and name intact.
    press_button(&mut app, DONE_BUTTON);
    assert_eq!(section(&app), HUB_SECTION);
    assert!(!state(&app).flow().expect("a flow").hangar_open());
    assert_eq!(hub(&app).text(), "Ace");
    assert_eq!(section(&app), HUB_SECTION);
}

#[test]
fn accept_parity5_cabin_plane_construction_hangar_a_sale_lowers_the_hubs_count_through_the_message_box()
 {
    use cs_app::ui::front_end::construction_hangar::HANGAR_SECTION;
    let mut app = hangar_app();
    press_button(&mut app, "PX_B_SELL");
    assert_eq!(section(&app), HANGAR_SECTION);
    // Row 1 is Bravo (record 1), which can be sold.
    let list = hub(&app).hangar().expect("the hangar").list().rect;
    let (x, y) = (list.x + 10, list.y + 5);
    super::parity5_cabin_plane_construction_hub::click_logical(&mut app, x, y);
    assert!(hub(&app).hangar().is_some_and(HangarPage::list_open));
    let row = hub(&app).hangar().expect("the hangar").item_rect(1);
    super::parity5_cabin_plane_construction_hub::click_logical(&mut app, row.x + 5, row.y + 5);
    assert_eq!(hub(&app).hangar().map(HangarPage::selected_slot), Some(1));
    press_button(&mut app, SELL_BUTTON);
    assert!(hub(&app).hangar().is_some_and(|h| h.asking().is_some()));
    // N keeps the plane.
    type_text(&mut app, "n");
    assert!(hub(&app).hangar().is_some_and(|h| h.asking().is_none()));
    assert_eq!(hub(&app).hangar().map(HangarPage::saved_count), Some(3));
    press_button(&mut app, SELL_BUTTON);
    type_text(&mut app, "y");
    assert_eq!(hub(&app).hangar().map(HangarPage::saved_count), Some(2));
    assert!(!hub(&app).sell_enabled(), "two planes are too few to sell");
    press_button(&mut app, DONE_BUTTON);
    assert_eq!(section(&app), HUB_SECTION);
}

/// **Retail.** The installation's `[@Hangar@]` rows and strings build the
/// page, and the hub's four picture panes are all there.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_plane_construction_hangar_retail_the_installations_rows_and_strings_build_the_page()
 {
    use cs_app::ui::front_end::FrontEndScreens;
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let root = std::path::PathBuf::from(game_dir);
    let screens = FrontEndScreens::open(&root).expect("the installation's screens open");
    let layout = screens.original_layout().expect("the real layout reads");
    let strings = cs_app::mission_session::FlightCheckStrings::read(&root, &screens)
        .expect("the string table reads");
    let page =
        HangarPage::from_layout(&layout, &|n: &str| strings.resolve_tagged(n), &|id: u32| {
            strings.resolve_id_tagged(id)
        })
        .expect("the real Hangar rows read");
    assert_eq!(page.list().displayed, 25);
    assert_eq!((page.list().rect.x, page.list().rect.y), (138, 132));
    assert!(!page.title().text.is_empty());
    assert!(!page.prompt().text.is_empty());
    // The strings the sale's boxes and lines are built from.
    for id in [207, 700, 702, 704, 1258, 3000, 3010, 3020, 3030] {
        let raw = strings
            .resolve_id_tagged(id)
            .unwrap_or_else(|| panic!("string {id} is in the table"));
        assert!(!raw.is_empty());
    }
    let section = layout.section(HUB_SECTION).expect("the hub section");
    let icons = SlotIcons::from_section(section).expect("the real hub has four picture panes");
    let order: Vec<u32> = icons.drawing_order().iter().map(|i| i.layer).collect();
    assert_eq!(order, [1, 2, 3, 0]);
    let hub = HubPage::from_layout(&layout, &|n: &str| strings.resolve_tagged(n), &|id: u32| {
        strings.resolve_id_tagged(id)
    })
    .expect("the real hub reads");
    assert!(hub.hangar().is_some());
    assert!(hub.slot_icons().is_some());
}
