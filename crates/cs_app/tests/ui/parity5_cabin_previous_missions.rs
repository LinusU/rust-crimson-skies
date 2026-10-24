//! Acceptance task PARITY5-CABIN-PREVIOUS-MISSIONS (#1956): the cabin's
//! Previous Missions screen (`ScrapBook_TOC`). Task test prefix:
//! `accept_parity5_cabin_previous_missions_`.
//!
//! * The synthetic tests run over layout text in the original's *format*
//!   (the real column layout, invented art extents and strings). They prove
//!   the page's rules and the route from the cabin button to the screen and
//!   back through the production start flow and the headless presenter; they
//!   run in CI and prove nothing about the retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows, the installation's
//!   own text and the header row. `#[ignore]`d because CI has no installation.

use bevy::app::App;
use bevy::ecs::message::MessageWriter;
use bevy::input::ButtonInput;
use bevy::input::InputSystems;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::{Entity, IntoScheduleConfigs, PreUpdate, ResMut, Resource, Vec2, With};
use bevy::window::{CursorMoved, PrimaryWindow};

use cs_app::ui::front_end::previous_missions::{
    CURRENT_BUTTON, REPLAY_BUTTON, RETURN_BUTTON, VIEW_BUTTON,
};
use cs_app::ui::front_end::{
    MenuShell, MissionEnd, MissionProgress, OriginalLayout, PreviousMissionsPage, SECTION_SURFACE,
    Screen, ShellResource, StartFlow, TOC_SECTION, TocOutcome, headless_app_previous_missions,
};

/// The real section's rows and column layout, with invented art and a cabin
/// to come from.
const LAYOUT: &str = "\
[@PassengerCabin@]
PC_BACKGROUND=P,bg.png,0,0,4,1,0,2,0
PC_B_NEWMISSION=B,btn.png,0,0,0,0,,0x1000,0
PC_B_PREVIOUS=B,btn.png,0,40,0,0,ScrapBook_TOC,0x1000,0

[@ScrapBook_TOC@]
V1=TCOLOR1,0xFF000000
V2=TCOLOR2,0xFF000000
V3=SLIDER,CM_B_ScrollBar.png
V4=UP,CM_B_ScrollUp.png
V5=DOWN,CM_B_ScrollDown.png
SBTOC_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
SBTOC_B_VIEW    =B,paper.png,440,505,0,IDS_SBTOC_B_VIEW,,,,,,,,,,<TCOLOR2>,<TCOLOR1>,<TCOLOR1>,<TCOLOR1>
SBTOC_B_REPLAY  =B,paper.png,596,505,0,IDS_SBTOC_B_REPLAY,,0x1000,,,,,,,,<TCOLOR2>,<TCOLOR1>,<TCOLOR1>,<TCOLOR1>
SBTOC_B_RETURN  =B,ret.png,593,561,0,0,PassengerCabin,0x1000,1,,,,,,
SBTOC_B_CURRENT =B,tab.png,558,7,200,IDS_SB_B_CURRENT,,,,,,,,,,<TCOLOR2>,<TCOLOR1>,<TCOLOR1>,<TCOLOR1>
SBTOC_T_CHARACTER   =T,,425,64,0,300,24,0xFF000000,2
SBTOC_T_MISSIONS    =T,IDS_SBTOC_PREVIOUS,425,114,0,300,24,0xFF000000,2
SBTOC_L_TOCList   =L,<SLIDER>,<UP>,<DOWN>,420,140,0,325,80,4
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "bg.png" => Some((800, 600)),
        // Four 32-row frames.
        "btn.png" => Some((200, 128)),
        "paper.png" => Some((150, 120)),
        "ret.png" => Some((150, 120)),
        "tab.png" => Some((200, 120)),
        "fc_planeicons.png" => Some((80, 12 * 80)),
        _ => None,
    }
}

fn names(name: &str) -> Option<String> {
    Some(
        match name {
            "IDS_SBTOC_B_VIEW" => "[TREB9B]View Selected",
            "IDS_SBTOC_B_REPLAY" => "[TREB9B]Replay Mission",
            "IDS_SB_B_CURRENT" => "[AB14I]Current Mission",
            "IDS_SBTOC_PREVIOUS" => "[TREB9B]Previous Missions",
            _ => return None,
        }
        .to_owned(),
    )
}

fn ids(id: u32) -> Option<String> {
    Some(match id {
        0x4c1 => "[TREB9B]Header one".to_owned(),
        0x4c2 => "[TREB9B]Header two".to_owned(),
        n if n > 0xd97 && n <= 0xd97 + 24 => format!("[TREB9B]Mission {}", n - 0xd97),
        _ => return None,
    })
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(LAYOUT).expect("the fixture parses")
}

fn page(progress: MissionProgress) -> PreviousMissionsPage {
    PreviousMissionsPage::from_layout(&layout(), &names, &ids, &extents, progress)
        .expect("the fixture has the rows")
}

fn progress() -> MissionProgress {
    MissionProgress {
        completed: 3,
        selected: 3,
        replayable: vec![2],
        icons: vec![(1, 4), (2, 5)],
        stored_text: vec![(2, "Flown".to_owned())],
    }
}

// ------------------------------------------------------- the synthetic half ---

#[test]
fn accept_parity5_cabin_previous_missions_a_the_page_reads_the_list_rows_and_captions() {
    let page = page(progress());
    let list = page.list();
    assert_eq!(
        (list.rect.x, list.rect.y, list.rect.width, list.row_height),
        (420, 140, 325, 80)
    );
    assert_eq!(list.visible, 4);
    assert_eq!(list.rect.height, 320);
    // The row without a resource id is filled at run time and draws nothing.
    let labels: Vec<&str> = page.labels().iter().map(|l| l.text.as_str()).collect();
    assert_eq!(labels, ["Previous Missions"]);
    let captions: Vec<(&str, &str)> = page
        .captions()
        .iter()
        .map(|c| (c.button.as_str(), c.text.as_str()))
        .collect();
    assert_eq!(
        captions,
        [
            ("SBTOC_B_VIEW", "View Selected"),
            ("SBTOC_B_REPLAY", "Replay Mission"),
            ("SBTOC_B_CURRENT", "Current Mission"),
        ]
    );
    // Header row plus one row per completed mission.
    let rows = page.rows();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].icon, Some(11));
    assert_eq!(
        rows[0].lines,
        [
            Some("Header one".to_owned()),
            Some("Header two".to_owned()),
            None
        ]
    );
    assert_eq!(rows[2].index, 2);
    assert_eq!(rows[2].icon, Some(5));
    assert_eq!(
        rows[2].lines,
        [Some("Mission 2".to_owned()), None, Some("Flown".to_owned())]
    );
    // The text starts at the icon frame's width plus 20.
    assert_eq!(page.text_offset(), 80 + 20);
    // A pilot who has done nothing sees the header row alone, with no replay.
    let fresh = self::page(MissionProgress::fresh());
    assert_eq!(fresh.rows().len(), 1);
    assert!(!fresh.replay_enabled());
}

#[test]
fn accept_parity5_cabin_previous_missions_b_a_layout_without_the_list_is_refused_by_name() {
    let text = LAYOUT.replace("SBTOC_L_TOCList", "SBTOC_L_Other");
    let layout = OriginalLayout::parse(&text).expect("parses");
    let error = PreviousMissionsPage::from_layout(
        &layout,
        &names,
        &ids,
        &extents,
        MissionProgress::fresh(),
    )
    .expect_err("no list row");
    assert!(error.to_string().contains("SBTOC_L_TOCList"), "{error}");
}

// -------------------------------------------------- the headless presenter ---

#[derive(Resource, Default)]
struct TestInput {
    cursor: Option<Vec2>,
    mouse: Vec<bool>,
}

fn inject(
    mouse: ResMut<ButtonInput<MouseButton>>,
    keys: ResMut<ButtonInput<KeyCode>>,
    mut cursor: MessageWriter<CursorMoved>,
    mut queue: ResMut<TestInput>,
    window: bevy::prelude::Query<Entity, With<PrimaryWindow>>,
) {
    let mut keys = keys;
    keys.clear();
    let mut mouse = mouse;
    mouse.clear();
    for down in queue.mouse.drain(..) {
        if down {
            mouse.press(MouseButton::Left);
        } else {
            mouse.release(MouseButton::Left);
        }
    }
    if let Some(position) = queue.cursor.take() {
        cursor.write(CursorMoved {
            window: window.single().unwrap_or(Entity::PLACEHOLDER),
            position,
            delta: None,
        });
    }
}

fn drive(app: &mut App, batch: impl FnOnce(&mut TestInput)) {
    batch(&mut app.world_mut().resource_mut::<TestInput>());
    app.update();
}

/// A press and release at the logical point `(x, y)` (the headless surface is
/// the 800x600 image, so the point is the window point).
fn press_at(app: &mut App, x: i32, y: i32) {
    drive(app, |queue| {
        queue.cursor = Some(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
        queue.mouse.push(true);
    });
    drive(app, |queue| queue.mouse.push(false));
}

fn press_button(app: &mut App, id: &str) {
    let (x, y) = {
        let state = app.world().resource::<ShellResource>();
        let view = state.shell.view(SECTION_SURFACE).expect("a view");
        let button = view
            .buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case(id))
            .unwrap_or_else(|| panic!("the section shows {id}"));
        (
            button.rect.x as f32 + button.rect.width as f32 * 0.5,
            button.rect.y as f32 + button.rect.height as f32 * 0.5,
        )
    };
    drive(app, |queue| {
        queue.cursor = Some(Vec2::new(x, y));
        queue.mouse.push(true);
    });
    drive(app, |queue| queue.mouse.push(false));
}

fn state(app: &App) -> &ShellResource {
    app.world().resource::<ShellResource>()
}

fn toc(app: &App) -> &PreviousMissionsPage {
    state(app)
        .previous_missions()
        .expect("the app has the Previous Missions page")
}

fn section(app: &App) -> String {
    state(app)
        .shell
        .section_name()
        .unwrap_or_default()
        .to_owned()
}

/// The cabin of a start flow that came back from a flight, with the page.
fn cabin_app(progress: MissionProgress) -> App {
    let layout = layout();
    let no_scripts = |_: &str| -> Option<String> { None };
    let mut shell = MenuShell::build(&layout, &extents, &no_scripts);
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    assert_eq!(flow.screen(), Screen::Cabin);
    let mut app = headless_app_previous_missions(shell, Some(flow), page(progress));
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    app
}

fn open_toc(app: &mut App) {
    press_button(app, "PC_B_PREVIOUS");
    assert_eq!(section(app), TOC_SECTION);
    assert_eq!(
        state(app).flow().expect("a flow").screen(),
        Screen::Scrapbook
    );
}

#[test]
fn accept_parity5_cabin_previous_missions_c_the_cabin_button_opens_the_screen_and_return_comes_back()
 {
    let mut app = cabin_app(progress());
    open_toc(&mut app);
    {
        let flow = state(&app).flow().expect("a flow");
        assert!(
            flow.notice().is_none(),
            "no marked notice: the screen is real"
        );
        assert!(flow.not_implemented_seen().is_empty());
        assert!(flow.refusals().is_empty(), "{:?}", flow.refusals());
    }
    press_button(&mut app, RETURN_BUTTON);
    assert_eq!(section(&app), "PassengerCabin");
    assert_eq!(state(&app).flow().expect("a flow").screen(), Screen::Cabin);
}

#[test]
fn accept_parity5_cabin_previous_missions_d_a_click_selects_a_mission_and_the_replay_gate_follows()
{
    let mut app = cabin_app(progress());
    open_toc(&mut app);
    // Mission 3 is the authored current one and is not replayable.
    assert_eq!(toc(&app).selected(), 3);
    assert!(!toc(&app).replay_enabled());
    press_button(&mut app, REPLAY_BUTTON);
    assert_eq!(state(&app).previous_outcomes(), [TocOutcome::ReplayOff]);
    // Row 2 (the third 80-px row of the list at y 140): replayable.
    let list = toc(&app).list().rect;
    press_at(&mut app, list.x + 100, list.y + 80 * 2 + 40);
    assert_eq!(toc(&app).selected(), 2);
    assert!(toc(&app).replay_enabled());
    press_button(&mut app, REPLAY_BUTTON);
    assert_eq!(
        state(&app).previous_outcomes().last(),
        Some(&TocOutcome::Replay { mission: 2 })
    );
    // A press below the last row selects nothing.
    press_at(&mut app, list.x + 100, list.y + 80 * 3 + 79);
    assert_eq!(toc(&app).selected(), 3, "row 3 is the last row");
    let beyond = list.y + list.height as i32 + 5;
    press_at(&mut app, list.x + 100, beyond);
    assert_eq!(toc(&app).selected(), 3);
}

#[test]
fn accept_parity5_cabin_previous_missions_e_view_and_current_name_the_unbuilt_journal() {
    let mut app = cabin_app(progress());
    open_toc(&mut app);
    press_button(&mut app, VIEW_BUTTON);
    {
        let flow = state(&app).flow().expect("a flow");
        let notice = flow.notice().expect("the journal is marked not built");
        assert_eq!(notice.button, VIEW_BUTTON);
        assert_eq!(flow.screen(), Screen::Scrapbook, "still on the list");
    }
    // A click closes the notice, and the screen under it is the list.
    press_at(&mut app, 10, 10);
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    assert_eq!(section(&app), TOC_SECTION);
    press_button(&mut app, CURRENT_BUTTON);
    assert_eq!(
        state(&app).previous_outcomes(),
        [
            TocOutcome::JournalNotBuilt {
                button: VIEW_BUTTON
            },
            TocOutcome::JournalNotBuilt {
                button: CURRENT_BUTTON
            }
        ]
    );
}

// ------------------------------------------------------------ the retail half ---

/// **Retail.** The real `[@ScrapBook_TOC@]` rows read, the installation's own
/// text answers the captions, the header row's two lines and the first
/// mission's title.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_previous_missions_retail_the_original_rows_present_with_the_installations_text()
 {
    use cs_app::ui::front_end::FrontEndScreens;
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let game_dir = std::path::Path::new(&game_dir);
    let screens = FrontEndScreens::open(game_dir).expect("the screens open");
    let layout = screens.original_layout().expect("the layout reads");
    let strings = cs_app::mission_session::FlightCheckStrings::read(game_dir, &screens)
        .expect("the string table reads");
    let page = PreviousMissionsPage::from_layout(
        &layout,
        &|name: &str| strings.resolve_tagged(name),
        &|id: u32| strings.resolve_id_tagged(id),
        &|art: &str| screens.art_extent(art),
        MissionProgress {
            completed: 2,
            ..MissionProgress::fresh()
        },
    )
    .expect("the retail rows read");
    let list = page.list();
    assert_eq!(
        (
            list.rect.x,
            list.rect.y,
            list.rect.width,
            list.row_height,
            list.visible
        ),
        (420, 140, 325, 80, 4)
    );
    let labels: Vec<&str> = page.labels().iter().map(|l| l.text.as_str()).collect();
    assert_eq!(labels, ["Previous Missions"]);
    let captions: Vec<&str> = page.captions().iter().map(|c| c.text.as_str()).collect();
    assert_eq!(
        captions,
        ["View Selected", "Replay Mission", "Current Mission"]
    );
    assert!(page.icon_frame().is_some(), "the icon strip measures");
    let rows = page.rows();
    assert_eq!(rows.len(), 3);
    assert!(
        rows[0].lines[0].is_some() && rows[0].lines[1].is_some(),
        "the header row's two strings answer: {:?}",
        rows[0].lines
    );
    assert!(
        rows[1].lines[0].is_some() && rows[2].lines[0].is_some(),
        "the missions' titles answer: {rows:?}"
    );
    assert_ne!(rows[1].lines[0], rows[2].lines[0]);
}
