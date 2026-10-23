//! Acceptance task FE-MENU-3 (#1835): the Campaign pilot-selection page.
//!
//! The `accept_fe_menu_3_` tests call the production page: [`PilotPage`]
//! reads the `E` and `L` rows of the `Campaign` section, owns the typing,
//! the list selection and the Continue / Delete rules, and drives the
//! existing [`ProfileSession`]; the headless presenter takes keyboard
//! messages, the pointer and the F45 machine through the same path the
//! windowed run uses.
//!
//! * The synthetic tests run over layout text in the original's *format*
//!   and a sandbox profile population; they prove the page's rules and the
//!   persistence and run in CI. They prove nothing about the retail data.
//! * The `retail` test reads `$CS_GAME_DIR`: the real rows' geometry,
//!   colours and limits. `#[ignore]`d because CI has no installation.
//! * The windowed test drives `cs --front-end --smoke-seconds` on a GPU
//!   over a seeded population and checks the page's framebuffers arrive.
//!   `#[ignore]`d because CI has neither the installation nor a GPU.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use bevy::app::App;
use bevy::ecs::message::MessageWriter;
use bevy::input::ButtonInput;
use bevy::input::ButtonState;
use bevy::input::InputSystems;
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput, NativeKeyCode};
use bevy::input::mouse::MouseButton;
use bevy::prelude::{Entity, IntoScheduleConfigs, PreUpdate, ResMut, Resource, Vec2, With};
use bevy::window::{CursorMoved, PrimaryWindow};

use cs_app::profile::ProfileSession;
use cs_app::ui::front_end::{
    FrontEndScreens, MAX_PILOTS, MenuShell, OriginalLayout, PILOT_SECTION, PilotNotice,
    PilotOutcome, PilotPage, PilotPageError, PilotSetup, ProfileIntent, Request, SECTION_SURFACE,
    START_SECTION, Screen, ShellResource, StartFlow, headless_app_pilots, script_member,
};
use cs_content::save::settings::SettingCatalog;

// ------------------------------------------------------- the synthetic half ---

/// An invented layout in the original's format: the real section's row
/// kinds and the real column layout (the data rows omit the header's
/// `HelpID`/`TabOrder`/`ScriptPointer` columns, as the file does).
const LAYOUT: &str = "\
[@MainMenu@]
MM_B_CAMPAIGN=B,strip.png,10,10,0,0,Campaign,0x1000,1

[@Campaign@]
CM_BACKGROUND=P,pane.png,0,0,0,1,0,1,0
CM_B_START=B,strip.png,300,10,0,0,,0x1000,0
CM_B_DELETEPLAYER=B,strip.png,300,60,0,0,,0x1000,0
CM_B_CANCEL=B,strip.png,300,110,0,0,MainMenu,0x1000,0
CM_E_NAME=E,FONT_CM_EDIT,20,20,0,200,22,8,0xFFFF0000,,0xFFEF0010
CM_L_PLAYERS=L,bar.png,up.png,down.png,20,60,0,200,20,3

[@PassengerCabin@]
PC_B_BACK=B,strip.png,5,5,0,0,MainMenu,0x1000,1
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "pane.png" => Some((400, 300)),
        "strip.png" => Some((80, 160)),
        _ => None,
    }
}

fn layout() -> OriginalLayout {
    OriginalLayout::parse(LAYOUT).expect("the fixture parses")
}

fn page() -> PilotPage {
    PilotPage::from_layout(&layout()).expect("the fixture has the rows")
}

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A fresh, empty directory a sandbox population lives in.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cs-fe-menu-3-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the temp directory is created");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn session(dir: &Path) -> ProfileSession {
    ProfileSession::open_sandbox(dir, &SettingCatalog::empty()).expect("the sandbox opens")
}

fn names(page: &PilotPage) -> Vec<&str> {
    page.entries()
        .iter()
        .map(|entry| entry.name.as_str())
        .collect()
}

#[test]
fn accept_fe_menu_3_a_page_reads_the_edit_and_list_rows_as_written() {
    let page = page();
    let edit = page.edit();
    assert_eq!(edit.font, "FONT_CM_EDIT");
    assert_eq!(
        (edit.rect.x, edit.rect.y, edit.rect.width, edit.rect.height),
        (20, 20, 200, 22)
    );
    assert_eq!(edit.max_chars, 8);
    assert_eq!(edit.text_color, 0xFFFF_0000);
    assert_eq!(edit.frame_color, None, "an empty colour is the default");
    assert_eq!(edit.cursor_color, 0xFFEF_0010);

    let list = page.list();
    assert_eq!(
        (list.slider.as_str(), list.up.as_str(), list.down.as_str()),
        ("bar.png", "up.png", "down.png")
    );
    assert_eq!((list.rect.x, list.rect.y, list.rect.width), (20, 60, 200));
    assert_eq!((list.row_height, list.visible), (20, 3));
    assert_eq!(list.rect.height, 60, "3 rows of 20");

    // A missing row and an unreadable column are named, never guessed.
    let no_edit = LAYOUT.replace("CM_E_NAME=E", "OTHER=E");
    assert_eq!(
        PilotPage::from_layout(&OriginalLayout::parse(&no_edit).unwrap()).unwrap_err(),
        PilotPageError::RowMissing { id: "CM_E_NAME" }
    );
    let bad_width = LAYOUT.replace("200,22,8", "wide,22,8");
    assert_eq!(
        PilotPage::from_layout(&OriginalLayout::parse(&bad_width).unwrap()).unwrap_err(),
        PilotPageError::FieldUnread {
            id: "CM_E_NAME",
            field: "Width"
        }
    );
    let no_section = "[@MainMenu@]\nX=B,strip.png,1,1,0,0,,0x1000,1\n";
    assert_eq!(
        PilotPage::from_layout(&OriginalLayout::parse(no_section).unwrap()).unwrap_err(),
        PilotPageError::NoSection
    );
}

#[test]
fn accept_fe_menu_3_b_typing_is_bounded_and_list_selection_copies_the_name() {
    let dir = TempDir::new("typing");
    let mut session = session(dir.path());
    let mut page = page();
    page.refresh(&session);

    // Printable characters up to MaxChars; control characters never land.
    page.type_text("Maverick\u{7}-extra");
    assert_eq!(page.text(), "Maverick", "8 characters at most");
    page.backspace();
    page.backspace();
    assert_eq!(page.text(), "Maveri");
    page.type_text("\r\n");
    assert_eq!(page.text(), "Maveri", "Enter's text is a control character");

    // Three pilots, then the arrows walk the list and copy each name.
    for name in ["Ace", "Bea", "Cy"] {
        while !page.text().is_empty() {
            page.backspace();
        }
        page.type_text(name);
        assert!(matches!(
            page.press_continue(&mut session),
            PilotOutcome::Continued { created: true, .. }
        ));
    }
    assert_eq!(names(&page), ["Ace", "Bea", "Cy"]);

    while !page.text().is_empty() {
        page.backspace();
    }
    assert_eq!(page.selected(), None);
    page.move_selection(true);
    assert_eq!((page.selected(), page.text()), (Some(0), "Ace"));
    page.move_selection(true);
    page.move_selection(true);
    page.move_selection(true);
    assert_eq!(
        (page.selected(), page.text()),
        (Some(2), "Cy"),
        "the selection stops at the last row"
    );
    page.move_selection(false);
    assert_eq!((page.selected(), page.text()), (Some(1), "Bea"));

    // Typing a listed name selects it (case-insensitively).
    while !page.text().is_empty() {
        page.backspace();
    }
    page.type_text("cy");
    assert_eq!(page.selected(), Some(2));

    // A press at a list row's pixels selects that row.
    let list = page.list().clone();
    let (x, y) = (list.rect.x + 5, list.rect.y + list.row_height as i32 + 3);
    assert_eq!(page.row_at(x, y), Some(1));
    assert_eq!(
        page.row_at(x, list.rect.y + 2 * list.row_height as i32 + 1),
        Some(2)
    );
    assert_eq!(page.row_at(x, list.rect.y - 1), None, "above the list");
    assert_eq!(
        page.row_at(x, list.rect.y + 3 * list.row_height as i32 + 1),
        None,
        "below the last displayed row"
    );
    page.select_row(1);
    assert_eq!((page.selected(), page.text()), (Some(1), "Bea"));
}

#[test]
fn accept_fe_menu_3_c_continue_creates_chooses_and_persists_pilots() {
    let dir = TempDir::new("continue");
    let mut session = session(dir.path());
    let mut page = page();
    page.refresh(&session);

    // An empty name is refused, as the original's message box 200 refuses it.
    assert_eq!(page.press_continue(&mut session), PilotOutcome::Nothing);
    assert_eq!(page.notice(), Some(&PilotNotice::NameRequired));
    assert!(page.entries().is_empty());

    page.type_text("Rook");
    let PilotOutcome::Continued { id, name, created } = page.press_continue(&mut session) else {
        panic!("a new name continues");
    };
    assert_eq!((name.as_str(), created), ("Rook", true));
    assert_eq!(session.selected(), Some(id));
    assert_eq!(names(&page), ["Rook"]);

    // The population is on disk: a second session in the same directory
    // lists the pilot and has it active.
    drop(session);
    let mut session = self::session(dir.path());
    let mut page = self::page();
    page.refresh(&session);
    assert_eq!(names(&page), ["Rook"]);
    assert_eq!(session.selected(), Some(id), "the active pointer persisted");

    // Continue with a listed name (any case) chooses it, creating nothing.
    page.type_text("rOOK");
    let PilotOutcome::Continued {
        id: again, created, ..
    } = page.press_continue(&mut session)
    else {
        panic!("a listed name continues");
    };
    assert_eq!((again, created), (id, false));
    assert_eq!(names(&page), ["Rook"]);
}

#[test]
fn accept_fe_menu_3_d_the_list_holds_at_most_the_scripts_limit() {
    let dir = TempDir::new("limit");
    let mut session = session(dir.path());
    let mut page = page();
    page.refresh(&session);
    for index in 0..MAX_PILOTS {
        while !page.text().is_empty() {
            page.backspace();
        }
        page.type_text(&format!("P{index}"));
        assert!(matches!(
            page.press_continue(&mut session),
            PilotOutcome::Continued { created: true, .. }
        ));
    }
    assert_eq!(page.entries().len(), MAX_PILOTS);
    while !page.text().is_empty() {
        page.backspace();
    }
    page.type_text("One more");
    assert_eq!(page.press_continue(&mut session), PilotOutcome::Nothing);
    assert_eq!(page.notice(), Some(&PilotNotice::ListFull));
    assert_eq!(page.entries().len(), MAX_PILOTS, "no 25th pilot");

    // The list scrolls over its 3 displayed rows without selecting.
    page.scroll(5);
    assert_eq!(page.top(), 5);
    page.scroll(1000);
    assert_eq!(page.top(), MAX_PILOTS - 3);
    page.scroll(-1000);
    assert_eq!(page.top(), 0);
}

#[test]
fn accept_fe_menu_3_e_delete_needs_a_second_press_and_removes_the_save() {
    let dir = TempDir::new("delete");
    let mut session = session(dir.path());
    let mut page = page();
    page.refresh(&session);
    for name in ["Ace", "Bea"] {
        while !page.text().is_empty() {
            page.backspace();
        }
        page.type_text(name);
        let _ = page.press_continue(&mut session);
    }

    // Delete with no listed name does nothing.
    while !page.text().is_empty() {
        page.backspace();
    }
    assert_eq!(page.press_delete(&mut session), PilotOutcome::Nothing);
    assert_eq!(page.notice(), Some(&PilotNotice::NothingToDelete));

    // The first press only arms, naming the pilot; typing disarms it.
    page.type_text("Ace");
    assert_eq!(page.press_delete(&mut session), PilotOutcome::Nothing);
    assert!(page.delete_armed());
    assert_eq!(
        page.notice(),
        Some(&PilotNotice::ConfirmDelete("Ace".to_owned()))
    );
    page.backspace();
    assert!(!page.delete_armed());
    page.type_text("e");
    assert_eq!(page.press_delete(&mut session), PilotOutcome::Nothing);
    assert_eq!(names(&page), ["Ace", "Bea"], "one press deletes nothing");

    // Esc cancels an armed delete and is consumed doing so.
    assert!(page.escape());
    assert!(!page.delete_armed());
    assert!(!page.escape(), "nothing armed: Esc is Cancel's");

    // Two presses delete: the edit box clears, the pilot is gone from the
    // list and from a reopened population.
    let _ = page.press_delete(&mut session);
    assert_eq!(
        page.press_delete(&mut session),
        PilotOutcome::Deleted {
            name: "Ace".to_owned()
        }
    );
    assert_eq!((names(&page), page.text()), (vec!["Bea"], ""));
    drop(session);
    let session = self::session(dir.path());
    let mut reopened = self::page();
    reopened.refresh(&session);
    assert_eq!(names(&reopened), ["Bea"]);
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

fn key_event(logical_key: Key, text: Option<&str>) -> KeyboardInput {
    KeyboardInput {
        key_code: KeyCode::Unidentified(NativeKeyCode::Unidentified),
        logical_key,
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: false,
        window: Entity::PLACEHOLDER,
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
            queue.typed.push(key_event(
                Key::Character(typed.as_str().into()),
                Some(&typed),
            ));
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

fn page_app(dir: &Path) -> App {
    let layout = layout();
    let no_scripts = |_: &str| -> Option<String> { None };
    let mut shell = MenuShell::build(&layout, &extents, &no_scripts);
    shell.open(START_SECTION).expect("MainMenu is a section");
    let setup = PilotSetup {
        page: PilotPage::from_layout(&layout).expect("the rows"),
        session: session(dir),
    };
    let mut app = headless_app_pilots(shell, Some(StartFlow::new()), setup);
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app.update();
    press_button(&mut app, "MM_B_CAMPAIGN");
    app
}

fn state(app: &App) -> &ShellResource {
    app.world().resource::<ShellResource>()
}

fn screen(app: &App) -> Screen {
    state(app).flow().expect("the flow").screen()
}

#[test]
fn accept_fe_menu_3_f_keyboard_pointer_and_buttons_drive_the_page_through_the_machine() {
    let dir = TempDir::new("route");
    let mut app = page_app(dir.path());
    assert_eq!(screen(&app), Screen::ProfileSelect);
    assert_eq!(state(&app).shell.section_name(), Some(PILOT_SECTION));

    // Enter on an empty name stays on the page with a notice.
    tap(&mut app, KeyCode::Enter);
    assert_eq!(screen(&app), Screen::ProfileSelect);
    assert_eq!(
        state(&app).pilot_page().unwrap().notice(),
        Some(&PilotNotice::NameRequired)
    );

    // Typed text and Backspace reach the edit box through KeyboardInput.
    type_text(&mut app, "Zed");
    drive(&mut app, |queue| {
        queue.typed.push(key_event(Key::Backspace, None));
    });
    type_text(&mut app, "x");
    assert_eq!(state(&app).pilot_page().unwrap().text(), "Zex");

    // The Continue button creates the pilot and takes the machine through
    // the table's NewProfile → ConfirmProfile rows to the cabin.
    press_button(&mut app, "CM_B_START");
    assert_eq!(screen(&app), Screen::Cabin);
    assert_eq!(state(&app).shell.section_name(), Some("PassengerCabin"));
    assert!(
        state(&app)
            .flow()
            .unwrap()
            .requests()
            .contains(&Request::OpenProfile(ProfileIntent::New)),
        "{:?}",
        state(&app).flow().unwrap().requests()
    );
    assert!(matches!(
        state(&app).pilot_outcomes(),
        [PilotOutcome::Continued {
            created: true,
            name,
            ..
        }] if name == "Zex"
    ));
    let id = state(&app).pilot_session().unwrap().selected();
    assert!(id.is_some(), "the new pilot is the session's selection");

    // Back to the menu and into Campaign again: the pilot is listed, Down
    // selects it, Enter continues it (not created).
    press_button(&mut app, "PC_B_BACK");
    assert_eq!(screen(&app), Screen::MainMenu);
    press_button(&mut app, "MM_B_CAMPAIGN");
    assert_eq!(screen(&app), Screen::ProfileSelect);
    assert_eq!(state(&app).pilot_page().unwrap().entries().len(), 1);
    tap(&mut app, KeyCode::ArrowDown);
    assert_eq!(state(&app).pilot_page().unwrap().text(), "Zex");
    tap(&mut app, KeyCode::Enter);
    assert_eq!(screen(&app), Screen::Cabin);
    assert!(matches!(
        state(&app).pilot_outcomes().last(),
        Some(PilotOutcome::Continued { created: false, .. })
    ));
    assert_eq!(
        state(&app).flow().unwrap().requests().last(),
        Some(&Request::OpenProfile(ProfileIntent::Existing))
    );
}

#[test]
fn accept_fe_menu_3_g_delete_cancel_and_escape_run_through_the_input_path() {
    let dir = TempDir::new("delete-route");
    let mut app = page_app(dir.path());
    type_text(&mut app, "Dax");
    tap(&mut app, KeyCode::Enter);
    press_button(&mut app, "PC_B_BACK");
    press_button(&mut app, "MM_B_CAMPAIGN");

    // Esc cancels an armed delete first, the screen's Cancel after.
    tap(&mut app, KeyCode::ArrowDown);
    press_button(&mut app, "CM_B_DELETEPLAYER");
    assert!(state(&app).pilot_page().unwrap().delete_armed());
    tap(&mut app, KeyCode::Escape);
    assert!(!state(&app).pilot_page().unwrap().delete_armed());
    assert_eq!(screen(&app), Screen::ProfileSelect, "Esc only disarmed");

    // Two Delete Player presses delete the save.
    press_button(&mut app, "CM_B_DELETEPLAYER");
    press_button(&mut app, "CM_B_DELETEPLAYER");
    assert!(state(&app).pilot_page().unwrap().entries().is_empty());
    assert!(matches!(
        state(&app).pilot_outcomes().last(),
        Some(PilotOutcome::Deleted { name }) if name == "Dax"
    ));
    assert_eq!(screen(&app), Screen::ProfileSelect);

    // Esc with nothing armed is Cancel: back to the main menu.
    tap(&mut app, KeyCode::Escape);
    assert_eq!(screen(&app), Screen::MainMenu);

    // And the Cancel button does the same from a fresh visit.
    press_button(&mut app, "MM_B_CAMPAIGN");
    press_button(&mut app, "CM_B_CANCEL");
    assert_eq!(screen(&app), Screen::MainMenu);
}

#[test]
fn accept_fe_menu_3_h_typing_off_the_page_never_reaches_it() {
    let dir = TempDir::new("off-page");
    let mut app = page_app(dir.path());
    press_button(&mut app, "CM_B_CANCEL");
    assert_eq!(screen(&app), Screen::MainMenu);
    type_text(&mut app, "stray");
    press_button(&mut app, "MM_B_CAMPAIGN");
    assert_eq!(
        state(&app).pilot_page().unwrap().text(),
        "",
        "keys typed on the menu are not replayed on the page"
    );
}

// --------------------------------------------------------- the retail half ---

/// **The retail half.** The installation's own `Campaign` rows: the edit
/// box and list as measured, and the panes and buttons the shell draws
/// around them.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_fe_menu_3_retail_the_campaign_rows_are_as_written() {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    let screens = FrontEndScreens::open(&dir).expect("the original screen container opens");
    let layout = screens.original_layout().expect("the layout member reads");
    let page = PilotPage::from_layout(&layout).expect("the retail rows read");

    let edit = page.edit();
    assert_eq!(edit.font, "FONT_CM_EDIT");
    assert_eq!(
        (
            edit.rect.x,
            edit.rect.y,
            edit.rect.width,
            edit.rect.height,
            edit.max_chars
        ),
        (243, 297, 221, 22, 16)
    );
    assert_eq!(edit.text_color, 0xFFFF_0000, "red text");
    assert_eq!(edit.frame_color, None);
    assert_eq!(edit.cursor_color, 0xFFEF_0010);

    let list = page.list();
    assert_eq!(list.slider, "CM_B_ScrollBar.png");
    assert_eq!(
        (list.up.as_str(), list.down.as_str()),
        ("CM_B_ScrollUp.png", "CM_B_ScrollDown.png")
    );
    assert_eq!(
        (
            list.rect.x,
            list.rect.y,
            list.rect.width,
            list.row_height,
            list.visible
        ),
        (245, 356, 305, 20, 7)
    );
    for art in [&list.slider, &list.up, &list.down] {
        assert!(screens.art_extent(art).is_some(), "{art} decodes");
    }

    // The shell draws the pane and the three buttons around the two rows.
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    shell.open(PILOT_SECTION).expect("Campaign is a section");
    let section = shell.section().expect("a section");
    let buttons: Vec<&str> = section.buttons.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(buttons, ["CM_B_START", "CM_B_DELETEPLAYER", "CM_B_CANCEL"]);
    assert_eq!(section.panes.len(), 1);
    assert_eq!(section.panes[0].id, "CM_BACKGROUND");
    assert!(
        section.skipped.is_empty(),
        "no Campaign row is skipped: {:?}",
        section.skipped
    );
}

// --------------------------------------------------------- the windowed run ---

fn front_end_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("CS_FRONT_END_BIN") {
        return PathBuf::from(bin);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_cs"))
}

/// **The windowed run.** `cs --cs-path $CS_GAME_DIR --front-end
/// --smoke-seconds 14 --capture-dir <dir>` over a population seeded with
/// three pilots: the Campaign visit types a name, selects the first pilot
/// with Down and photographs both. The test asserts the harness contract —
/// the two page framebuffers arrive and the run passes — never the verdict
/// a human would give the art.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu and a built windowed binary"]
fn accept_fe_menu_3_the_windowed_smoke_draws_the_typed_name_and_the_pilot_list() {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let Ok(capabilities) = std::env::var("CS_CAPABILITIES") else {
        panic!("CS_CAPABILITIES is unset: the front-end launch needs gpu");
    };
    assert!(
        capabilities.split(',').any(|part| part.trim() == "gpu"),
        "CS_CAPABILITIES must list gpu: {capabilities}"
    );
    let dir = std::env::var("CS_FE_MENU_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("fe-menu-3-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    {
        // The smoke's own population: the synthetic one under the capture
        // directory, seeded with three pilots.
        let mut seed = session(&dir.join(cs_app::ui::front_end::SMOKE_PROFILE_SUBDIR));
        for name in ["Ace", "Bea", "Cy"] {
            seed.create(name).expect("a pilot is created");
        }
    }
    let output = std::process::Command::new(front_end_bin())
        .args(["--cs-path", &game_dir, "--front-end"])
        .args(["--smoke-seconds", "14", "--capture-dir"])
        .arg(&dir)
        .env("CS_CAPABILITIES", capabilities)
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.code().is_some(),
        "the run must exit by itself: {output:?}\n{stderr}"
    );
    let report = std::fs::read_to_string(dir.join("report.json"))
        .unwrap_or_else(|error| panic!("report.json is written: {error}\n{stderr}"));
    assert!(
        report.contains("\"passed\":true"),
        "the run passed its own checks: {report}\nstderr: {stderr}"
    );
    for label in ["campaign", "campaign-typed", "campaign-list"] {
        assert!(
            report.contains(&format!("{label}.png")) && dir.join(format!("{label}.png")).is_file(),
            "{label}.png was captured: {report}"
        );
    }
    assert!(output.status.success(), "exit 0; stderr: {stderr}");
    println!("pilot-page smoke artifacts in {}", dir.display());
}
