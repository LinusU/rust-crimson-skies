//! Acceptance task PARITY5-CABIN-INTRO.06 (#1842): the owner-command smoke of
//! the whole start flow, framebuffers of the intro movie and the cabin.
//!
//! * The synthetic test proves the scene selector (runs in CI).
//! * The windowed test drives `cs --front-end --smoke-seconds 26` on the flow
//!   scene on a GPU: main menu, Campaign, a typed new pilot, the intro movie
//!   (three frames, then skipped), the cabin, the Previous Missions screen
//!   (#1956), its Return to Cabin, the Plane Name dialog (#1958) with a typed name and
//!   its OK, the construction hub (#2042) with its Guns tab, Ready's purchase panel
//!   (#2053) and Purchase Now, Return to Main Menu, Campaign and Continue again
//!   (the intro is skipped a second time) and Next Mission (#1965), whose
//!   hand-over to the first mission ends the run. It is `#[ignore]`d
//!   because CI has neither the installation nor a GPU. Nothing here is
//!   `human_play` or `verified_original`.

use std::path::PathBuf;

use cs_app::ui::front_end::SmokeScene;

#[test]
fn accept_parity5_cabin_intro_06_the_flow_scene_is_selected_by_name() {
    assert_eq!(SmokeScene::from_name("flow"), SmokeScene::Flow);
    assert_eq!(SmokeScene::from_name(" Flow "), SmokeScene::Flow);
    assert_eq!(SmokeScene::from_name("cabin"), SmokeScene::Cabin);
    assert_eq!(SmokeScene::from_name(""), SmokeScene::MainMenu);
    assert_eq!(SmokeScene::from_name("flows"), SmokeScene::MainMenu);
}

#[test]
#[ignore = "requires CS_GAME_DIR + gpu and a built windowed binary"]
fn accept_parity5_cabin_intro_06_the_windowed_smoke_plays_the_intro_then_the_cabin_and_its_buttons()
{
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
    let dir = std::env::var("CS_FLOW_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("parity5-flow-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let output = std::process::Command::new(
        std::env::var("CS_FRONT_END_BIN")
            .map_or_else(|_| PathBuf::from(env!("CARGO_BIN_EXE_cs")), PathBuf::from),
    )
    .args(["--cs-path", &game_dir, "--front-end"])
    .args(["--smoke-seconds", "26", "--capture-dir"])
    .arg(&dir)
    .env("CS_CAPABILITIES", capabilities)
    .env(cs_app::ui::front_end::SMOKE_SCENE_ENV, "flow")
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
        report.contains("\"visited\":[\"MainMenu\",\"Campaign\",\"PassengerCabin\",\"ScrapBook_TOC\",\"PassengerCabin\",\"PlaneName\",\"PlaneConstruction\",\"PassengerCabin\",\"MainMenu\",\"Campaign\",\"PassengerCabin\"]"),
        "{report}"
    );
    for label in [
        "flow-mainmenu",
        "flow-pilot",
        "flow-pilot-typed",
        "intro-a",
        "intro-b",
        "intro-c",
        "flow-cabin",
        "flow-previous-missions",
        "flow-cabin-after",
        "flow-plane-name",
        "flow-plane-name-typed",
        "flow-plane-construction",
        "flow-plane-construction-guns",
        "flow-plane-construction-purchase",
        "flow-plane-construction-purchased",
        "flow-cabin-again",
        "flow-mainmenu-again",
        "flow-campaign-again",
        "flow-cabin-resumed",
    ] {
        assert!(
            dir.join(format!("{label}.png")).is_file(),
            "{label}.png captured: {report}"
        );
    }
    assert!(output.status.success(), "exit 0; stderr: {stderr}");
}
