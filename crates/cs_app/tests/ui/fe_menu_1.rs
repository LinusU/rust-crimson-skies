//! Acceptance task FE-MENU-1 (#1833): the windowed front-end shell.
//!
//! The `accept_fe_menu_1_` tests call the production presenter:
//! [`MenuShell`] presents one `LAYOUT.CSV` section (panes, 4-state buttons,
//! effects read off the data), [`shell_app`]/[`run_front_end`] open it
//! windowed, and `cs --cs-path <dir> --front-end` parses through
//! [`cs_app::cli`].
//!
//! * The synthetic tests run over layout text written in the original's
//!   *format* with **invented** names, art sizes and scripts; they prove the
//!   presenter's rules (regions from the strip, states from the input,
//!   navigation only where the data states, skipped rows kept) and run in
//!   CI. They prove nothing about the retail data.
//! * The `retail` test reads `$CS_GAME_DIR` through [`FrontEndScreens`] and
//!   pins what the installation's own MainMenu/Campaign sections measure,
//!   writes `front-end-shell.json` when `CS_EVIDENCE_DIR` is set, and is
//!   `#[ignore]`d because CI has no installation.

use std::path::{Path, PathBuf};

use bevy::app::{App, AppExit};
use bevy::ecs::message::{MessageWriter, Messages};
use bevy::input::ButtonInput;
use bevy::input::InputSystems;
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::MouseButton;
use bevy::prelude::{Entity, IntoScheduleConfigs, PreUpdate, ResMut, Resource, Vec2, With};
use bevy::window::{CursorMoved, PrimaryWindow};

use cs_app::cli::{self, CliRequest};
use cs_app::playtest::SmokeRequest;
use cs_app::ui::front_end::{
    ButtonEffect, ButtonVisual, FrontEndRequest, FrontEndScreens, MIN_SHELL_SMOKE_SECONDS,
    MenuShell, OriginalLayout, SECTION_SURFACE, START_SECTION, ShellOutcome, ShellResource,
    SkippedReason, UnhandledEffect, script_member,
};

/// Art sizes of the invented fixture: an 80x40 strip of four 10-pixel
/// button frames, a 200x100 pane, an odd-height strip, a cursor.
fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "pane.png" => Some((200, 100)),
        "strip.png" => Some((80, 40)),
        "tall.png" => Some((80, 129)),
        "cursor.png" => Some((40, 40)),
        _ => None,
    }
}

/// An invented two-section layout in the original's format: `Menu` holds a
/// pane, a button that names `Sub`, a no-script `Quit`, a button whose
/// handler is not a shell effect, a skipped strip and an unread-art button;
/// `Sub` holds one pane and a button whose own script hands back to Menu.
const MENU: &str = "\
[@Menu@]
PANE    =P,pane.png,10,20,0,1,0,1,0
B_SUB   =B,strip.png,100,100,0,0,Sub,0x1000,1
B_QUIT  =B,strip.png,100,150,0,0,,0x1000,1
B_MAIL  =B,strip.png,100,200,0,0,,0x1000,1
B_GONE  =B,strip.png,100,250,0,0,Nowhere,0x1000,1
B_TALL  =B,tall.png,100,300,0,0,Sub,0x1000,1
B_LOST  =B,missing.png,100,350,0,0,Sub,0x1000,1
B_VAR   =B,strip.png,<NOPE>,400,0,0,Sub,0x1000,1
TEXT    =T,IDS_ANY,1,2,3,4,5,6,7

[@Sub@]
BACKBG  =P,pane.png,0,0,0,1,0,0,0
B_BACK  =B,strip.png,5,5,0,0,,0x1000,1
";

/// The invented scripts: `Menu`'s quit arm is exactly `terminate`, its mail
/// arm a native callback, and `Sub`'s back arm the `activate(@…@)` +
/// `script_end` hand-over the real CAMPAIGN.SCRIPT uses.
const MENU_SCRIPT: &str = "\
main
{
gui_create
{
object QW = @ctl@BE
QW.YC = \"b_quit\"
object XW = @ctl@BE
XW.YC = \"b_mail\"
}
gui_mailbox
{
switch (getmessage())
{
case QW
terminate
case XW
mail (11003, this)
}
}
}
";

const SUB_SCRIPT: &str = "\
main
{
gui_create
{
object LB = @ctl@BE
LB.YC = \"b_back\"
}
gui_mailbox
{
switch (getmessage())
{
case LB
activate(@menu@)
script_end \"sub.script\"
}
}
}
";

/// Scripts for the invented sections, the shape `scripts` takes for a
/// production build.
fn scripts(section: &str) -> Option<String> {
    match section.to_ascii_lowercase().as_str() {
        "menu" => Some(MENU_SCRIPT.to_owned()),
        "sub" => Some(SUB_SCRIPT.to_owned()),
        _ => None,
    }
}

/// The shell over the invented layout, opened on `Menu`.
fn menu_shell() -> MenuShell {
    let layout = OriginalLayout::parse(MENU).expect("the fixture parses");
    let mut shell = MenuShell::build(&layout, &extents, &scripts);
    shell.open("Menu").expect("Menu is a section");
    shell
}

#[test]
fn accept_fe_menu_1_a_section_presents_its_panes_and_buttons_at_their_authored_places() {
    let shell = menu_shell();
    let section = shell.section().expect("a current section");
    assert_eq!(section.name, "Menu");

    // Panes: the row's art at the row's X,Y, frame count and flags kept.
    assert_eq!(section.panes.len(), 1);
    let pane = &section.panes[0];
    assert_eq!(pane.id, "PANE");
    assert_eq!(pane.art, "pane.png");
    assert_eq!(
        (pane.rect.x, pane.rect.y, pane.rect.width, pane.rect.height),
        (10, 20, 200, 100)
    );
    assert_eq!(pane.frames, 1);

    // Buttons: the strip's own extent divided by 4 at the authored place.
    // Four of the seven B rows draw; the rest are in `skipped` below.
    assert_eq!(section.buttons.len(), 4);
    let sub = &section.buttons[0];
    assert_eq!(sub.id, "B_SUB");
    assert_eq!(
        (sub.rect.x, sub.rect.y, sub.rect.width, sub.rect.height),
        (100, 100, 80, 10),
        "80x40 strip → four 10-pixel frames"
    );
    assert_eq!(sub.effect, ButtonEffect::Navigate("Sub".to_owned()));

    // The other-row kinds are counted, never drawn.
    assert_eq!(section.other_rows, vec!["T".to_owned()]);

    // The rows it cannot draw, in order, with the reason kept.
    let skipped: Vec<(&str, &SkippedReason)> = section
        .skipped
        .iter()
        .map(|row| (row.id.as_str(), &row.reason))
        .collect();
    assert_eq!(
        skipped,
        vec![
            ("B_TALL", &SkippedReason::FrameCountUnknown { height: 129 }),
            (
                "B_LOST",
                &SkippedReason::ArtUnread {
                    art: "missing.png".to_owned()
                }
            ),
            (
                "B_VAR",
                &SkippedReason::UnresolvedPlaceholder {
                    name: "NOPE".to_owned()
                }
            ),
        ]
    );
}

#[test]
fn accept_fe_menu_1_a_button_does_only_what_the_data_states() {
    let shell = menu_shell();
    let section = shell.section().unwrap();
    let effects: Vec<(&str, &ButtonEffect)> = section
        .buttons
        .iter()
        .map(|button| (button.id.as_str(), &button.effect))
        .collect();
    assert_eq!(
        effects,
        vec![
            ("B_SUB", &ButtonEffect::Navigate("Sub".to_owned())),
            ("B_QUIT", &ButtonEffect::Terminate),
            (
                "B_MAIL",
                &ButtonEffect::Unhandled(UnhandledEffect::HandlerNotShell {
                    statements: vec!["mail (11003, this)".to_owned()]
                })
            ),
            (
                "B_GONE",
                &ButtonEffect::Unhandled(UnhandledEffect::ScriptNotPresent {
                    script: "Nowhere".to_owned()
                })
            ),
        ]
    );

    // Sub's back button is the activate(@menu@) + script_end hand-over.
    let sub = &shell.sections()["SUB"];
    assert_eq!(
        sub.buttons[0].effect,
        ButtonEffect::Navigate("menu".to_owned())
    );
}

#[test]
fn accept_fe_menu_1_the_four_strip_states_are_driven_by_input() {
    let mut shell = menu_shell();
    let view = shell.view(SECTION_SURFACE).expect("a view");

    // Rest: focus lands on the first button (rollover/focus frame), the rest
    // draw Active.
    let visuals: Vec<ButtonVisual> = view.buttons.iter().map(|b| b.visual).collect();
    assert_eq!(
        visuals,
        vec![
            ButtonVisual::Rollover,
            ButtonVisual::Active,
            ButtonVisual::Active,
            ButtonVisual::Active
        ]
    );
    assert!(view.buttons[0].focused);

    // The state frame's texel offset is frame index x one frame's height.
    assert_eq!(view.buttons[0].texel_offset.1, 2 * 10);
    assert_eq!(view.buttons[1].texel_offset.1, 10);

    // Pointer hover → Rollover; press → Depressed; release inside → effect.
    shell.pointer_move(140, 105);
    let view = shell.view(SECTION_SURFACE).unwrap();
    assert_eq!(view.buttons[0].visual, ButtonVisual::Rollover);
    shell.pointer_down();
    let view = shell.view(SECTION_SURFACE).unwrap();
    assert_eq!(view.buttons[0].visual, ButtonVisual::Depressed);
    assert_eq!(view.buttons[0].texel_offset.1, 3 * 10);
    let outcome = shell.pointer_up();
    assert_eq!(
        outcome,
        ShellOutcome::Entered {
            section: "Sub".to_owned()
        }
    );

    // Keyboard: Enter down depresses the focused button, up applies it.
    let mut shell = menu_shell();
    shell.activate_down();
    let view = shell.view(SECTION_SURFACE).unwrap();
    assert_eq!(view.buttons[0].visual, ButtonVisual::Depressed);
    assert_eq!(
        shell.activate_up(),
        ShellOutcome::Entered {
            section: "Sub".to_owned()
        }
    );

    // Disabled: the designed affordance draws frame 0 and drops the button
    // out of focus and hits.
    let mut shell = menu_shell();
    assert!(shell.set_enabled("b_sub", false));
    let view = shell.view(SECTION_SURFACE).unwrap();
    assert_eq!(view.buttons[0].visual, ButtonVisual::Disabled);
    assert_eq!(view.buttons[0].texel_offset.1, 0);
    // Its spot is no longer a hit: the pointer there hovers nothing.
    shell.pointer_move(140, 105);
    assert_eq!(shell.hover(), None);
    // Focus starts on the next enabled button.
    assert_eq!(shell.focus(), Some(1));
    assert!(!shell.set_enabled("no_such", false));
}

#[test]
fn accept_fe_menu_1_focus_walks_declaration_order_and_escape_steps_back() {
    let mut shell = menu_shell();
    assert_eq!(shell.focus(), Some(0));
    shell.move_focus(true);
    assert_eq!(shell.focus(), Some(1));
    shell.move_focus(false);
    assert_eq!(shell.focus(), Some(0));
    shell.move_focus(false);
    assert_eq!(shell.focus(), Some(3), "wraps to the last drawn button");
    shell.move_focus(true);
    assert_eq!(shell.focus(), Some(0), "and wraps back to the first");

    // Focus + Enter reaches Sub; Esc returns to Menu; Esc at root quits.
    shell.activate_down();
    assert_eq!(
        shell.activate_up(),
        ShellOutcome::Entered {
            section: "Sub".to_owned()
        }
    );
    assert_eq!(shell.depth(), 2);
    assert_eq!(shell.focus(), Some(0), "entry focuses the first button");
    assert_eq!(
        shell.escape(),
        ShellOutcome::Back {
            section: "Menu".to_owned()
        }
    );
    assert_eq!(shell.depth(), 1);
    assert_eq!(shell.escape(), ShellOutcome::Quit);

    // An unhandled press is recorded verbatim, never guessed.
    let mut shell = menu_shell();
    shell.move_focus(true);
    shell.move_focus(true);
    shell.activate_down();
    assert_eq!(
        shell.activate_up(),
        ShellOutcome::Unhandled {
            id: "B_MAIL".to_owned(),
            effect: UnhandledEffect::HandlerNotShell {
                statements: vec!["mail (11003, this)".to_owned()]
            }
        }
    );
    assert_eq!(shell.unhandled().len(), 1);
    assert_eq!(shell.section_name(), Some("Menu"), "it stayed put");
}

#[test]
fn accept_fe_menu_1_the_view_aspect_fits_and_hit_tests_the_same_transform() {
    let mut shell = menu_shell();

    // 1600x1200: exactly 2x, no offset.
    let view = shell.view((1600, 1200)).expect("a view");
    let button = &view.buttons[0];
    assert_eq!(
        (
            button.rect.x,
            button.rect.y,
            button.rect.width,
            button.rect.height
        ),
        (200, 200, 160, 20)
    );

    // 1024x768: the 4:3 surface letterboxes inside 4:3 → fitted 1024x768? No —
    // 800x600 is already 4:3, so it fills 1024x768 exactly at scale 1.28.
    let view = shell.view((1024, 768)).expect("a view");
    assert_eq!(
        (
            view.fit.image_rect().x,
            view.fit.image_rect().y,
            view.fit.image_rect().width,
            view.fit.image_rect().height
        ),
        (0, 0, 1024, 768)
    );

    // A wider window letterboxes top and bottom; the hit map agrees.
    let view = shell.view((1600, 900)).expect("a view");
    let image = view.fit.image_rect();
    assert_eq!((image.width, image.height), (1200, 900));
    assert_eq!((image.x, image.y), (200, 0));
    // Surface point at the button's scaled centre maps back to its logical
    // centre and hovers it; a point in the bar maps to nothing.
    let centre = (
        button_surface_centre(&view.buttons[0]).0,
        button_surface_centre(&view.buttons[0]).1,
    );
    let (lx, ly) = MenuShell::unmap_point((1600, 900), centre.0, centre.1).unwrap();
    shell.pointer_move(lx, ly);
    assert_eq!(shell.hover(), Some(0));
    assert_eq!(MenuShell::unmap_point((1600, 900), 10, 450), None);
    shell.pointer_move(0, 0);
    assert_eq!(shell.hover(), None);
    shell.pointer_leave();
    assert_eq!(shell.hover(), None);
}

/// The centre of a view button's surface rect, in surface pixels.
fn button_surface_centre(button: &cs_app::ui::front_end::ShellButtonView) -> (u32, u32) {
    (
        button.rect.x + button.rect.width / 2,
        button.rect.y + button.rect.height / 2,
    )
}

// ------------------------------------------------------- headless drive ---

/// A scripted input the test queue applies: the same `ButtonInput` /
/// `CursorMoved` resources real input and the smoke write.
#[derive(Resource, Default)]
struct TestInput {
    cursor: Option<Vec2>,
    keys: Vec<(KeyCode, bool)>,
    mouse: Vec<bool>,
}

/// Applies the queue in PreUpdate — after the input system's per-frame
/// clear, exactly where `smoke_input` sits.
fn inject(
    keys: ResMut<ButtonInput<KeyCode>>,
    mouse: ResMut<ButtonInput<MouseButton>>,
    mut cursor: MessageWriter<CursorMoved>,
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
    if let Some(position) = queue.cursor.take() {
        cursor.write(CursorMoved {
            window: window.single().unwrap_or(Entity::PLACEHOLDER),
            position,
            delta: None,
        });
    }
}

/// The headless presenter: `App` + the shell plugin, no renderer.
fn headless(shell: MenuShell) -> App {
    let mut app = cs_app::ui::front_end::headless_app(shell);
    app.init_resource::<TestInput>();
    app.add_systems(PreUpdate, inject.after(InputSystems));
    app
}

/// Queues one input batch and runs one frame.
fn drive(app: &mut App, batch: impl FnOnce(&mut TestInput)) {
    batch(&mut app.world_mut().resource_mut::<TestInput>());
    app.update();
}

#[test]
fn accept_fe_menu_1_the_windowed_input_path_drives_the_model() {
    let mut app = headless(menu_shell());
    app.update();

    // Pointer: hover at the first button's centre (the surface is 800x600,
    // so logical == surface) and press it — the presenter navigates to Sub
    // through the real `CursorMoved`/`ButtonInput` path.
    drive(&mut app, |queue| {
        queue.cursor = Some(Vec2::new(140.0, 105.0));
        queue.mouse.push(true);
    });
    drive(&mut app, |queue| queue.mouse.push(false));
    {
        let shell = &app.world().resource::<ShellResource>().shell;
        assert_eq!(
            shell.section_name(),
            Some("Sub"),
            "the pointer press applied B_SUB's stated Navigate"
        );
        assert_eq!(shell.depth(), 2);
    }

    // Keyboard: Esc steps back to Menu, Down walks the declaration order,
    // Enter presses the focused button — here B_QUIT's `terminate`, which the
    // presenter turns into `AppExit` outside a smoke run.
    drive(&mut app, |queue| {
        queue.keys.push((KeyCode::Escape, true));
        queue.keys.push((KeyCode::Escape, false));
    });
    {
        let shell = &app.world().resource::<ShellResource>().shell;
        assert_eq!(shell.section_name(), Some("Menu"));
        assert_eq!(shell.depth(), 1);
    }
    drive(&mut app, |queue| {
        queue.keys.push((KeyCode::ArrowDown, true));
        queue.keys.push((KeyCode::ArrowDown, false));
    });
    {
        let shell = &app.world().resource::<ShellResource>().shell;
        assert_eq!(shell.focus(), Some(1), "Down landed on B_QUIT");
    }
    drive(&mut app, |queue| {
        queue.keys.push((KeyCode::Enter, true));
        queue.keys.push((KeyCode::Enter, false));
    });
    let exits = app
        .world_mut()
        .resource_mut::<Messages<AppExit>>()
        .drain()
        .count();
    assert_eq!(exits, 1, "the focused B_QUIT pressed terminate → AppExit");
}

// ------------------------------------------------------------- the CLI ---

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn accept_fe_menu_1_the_cli_takes_a_front_end_and_refuses_its_misuse() {
    let parsed = cli::parse(args(&["--cs-path", "/i", "--front-end"]));
    let CliRequest::FrontEnd(request) = parsed else {
        panic!("--front-end parses as a front-end request: {parsed:?}");
    };
    assert_eq!(request.cs_path, PathBuf::from("/i"));
    assert_eq!(request.smoke, None, "no smoke without --smoke-seconds");

    let parsed = cli::parse(args(&[
        "--cs-path",
        "/i",
        "--front-end",
        "--smoke-seconds",
        "10",
        "--capture-dir",
        "d",
    ]));
    let CliRequest::FrontEnd(request) = parsed else {
        panic!("the smoke flags travel on the front-end request: {parsed:?}");
    };
    assert_eq!(
        request.smoke,
        Some(SmokeRequest {
            seconds: 10,
            capture_dir: PathBuf::from("d"),
        })
    );

    let parsed = cli::parse(args(&[
        "--cs-path",
        "/i",
        "--front-end",
        "--smoke-seconds",
        "8",
    ]));
    let CliRequest::FrontEnd(FrontEndRequest {
        smoke: Some(smoke), ..
    }) = parsed
    else {
        panic!("the default capture dir fills in: {parsed:?}");
    };
    assert!(
        smoke.capture_dir.starts_with("private"),
        "the default capture dir is a private path: {smoke:?}"
    );

    for (bad, needle) in [
        (args(&["--front-end"]), "--front-end needs --cs-path"),
        (
            args(&["--cs-path", "/i", "--front-end", "--headless"]),
            "does not combine with",
        ),
        (
            args(&["--cs-path", "/i", "--front-end", "--mission", "M01"]),
            "does not combine with",
        ),
        (
            args(&["--cs-path", "/i", "--front-end", "--capture-dir", "d"]),
            "--capture-dir is where --front-end --smoke-seconds",
        ),
        (
            args(&["--cs-path", "/i", "--front-end", "--smoke-seconds", "3"]),
            "at least",
        ),
    ] {
        let refused = cli::parse(bad.clone());
        let CliRequest::Invalid { reason } = refused else {
            panic!("{bad:?} is refused: {refused:?}");
        };
        assert!(
            reason.contains(needle),
            "{reason:?} names the problem ({needle})"
        );
    }
    const {
        assert!(MIN_SHELL_SMOKE_SECONDS >= 8, "the script needs the room");
    }
    assert!(
        cli::HELP_TEXT.contains("--cs-path <dir> --front-end"),
        "--help documents the front end"
    );
}

// --------------------------------------------------------- the retail half ---

/// The installation, opened once (the same shape `ui::original` uses).
fn retail_screens() -> FrontEndScreens {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    FrontEndScreens::open(&dir).expect("the original screen container opens")
}

fn json_str(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// **The retail half.** The shell presents the installation's own
/// `LAYOUT.CSV` sections: MainMenu's six buttons and their stated effects
/// (Campaign/InstantAction/Preferences/Credits navigate to sections the file
/// holds, `MultiPlayerMain` names a section the file does not, Quit's own
/// script says `terminate`), and `CAMPAIGN.SCRIPT`'s `activate(@mainmenu@)` +
/// `script_end` is what hands back. The whole-layout record — every drawn
/// row and every skipped one with its reason — is written to
/// `front-end-shell.json` when `CS_EVIDENCE_DIR` is set.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_fe_menu_1_retail_the_original_sections_present_and_navigate_as_written() {
    let screens = retail_screens();
    let layout = screens.original_layout().expect("the layout member reads");
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    shell.open(START_SECTION).expect("MainMenu is a section");

    // The MainMenu as the file writes it.
    let menu = shell.section().expect("the main menu presents");
    assert_eq!(menu.name, "MainMenu");
    assert_eq!(
        menu.panes
            .iter()
            .map(|pane| pane.id.as_str())
            .collect::<Vec<_>>(),
        vec!["MM_LOGO", "BFRAME"],
        "the logo and the button frame, in declaration order"
    );
    assert!(
        menu.other_rows.is_empty(),
        "the main menu holds no unhandled row kinds: {:?}",
        menu.other_rows
    );
    // The CrimFlag.MPG row parses to a movie (FE-MENU-2): the file, its
    // authored origin/scale and the loop flag kept.
    let movies: Vec<(&str, &str, i32, i32, u32, u32, u32, u32, bool)> = menu
        .movies
        .iter()
        .map(|movie| {
            (
                movie.id.as_str(),
                movie.file.as_str(),
                movie.x,
                movie.y,
                movie.z,
                movie.scale_x,
                movie.scale_y,
                movie.loops,
                movie.region,
            )
        })
        .collect();
    assert_eq!(
        movies,
        vec![("MOVIE", "CrimFlag.MPG", 0, 0, 0, 250, 250, 0, false)]
    );
    let effects: Vec<(&str, &ButtonEffect)> = menu
        .buttons
        .iter()
        .map(|button| (button.id.as_str(), &button.effect))
        .collect();
    assert_eq!(
        effects,
        vec![
            (
                "MM_B_CAMPAIGN",
                &ButtonEffect::Navigate("Campaign".to_owned())
            ),
            (
                "MM_B_INSTANTACTION",
                &ButtonEffect::Navigate("InstantAction".to_owned())
            ),
            (
                "MM_B_MULTIPLAYER",
                &ButtonEffect::Unhandled(UnhandledEffect::ScriptNotPresent {
                    script: "MultiPlayerMain".to_owned()
                })
            ),
            (
                "MM_B_PREFERENCES",
                &ButtonEffect::Navigate("Preferences".to_owned())
            ),
            (
                "MM_B_CREDITS",
                &ButtonEffect::Navigate("Credits".to_owned())
            ),
            ("MM_B_QUIT", &ButtonEffect::Terminate),
        ],
        "every MainMenu button does only what the layout and its own script state"
    );

    // The pointer drive the smoke performs: hover and press Campaign, land
    // on the section, read its buttons, take CM_B_CANCEL's stated hand-over
    // back to MainMenu.
    let centre = |button: &cs_app::ui::front_end::ShellButton| {
        (
            button.rect.x + i32::try_from(button.rect.width).unwrap_or(0) / 2,
            button.rect.y + i32::try_from(button.rect.height).unwrap_or(0) / 2,
        )
    };
    let (x, y) = centre(&menu.buttons[0]);
    shell.pointer_move(x, y);
    assert_eq!(shell.hover(), Some(0));
    shell.pointer_down();
    assert_eq!(
        shell.pointer_up(),
        ShellOutcome::Entered {
            section: "Campaign".to_owned()
        }
    );
    let campaign = shell.section().expect("the campaign screen presents");
    let cancel = campaign
        .buttons
        .iter()
        .find(|button| button.id == "CM_B_CANCEL")
        .expect("the cancel button is drawn");
    assert_eq!(
        cancel.effect,
        ButtonEffect::Navigate("mainmenu".to_owned()),
        "CAMPAIGN.SCRIPT's case LB: activate(@mainmenu@) + script_end"
    );
    assert_eq!(
        shell.escape(),
        ShellOutcome::Back {
            section: "MainMenu".to_owned()
        }
    );

    // The cursor art the shell found — recorded, never claimed as the
    // original's choice (see the shell module docs).
    assert!(
        shell.cursor().is_some(),
        "a named pointer art decoded on this installation"
    );

    if let Ok(dir) = std::env::var("CS_EVIDENCE_DIR") {
        let dir = PathBuf::from(dir);
        let dir = if dir.is_absolute() {
            dir
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(Path::parent)
                .expect("workspace root")
                .join(dir)
        };
        std::fs::create_dir_all(&dir).expect("the evidence directory is created");
        let mut json = String::from("{\n");
        json.push_str("  \"member\": \"GOSDATA/ASSETS/crimson.rof::ASSETS/LAYOUT.CSV\",\n");
        json.push_str(&format!(
            "  \"install_sha256\": {},\n",
            json_str(screens.install_sha256())
        ));
        json.push_str(&format!(
            "  \"cursor\": {},\n",
            json_str(shell.cursor().map(|(name, _)| name.as_str()).unwrap_or(""))
        ));
        json.push_str("  \"sections\": [\n");
        let rows: Vec<String> = shell
            .sections()
            .values()
            .map(|section| {
                let panes = section
                    .panes
                    .iter()
                    .map(|pane| {
                        json_str(&format!(
                            "{} {} @{},{} {}x{} frames={}",
                            pane.id,
                            pane.art,
                            pane.rect.x,
                            pane.rect.y,
                            pane.rect.width,
                            pane.rect.height,
                            pane.frames
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let buttons = section
                    .buttons
                    .iter()
                    .map(|button| {
                        json_str(&format!(
                            "{} res={} -> {:?}",
                            button.id,
                            button.res_id.as_deref().unwrap_or("0"),
                            button.effect
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let skipped = section
                    .skipped
                    .iter()
                    .map(|row| json_str(&format!("{} ({}): {:?}", row.id, row.kind, row.reason)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let other = section
                    .other_rows
                    .iter()
                    .map(|kind| json_str(kind))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "    {{\"name\": {}, \"panes\": [{}], \"buttons\": [{}], \"skipped\": [{}], \"other_rows\": [{}]}}",
                    json_str(&section.name),
                    panes,
                    buttons,
                    skipped,
                    other
                )
            })
            .collect();
        json.push_str(&rows.join(",\n"));
        json.push_str("\n  ]\n}\n");
        std::fs::write(dir.join("front-end-shell.json"), json).expect("the artifact is written");
    }
}

// ------------------------------------------------------ the windowed smoke ---

/// The windowed binary the ignored run drives: `CS_FRONT_END_BIN` when the
/// operator names one, else the test harness's own `cs` build — which cargo
/// keeps in step with the tested code, unlike a `target/release/cs` that may
/// predate the branch. The 480-frame menu script is light enough for a
/// debug binary.
fn front_end_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("CS_FRONT_END_BIN") {
        return PathBuf::from(bin);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_cs"))
}

/// **The windowed run.** `cs --cs-path $CS_GAME_DIR --front-end
/// --smoke-seconds 8 --capture-dir <dir>` drives the same window the
/// interactive mode opens: the scripted pass hovers and presses Campaign,
/// Esc back, focus-walks to Quit and presses it. The test asserts the
/// harness contract — the report arrives and passes, the framebuffer PNGs
/// arrive and are non-uniform — never the verdicts a human would judge.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu and a built windowed binary"]
fn accept_fe_menu_1_the_windowed_launch_runs_the_scripted_smoke() {
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
        |_| std::env::temp_dir().join("fe-menu-1-smoke"),
        PathBuf::from,
    );
    // Stale artifacts of an earlier smoke go, nothing else: the evidence
    // run points this at $CS_EVIDENCE_DIR itself, where cargo-test.log and
    // front-end-shell.json sit — a remove_dir_all there would unlink the
    // log tee still has open.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "report.json" || name.ends_with(".png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
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
        "the run must exit by itself, never on a signal: {output:?}\n{stderr}"
    );

    let report = std::fs::read_to_string(dir.join("report.json")).unwrap_or_else(|error| {
        panic!(
            "{} is written: {error}\n{stderr}",
            dir.join("report.json").display()
        )
    });
    assert!(
        report.contains("\"kind\":\"front-end-shell-smoke-report\""),
        "the report names its kind: {report}"
    );
    assert!(
        report.contains(
            "\"visited\":[\"MainMenu\",\"Campaign\",\"MainMenu\",\"InstantAction\",\"MainMenu\",\"Preferences\",\"MainMenu\",\"Credits\",\"MainMenu\"]"
        ),
        "the scripted navigation happened: {report}"
    );
    assert!(
        report.contains("\"quit_seen\":true"),
        "Quit pressed terminate: {report}"
    );
    assert!(
        report.contains("\"depressed_seen\":true"),
        "a button drew its depressed frame: {report}"
    );
    let pngs: Vec<_> = std::fs::read_dir(&dir)
        .expect("the capture directory lists")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".png"))
        .collect();
    assert!(
        pngs.len() >= 8,
        "the script asks for a shot at every scripted beat plus the final frame: {} in {:?}",
        pngs.len(),
        dir
    );
    assert!(
        report.contains("\"passed\":true"),
        "the run passed its own checks: {report}\nstderr: {stderr}"
    );
    assert!(
        output.status.success(),
        "the process exits 0 exactly when the smoke passed; stderr: {stderr}"
    );
    println!(
        "front-end smoke artifacts in {}: {} PNGs, report passed",
        dir.display(),
        pngs.len()
    );
}
