//! Acceptance task PARITY5 P5-CONS-B (#2043): the Plane Construction hub's
//! Airframe and Engine panels. Task test prefix:
//! `accept_parity5_cabin_plane_construction_airframe_engine_`.
//!
//! * The synthetic tests run over layout text in the original's *format* (the
//!   real `[@AirFrame@]` and `[@Engine@]` columns, invented strings and
//!   names). They prove the panels' rules and the route from the hub's tab
//!   through the drop-down to the production [`ConstructionScreen`]'s draft;
//!   they run in CI and prove nothing about retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows and strings.
//!   `#[ignore]`d because CI has no installation.

use bevy::app::App;
use bevy::input::InputSystems;
use bevy::prelude::{IntoScheduleConfigs, PreUpdate, Vec2};

use cs_app::construction::{ConstructionContext, ConstructionScreen};
use cs_app::ui::front_end::construction_panels::{
    ChoiceError, ChoiceKind, ChoiceOption, ChoicePanel, ChoicePress, apply_choice,
};
use cs_app::ui::front_end::{
    HUB_SECTION, HubFeed, HubOutcome, HubPage, HubPanel, MissionEnd, OriginalLayout, Screen,
    StartFlow, headless_app_construction,
};
use cs_content::construction::{
    SYNTHETIC_AIRFRAME_KEY, SYNTHETIC_ENGINE_KEY, SYNTHETIC_OTHER_AIRFRAME_KEY,
    SYNTHETIC_UNMEASURED_ENGINE_KEY, declared_synthetic_blueprint,
};
use cs_types::content::{ContentId, ContentKind};

use super::parity5_cabin_plane_construction_hub::{
    LAYOUT, TestInput, drive, hub, ids, inject, name_page, names, press_button, section, shell,
    state, type_text,
};

/// The two panels' rows as the installation writes them.
const PANEL_ROWS: &str = "\
	[@AirFrame@]
	;Text
    AF_T_TITLE      =T,IDS_AF_TITLE,426,62,0,<PX_TITLEW>,18,0xff261E47,0
    AF_T_AIRFRAME   =T,,422,310,0,0,0,0xFF000000,0
    AF_S_AIRFRAMEDESC=S,0xFF554C67,0xFFE6DABE,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,412,334,0,328,167,,0xFF000000
    AF_D_AIRFRAME   =D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,422,116,0,330,15,11

	[@Engine@]
    EN_T_TITLE      =T,IDS_EN_TITLE,426,62,0,<PX_TITLEW>,22,0xff261E47,0
    EN_T_ENGINENAME =T,,422,310,0,335,18,0xFF000000,0
    EN_S_ENGINEDESC =S,0xFF554C67,0xFFE6DABE,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,412,334,0,325,165,,0xFF000000
    EN_D_ENGINE     =D,<PX_SLIDER>,<PX_UP>,<PX_DOWN>,<GN_DROPUP>,<GN_DROPDOWN>,422,116,0,330,15,7
";

/// The global variables the panels' rows use, as the installation defines them
/// (`G19` to `G28` of the layout's header).
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
    let text = format!("{GLOBALS}{}", LAYOUT)
        .replace(
            "\t[@AirFrame@]\nAF_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0\n\t[@Engine@]\nEN_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0\n",
            PANEL_ROWS,
        );
    assert!(text.contains("AF_D_AIRFRAME"), "the fixture was replaced");
    OriginalLayout::parse(&text).expect("the fixture parses")
}

fn panel_names(name: &str) -> Option<String> {
    match name {
        "IDS_AF_TITLE" => Some("[AB14I]Choose an airframe".to_owned()),
        "IDS_EN_TITLE" => Some("[AB14I]Choose an engine".to_owned()),
        other => names(other),
    }
}

fn panel_page() -> HubPage {
    HubPage::from_layout(&panel_layout(), &panel_names, &ids).expect("the fixture has the rows")
}

fn airframe(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Airframe, key).expect("airframe id")
}

fn engine(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Engine, key).expect("engine id")
}

fn airframes() -> Vec<ChoiceOption> {
    vec![
        ChoiceOption {
            id: airframe(SYNTHETIC_AIRFRAME_KEY),
            name: "Ford Hoplite".to_owned(),
        },
        ChoiceOption {
            id: airframe(SYNTHETIC_OTHER_AIRFRAME_KEY),
            name: "Other Fighter".to_owned(),
        },
    ]
}

fn engines() -> Vec<ChoiceOption> {
    vec![
        ChoiceOption {
            id: engine(SYNTHETIC_ENGINE_KEY),
            name: "Inline".to_owned(),
        },
        ChoiceOption {
            id: engine(SYNTHETIC_UNMEASURED_ENGINE_KEY),
            name: "Unmeasured".to_owned(),
        },
    ]
}

fn panel(kind: ChoiceKind) -> ChoicePanel {
    ChoicePanel::from_layout(&panel_layout(), kind, &panel_names).expect("the panel's rows read")
}

#[test]
fn accept_parity5_cabin_plane_construction_airframe_engine_a_the_panels_read_the_real_rows() {
    let airframe = panel(ChoiceKind::Airframe);
    let rect = airframe.dropdown().rect;
    assert_eq!(
        (rect.x, rect.y, rect.width, rect.height),
        (422, 116, 330, 15)
    );
    assert_eq!(airframe.dropdown().displayed, 11);
    let description = airframe.description_rect();
    assert_eq!(
        (
            description.x,
            description.y,
            description.width,
            description.height
        ),
        (412, 334, 328, 167)
    );
    let texts = airframe.texts();
    assert_eq!(texts.len(), 1, "no choice yet: only the title is drawn");
    assert_eq!(texts[0].id, "AF_T_TITLE");
    assert_eq!(texts[0].text, "Choose an airframe");
    assert_eq!((texts[0].rect.x, texts[0].rect.y), (426, 62));
    assert_eq!(texts[0].color, 0xFF26_1E47);

    let engine = panel(ChoiceKind::Engine);
    assert_eq!(engine.dropdown().displayed, 7);
    assert_eq!(engine.description_rect().width, 325);
    assert_eq!(engine.texts()[0].text, "Choose an engine");
}

#[test]
fn accept_parity5_cabin_plane_construction_airframe_engine_b_a_layout_without_the_rows_leaves_the_tabs_unbuilt()
 {
    // The hub test's own fixture nests the sections with a background pane only.
    let bare = OriginalLayout::parse(LAYOUT).expect("the fixture parses");
    assert!(matches!(
        ChoicePanel::from_layout(&bare, ChoiceKind::Airframe, &names),
        Err(ChoiceError::RowMissing(_))
    ));
    let page = HubPage::from_layout(&bare, &names, &ids).expect("the hub reads");
    assert!(!page.panel_built(HubPanel::Airframe));
    assert!(!page.panel_built(HubPanel::Engine));
    // The panels the layout supplies are built, and only those.
    let page = panel_page();
    assert!(page.panel_built(HubPanel::Airframe));
    assert!(page.panel_built(HubPanel::Engine));
    assert!(!page.panel_built(HubPanel::Armor));
    assert!(!page.panel_built(HubPanel::Guns));
}

#[test]
fn accept_parity5_cabin_plane_construction_airframe_engine_c_the_list_opens_rolls_over_and_chooses()
{
    let mut panel = panel(ChoiceKind::Airframe);
    // Nothing to choose from: a press on the box is not the panel's.
    assert_eq!(panel.press_at(430, 120), ChoicePress::Nothing);
    panel.set_options(airframes(), Some(&airframe(SYNTHETIC_AIRFRAME_KEY)));
    assert_eq!(panel.selected(), Some(0));
    assert_eq!(panel.texts().len(), 2, "the title and the chosen name");
    assert_eq!(panel.texts()[1].id, "AF_T_AIRFRAME");
    assert_eq!(panel.texts()[1].text, "Ford Hoplite");
    // A press outside is nothing and leaves the list shut.
    assert_eq!(panel.press_at(10, 10), ChoicePress::Nothing);
    assert!(!panel.is_open());
    // On the box it opens the list; the rows are one box height each, below.
    assert_eq!(panel.press_at(430, 120), ChoicePress::Toggled);
    assert!(panel.is_open());
    assert_eq!(panel.item_rect(1).y, 116 + 2 * 15);
    // Rolling over the other entry is the weight line's Pending.
    panel.hover_at(Some((430, 116 + 2 * 15 + 3)));
    assert_eq!(panel.rolled(), Some(1));
    assert!(panel.weight_pending());
    panel.hover_at(Some((430, 116 + 15 + 3)));
    assert_eq!(panel.rolled(), Some(0));
    assert!(
        !panel.weight_pending(),
        "over the choice itself: not pending"
    );
    // A press on the second row chooses it and closes the list.
    let chose = panel.press_at(430, 116 + 2 * 15 + 3);
    assert_eq!(
        chose,
        ChoicePress::Chose(airframe(SYNTHETIC_OTHER_AIRFRAME_KEY))
    );
    assert!(!panel.is_open());
    assert_eq!(panel.selected(), Some(1));
    assert!(!panel.weight_pending());
    // Esc shuts an open list without choosing.
    assert_eq!(panel.press_at(430, 120), ChoicePress::Toggled);
    assert!(panel.close_list());
    assert!(!panel.close_list());
    assert_eq!(panel.selected(), Some(1));
    // A press beside an open list closes it and is not the panel's.
    assert_eq!(panel.press_at(430, 120), ChoicePress::Toggled);
    assert_eq!(panel.press_at(10, 10), ChoicePress::Nothing);
    assert!(!panel.is_open());
    // An id the list does not hold leaves no choice (never a guess).
    panel.set_options(airframes(), Some(&airframe("fixture.missing")));
    assert_eq!(panel.selected(), None);
    assert_eq!(panel.texts().len(), 1);
}

#[test]
fn accept_parity5_cabin_plane_construction_airframe_engine_d_only_the_airframe_list_makes_the_weight_pending()
 {
    let mut page = panel_page();
    page.set_feed(HubFeed {
        weight_capacity: Some(900),
        current_weight: Some(800),
        ..HubFeed::default()
    });
    let weight = |page: &HubPage| {
        page.texts()
            .into_iter()
            .find(|text| text.id == "PX_T_CURRENTWEIGHT")
            .map(|text| text.text)
    };
    page.set_choices(
        HubPanel::Airframe,
        airframes(),
        Some(&airframe(SYNTHETIC_AIRFRAME_KEY)),
    );
    page.set_choices(
        HubPanel::Engine,
        engines(),
        Some(&engine(SYNTHETIC_ENGINE_KEY)),
    );
    assert_eq!(weight(&page).as_deref(), Some("CURRENT WEIGHT:\n 800 lbs."));
    assert_eq!(page.press_at(430, 120), HubOutcome::List);
    assert!(page.list_open());
    page.hover_at(Some((430, 116 + 2 * 15 + 3)));
    assert_eq!(weight(&page).as_deref(), Some("CURRENT WEIGHT:\n Pending"));
    // The engine tab: its list's roll-over never touches the weight line
    // (`E 2219(1, item)` does not set `[0x647ba4]`).
    assert_eq!(
        page.press("PX_B_ENGINE"),
        HubOutcome::Panel(HubPanel::Engine)
    );
    assert!(!page.list_open(), "a tab shuts the list");
    assert_eq!(page.press_at(430, 120), HubOutcome::List);
    page.hover_at(Some((430, 116 + 2 * 15 + 3)));
    assert_eq!(weight(&page).as_deref(), Some("CURRENT WEIGHT:\n 800 lbs."));
}

#[test]
fn accept_parity5_cabin_plane_construction_airframe_engine_e_a_choice_edits_the_production_draft() {
    use super::parity5_cabin_plane_construction_hub::{Draft, draft};
    let Draft {
        graph,
        rules,
        policy,
        book,
        state: campaign,
    } = draft();
    let ctx = ConstructionContext {
        rules: &rules,
        policy: &policy,
        book: &book,
        graph: &graph,
    };
    let mut screen = ConstructionScreen::open(&campaign, declared_synthetic_blueprint(), vec![]);
    let held = screen.blueprint().clone();
    assert!(!screen.is_dirty());

    let mut page = panel_page();
    page.set_choices(HubPanel::Airframe, airframes(), Some(held.airframe()));
    page.set_choices(HubPanel::Engine, engines(), Some(held.engine()));
    // The airframe: the second entry.
    assert_eq!(page.press_at(430, 120), HubOutcome::List);
    let HubOutcome::Choose { panel, id } = page.press_at(430, 116 + 2 * 15 + 3) else {
        panic!("the second row is a choice");
    };
    assert_eq!(panel, HubPanel::Airframe);
    apply_choice(ChoiceKind::Airframe, id.clone(), &mut screen).expect("an airframe id");
    assert_eq!(screen.blueprint().airframe(), &id);
    assert!(screen.is_dirty(), "the draft differs from the saved plane");
    // The page's feed follows the view (cancel would now ask).
    let view = screen.view(&ctx, &campaign);
    assert!(HubFeed::from_view(&view, None).dirty);
    page.follow_choice(HubPanel::Airframe, Some(screen.blueprint().airframe()));
    assert_eq!(
        page.choice_panel(HubPanel::Airframe)
            .and_then(|p| p.selected()),
        Some(1)
    );

    // The engine: the tab, then the second entry.
    assert_eq!(
        page.press("PX_B_ENGINE"),
        HubOutcome::Panel(HubPanel::Engine)
    );
    assert_eq!(page.press_at(430, 120), HubOutcome::List);
    let HubOutcome::Choose { panel, id } = page.press_at(430, 116 + 2 * 15 + 3) else {
        panic!("the second row is a choice");
    };
    assert_eq!(panel, HubPanel::Engine);
    apply_choice(ChoiceKind::Engine, id.clone(), &mut screen).expect("an engine id");
    assert_eq!(screen.blueprint().engine(), &id);
    // A choice of the wrong kind is refused and leaves the draft alone.
    let before = screen.blueprint().clone();
    assert!(
        apply_choice(
            ChoiceKind::Engine,
            airframe(SYNTHETIC_AIRFRAME_KEY),
            &mut screen
        )
        .is_err()
    );
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

#[test]
fn accept_parity5_cabin_plane_construction_airframe_engine_f_the_window_routes_the_pointer_to_the_list_and_marks_only_unbuilt_panels()
 {
    let mut app = panel_app();
    press_button(&mut app, "PC_B_PLANEX");
    type_text(&mut app, "Ace");
    press_button(&mut app, "PN_B_OK");
    assert_eq!(section(&app), HUB_SECTION);
    // The hub opens on the Airframe tab, whose panel is built: no notice, and
    // nothing counted as missing.
    let flow = state(&app).flow().expect("a flow");
    assert!(flow.notice().is_none());
    assert!(flow.not_implemented_seen().is_empty());
    assert_eq!(flow.screen(), Screen::Construction);
    // The pilot's draft reaches the panels through the page.
    state_mut(&mut app).set_hub_choices(
        HubPanel::Airframe,
        airframes(),
        Some(&airframe(SYNTHETIC_AIRFRAME_KEY)),
    );
    app.update();
    // Open the list, then pick the second row.
    click(&mut app, 430.0, 120.0);
    assert!(hub(&app).list_open());
    click(&mut app, 430.0, (116 + 2 * 15 + 3) as f32);
    assert!(!hub(&app).list_open());
    assert_eq!(
        state(&app).hub_outcomes(),
        [
            HubOutcome::List,
            HubOutcome::Choose {
                panel: HubPanel::Airframe,
                id: airframe(SYNTHETIC_OTHER_AIRFRAME_KEY),
            }
        ]
    );
    // The Engine tab is built too: no notice. The Armor tab is not: marked.
    press_button(&mut app, "PX_B_ENGINE");
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    assert_eq!(hub(&app).selected(), HubPanel::Engine);
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

fn state_mut(app: &mut App) -> bevy::prelude::Mut<'_, cs_app::ui::front_end::ShellResource> {
    app.world_mut()
        .resource_mut::<cs_app::ui::front_end::ShellResource>()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_plane_construction_airframe_engine_retail_the_installations_rows_and_strings_build_both_panels()
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
    let airframe = ChoicePanel::from_layout(&layout, ChoiceKind::Airframe, &name)
        .expect("the real AirFrame rows read");
    let engine = ChoicePanel::from_layout(&layout, ChoiceKind::Engine, &name)
        .expect("the real Engine rows read");
    assert_eq!(airframe.dropdown().displayed, 11);
    assert_eq!(engine.dropdown().displayed, 7);
    let rect = airframe.dropdown().rect;
    assert_eq!((rect.x, rect.y, rect.width), (422, 116, 330));
    let title = &airframe.texts()[0];
    assert_eq!(title.id, "AF_T_TITLE");
    assert!(!title.text.is_empty(), "the installation's title string");
    assert!(!engine.texts()[0].text.is_empty());
    // The airframes' names are the installation's strings 3000 to 3010.
    for id in 3000..=3010 {
        let raw = strings
            .resolve_id_tagged(id)
            .unwrap_or_else(|| panic!("string {id} is in the table"));
        assert!(!raw.is_empty());
    }
    // The built hub carries both.
    let page = HubPage::from_layout(&layout, &|n: &str| strings.resolve_tagged(n), &|id: u32| {
        strings.resolve_id_tagged(id)
    })
    .expect("the real hub reads");
    assert!(page.panel_built(HubPanel::Airframe));
    assert!(page.panel_built(HubPanel::Engine));
}
