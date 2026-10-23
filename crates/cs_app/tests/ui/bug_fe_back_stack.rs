//! Task #1997 `BUG-FE-BACK-STACK`: Cancel/Back must unwind the shell's screen
//! stack, not push the screen it came from again. Task test prefix:
//! `accept_bug_fe_back_stack_`.
//!
//! The synthetic half models the owner's sequence on an invented layout in the
//! original's format (`Menu` -> `Prefs` -> `Audio`, `Audio`'s Cancel naming
//! `Prefs` through `ScriptToExe`, `Prefs`' back button the `activate` +
//! `script_end` hand-over). The `#[ignore]`d retail half runs the same
//! sequence on the installation's `LAYOUT.CSV` and screen scripts.

use std::path::PathBuf;

use cs_app::ui::front_end::{
    ButtonEffect, FrontEndScreens, MenuShell, OriginalLayout, START_SECTION, ShellOutcome,
    script_member,
};

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "strip.png" => Some((80, 40)),
        _ => None,
    }
}

const LAYOUT: &str = "\
[@Menu@]
B_PREFS =B,strip.png,100,100,0,0,Prefs,0x1000,1

[@Prefs@]
B_AUDIO =B,strip.png,100,100,0,0,Audio,0x1000,1
B_MENU  =B,strip.png,100,200,0,0,,0x1000,1

[@Audio@]
B_CANCEL =B,strip.png,100,100,0,0,Prefs,0x1000,1
B_ACCEPT =B,strip.png,100,200,0,0,Prefs,0x1000,1
";

const PREFS_SCRIPT: &str = "\
main
{
gui_create
{
object LB = @ctl@BE
LB.YC = \"b_menu\"
}
gui_mailbox
{
switch (getmessage())
{
case LB
activate(@menu@)
script_end \"prefs.script\"
}
}
}
";

fn scripts(section: &str) -> Option<String> {
    section
        .eq_ignore_ascii_case("prefs")
        .then(|| PREFS_SCRIPT.to_owned())
}

fn shell() -> MenuShell {
    let layout = OriginalLayout::parse(LAYOUT).expect("the fixture parses");
    let mut shell = MenuShell::build(&layout, &extents, &scripts);
    shell.open("Menu").expect("Menu is a section");
    shell
}

/// Presses the button `id` on the current section with the keyboard.
fn press(shell: &mut MenuShell, id: &str) -> ShellOutcome {
    let index = shell
        .section()
        .and_then(|section| section.buttons.iter().position(|button| button.id == id))
        .unwrap_or_else(|| panic!("no button {id}"));
    shell.move_focus(true);
    for _ in 0..8 {
        if shell.focus() == Some(index) {
            break;
        }
        shell.move_focus(true);
    }
    assert_eq!(shell.focus(), Some(index));
    shell.activate_down();
    shell.activate_up()
}

#[test]
fn accept_bug_fe_back_stack_the_handover_is_a_return_not_a_navigation() {
    let shell = shell();
    let mut shell = shell;
    press(&mut shell, "B_PREFS");
    let back = shell
        .section()
        .and_then(|section| section.buttons.iter().find(|b| b.id == "B_MENU"))
        .expect("the back button is drawn");
    assert_eq!(back.effect, ButtonEffect::Return("menu".to_owned()));
}

#[test]
fn accept_bug_fe_back_stack_cancel_unwinds_and_leaves_no_history() {
    let mut shell = shell();
    for cycle in 0..3 {
        assert!(matches!(
            press(&mut shell, "B_PREFS"),
            ShellOutcome::Entered { .. }
        ));
        assert_eq!(shell.depth(), 2, "cycle {cycle}");
        assert!(matches!(
            press(&mut shell, "B_AUDIO"),
            ShellOutcome::Entered { .. }
        ));
        assert_eq!(shell.depth(), 3);
        // Cancel Changes: back to the Prefs that is already open.
        match press(&mut shell, if cycle == 1 { "B_ACCEPT" } else { "B_CANCEL" }) {
            ShellOutcome::Returned { section, .. } => assert_eq!(section, "Prefs"),
            other => panic!("expected a return, got {other:?}"),
        }
        assert_eq!(shell.section_name(), Some("Prefs"));
        assert_eq!(shell.depth(), 2, "no Audio and no second Prefs underneath");
        // Return to the main menu.
        match press(&mut shell, "B_MENU") {
            ShellOutcome::Returned { section, .. } => assert_eq!(section, "Menu"),
            other => panic!("expected a return, got {other:?}"),
        }
        assert_eq!(shell.section_name(), Some("Menu"));
        assert_eq!(shell.depth(), 1);
        // Esc at the root reveals nothing underneath.
        assert_eq!(shell.escape(), ShellOutcome::Quit);
    }
}

#[test]
fn accept_bug_fe_back_stack_a_return_to_a_closed_section_replaces_this_one() {
    let layout = OriginalLayout::parse(LAYOUT).expect("the fixture parses");
    let mut shell = MenuShell::build(&layout, &extents, &scripts);
    // Prefs opened as the root (no Menu beneath): its hand-over replaces it.
    shell.open("Prefs").expect("Prefs is a section");
    match press(&mut shell, "B_MENU") {
        ShellOutcome::Returned { section, previous } => {
            assert_eq!(section, "Menu");
            assert_eq!(previous, vec!["Prefs".to_owned()]);
            shell.reopen(&previous);
            assert_eq!(shell.section_name(), Some("Prefs"), "a refusal undoes it");
        }
        other => panic!("expected a return, got {other:?}"),
    }
    press(&mut shell, "B_MENU");
    assert_eq!(shell.section_name(), Some("Menu"));
    assert_eq!(shell.depth(), 1);
}

#[test]
fn accept_bug_fe_back_stack_a_forward_open_still_stacks() {
    let mut shell = shell();
    press(&mut shell, "B_PREFS");
    press(&mut shell, "B_AUDIO");
    assert_eq!(shell.depth(), 3);
    assert_eq!(
        shell.escape(),
        ShellOutcome::Back {
            section: "Prefs".to_owned()
        }
    );
}

fn retail_screens() -> FrontEndScreens {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    FrontEndScreens::open(&dir).expect("the original screen container opens")
}

/// The owner's sequence on the installation's own layout, for every settings
/// page: MainMenu -> Preferences -> page -> Cancel/Accept -> Preferences ->
/// Return to Main Menu leaves MainMenu alone on the stack.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_bug_fe_back_stack_retail_settings_pages_return_to_their_parent() {
    let screens = retail_screens();
    let layout = screens.original_layout().expect("the layout member reads");
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    for (page, cancel, accept) in [
        ("AUDIO", "AP_B_CANCELCHANGES", "AP_B_ACCEPTCHANGES"),
        ("GAMEOPTIONS", "GO_B_CANCELCHANGES", "GO_B_ACCEPTCHANGES"),
        ("CONTROLSPREFS", "CP_B_CANCELCHANGES", "CP_B_ACCEPTCHANGES"),
    ] {
        for button in [cancel, accept] {
            shell.open(START_SECTION).expect("MainMenu is a section");
            press(&mut shell, "MM_B_PREFERENCES");
            assert_eq!(shell.depth(), 2);
            let prefs = shell.section().expect("preferences presents");
            let opener = prefs
                .buttons
                .iter()
                .find(|b| matches!(&b.effect, ButtonEffect::Navigate(n) if n.eq_ignore_ascii_case(page) || n.eq_ignore_ascii_case("ControlsPrefs") && page == "CONTROLSPREFS"))
                .unwrap_or_else(|| panic!("Preferences has a button for {page}"))
                .id
                .clone();
            press(&mut shell, &opener);
            assert_eq!(shell.depth(), 3, "{page} opens on top of Preferences");
            let outcome = press(&mut shell, button);
            assert!(
                matches!(&outcome, ShellOutcome::Returned { section, .. } if section.eq_ignore_ascii_case("Preferences")),
                "{button}: {outcome:?}"
            );
            assert_eq!(shell.depth(), 2, "{button} leaves Preferences open once");
        }
    }
    // Campaign's Cancel hands back to the open main menu.
    shell.open(START_SECTION).expect("MainMenu is a section");
    press(&mut shell, "MM_B_CAMPAIGN");
    assert_eq!(shell.depth(), 2);
    let outcome = press(&mut shell, "CM_B_CANCEL");
    assert!(
        matches!(outcome, ShellOutcome::Returned { .. }),
        "{outcome:?}"
    );
    assert_eq!(shell.depth(), 1);
    assert_eq!(shell.escape(), ShellOutcome::Quit);
}
