//! Acceptance task PARITY5 P5-CONS-D (#2045): the Plane Construction hub's
//! Guns and Hardpoints panels. Task test prefix:
//! `accept_parity5_cabin_plane_construction_guns_hardpoints_`.
//!
//! * The synthetic tests run over layout text in the original's *format* (the
//!   real `[@Guns@]` and `[@HardPoints@]` columns, invented strings and
//!   names). They prove the panels' rules and the route from the hub's tab
//!   through the drop-downs to the production [`ConstructionScreen`]'s draft;
//!   they run in CI and prove nothing about retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows and strings.
//!   `#[ignore]`d because CI has no installation.

use bevy::app::App;
use bevy::input::InputSystems;
use bevy::prelude::{IntoScheduleConfigs, PreUpdate, Vec2};

use cs_app::construction::ConstructionScreen;
use cs_app::ui::front_end::construction_slots::{
    SlotApplyError, SlotError, SlotKind, SlotOption, SlotPanel, SlotPress, apply_slot_choice,
    held_in_positions,
};
use cs_app::ui::front_end::{
    HUB_SECTION, HubOutcome, HubPage, HubPanel, MissionEnd, OriginalLayout, Screen, StartFlow,
    headless_app_construction,
};
use cs_content::construction::{
    SYNTHETIC_GUN_KEY, SYNTHETIC_MISSILE_KEY, declared_synthetic_blueprint,
};
use cs_types::content::{ContentId, ContentKind, Resolved};

use super::parity5_cabin_plane_construction_hub::{
    Draft, LAYOUT, TestInput, draft, drive, hub, ids, inject, name_page, names, press_button,
    section, shell, state, type_text,
};

/// The two panels' rows as the installation writes them.
const PANEL_ROWS: &str = "\
	[@Guns@]
    V1=GX,422
    V2=V2,440

	GN_P_LINE0		=P,PX_Rule.png,422,156,5,1,0,0,0
    GN_T_TITLE=T,IDS_GN_TITLE,426,62,0,<PX_TITLEW>,22,0xff261E47,0
    GN_T_GUNTITLE0  =T,,<GX>,110,0,0,0,0xFF000000,0
    GN_T_GUNTITLE1  =T,,<GX>,159,0,0,0,0xFF000000,0
    GN_T_GUNTITLE2  =T,,<GX>,205,0,0,0,0xFF000000,0
    GN_T_GUNTITLE3  =T,,<GX>,252,0,0,0,0xFF000000,0
    GN_T_GUNNAME    =T,,<GX>,308,0,0,0,0xFF000000,0
    GN_S_GUNDESC    =S,0xFF554C67,0xFFE6DABE,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,412,334,0,325,165,,0xFF000000
    GN_D_GUN0=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,133,100,305,15,11
    GN_D_GUN1=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,180,90,305,15,11
    GN_D_GUN2=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,228,80,305,15,11
    GN_D_GUN3=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,275,70,305,15,11

	[@HardPoints@]
    V1=GX,422
    V2=V2,490

	HP_P_LINE0	=P,PX_Rule.png,422,156,5,1,0,0,0
    HP_T_TITLE          =T,IDS_HP_TITLE,426,62,0,<PX_TITLEW>,22,0xff261E47,0
	HP_T_POINTTITLE0	=T,IDS_HP_POINT1_TITLE,<GX>,110,0,0,0,0xFF000000,0
	HP_T_POINTTITLE1	=T,IDS_HP_POINT2_TITLE,<GX>,157,0,0,0,0xFF000000,0
    HP_T_HARDPOINTNAME  =T,,<GX>,310,0,335,18,0xFF000000,0
    HP_S_HARDPOINTDESC  =S,0xFF554C67,0xFFE6DABE,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,412,334,0,325,165,,0xFF000000
    HP_D_POINT0=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,133,80,330,15,5
    HP_D_POINT1=D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,<V2>,180,70,330,15,5
";

/// The global variables the panels' rows use, as the installation defines them.
const GLOBALS: &str = "\
[GLOBALVARS]
G19=PX_TEXTH,15
G21=PX_SLIDER,PX_B_ScrollBar.png
G22=PX_DOWN,PX_B_ScrollDown.png
G23=PX_UP,PX_B_ScrollUP.png
G24=PX_TITLEW,320
G27=GN_DROPUP,GN_B_ListboxarrowSMALLup.png
G28=GN_DROPDOWN,GN_B_ListboxarrowSMALLdown.png
";

fn panel_layout() -> OriginalLayout {
    let text = format!("{GLOBALS}{LAYOUT}").replace(
        "\t[@Guns@]\nGU_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0\n\t[@HardPoints@]\nHP_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0\n",
        PANEL_ROWS,
    );
    assert!(text.contains("GN_D_GUN0"), "the fixture was replaced");
    OriginalLayout::parse(&text).expect("the fixture parses")
}

fn panel_names(name: &str) -> Option<String> {
    match name {
        "IDS_GN_TITLE" => Some("[AB14I]Guns".to_owned()),
        "IDS_HP_TITLE" => Some("[AB14I]Hardpoints".to_owned()),
        "IDS_HP_POINT1_TITLE" => Some("[AB12]Point one".to_owned()),
        "IDS_HP_POINT2_TITLE" => Some("[AB12]Point two".to_owned()),
        other => names(other),
    }
}

fn panel_page() -> HubPage {
    HubPage::from_layout(&panel_layout(), &panel_names, &ids).expect("the fixture has the rows")
}

fn panel(kind: SlotKind) -> SlotPanel {
    SlotPanel::from_layout(&panel_layout(), kind, &panel_names).expect("the panel's rows read")
}

fn weapon(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Weapon, key).expect("weapon id")
}

/// The list the guns offer: the empty position and one gun.
fn gun_options() -> Vec<SlotOption> {
    vec![
        SlotOption {
            id: None,
            name: "Empty".to_owned(),
        },
        SlotOption {
            id: Some(weapon(SYNTHETIC_GUN_KEY)),
            name: "Machine gun".to_owned(),
        },
        SlotOption {
            id: Some(weapon("fixture.other_gun")),
            name: "Cannon".to_owned(),
        },
    ]
}

fn point_options() -> Vec<SlotOption> {
    vec![
        SlotOption {
            id: None,
            name: "Empty".to_owned(),
        },
        SlotOption {
            id: Some(weapon(SYNTHETIC_MISSILE_KEY)),
            name: "Rocket".to_owned(),
        },
    ]
}

/// A logical point inside the `index`-th row of the list under a box at `y`.
fn row_y(box_y: i32, index: i32) -> i32 {
    box_y + (index + 1) * 15 + 3
}

#[test]
fn accept_parity5_cabin_plane_construction_guns_hardpoints_a_the_panels_read_the_real_rows() {
    let guns = panel(SlotKind::Guns);
    let points = panel(SlotKind::HardPoints);
    // Four gun positions and two hardpoints: the numbers the scripts' loops
    // state and `cs_content` measured on the other screens.
    assert_eq!(SlotKind::Guns.slots(), 4);
    assert_eq!(SlotKind::HardPoints.slots(), 2);
    for slot in 0..4 {
        let dropdown = guns.dropdown(slot).expect("four gun boxes");
        assert_eq!(dropdown.displayed, 11);
        assert_eq!(dropdown.rect.width, 305);
    }
    assert!(guns.dropdown(4).is_none());
    assert_eq!(guns.dropdown(1).map(|d| d.rect.y), Some(180));
    for slot in 0..2 {
        assert_eq!(points.dropdown(slot).map(|d| d.displayed), Some(5));
    }
    assert!(points.dropdown(2).is_none());
    assert_eq!(guns.description_rect().width, 325);
    assert_eq!(guns.dropdown(0).map(|d| d.rect.x), Some(440));
    assert_eq!(points.dropdown(0).map(|d| d.rect.x), Some(490));
    // The panel title is the installation's string; a gun position's title
    // has no resource id (`E 2250` fills it), so only the title is drawn.
    let texts = guns.texts();
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0].id, "GN_T_TITLE");
    assert_eq!(texts[0].text, "Guns");
    assert_eq!(texts[0].size, Some(14));
    // The hardpoints name their points by string.
    let texts: Vec<_> = points.texts().into_iter().map(|t| t.text).collect();
    assert_eq!(texts, ["Hardpoints", "Point one", "Point two"]);
}

#[test]
fn accept_parity5_cabin_plane_construction_guns_hardpoints_b_a_layout_without_the_rows_leaves_the_tabs_unbuilt()
 {
    let bare = OriginalLayout::parse(LAYOUT).expect("the hub fixture parses");
    assert_eq!(
        SlotPanel::from_layout(&bare, SlotKind::Guns, &panel_names).err(),
        Some(SlotError::RowMissing("GN_T_TITLE".to_owned()))
    );
    let page = HubPage::from_layout(&bare, &names, &ids).expect("the bare hub reads");
    assert!(!page.panel_built(HubPanel::Guns));
    assert!(!page.panel_built(HubPanel::Hardpoints));
    let built = panel_page();
    assert!(built.panel_built(HubPanel::Guns));
    assert!(built.panel_built(HubPanel::Hardpoints));
    assert!(!built.panel_built(HubPanel::Armor));
}

#[test]
fn accept_parity5_cabin_plane_construction_guns_hardpoints_c_the_lists_open_roll_over_and_choose() {
    let mut guns = panel(SlotKind::Guns);
    // No entries yet: nothing to open.
    assert_eq!(guns.press_at(450, 140), SlotPress::Nothing);
    let held = vec![
        Some(weapon(SYNTHETIC_GUN_KEY)),
        None,
        None,
        Some(weapon("fixture.other_gun")),
    ];
    guns.set_options(gun_options(), &held);
    assert_eq!(
        [0, 1, 2, 3].map(|slot| guns.selected(slot)),
        [Some(1), Some(0), Some(0), Some(2)]
    );
    // The name line follows the focused position (`BS`).
    let name = |panel: &SlotPanel| {
        panel
            .texts()
            .into_iter()
            .find(|t| t.id == "GN_T_GUNNAME")
            .map(|t| t.text)
    };
    assert_eq!(name(&guns), Some("Machine gun".to_owned()));
    // Opening position 3's list moves the focus there and shows its choice.
    assert_eq!(guns.press_at(450, 280), SlotPress::Toggled);
    assert_eq!(guns.open_slot(), Some(3));
    assert_eq!(guns.focus(), 3);
    assert_eq!(name(&guns), Some("Cannon".to_owned()));
    // Rolling over an entry (`mail 10013`) does not choose it.
    guns.hover_at(Some((450, row_y(275, 1))));
    assert_eq!(guns.rolled(), Some(1));
    assert_eq!(guns.selected(3), Some(2));
    guns.hover_at(Some((10, 10)));
    assert_eq!(guns.rolled(), None);
    // Choosing the empty entry (`mail 10015`) answers `None` for position 3.
    assert_eq!(
        guns.press_at(450, row_y(275, 0)),
        SlotPress::Chose { slot: 3, id: None }
    );
    assert_eq!(guns.open_slot(), None);
    assert_eq!(guns.selected(3), Some(0));
    // A press away from every box and list is not the panel's, and an open
    // list closes on it.
    assert_eq!(guns.press_at(450, 140), SlotPress::Toggled);
    assert_eq!(guns.press_at(10, 10), SlotPress::Nothing);
    assert_eq!(guns.open_slot(), None);
    assert!(!guns.close_list());
    assert_eq!(guns.press_at(450, 140), SlotPress::Toggled);
    assert!(guns.close_list());
    // Only the guns' name line is read; the hardpoints' (`E 2220` mode 5) is
    // not, so it is never drawn.
    let mut points = panel(SlotKind::HardPoints);
    points.set_options(
        point_options(),
        &[Some(weapon(SYNTHETIC_MISSILE_KEY)), None],
    );
    assert!(points.texts().iter().all(|t| t.id != "HP_T_HARDPOINTNAME"));
}

#[test]
fn accept_parity5_cabin_plane_construction_guns_hardpoints_d_a_choice_edits_the_production_draft() {
    let Draft {
        state: campaign, ..
    } = draft();
    let mut screen = ConstructionScreen::open(&campaign, declared_synthetic_blueprint(), vec![]);
    let blueprint = screen.blueprint().clone();
    let gun_mounts: Vec<_> = blueprint.guns().iter().map(|g| g.mount().clone()).collect();
    let point_mounts: Vec<_> = blueprint
        .ordnance()
        .iter()
        .take(2)
        .map(|o| o.hardpoint().clone())
        .collect();
    let positions = Resolved::<u32>::unknown(
        cs_types::evidence::ClaimId::new("fixture.positions").expect("claim id"),
        "the positions of a new gun are the caller's",
    )
    .expect("a reason");

    // The panel's choices begin as the draft's.
    let held = held_in_positions(SlotKind::Guns, &gun_mounts, &screen);
    assert_eq!(held.len(), 4);
    assert!(
        held.iter()
            .all(|g| g.as_ref() == Some(&weapon(SYNTHETIC_GUN_KEY)))
    );
    let mut page = panel_page();
    page.set_slot_options(HubPanel::Guns, gun_options(), &held);
    assert_eq!(page.press("PX_B_GUNS"), HubOutcome::Panel(HubPanel::Guns));
    assert_eq!(page.press_at(450, 140), HubOutcome::List);
    // Position 0: the third entry.
    let HubOutcome::ChooseSlot { panel, slot, id } = page.press_at(450, row_y(133, 2)) else {
        panic!("the third row is a choice");
    };
    assert_eq!((panel, slot), (HubPanel::Guns, 0));
    apply_slot_choice(
        SlotKind::Guns,
        slot,
        id.clone(),
        &gun_mounts,
        positions.clone(),
        &mut screen,
    )
    .expect("a weapon id");
    assert_eq!(screen.blueprint().guns().len(), 4);
    assert_eq!(
        screen.blueprint().guns()[0].gun(),
        id.as_ref().expect("a gun")
    );
    // The replaced fitment kept its positions.
    assert_eq!(screen.blueprint().guns()[0].known_positions(), Some(1));
    assert!(screen.is_dirty(), "the draft differs from the saved plane");
    page.follow_slots(
        HubPanel::Guns,
        &held_in_positions(SlotKind::Guns, &gun_mounts, &screen),
    );
    assert_eq!(
        page.slot_panel(HubPanel::Guns).and_then(|p| p.selected(0)),
        Some(2)
    );

    // The empty entry removes the fitment from position 1.
    apply_slot_choice(
        SlotKind::Guns,
        1,
        None,
        &gun_mounts,
        positions.clone(),
        &mut screen,
    )
    .expect("an empty position");
    assert_eq!(screen.blueprint().guns().len(), 3);
    assert_eq!(
        held_in_positions(SlotKind::Guns, &gun_mounts, &screen)[1],
        None
    );
    // A gun put back into the empty mount takes the caller's positions.
    apply_slot_choice(
        SlotKind::Guns,
        1,
        Some(weapon(SYNTHETIC_GUN_KEY)),
        &gun_mounts,
        positions.clone(),
        &mut screen,
    )
    .expect("a weapon id");
    assert_eq!(screen.blueprint().guns().len(), 4);
    assert!(matches!(
        screen.blueprint().guns()[3].positions(),
        Resolved::Unknown { .. }
    ));

    // The hardpoints: the empty entry empties point 0, a rocket fills it.
    apply_slot_choice(
        SlotKind::HardPoints,
        0,
        None,
        &point_mounts,
        positions.clone(),
        &mut screen,
    )
    .expect("an empty point");
    assert_eq!(
        held_in_positions(SlotKind::HardPoints, &point_mounts, &screen),
        [None, Some(weapon(SYNTHETIC_MISSILE_KEY))]
    );
    apply_slot_choice(
        SlotKind::HardPoints,
        0,
        Some(weapon(SYNTHETIC_MISSILE_KEY)),
        &point_mounts,
        positions.clone(),
        &mut screen,
    )
    .expect("a weapon id");
    assert_eq!(
        held_in_positions(SlotKind::HardPoints, &point_mounts, &screen)[0],
        Some(weapon(SYNTHETIC_MISSILE_KEY))
    );

    // A position with no mount, and a choice of the wrong kind, are refused
    // and leave the draft alone.
    let before = screen.blueprint().clone();
    assert_eq!(
        apply_slot_choice(
            SlotKind::Guns,
            4,
            None,
            &gun_mounts,
            positions.clone(),
            &mut screen
        ),
        Err(SlotApplyError::NoMount(4))
    );
    let engine = ContentId::from_source(ContentKind::Engine, "fixture.engine").expect("engine id");
    assert!(matches!(
        apply_slot_choice(
            SlotKind::Guns,
            0,
            Some(engine),
            &gun_mounts,
            positions,
            &mut screen
        ),
        Err(SlotApplyError::Schema(_))
    ));
    assert_eq!(screen.blueprint(), &before);
}

fn panel_app() -> App {
    let mut shell = shell();
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    let mut app = headless_app_construction(shell, Some(flow), name_page(), panel_page());
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    app
}

fn click(app: &mut App, x: f32, y: f32) {
    drive(app, |queue| {
        queue.cursor = Some(Vec2::new(x, y));
        queue.mouse.push(true);
    });
    drive(app, |queue| queue.mouse.push(false));
}

fn state_mut(app: &mut App) -> bevy::prelude::Mut<'_, cs_app::ui::front_end::ShellResource> {
    app.world_mut()
        .resource_mut::<cs_app::ui::front_end::ShellResource>()
}

#[test]
fn accept_parity5_cabin_plane_construction_guns_hardpoints_e_the_window_routes_the_pointer_to_the_lists_and_marks_only_unbuilt_panels()
 {
    let mut app = panel_app();
    press_button(&mut app, "PC_B_PLANEX");
    type_text(&mut app, "Ace");
    press_button(&mut app, "PN_B_OK");
    assert_eq!(section(&app), HUB_SECTION);
    assert_eq!(
        state(&app).flow().expect("a flow").screen(),
        Screen::Construction
    );
    // The draft's entries reach the panels through the resource.
    state_mut(&mut app).set_hub_slots(HubPanel::Guns, gun_options(), &[None, None, None, None]);
    state_mut(&mut app).set_hub_slots(HubPanel::Hardpoints, point_options(), &[None, None]);
    app.update();
    // The Guns tab is built: no notice, the tab is held.
    press_button(&mut app, "PX_B_GUNS");
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    assert_eq!(hub(&app).selected(), HubPanel::Guns);
    // Open position 0's list, then pick the third row.
    click(&mut app, 450.0, 140.0);
    assert!(hub(&app).list_open());
    click(&mut app, 450.0, row_y(133, 2) as f32);
    assert!(!hub(&app).list_open());
    assert_eq!(
        state(&app).hub_outcomes().last(),
        Some(&HubOutcome::ChooseSlot {
            panel: HubPanel::Guns,
            slot: 0,
            id: Some(weapon("fixture.other_gun")),
        })
    );
    // Esc shuts an open list without leaving the hub.
    click(&mut app, 450.0, 140.0);
    assert!(hub(&app).list_open());
    drive(&mut app, |queue| {
        queue
            .keys
            .push((bevy::input::keyboard::KeyCode::Escape, true));
    });
    drive(&mut app, |queue| {
        queue
            .keys
            .push((bevy::input::keyboard::KeyCode::Escape, false));
    });
    assert!(!hub(&app).list_open());
    assert_eq!(section(&app), HUB_SECTION);
    // The Hardpoints tab is built too; the Armor tab is not: marked.
    press_button(&mut app, "PX_B_HARDPOINTS");
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    assert_eq!(hub(&app).selected(), HubPanel::Hardpoints);
    press_button(&mut app, "PX_B_ARMOR");
    assert_eq!(
        state(&app)
            .flow()
            .expect("a flow")
            .notice()
            .map(|entry| entry.button),
        Some("PX_B_ARMOR")
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_plane_construction_guns_hardpoints_retail_the_installations_rows_and_strings_build_both_panels()
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
    let name = |name: &str| strings.resolve_tagged(name);
    let guns =
        SlotPanel::from_layout(&layout, SlotKind::Guns, &name).expect("the real Guns rows read");
    let points = SlotPanel::from_layout(&layout, SlotKind::HardPoints, &name)
        .expect("the real HardPoints rows read");
    // Four gun boxes of 11 rows, two hardpoint boxes of 5, as the scripts'
    // loops and the list-length readings say.
    for slot in 0..4 {
        let dropdown = guns.dropdown(slot).expect("four gun boxes");
        assert_eq!((dropdown.displayed, dropdown.rect.width), (11, 305));
    }
    assert!(guns.dropdown(4).is_none());
    for slot in 0..2 {
        assert_eq!(points.dropdown(slot).map(|d| d.displayed), Some(5));
    }
    assert_eq!(guns.dropdown(0).map(|d| d.rect.y), Some(133));
    assert_eq!(guns.dropdown(3).map(|d| d.rect.y), Some(275));
    // The installation's own title strings.
    assert!(!guns.texts()[0].text.is_empty());
    let texts: Vec<_> = points.texts().into_iter().map(|t| t.text).collect();
    assert_eq!(texts.len(), 3, "the title and the two point titles");
    assert!(texts.iter().all(|text| !text.is_empty()));
    // The gun kinds' names (`3310 + kind`, six strings with the empty one)
    // and the hardpoint entries' strings are in the table.
    for id in (3310..=3315).chain([1165, 1168, 1169, 506]) {
        let raw = strings
            .resolve_id_tagged(id)
            .unwrap_or_else(|| panic!("string {id} is in the table"));
        assert!(!raw.is_empty());
    }
    let page = HubPage::from_layout(&layout, &|n: &str| strings.resolve_tagged(n), &|id: u32| {
        strings.resolve_id_tagged(id)
    })
    .expect("the real hub reads");
    assert!(page.panel_built(HubPanel::Guns));
    assert!(page.panel_built(HubPanel::Hardpoints));
}
