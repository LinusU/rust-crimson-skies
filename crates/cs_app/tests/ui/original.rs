//! Acceptance task F45-B.1 (#742): the original front-end layout loader.
//!
//! The `accept_f45_b_1_` tests call the production reader of the original's
//! `ASSETS/LAYOUT.CSV` ([`OriginalLayout`], [`OriginalScreens`]).
//!
//! * The synthetic tests run it over layout text written in the original's
//!   *format* with **invented** names and art sizes; they prove the reader's
//!   rules (substitution, binding through the script a button starts, every
//!   refusal) and run in CI. They prove nothing about the retail data.
//! * The `retail` test reads `$CS_GAME_DIR` through [`FrontEndScreens`], pins
//!   what the installation measured and writes the machine-readable artifact
//!   `original-layout.json` when `CS_EVIDENCE_DIR` is set. It is `#[ignore]`d
//!   because CI has no installation.

use std::path::{Path, PathBuf};

use cs_app::ui::front_end::{
    Action, DeckError, FrontEndScreens, LayoutTextError, OriginalLayout, OriginalScreens, Screen,
    ScreenRefusal, UnboundReason,
};
use cs_content::ui_layout::Rect;
use cs_types::content::ContentKind;

/// Art sizes of the invented fixture: a screen-sized background, a button strip
/// of four 20-pixel frames, an odd-height strip and an eight-state strip.
fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "bg.png" | "bg2.png" => Some((800, 600)),
        "strip.png" => Some((200, 80)),
        "tall.png" => Some((100, 129)),
        "checks8states.png" => Some((50, 160)),
        "wide.png" => Some((900, 40)),
        _ => None,
    }
}

const CABIN: &str = "\
; invented layout in the original's format
[GLOBALVARS]
G1=SHARED_Y,500
G2=LOCAL_X,1

[@PassengerCabin@]
V1=LOCAL_X,40
BG    =P,bg.png,0,0,0,1,0,0,0
SIDE  =P,strip.png,10,10,0,1,0,0,0

B_BACK  =B,strip.png,<LOCAL_X>,<SHARED_Y>,0,0,MainMenu,0x1000,1,,,
B_BOOK  =B,strip.png,300,100,0,0,ScrapBook_TOC,0x1000,1
B_NONE  =B,strip.png,300,200,0,0,,0x1000,1
B_SELF  =B,strip.png,300,300,0,0,PassengerCabin,0x1000,1
B_ELSE  =B,strip.png,300,400,0,0,Unmodelled,0x1000,1
B_TALL  =B,tall.png,500,100,0,0,MainMenu,0x1000,1
B_8     =B,checks8states.png,500,200,0,0,MainMenu,0x1000,1
B_LOST  =B,missing.png,500,300,0,0,MainMenu,0x1000,1
B_VAR   =B,strip.png,<NOPE>,100,0,0,MainMenu,0x1000,1
B_OUT   =B,strip.png,700,590,0,0,MainMenu,0x1000,1
TXT     =T,IDS_SOMETHING,1,2,3,4,5,6,7
";

#[test]
fn accept_f45_b_1_the_parser_reads_sections_variables_and_keeps_unresolved_placeholders() {
    let layout = OriginalLayout::parse(CABIN).expect("the fixture parses");
    assert_eq!(layout.sections().len(), 1);
    let section = layout.section("passengercabin").expect("case-insensitive");
    assert_eq!(section.panes.len(), 2);
    assert_eq!(section.buttons.len(), 10);
    assert_eq!(section.other_rows, 1, "the text row is counted, not read");

    let back = &section.buttons[0];
    assert_eq!(back.id, "B_BACK");
    // The section's variable wins over the global of the same name; the global
    // fills the name the section does not define.
    assert_eq!(back.fields[1], "40");
    assert_eq!(back.fields[2], "500");
    assert!(back.unresolved.is_empty());

    let var = section.buttons.iter().find(|b| b.id == "B_VAR").unwrap();
    assert_eq!(var.unresolved, vec!["NOPE".to_owned()]);
    assert_eq!(
        var.fields[1], "<NOPE>",
        "an undefined placeholder is kept, never replaced by a number"
    );

    assert_eq!(
        OriginalLayout::parse("A=B,x,1,2\n").unwrap_err(),
        LayoutTextError::RowOutsideSection { line: 1 }
    );
    assert_eq!(
        OriginalLayout::parse("[@A@]\nnot a row\n").unwrap_err(),
        LayoutTextError::Malformed { line: 2 }
    );
    assert_eq!(
        OriginalLayout::parse("[@A@]\n[@a@]\n").unwrap_err(),
        LayoutTextError::DuplicateSection {
            name: "a".to_owned()
        }
    );
}

#[test]
fn accept_f45_b_1_a_button_is_bound_only_through_the_script_it_starts() {
    let layout = OriginalLayout::parse(CABIN).unwrap();
    let report = layout.screen(Screen::Cabin, &extents);

    // Exactly the two buttons whose script is a paired section and whose
    // transition the table states once. Rectangle = art width x one of four
    // frames, at the authored position, in declaration order.
    let bound: Vec<_> = report
        .bound
        .iter()
        .map(|b| (b.id.key().to_owned(), b.action, b.rect))
        .collect();
    assert_eq!(
        bound,
        vec![
            (
                "b_back".to_owned(),
                Action::Back,
                Rect {
                    x: 40,
                    y: 500,
                    width: 200,
                    height: 20
                }
            ),
            (
                "b_book".to_owned(),
                Action::OpenScrapbook,
                Rect {
                    x: 300,
                    y: 100,
                    width: 200,
                    height: 20
                }
            ),
        ]
    );
    assert!(
        report
            .bound
            .iter()
            .all(|b| b.id.kind() == ContentKind::UiResource)
    );

    let unbound: Vec<_> = report
        .unbound
        .iter()
        .map(|u| (u.id.as_str(), u.reason.clone()))
        .collect();
    assert_eq!(
        unbound,
        vec![
            ("B_NONE", UnboundReason::NoScript),
            ("B_SELF", UnboundReason::SelfScript),
            (
                "B_ELSE",
                UnboundReason::ScriptNotPaired {
                    script: "Unmodelled".to_owned()
                }
            ),
            ("B_TALL", UnboundReason::FrameCountUnknown { height: 129 }),
            ("B_8", UnboundReason::FrameCountUnknown { height: 160 }),
            ("B_LOST", UnboundReason::ArtUnread),
            (
                "B_VAR",
                UnboundReason::UnresolvedPlaceholder {
                    name: "NOPE".to_owned()
                }
            ),
            ("B_OUT", UnboundReason::OutsideArt),
        ]
    );

    // The art is the one single-frame pane at the origin, at its own size.
    assert_eq!(report.art_name.as_deref(), Some("bg.png"));
    let assets = report.result.as_ref().expect("the cabin assets are built");
    assert_eq!(assets.image(), (800, 600));
    assert_eq!(assets.art().kind(), ContentKind::Image);
    assert_eq!(assets.first_action(), Some(Action::Back));
}

#[test]
fn accept_f45_b_1_the_deck_refuses_a_screen_whose_original_buttons_the_table_cannot_all_bind() {
    let layout = OriginalLayout::parse(CABIN).unwrap();
    let built = OriginalScreens::build(&layout, &extents);

    // The cabin's art has a back button and a scrapbook button but none for
    // the designed OpenBriefing/OpenConstruction: the deck names what is
    // hidden instead of relaxing its coverage rule.
    assert!(matches!(
        built.verdict(Screen::Cabin),
        Some(Err(DeckError::HiddenAction {
            screen: Screen::Cabin,
            action: Action::OpenBriefing
        }))
    ));
    assert!(built.deck().assets(Screen::Cabin).is_none());

    // Screens with no section in this layout are reported, not defaulted.
    let main = built.report(Screen::MainMenu).unwrap();
    assert_eq!(
        main.result.as_ref().unwrap_err(),
        &ScreenRefusal::SectionMissing
    );
    assert!(built.verdict(Screen::MainMenu).is_none());
    assert_eq!(built.reports().len(), 6);
}

#[test]
fn accept_f45_b_1_a_screen_that_the_table_fully_covers_enters_the_deck() {
    let text = "\
[@Preferences@]
BG   =P,bg.png,0,0,0,1,0,0,0
BACK =B,strip.png,593,561,0,0,MainMenu,0x1000,1
MORE =B,strip.png,440,505,0,0,,,
";
    let layout = OriginalLayout::parse(text).unwrap();
    let built = OriginalScreens::build(&layout, &extents);
    assert_eq!(built.verdict(Screen::Settings), Some(&Ok(())));
    let assets = built.deck().assets(Screen::Settings).expect("in the deck");
    assert_eq!(
        assets.buttons().len(),
        1,
        "only the bound button is a hotspot"
    );
    assert_eq!(assets.buttons()[0].action, Action::Back);
    // The unbound original button stays visible in the report.
    let report = built.report(Screen::Settings).unwrap();
    assert_eq!(report.unbound.len(), 1);
    assert_eq!(report.unbound[0].reason, UnboundReason::NoScript);
}

/// F47-C1's consequence for an original deck: the scrapbook's page and
/// entry actions live *inside* the screen, so a button could only ever bind
/// them through a script that names the screen's own section — which the
/// binder refuses as `SelfScript`. An original scrapbook layout therefore
/// cannot cover them, and the deck names the first one it is missing
/// instead of relaxing its coverage rule.
#[test]
fn accept_f45_b_1_the_original_scrapbook_is_deck_refused_by_the_action_it_cannot_bind() {
    let text = "\
[@ScrapBook_TOC@]
BG   =P,bg.png,0,0,0,1,0,0,0
BACK =B,strip.png,593,561,0,0,PassengerCabin,0x1000,1
MORE =B,strip.png,440,505,0,0,,,
";
    let layout = OriginalLayout::parse(text).unwrap();
    let built = OriginalScreens::build(&layout, &extents);
    assert_eq!(
        built.verdict(Screen::Scrapbook),
        Some(&Err(DeckError::HiddenAction {
            screen: Screen::Scrapbook,
            action: Action::NextPage,
        }))
    );
    // The screen's assets still build — the refusal is about coverage, so
    // the bound Back button and the unbound original button are both
    // reported, not dropped.
    let report = built.report(Screen::Scrapbook).unwrap();
    let assets = report.result.as_ref().expect("the screen itself builds");
    assert_eq!(assets.buttons().len(), 1);
    assert_eq!(assets.buttons()[0].action, Action::Back);
    assert_eq!(report.unbound.len(), 1);
    assert_eq!(report.unbound[0].reason, UnboundReason::NoScript);
}

#[test]
fn accept_f45_b_1_art_that_is_missing_ambiguous_or_unreadable_is_refused_not_chosen() {
    let none = OriginalLayout::parse("[@FlightCheck@]\nP1=P,bg.png,5,0,0,1,0,0,0\n").unwrap();
    assert_eq!(
        none.screen(Screen::FlightCheck, &extents)
            .result
            .unwrap_err(),
        ScreenRefusal::NoFullPane,
        "a pane that is not at the origin is not promoted to be the art"
    );
    let animated = OriginalLayout::parse("[@FlightCheck@]\nP1=P,bg.png,0,0,0,12,0,0,0\n").unwrap();
    assert_eq!(
        animated
            .screen(Screen::FlightCheck, &extents)
            .result
            .unwrap_err(),
        ScreenRefusal::NoFullPane,
        "a multi-frame pane is a strip, not a screen picture"
    );
    let two = OriginalLayout::parse(
        "[@FlightCheck@]\nA=P,bg.png,0,0,0,1,0,0,0\nB=P,bg2.png,0,0,0,1,0,0,0\n",
    )
    .unwrap();
    assert_eq!(
        two.screen(Screen::FlightCheck, &extents)
            .result
            .unwrap_err(),
        ScreenRefusal::AmbiguousFullPane {
            panes: vec!["A".to_owned(), "B".to_owned()]
        }
    );
    let unread = OriginalLayout::parse("[@FlightCheck@]\nA=P,gone.png,0,0,0,1,0,0,0\n").unwrap();
    assert_eq!(
        unread
            .screen(Screen::FlightCheck, &extents)
            .result
            .unwrap_err(),
        ScreenRefusal::ArtUnread {
            art: "gone.png".to_owned()
        }
    );
    // A designed screen with no paired section is reported.
    assert_eq!(
        none.screen(Screen::Recon, &extents).result.unwrap_err(),
        ScreenRefusal::ScreenNotPaired
    );
}

/// The installation, opened once.
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
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// **The retail half.** The original `LAYOUT.CSV` is read end to end over the
/// installation's own art and what it supports is pinned: which paired screens
/// became assets, which entered the deck, and why each of the others did not.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f45_b_1_retail_the_original_layout_is_read_and_each_screen_is_built_or_refused_by_name() {
    let screens = retail_screens();
    let (text, layout_sha) = screens.layout_text().expect("the layout member reads");
    let layout = OriginalLayout::parse(&text).expect("the original layout parses");

    // What the file holds (measured on this installation).
    let panes: usize = layout.sections().iter().map(|s| s.panes.len()).sum();
    let buttons: usize = layout.sections().iter().map(|s| s.buttons.len()).sum();
    assert_eq!(layout.sections().len(), 34, "[@…@] sections");
    assert_eq!(panes, 106);
    assert_eq!(buttons, 119);

    let built = screens.original_screens().expect("the screens are read");

    // Every paired section exists in the original; nothing is `SectionMissing`.
    for report in built.reports() {
        assert_ne!(
            report.result.as_ref().err(),
            Some(&ScreenRefusal::SectionMissing),
            "{:?}",
            report.screen
        );
    }

    // Built screens carry the measured 800x600 art and in-bounds hotspots.
    let mut built_count = 0;
    for report in built.reports() {
        if let Ok(assets) = &report.result {
            built_count += 1;
            assert_eq!(assets.image(), (800, 600), "{:?}", report.screen);
            for button in assets.buttons() {
                assert!(button.rect.x + button.rect.width <= 800);
                assert!(button.rect.y + button.rect.height <= 600);
            }
        }
    }
    assert_eq!(
        built_count, 4,
        "cabin, scrapbook, flight check, construction"
    );

    // No original screen enters the deck: the scrapbook's only bindable
    // button is Back — its page and entry actions live inside the screen,
    // so no original script can name a section that binds them (F47-C1).
    // The deck names what is hidden rather than relaxing the rule.
    assert!(matches!(
        built.verdict(Screen::Scrapbook),
        Some(Err(DeckError::HiddenAction {
            action: Action::NextPage,
            ..
        }))
    ));
    assert_eq!(
        built.deck().screens().collect::<Vec<_>>(),
        Vec::<Screen>::new()
    );
    // The scrapbook's assets still build: its bound Back button keeps the
    // measured rectangle.
    let scrapbook = built
        .report(Screen::Scrapbook)
        .unwrap()
        .result
        .as_ref()
        .unwrap();
    assert_eq!(scrapbook.buttons().len(), 1);
    assert_eq!(scrapbook.buttons()[0].action, Action::Back);
    assert_eq!(
        scrapbook.buttons()[0].rect,
        Rect {
            x: 593,
            y: 561,
            width: 200,
            height: 32
        }
    );

    // The refusals the original data produces, by name.
    assert!(matches!(
        built.verdict(Screen::Cabin),
        Some(Err(DeckError::HiddenAction {
            action: Action::OpenBriefing,
            ..
        }))
    ));
    assert!(matches!(
        built.verdict(Screen::FlightCheck),
        Some(Err(DeckError::NoEscapeButton { .. }))
    ));
    assert!(matches!(
        built.verdict(Screen::Construction),
        Some(Err(DeckError::NoEscapeButton { .. }))
    ));
    for screen in [Screen::MainMenu, Screen::Settings] {
        assert_eq!(
            built.report(screen).unwrap().result.as_ref().unwrap_err(),
            &ScreenRefusal::NoFullPane,
            "{screen:?} has no single-frame origin pane"
        );
    }

    // No placeholder survived into a bound button.
    for report in built.reports() {
        for button in &report.bound {
            assert!(button.rect.width > 0 && button.rect.height > 0);
        }
    }

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
            "  \"layout_sha256\": {},\n",
            json_str(&layout_sha)
        ));
        json.push_str(&format!(
            "  \"sections\": {}, \"panes\": {panes}, \"buttons\": {buttons},\n",
            layout.sections().len()
        ));
        json.push_str("  \"screens\": [\n");
        let rows: Vec<String> = built
            .reports()
            .iter()
            .map(|report| {
                let outcome = match &report.result {
                    Ok(_) => "assets".to_owned(),
                    Err(error) => format!("refused: {error}"),
                };
                let verdict = match built.verdict(report.screen) {
                    Some(Ok(())) => "in-deck".to_owned(),
                    Some(Err(error)) => format!("deck refused: {error}"),
                    None => "no assets".to_owned(),
                };
                let unbound: Vec<String> = report
                    .unbound
                    .iter()
                    .map(|u| json_str(&format!("{}: {:?}", u.id, u.reason)))
                    .collect();
                let bound: Vec<String> = report
                    .bound
                    .iter()
                    .map(|b| json_str(&format!("{} -> {} at {:?}", b.id, b.action.key(), b.rect)))
                    .collect();
                format!(
                    "    {{\"screen\": {}, \"section\": {}, \"art\": {}, \"assets\": {}, \"deck\": {}, \"bound\": [{}], \"unbound\": [{}]}}",
                    json_str(&format!("{:?}", report.screen)),
                    json_str(&report.section),
                    json_str(report.art_name.as_deref().unwrap_or("")),
                    json_str(&outcome),
                    json_str(&verdict),
                    bound.join(", "),
                    unbound.join(", ")
                )
            })
            .collect();
        json.push_str(&rows.join(",\n"));
        json.push_str("\n  ]\n}\n");
        std::fs::write(dir.join("original-layout.json"), json).expect("the artifact is written");
    }
}

// ---- F45-B.2 (#1456): the handlers of buttons that start no screen script ----

use cs_app::ui::front_end::ScreenHandlers;

/// An invented script in the original's *shape*: buttons created by
/// `object X = @ctl@BE` / `X.YC = "id"`, arms in `gui_mailbox`.
const MENU_SCRIPT: &str = "\
main
{
gui_create
{
object GC = @ctl@AL
GC.YC = \"movie\"
object QW = @ctl@BE
QW.YC = \"m_b_go\"
object TW = @ctl@BE
TW.YC = \"m_b_pref\"
object VW = @ctl@BE
VW.YC = \"m_b_quit\"
object XW = @ctl@BE
XW.YC = \"m_b_odd\"
}
gui_mailbox
{
if (10003 == getmessage())
{
switch (sender)
{
case QW:
case TW:
deactivate(this)
break
case XW:
mail (11003, this)
callback($$A$$, 6)
break
case VW
terminate
}
}
}
}
";

const MENU_LAYOUT: &str = "\
[@MainMenu@]
BG   =P,bg.png,0,0,0,1,0,0,0
M_B_GO =B,strip.png,10,10,0,0,,0x1000,1
M_B_PREF =B,strip.png,10,110,0,0,Preferences,0x1000,1
M_B_QUIT =B,strip.png,10,210,0,0,,0x1000,1
M_B_ODD =B,strip.png,10,310,0,0,,0x1000,1
M_B_GONE =B,strip.png,10,410,0,0,,0x1000,1
";

#[test]
fn accept_f45_b_2_the_handler_reader_places_arms_by_control_and_shares_stacked_labels() {
    let handlers = ScreenHandlers::parse(MENU_SCRIPT);
    assert_eq!(
        handlers.buttons(),
        ["m_b_go", "m_b_pref", "m_b_quit", "m_b_odd"]
    );
    assert_eq!(handlers.handler("M_B_QUIT").unwrap(), ["terminate"]);
    // Stacked `case` labels share the arm that follows them.
    assert_eq!(handlers.handler("m_b_go").unwrap(), ["deactivate(this)"]);
    assert_eq!(handlers.handler("m_b_pref"), handlers.handler("m_b_go"));
    assert_eq!(
        handlers.handler("m_b_odd").unwrap(),
        ["mail (11003, this)", "callback($$A$$, 6)"]
    );
    assert!(handlers.handler("m_b_missing").is_none());
}

#[test]
fn accept_f45_b_2_only_a_bare_terminate_binds_and_everything_else_is_kept_verbatim() {
    let layout = OriginalLayout::parse(MENU_LAYOUT).unwrap();
    let scripts = |section: &str| (section == "MainMenu").then(|| MENU_SCRIPT.to_owned());
    let built = OriginalScreens::build_with_scripts(&layout, &extents, &scripts);
    let report = built.report(Screen::MainMenu).unwrap();

    let quit = report
        .bound
        .iter()
        .find(|b| b.script == "terminate")
        .unwrap();
    assert_eq!(quit.action, Action::Quit);
    assert_eq!(
        quit.rect,
        Rect {
            x: 10,
            y: 210,
            width: 200,
            height: 20
        }
    );
    // The script-started button still binds through its section.
    assert!(
        report
            .bound
            .iter()
            .any(|b| b.action == Action::OpenSettings)
    );

    let reason = |id: &str| &report.unbound.iter().find(|u| u.id == id).unwrap().reason;
    assert_eq!(
        reason("M_B_GO"),
        &UnboundReason::HandlerNotATransition {
            statements: vec!["deactivate(this)".to_owned()]
        },
        "a handler that states no destination is not guessed into one"
    );
    assert_eq!(
        reason("M_B_ODD"),
        &UnboundReason::HandlerNotATransition {
            statements: vec![
                "mail (11003, this)".to_owned(),
                "callback($$A$$, 6)".to_owned()
            ]
        }
    );
    assert_eq!(reason("M_B_GONE"), &UnboundReason::NoHandler);

    // Without the script nothing changes from F45-B.1.
    let plain = OriginalScreens::build(&layout, &extents);
    let plain = plain.report(Screen::MainMenu).unwrap();
    assert!(plain.bound.iter().all(|b| b.script != "terminate"));
    assert!(
        plain
            .unbound
            .iter()
            .any(|u| u.reason == UnboundReason::NoScript)
    );
}

/// **The retail half.** What the installation's own screen scripts state for
/// the buttons that start no screen script.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f45_b_2_retail_the_main_menu_quit_button_is_bound_by_its_terminate_handler() {
    let screens = retail_screens();
    let built = screens.original_screens().expect("the screens are read");

    let menu = built.report(Screen::MainMenu).unwrap();
    let quit = menu
        .bound
        .iter()
        .find(|b| b.script == "terminate")
        .expect("MM_B_QUIT binds through `case VW: terminate`");
    assert_eq!(quit.action, Action::Quit);
    assert_eq!(menu.bound.len(), 2, "quit and preferences");
    let handlers = menu.handlers.as_ref().expect("MAINMENU.SCRIPT was read");
    assert_eq!(handlers.handler("mm_b_quit").unwrap(), ["terminate"]);
    // Campaign shares a `deactivate(this)` arm and starts its screen elsewhere.
    assert_eq!(
        handlers.handler("mm_b_campaign").unwrap(),
        ["deactivate(this)"]
    );

    // Flight check: fly mission / return to briefing end the screen and call
    // natives whose meaning is unknown, so they stay unbound with the text.
    let check = built.report(Screen::FlightCheck).unwrap();
    for id in ["FC_B_FLYMISSION", "FC_B_RETURNBRIEF"] {
        let button = check.unbound.iter().find(|u| u.id == id).unwrap();
        assert!(
            matches!(&button.reason, UnboundReason::HandlerNotATransition { statements }
                if statements.iter().any(|s| s.starts_with("script_end"))),
            "{id}: {:?}",
            button.reason
        );
    }
    // No button of any screen binds an action the scripts do not state.
    for report in built.reports() {
        for bound in report.bound.iter().filter(|b| b.script == "terminate") {
            assert_eq!(bound.action, Action::Quit, "{:?}", report.screen);
        }
    }
}
