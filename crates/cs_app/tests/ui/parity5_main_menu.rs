//! Acceptance task PARITY5-FRONTEND-MAIN-MENU (#1748): the main menu and the
//! Campaign pilot-selection screen composed like the original's.
//!
//! The four presenter stages (FE-MENU-1..4) built the screens; the owner's
//! Drydock capture of the pilot page showed what they left out: the flag
//! movie and the logo stay behind the page (the main menu is *hidden, not
//! ended*, per `MAINMENU.SCRIPT`'s `deactivate(this)`), while the button
//! frame and the six buttons are gone. [`MenuShell::carry_under`] is that
//! rule; the `accept_parity5_main_menu_` tests call it.
//!
//! * The synthetic tests run over layout text in the original's format and
//!   prove the carry-over rules; they run in CI.
//! * The `retail` test reads `$CS_GAME_DIR`'s real `MainMenu` and `Campaign`
//!   sections. The windowed test drives `cs --front-end --smoke-seconds` on a
//!   GPU and checks the Campaign visit presents the flag movie. Both are
//!   `#[ignore]`d because CI has neither the installation nor a GPU.

use std::path::PathBuf;

use cs_app::ui::front_end::{
    FrontEndScreens, MenuShell, OriginalLayout, PILOT_SECTION, START_SECTION, script_member,
};

const LAYOUT: &str = "\
[@MainMenu@]
LOGO=P,logo.png,10,5,0,1,0,1,0
FRAME=P,frame.png,20,40,0,1,0,1,0
MOVIE=M,flag.mpg,0,0,0,250,250,0,0
MM_B_CAMPAIGN=B,strip.png,30,50,0,0,Campaign,0x1000,1

[@Campaign@]
CM_BACKGROUND=P,pane.png,0,0,0,1,0,1,0
CM_B_CANCEL=B,strip.png,30,110,0,0,MainMenu,0x1000,0
";

fn extents(art: &str) -> Option<(u32, u32)> {
    match art.to_ascii_lowercase().as_str() {
        "logo.png" | "frame.png" | "pane.png" => Some((100, 50)),
        "strip.png" => Some((80, 160)),
        _ => None,
    }
}

fn shell() -> MenuShell {
    let layout = OriginalLayout::parse(LAYOUT).expect("the fixture parses");
    MenuShell::build(&layout, &|art: &str| extents(art), &|_: &str| None)
}

fn pane_ids(shell: &MenuShell, section: &str) -> Vec<String> {
    shell.sections()[&section.to_ascii_uppercase()]
        .panes
        .iter()
        .map(|pane| pane.id.clone())
        .collect()
}

#[test]
fn accept_parity5_main_menu_a_the_logo_and_flag_stand_under_the_campaign_page() {
    let mut shell = shell();
    assert!(shell.carry_under("Campaign", "MainMenu", &["LOGO"]));
    assert_eq!(pane_ids(&shell, "Campaign"), ["LOGO", "CM_BACKGROUND"]);
    let campaign = &shell.sections()["CAMPAIGN"];
    assert_eq!(campaign.movies.len(), 1, "the flag movie is carried");
    assert_eq!(campaign.movies[0].file, "flag.mpg");
    let ids: Vec<&str> = campaign.buttons.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["CM_B_CANCEL"], "no menu button is carried");
    assert!(
        !pane_ids(&shell, "Campaign").contains(&"FRAME".to_owned()),
        "the button frame is hidden"
    );
    // The source is untouched, and the shown section reads the carried rows.
    assert_eq!(pane_ids(&shell, "MainMenu"), ["LOGO", "FRAME"]);
    shell.open("Campaign").expect("Campaign opens");
    let section = shell.section().expect("a section");
    assert_eq!((section.panes.len(), section.movies.len()), (2, 1));
}

#[test]
fn accept_parity5_main_menu_b_carrying_twice_or_from_nowhere_changes_nothing() {
    let mut shell = shell();
    assert!(shell.carry_under("Campaign", "MainMenu", &["LOGO"]));
    assert!(shell.carry_under("Campaign", "MainMenu", &["LOGO"]));
    assert_eq!(pane_ids(&shell, "Campaign"), ["LOGO", "CM_BACKGROUND"]);
    assert_eq!(shell.sections()["CAMPAIGN"].movies.len(), 1);
    assert!(!shell.carry_under("Campaign", "NoSuchScreen", &["LOGO"]));
    assert!(!shell.carry_under("NoSuchScreen", "MainMenu", &["LOGO"]));
    // A pane id the source does not hold carries nothing.
    assert!(shell.carry_under("Campaign", "MainMenu", &["NOPE"]));
    assert_eq!(pane_ids(&shell, "Campaign"), ["LOGO", "CM_BACKGROUND"]);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_main_menu_retail_the_pilot_page_stands_on_the_logo_and_flag() {
    let dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    let screens = FrontEndScreens::open(&dir).expect("the original screen container opens");
    let layout = screens.original_layout().expect("the layout member reads");
    let mut shell = MenuShell::build(
        &layout,
        &|art: &str| screens.art_extent(art),
        &|section: &str| screens.script_text(&script_member(section)),
    );
    assert!(shell.carry_under(PILOT_SECTION, START_SECTION, &["MM_LOGO"]));
    shell.open(PILOT_SECTION).expect("Campaign is a section");
    let section = shell.section().expect("a section");
    let panes: Vec<&str> = section.panes.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(panes, ["MM_LOGO", "CM_BACKGROUND"]);
    assert_eq!(
        (section.panes[0].rect.x, section.panes[0].rect.y),
        (134, 13)
    );
    assert_eq!(section.movies.len(), 1);
    assert_eq!(section.movies[0].file, "CrimFlag.MPG");
    let buttons: Vec<&str> = section.buttons.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(buttons, ["CM_B_START", "CM_B_DELETEPLAYER", "CM_B_CANCEL"]);
}

#[test]
#[ignore = "requires CS_GAME_DIR + gpu and a built windowed binary"]
fn accept_parity5_main_menu_the_windowed_smoke_shows_the_flag_behind_the_pilot_page() {
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
        |_| std::env::temp_dir().join("parity5-main-menu-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(dir.join(cs_app::ui::front_end::SMOKE_PROFILE_SUBDIR));
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "report.json" || name.ends_with(".png") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    {
        let mut seed = cs_app::profile::ProfileSession::open_sandbox(
            &dir.join(cs_app::ui::front_end::SMOKE_PROFILE_SUBDIR),
            &cs_content::save::settings::SettingCatalog::empty(),
        )
        .expect("the sandbox opens");
        for name in ["Ace", "Bea", "Cy"] {
            seed.create(name).expect("a pilot is created");
        }
    }
    let output = std::process::Command::new(
        std::env::var("CS_FRONT_END_BIN")
            .map_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_cs")), PathBuf::from),
    )
    .args(["--cs-path", &game_dir, "--front-end"])
    .args(["--smoke-seconds", "14", "--capture-dir"])
    .arg(&dir)
    .env("CS_CAPABILITIES", capabilities)
    .output()
    .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report = std::fs::read_to_string(dir.join("report.json"))
        .unwrap_or_else(|error| panic!("report.json is written: {error}\n{stderr}"));
    assert!(
        report.contains("\"passed\":true"),
        "the run passed its own checks: {report}\nstderr: {stderr}"
    );
    assert!(
        report.contains("MOVIE MOVIE on Campaign: CrimFlag.MPG"),
        "the Campaign visit presented the flag movie: {report}"
    );
    for label in ["mainmenu", "campaign", "campaign-list"] {
        assert!(
            dir.join(format!("{label}.png")).is_file(),
            "{label}.png was captured: {report}"
        );
    }
    assert!(output.status.success(), "exit 0; stderr: {stderr}");
}
