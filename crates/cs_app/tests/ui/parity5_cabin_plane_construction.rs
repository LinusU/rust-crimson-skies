//! Acceptance task PARITY5-CABIN-PLANE-CONSTRUCTION (#1958): the cabin's Plane
//! Construction button opens the `PlaneName` dialog. Task test prefix:
//! `accept_parity5_cabin_plane_construction_`.
//!
//! * The synthetic tests run over layout text in the original's *format* (the
//!   real column layout, invented art extents and strings). They prove the
//!   page's rules and the route from the cabin button to the dialog, through
//!   OK and Cancel, using the production start flow and the headless
//!   presenter; they run in CI and prove nothing about the retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows, the installation's
//!   own text and the plate's placement. `#[ignore]`d because CI has no
//!   installation.

use bevy::app::App;
use bevy::ecs::message::MessageWriter;
use bevy::input::ButtonInput;
use bevy::input::ButtonState;
use bevy::input::InputSystems;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode};
use bevy::input::mouse::MouseButton;
use bevy::prelude::{Entity, IntoScheduleConfigs, PreUpdate, ResMut, Resource, Vec2, With};
use bevy::window::{CursorMoved, PrimaryWindow};

use cs_app::ui::front_end::plane_name::{
    CANCEL_BUTTON, DEFAULT_BUTTON, NAME_REQUIRED_STRING, OK_BUTTON,
};
use cs_app::ui::front_end::{
    CONSTRUCTION_NOT_BUILT, MenuShell, MissionEnd, NAME_SECTION, NOT_IMPLEMENTED, NameOutcome,
    OriginalLayout, PlaneNamePage, SECTION_SURFACE, Screen, ShellResource, StartFlow,
    headless_app_plane_name,
};

/// The real section's rows and column layout, with invented art and a cabin
/// to come from.
const LAYOUT: &str = "\
[@PassengerCabin@]
PC_BACKGROUND=P,bg.png,0,0,4,1,0,2,0
PC_B_NEWMISSION=B,btn.png,0,0,0,0,,0x1000,0
PC_B_PLANEX=B,btn.png,0,40,0,0,PlaneName,0x1000,0

[@PlaneName@]
PX_P_BACKGROUND=P,bg.png,0,0,1,1,0,0,0
PN_P_BACKGROUND=P,plate.png,0,0,2,1,0,1,0
PN_B_OK=B,okcancel.png,74,130,0,IDS_OK,,,,,,,,,0x41A5A500,0xFFFFFF92,0xFFFFFF00,0x37277A00
PN_B_CANCEL=B,okcancel.png,161,130,0,IDS_CANCEL,,,,,,,,,0x41A5A500,0xFFFFFF92,0xFFFFFF00,0x37277A00
PN_B_DEFAULT=B,check.png,27,83,0,0,,,,,,,,1,1,
PN_E_NAME=E,FONT_PN_EDIT,23,40,0,211,16,7,0xFFFF0020,,0xFFEF0010
PN_T_TITLE=T,IDS_PN_TITLE,30,20,0,205,18,0xFF000000,0
PN_T_DEFAULT=T,IDS_PN_DEFAULTLOADOUT,49,82,0,191,36,0xFF000000,0
";

/// The plate is 300x200 on the 800x600 surface: its corner is at (250, 200).
const ORIGIN: (i32, i32) = (250, 200);

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "bg.png" => Some((800, 600)),
        "plate.png" => Some((300, 200)),
        // Four 32-row frames.
        "btn.png" | "okcancel.png" => Some((80, 128)),
        // Two sets of four state frames of 29.
        "check.png" => Some((20, 232)),
        _ => None,
    }
}

fn names(name: &str) -> Option<String> {
    Some(
        match name {
            "IDS_OK" => "[TREB9B]OK",
            "IDS_CANCEL" => "[TREB9B]Cancel",
            "IDS_PN_TITLE" => "[AB14I]Name your plane",
            "IDS_PN_DEFAULTLOADOUT" => "[TREB9B]Default loadout",
            _ => return None,
        }
        .to_owned(),
    )
}

fn ids(id: u32) -> Option<String> {
    (id == NAME_REQUIRED_STRING).then(|| "[TREB9B]Please enter a name.".to_owned())
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(LAYOUT).expect("the fixture parses")
}

fn page() -> PlaneNamePage {
    PlaneNamePage::from_layout(&layout(), &names, &ids, &extents).expect("the fixture has the rows")
}

fn shell() -> MenuShell {
    MenuShell::build(&layout(), &extents, &|_: &str| None)
}

// ------------------------------------------------------- the synthetic half ---

#[test]
fn accept_parity5_cabin_plane_construction_a_the_page_reads_its_rows_relative_to_the_plate() {
    let page = page();
    let edit = page.edit();
    assert_eq!(edit.font, "FONT_PN_EDIT");
    // The row's own 23,40 is an offset from the centred plate.
    assert_eq!(
        (edit.rect.x, edit.rect.y, edit.rect.width, edit.rect.height),
        (23 + ORIGIN.0, 40 + ORIGIN.1, 211, 16)
    );
    assert_eq!(edit.max_chars, 7);
    assert_eq!(edit.text_color, 0xFFFF_0020);
    assert_eq!(edit.frame_color, None);
    let labels: Vec<(&str, i32, i32)> = page
        .labels()
        .iter()
        .map(|l| (l.text.as_str(), l.rect.x, l.rect.y))
        .collect();
    assert_eq!(
        labels,
        [
            ("Name your plane", 30 + ORIGIN.0, 20 + ORIGIN.1),
            ("Default loadout", 49 + ORIGIN.0, 82 + ORIGIN.1)
        ]
    );
    let captions: Vec<(&str, &str)> = page
        .captions()
        .iter()
        .map(|c| (c.button.as_str(), c.text.as_str()))
        .collect();
    assert_eq!(captions, [("PN_B_OK", "OK"), ("PN_B_CANCEL", "Cancel")]);
    // The check box row says it starts checked.
    assert!(page.default_loadout());
    assert_eq!(page.text(), "");
}

#[test]
fn accept_parity5_cabin_plane_construction_b_the_shell_centres_the_plate_and_offsets_the_rest() {
    let mut shell = shell();
    shell.open(NAME_SECTION).expect("the section opens");
    let view = shell.view(SECTION_SURFACE).expect("a view");
    let pane = |id: &str| {
        view.panes
            .iter()
            .find(|pane| pane.id.eq_ignore_ascii_case(id))
            .map(|pane| (pane.logical.x as i32, pane.logical.y as i32))
            .unwrap_or_else(|| panic!("{id} is drawn"))
    };
    // The backdrop is the one absolute row.
    assert_eq!(pane("PX_P_BACKGROUND"), (0, 0));
    assert_eq!(pane("PN_P_BACKGROUND"), ORIGIN);
    let button = |id: &str| {
        view.buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case(id))
            .map(|button| (button.rect.x as i32, button.rect.y as i32))
            .unwrap_or_else(|| panic!("{id} is shown"))
    };
    assert_eq!(button("PN_B_OK"), (74 + ORIGIN.0, 130 + ORIGIN.1));
    assert_eq!(button("PN_B_CANCEL"), (161 + ORIGIN.0, 130 + ORIGIN.1));
    assert_eq!(button("PN_B_DEFAULT"), (27 + ORIGIN.0, 83 + ORIGIN.1));
}

#[test]
fn accept_parity5_cabin_plane_construction_c_a_layout_without_the_edit_row_or_plate_is_refused_by_name()
 {
    let text = LAYOUT.replace("PN_E_NAME", "PN_E_OTHER");
    let broken = OriginalLayout::parse(&text).expect("parses");
    let error = PlaneNamePage::from_layout(&broken, &names, &ids, &extents).expect_err("no edit");
    assert!(error.to_string().contains("PN_E_NAME"), "{error}");
    let unmeasured = |art: &str| (art != "plate.png").then(|| extents(art)).flatten();
    let error = PlaneNamePage::from_layout(&layout(), &names, &ids, &unmeasured)
        .expect_err("the plate is unread");
    assert!(error.to_string().contains("plate"), "{error}");
}

#[test]
fn accept_parity5_cabin_plane_construction_d_ok_needs_a_name_and_typing_stops_at_max_chars() {
    let mut page = page();
    assert_eq!(page.press(OK_BUTTON), NameOutcome::NameRequired);
    assert_eq!(page.notice(), Some("Please enter a name."));
    // The next key closes the message.
    page.type_text("Banshee-X\u{8}");
    assert_eq!(page.text(), "Banshee", "seven characters, no control codes");
    assert_eq!(page.notice(), None);
    page.backspace();
    page.backspace();
    assert_eq!(page.text(), "Bansh");
    assert_eq!(
        page.press(OK_BUTTON),
        NameOutcome::Construct {
            name: "Bansh".to_owned(),
            default_loadout: true
        }
    );
    assert_eq!(page.press(CANCEL_BUTTON), NameOutcome::Cancel);
    assert_eq!(page.escape(), NameOutcome::Cancel);
    assert_eq!(page.press("PN_B_OTHER"), NameOutcome::Nothing);
}

// -------------------------------------------------- the headless presenter ---

#[derive(Resource, Default)]
struct TestInput {
    cursor: Option<Vec2>,
    keys: Vec<(KeyCode, bool)>,
    mouse: Vec<bool>,
    typed: Vec<KeyboardInput>,
}

fn inject(
    keys: ResMut<ButtonInput<KeyCode>>,
    mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: MessageWriter<CursorMoved>,
    mut typed: MessageWriter<KeyboardInput>,
    mut queue: ResMut<TestInput>,
    window: bevy::prelude::Query<Entity, With<PrimaryWindow>>,
) {
    let mut keys = keys;
    keys.clear();
    for (key, down) in queue.keys.drain(..) {
        if down {
            keys.press(key);
        } else {
            keys.release(key);
        }
    }
    let mut mouse = mouse;
    mouse.clear();
    for down in queue.mouse.drain(..) {
        if down {
            mouse.press(MouseButton::Left);
        } else {
            mouse.release(MouseButton::Left);
        }
    }
    for event in queue.typed.drain(..) {
        typed.write(event);
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

fn type_text(app: &mut App, text: &str) {
    for character in text.chars() {
        let typed = character.to_string();
        drive(app, |queue| {
            queue.typed.push(KeyboardInput {
                key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
                logical_key: Key::Character(typed.as_str().into()),
                state: ButtonState::Pressed,
                text: Some(typed.as_str().into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
        });
    }
}

fn tap(app: &mut App, key: KeyCode) {
    drive(app, |queue| queue.keys.push((key, true)));
    drive(app, |queue| queue.keys.push((key, false)));
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

fn plane(app: &App) -> &PlaneNamePage {
    state(app)
        .plane_name()
        .expect("the app has the Plane Name page")
}

fn section(app: &App) -> String {
    state(app)
        .shell
        .section_name()
        .unwrap_or_default()
        .to_owned()
}

/// The cabin of a start flow that came back from a flight, with the page.
fn cabin_app() -> App {
    let mut shell = shell();
    shell.open("PassengerCabin").expect("the cabin opens");
    let flow = StartFlow::after_mission(MissionEnd::ReturnedToCabin).expect("the walk");
    assert_eq!(flow.screen(), Screen::Cabin);
    let mut app = headless_app_plane_name(shell, Some(flow), page());
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    app
}

fn open_dialog(app: &mut App) {
    press_button(app, "PC_B_PLANEX");
    assert_eq!(section(app), NAME_SECTION);
    assert_eq!(
        state(app).flow().expect("a flow").screen(),
        Screen::Construction
    );
}

#[test]
fn accept_parity5_cabin_plane_construction_e_the_cabin_button_opens_the_dialog_not_a_notice() {
    assert!(
        NOT_IMPLEMENTED
            .iter()
            .all(|entry| entry.button != "PC_B_PLANEX"),
        "the button's marked entry is gone"
    );
    let mut app = cabin_app();
    open_dialog(&mut app);
    let flow = state(&app).flow().expect("a flow");
    assert!(flow.notice().is_none());
    assert!(flow.not_implemented_seen().is_empty());
    assert!(flow.refusals().is_empty(), "{:?}", flow.refusals());
}

#[test]
fn accept_parity5_cabin_plane_construction_f_cancel_and_esc_return_to_the_cabin() {
    let mut app = cabin_app();
    open_dialog(&mut app);
    press_button(&mut app, CANCEL_BUTTON);
    assert_eq!(section(&app), "PassengerCabin");
    assert_eq!(state(&app).flow().expect("a flow").screen(), Screen::Cabin);
    open_dialog(&mut app);
    tap(&mut app, KeyCode::Escape);
    assert_eq!(section(&app), "PassengerCabin");
    assert_eq!(state(&app).flow().expect("a flow").screen(), Screen::Cabin);
    assert_eq!(
        state(&app).plane_outcomes(),
        [NameOutcome::Cancel, NameOutcome::Cancel]
    );
    // The dialog opens again from the cabin.
    open_dialog(&mut app);
}

#[test]
fn accept_parity5_cabin_plane_construction_g_ok_with_a_name_reaches_the_construction_step() {
    let mut app = cabin_app();
    open_dialog(&mut app);
    // An empty name keeps the dialog and shows message box 203's text.
    press_button(&mut app, OK_BUTTON);
    assert_eq!(plane(&app).notice(), Some("Please enter a name."));
    assert_eq!(section(&app), NAME_SECTION);
    type_text(&mut app, "Ace");
    assert_eq!(plane(&app).text(), "Ace");
    assert_eq!(plane(&app).notice(), None);
    // The check box toggles through the shell and reaches the page.
    press_button(&mut app, DEFAULT_BUTTON);
    assert!(!plane(&app).default_loadout());
    tap(&mut app, KeyCode::Enter);
    assert_eq!(
        state(&app).plane_outcomes().last(),
        Some(&NameOutcome::Construct {
            name: "Ace".to_owned(),
            default_loadout: false
        })
    );
    {
        // The construction screen proper is not built: the marked notice is
        // up over the dialog and recorded, which blocks a cabin release.
        let flow = state(&app).flow().expect("a flow");
        assert_eq!(flow.notice(), Some(&CONSTRUCTION_NOT_BUILT));
        assert_eq!(flow.not_implemented_seen(), [CONSTRUCTION_NOT_BUILT]);
        assert_eq!(flow.screen(), Screen::Construction);
    }
    // Typing is ignored under the notice; a click closes it, and the dialog
    // still holds the name.
    type_text(&mut app, "zz");
    assert_eq!(plane(&app).text(), "Ace");
    drive(&mut app, |queue| queue.mouse.push(true));
    drive(&mut app, |queue| queue.mouse.push(false));
    assert!(state(&app).flow().expect("a flow").notice().is_none());
    assert_eq!(section(&app), NAME_SECTION);
    assert_eq!(plane(&app).text(), "Ace");
}

// ------------------------------------------------------------ the retail half ---

/// **Retail.** The real `[@PlaneName@]` rows read, the installation's own text
/// answers the captions and the empty-name message, and the plate is placed
/// by its measured extent.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_cabin_plane_construction_retail_the_original_rows_present_with_the_installations_text()
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
    let page = PlaneNamePage::from_layout(
        &layout,
        &|name: &str| strings.resolve_tagged(name),
        &|id: u32| strings.resolve_id_tagged(id),
        &|art: &str| screens.art_extent(art),
    )
    .expect("the retail rows read");
    let edit = page.edit();
    assert_eq!(edit.font, "FONT_PN_EDIT");
    assert_eq!(
        (edit.rect.width, edit.rect.height, edit.max_chars),
        (211, 16, 16)
    );
    let plate = screens
        .art_extent("PX_PlaneNameBackground.Png")
        .expect("the plate measures");
    let origin = cs_app::ui::front_end::plane_name::plate_origin(plate);
    assert_eq!(
        (edit.rect.x, edit.rect.y),
        (23 + origin.0, 40 + origin.1),
        "the edit box is relative to the centred plate"
    );
    let captions: Vec<&str> = page.captions().iter().map(|c| c.text.as_str()).collect();
    assert_eq!(captions.len(), 2, "OK and Cancel: {captions:?}");
    assert_eq!(page.labels().len(), 2, "title and default-loadout text");
    assert!(page.default_loadout());
    let mut page = page;
    assert_eq!(page.press(OK_BUTTON), NameOutcome::NameRequired);
    assert!(
        page.notice().is_some_and(|text| !text.is_empty()),
        "string {NAME_REQUIRED_STRING} answers"
    );
}
