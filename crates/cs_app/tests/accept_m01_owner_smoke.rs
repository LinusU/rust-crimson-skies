//! Task #1535 `M01-OWNER-SMOKE` acceptance tests.
//! Prefix: `accept_m01_owner_smoke_`.
//!
//! The smoke is the scripted run of the owner's own command
//! (`cs --cs-path <retail> --mission M01 --smoke-seconds <n> --capture-dir
//! <dir>`, `crates/cs_app/src/mission_session/smoke.rs`). These tests drive
//! the parts that do not need a GPU — the CLI request and the check judge —
//! against the two worlds that matter: the one the owner actually saw on
//! 2026-10-17 (nothing visible, nothing audible but the radio) and one every
//! check passes. The windowed half, which produces the framebuffer PNGs the
//! finding cites, is the ignored test at the bottom and is run by the
//! implementer and the reviewer with `--include-ignored`.

use std::path::PathBuf;

use cs_app::cli::{self, CliRequest, parse};
use cs_app::mission_session::{
    ActorMetrics, AudioMetrics, Check, GunMetrics, HudMetrics, MIN_ACTOR_PIXELS, MIN_GUN_ROUNDS,
    MIN_MISSION_SMOKE_SECONDS, MIN_OBJECTIVE_TEXT_PIXELS, MIN_SEA_COVERAGE_PERMILLE,
    MIN_TERRAIN_DETAIL_PERMILLE, MIN_TERRAIN_DISTINCT_COLOURS, MIN_TERRAIN_LUMINANCE_SPREAD,
    RocketMetrics, SMOKE_KIND, SMOKE_REPORT_FILE, SeaMetrics, SmokeMeasurements, TerrainMetrics,
    evaluate_mission_smoke,
};
use cs_app::playtest::SmokeRequest;

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// The check judge's own verdict: the applicable checks that failed.
fn failures(checks: &[Check]) -> Vec<&str> {
    checks
        .iter()
        .filter(|check| check.applies && !check.passed)
        .map(|check| check.id.as_str())
        .collect()
}

fn check<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("the smoke reports a {id} check: {checks:?}"))
}

/// One healthy census member: on screen, drawn, well over the pixel floor.
fn seen(kind: &str, name: &str) -> ActorMetrics {
    ActorMetrics {
        kind: kind.to_owned(),
        name: name.to_owned(),
        asleep: false,
        placed: kind != "player",
        placement_m: Some(400.0),
        on_screen: true,
        pixels: 4_096,
        total_pixels: 3_686_400,
        note: None,
    }
}

/// The measurements of a mission the player can see, hear and shoot.
fn healthy() -> SmokeMeasurements {
    SmokeMeasurements {
        mission: "M01".to_owned(),
        frames: 1_800,
        ticks: 21_600,
        terrain: Some(TerrainMetrics {
            world_pixels: 1_971_178,
            total_pixels: 3_686_400,
            covered_permille: 535,
            distinct_colours: 12_000,
            detail_permille: 420,
            luminance_min: 24,
            luminance_max: 210,
            near_luminance_min: 40,
            near_luminance_max: 200,
        }),
        sea: Some(SeaMetrics {
            sea_pixels: 1_180_000,
            total_pixels: 3_686_400,
            covered_permille: 320,
            water_objects: 1,
        }),
        hud: HudMetrics {
            model_line: Some("Find the lost treasure".to_owned()),
            drawn_line: Some("Find the lost treasure".to_owned()),
            drawn_visible: true,
            region_text_pixels: Some(900),
            overlay_hidden: false,
        },
        actors: vec![
            seen("player", "player"),
            seen("aircraft", "wingman_1"),
            seen("zeppelin", "workersvoyagezep"),
        ],
        guns: GunMetrics {
            effects: 45,
            max_live_rounds: 6,
            rounds_drawn: Some(true),
            rounds_examined: 6,
            flash_frames: 12,
            flashes_undrawn: 0,
        },
        rocket: RocketMetrics {
            launched: true,
            max_rockets: 1,
            refusals: vec![],
        },
        audio: AudioMetrics {
            radio_seen: true,
            engine_seen: true,
            engine_bindings: 1,
            active_loops: 1,
            radio_started: 1,
        },
        approach: cs_app::mission_session::ApproachMetrics {
            placed: true,
            anchor: Some("workersvoyagezep".to_owned()),
            frames: 420,
            objective_woke: true,
        },
        missing_shots: vec![],
        ..SmokeMeasurements::default()
    }
}

/// **A mission the player can see, hear and shoot passes every check the
/// smoke makes — and the sea check is the one check that can honestly not
/// apply, which it records instead of passing.**
#[test]
fn accept_m01_owner_smoke_the_judge_passes_a_mission_the_player_can_see_and_shoot() {
    let checks = evaluate_mission_smoke(&healthy());
    assert!(
        failures(&checks).is_empty(),
        "a healthy mission passes: {failures:?}",
        failures = failures(&checks)
    );
    for id in [
        "world_terrain_drawn",
        "world_terrain_textured",
        "world_terrain_lit",
        "world_sea_visible",
        "hud_objective_current",
        "hud_objective_matches",
        "actors_project_and_cover",
        "weapon_guns_fire",
        "weapon_rounds_drawn",
        "weapon_muzzle_flash_drawn",
        "weapon_rocket_launches",
        "audio_radio_queued",
        "audio_engine_queued",
        "smoke_shots_delivered",
    ] {
        let verdict = check(&checks, id);
        assert!(verdict.applies, "{id} applies to an ordinary run");
        assert!(verdict.passed, "{id}: {}", verdict.detail);
    }

    // A world whose records declare no water is the one honest skip.
    let mut dry = healthy();
    if let Some(sea) = dry.sea.as_mut() {
        sea.water_objects = 0;
    }
    let checks = evaluate_mission_smoke(&dry);
    let sea = check(&checks, "world_sea_visible");
    assert!(!sea.applies, "a dry world requires no sea: {}", sea.detail);
    assert!(
        failures(&checks).is_empty(),
        "the skip is not a failure: {:?}",
        failures(&checks)
    );
}

/// **Every defect the owner's 2026-10-17 playtest saw fails its own named
/// check: untextured and unlit terrain, no sea, no current objective, an
/// invisible actor, invisible shots and an unlaunched rocket.**
///
/// The world measurements are the real capture of main `9b75d132`'s own
/// world-probe frame (`private/evidence/M01-OWNER-SMOKE-baseline/`): the
/// terrain drew 2 334 389 pixels as one flat dark grey, the objective widget
/// drew nothing, the scripted aircraft and the zeppelins drew nothing near
/// the player, and no shot has ever carried a mesh.
#[test]
fn accept_m01_owner_smoke_the_judge_fails_everything_the_owner_could_not_see() {
    let owner = SmokeMeasurements {
        mission: "M01".to_owned(),
        frames: 780,
        ticks: 9_360,
        terrain: Some(TerrainMetrics {
            world_pixels: 1_971_178,
            total_pixels: 3_686_400,
            covered_permille: 535,
            // The measured run of 2026-10-17: 711 colours and a whole-frame
            // spread of 120, all of it the horizon fog — but only 1 per mille
            // of local detail and a near field of 82..83.
            distinct_colours: 711,
            detail_permille: 1,
            luminance_min: 82,
            luminance_max: 202,
            near_luminance_min: 82,
            near_luminance_max: 83,
        }),
        sea: Some(SeaMetrics {
            sea_pixels: 0,
            total_pixels: 3_686_400,
            covered_permille: 0,
            water_objects: 0,
        }),
        hud: HudMetrics::default(),
        actors: vec![
            ActorMetrics {
                kind: "player".to_owned(),
                name: "player".to_owned(),
                asleep: false,
                placed: false,
                placement_m: None,
                on_screen: true,
                pixels: 40_000,
                total_pixels: 3_686_400,
                note: None,
            },
            ActorMetrics {
                kind: "aircraft".to_owned(),
                name: "wingman_1".to_owned(),
                asleep: false,
                placed: true,
                placement_m: Some(400.0),
                on_screen: true,
                pixels: 0,
                total_pixels: 3_686_400,
                note: None,
            },
        ],
        guns: GunMetrics {
            effects: 45,
            max_live_rounds: 6,
            rounds_drawn: Some(false),
            rounds_examined: 6,
            flash_frames: 0,
            flashes_undrawn: 0,
        },
        rocket: RocketMetrics {
            launched: false,
            max_rockets: 0,
            refusals: vec!["no declared-hostile damage target is live".to_owned()],
        },
        audio: AudioMetrics {
            radio_seen: true,
            engine_seen: true,
            engine_bindings: 1,
            active_loops: 1,
            radio_started: 1,
        },
        approach: cs_app::mission_session::ApproachMetrics {
            placed: true,
            anchor: Some("workersvoyagezep".to_owned()),
            frames: 1_080,
            objective_woke: false,
        },
        missing_shots: vec![],
        ..SmokeMeasurements::default()
    };
    let checks = evaluate_mission_smoke(&owner);
    let failed = failures(&checks);
    for id in [
        "world_terrain_textured",
        "world_terrain_lit",
        "hud_objective_current",
        "actors_project_and_cover",
        "weapon_rounds_drawn",
        "weapon_muzzle_flash_drawn",
        "weapon_rocket_launches",
    ] {
        assert!(
            failed.contains(&id),
            "{id} must fail on what the owner saw: {failed:?}"
        );
    }
    // The radio and the engine did work, and the world was drawn at all: the
    // smoke must not fail what the player really had.
    for id in [
        "world_terrain_drawn",
        "audio_radio_queued",
        "audio_engine_queued",
        "weapon_guns_fire",
    ] {
        assert!(!failed.contains(&id), "{id} must not fail: {failed:?}");
    }

    // The owner's own run never woke an objective (they never flew into the
    // trigger), so the widget agrees with the model — nothing is drawn
    // because nothing is current. Once the model *does* mark a current line
    // and the screen still draws nothing, the HUD check itself fails.
    let mut woke_but_black = owner.clone();
    woke_but_black.approach.objective_woke = true;
    woke_but_black.hud.model_line = Some("OBJECTIVE_MSG_1".to_owned());
    let checks = evaluate_mission_smoke(&woke_but_black);
    assert!(
        check(&checks, "hud_objective_current").passed,
        "the model did mark a line"
    );
    assert!(
        failures(&checks).contains(&"hud_objective_matches"),
        "a current objective the screen does not draw fails: {:?}",
        failures(&checks)
    );

    // Floors are floors: one step under any of them is a failure.
    let mut barely = healthy();
    barely.terrain.as_mut().expect("a terrain").distinct_colours = MIN_TERRAIN_DISTINCT_COLOURS - 1;
    let barely_checks = evaluate_mission_smoke(&barely);
    let failed = failures(&barely_checks);
    assert!(failed.contains(&"world_terrain_textured"), "{failed:?}");
    let mut barely = healthy();
    barely.terrain.as_mut().expect("a terrain").detail_permille = MIN_TERRAIN_DETAIL_PERMILLE - 1;
    assert!(
        failures(&evaluate_mission_smoke(&barely)).contains(&"world_terrain_textured"),
        "one per mille under the detail floor fails — a smooth fog gradient is not a texture"
    );
    let mut barely = healthy();
    barely
        .terrain
        .as_mut()
        .expect("a terrain")
        .near_luminance_min = 60;
    barely
        .terrain
        .as_mut()
        .expect("a terrain")
        .near_luminance_max = 60 + MIN_TERRAIN_LUMINANCE_SPREAD - 1;
    assert!(
        failures(&evaluate_mission_smoke(&barely)).contains(&"world_terrain_lit"),
        "a near field one level under the lit floor fails"
    );
    let mut barely = healthy();
    barely.sea.as_mut().expect("a sea").covered_permille = MIN_SEA_COVERAGE_PERMILLE - 1;
    assert!(
        failures(&evaluate_mission_smoke(&barely)).contains(&"world_sea_visible"),
        "one per mille under the sea floor fails"
    );
    let mut barely = healthy();
    barely.actors[1].pixels = MIN_ACTOR_PIXELS - 1;
    assert!(
        failures(&evaluate_mission_smoke(&barely)).contains(&"actors_project_and_cover"),
        "one pixel under the actor floor fails"
    );
    let mut barely = healthy();
    barely.hud.region_text_pixels = Some(MIN_OBJECTIVE_TEXT_PIXELS - 1);
    assert!(
        failures(&evaluate_mission_smoke(&barely)).contains(&"hud_objective_matches"),
        "one pixel under the text floor fails"
    );
    let mut barely = healthy();
    barely.guns.effects = MIN_GUN_ROUNDS - 1;
    assert!(
        failures(&evaluate_mission_smoke(&barely)).contains(&"weapon_guns_fire"),
        "one round under the gun floor fails"
    );
    let mut barely = healthy();
    barely.terrain = None;
    let barely_checks = evaluate_mission_smoke(&barely);
    let failed = failures(&barely_checks);
    assert!(
        failed.contains(&"world_terrain_drawn")
            && failed.contains(&"world_terrain_textured")
            && failed.contains(&"world_terrain_lit"),
        "a missing probe frame fails every world claim: {failed:?}"
    );
    let mut barely = healthy();
    barely.missing_shots = vec!["guns".to_owned()];
    assert!(
        failures(&evaluate_mission_smoke(&barely)).contains(&"smoke_shots_delivered"),
        "a screenshot the renderer never delivered fails the run"
    );
}

/// **`--mission` takes the smoke flags, and every half-stated form is
/// refused by name.**
#[test]
fn accept_m01_owner_smoke_the_cli_takes_a_mission_smoke_and_refuses_its_misuse() {
    let smoke = parse(args(&[
        "--cs-path",
        "/install",
        "--mission",
        "M01",
        "--smoke-seconds",
        "45",
        "--capture-dir",
        "private/m01-smoke",
    ]));
    let CliRequest::Mission(request) = smoke else {
        panic!("the mission smoke parses as a mission request: {smoke:?}");
    };
    assert_eq!(
        request.smoke,
        Some(SmokeRequest {
            seconds: 45,
            capture_dir: PathBuf::from("private/m01-smoke"),
        }),
        "the smoke travels on the mission request"
    );

    // The default capture directory when `--capture-dir` is omitted.
    let default = parse(args(&[
        "--cs-path",
        "/i",
        "--mission",
        "M01",
        "--smoke-seconds",
        "45",
    ]));
    let CliRequest::Mission(request) = default else {
        panic!("the mission smoke parses as a mission request: {default:?}");
    };
    assert_eq!(
        request.smoke.map(|smoke| smoke.capture_dir),
        Some(PathBuf::from(
            cs_app::mission_session::DEFAULT_MISSION_CAPTURE_DIR
        )),
        "an omitted --capture-dir lands in the documented default"
    );

    // An ordinary mission run is unchanged: no smoke.
    let plain = parse(args(&["--cs-path", "/i", "--mission", "M01"]));
    let CliRequest::Mission(request) = plain else {
        panic!("the plain mission launch parses: {plain:?}");
    };
    assert_eq!(request.smoke, None, "an ordinary run writes no smoke");

    // Misuse, each refused by name.
    for (bad, needle) in [
        (
            args(&[
                "--cs-path",
                "/i",
                "--mission",
                "M01",
                "--smoke-seconds",
                "20",
            ]),
            "at least",
        ),
        (
            args(&["--cs-path", "/i", "--mission", "M01", "--capture-dir", "d"]),
            "--capture-dir is where --mission --smoke-seconds",
        ),
        (
            args(&["--mission", "M01", "--smoke-seconds", "not a number"]),
            "whole number of seconds",
        ),
    ] {
        let refused = parse(bad.clone());
        let CliRequest::Invalid { reason } = refused else {
            panic!("{bad:?} is refused: {refused:?}");
        };
        assert!(
            reason.contains(needle),
            "{reason:?} names the problem ({needle})"
        );
    }
    // The minimum is the documented one.
    const {
        assert!(MIN_MISSION_SMOKE_SECONDS >= 45, "the script needs the room");
    }
    assert!(
        cli::HELP_TEXT.contains("--mission <id> --smoke-seconds <n>"),
        "--help documents the mission smoke"
    );
}

/// The windowed binary the ignored run drives: `CS_MISSION_BIN` when the
/// operator names one, else the release build (the documented way to run this
/// task's windowed evidence — debug Bevy steps far too slowly for a
/// 2 700-frame script), else the test harness's own binary.
fn mission_bin() -> PathBuf {
    if let Ok(bin) = std::env::var("CS_MISSION_BIN") {
        return PathBuf::from(bin);
    }
    let release = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/release/cs");
    if release.is_file() {
        return release;
    }
    PathBuf::from(env!("CARGO_BIN_EXE_cs"))
}

/// **The windowed run, on the owner's own command.**
///
/// This is the half that needs the installation, the GPU and the audio device:
/// it runs the exact launch the owner plays, with the smoke flags, and
/// asserts the *harness contract* — the report names every check with its own
/// verdict, the framebuffer PNGs arrive, and the process exits non-zero
/// exactly when a check failed. Which checks fail is the product's business,
/// not this test's: the smoke exists to say so out loud.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_m01_owner_smoke_the_windowed_launch_runs_the_scripted_smoke() {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let Ok(capabilities) = std::env::var("CS_CAPABILITIES") else {
        panic!("CS_CAPABILITIES is unset: the mission launch needs gpu and audio");
    };
    for capability in ["gpu", "audio"] {
        assert!(
            capabilities
                .split(',')
                .any(|part| part.trim() == capability),
            "CS_CAPABILITIES must list {capability}: {capabilities}"
        );
    }
    let engine_image = std::env::var("CS_ENGINE_IMAGE")
        .expect("CS_ENGINE_IMAGE names the owner's decrypted executable");
    let dir = std::env::var("CS_M01_OWNER_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("m01-owner-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let output = std::process::Command::new(mission_bin())
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .args(["--smoke-seconds", "45", "--capture-dir"])
        .arg(&dir)
        .env("CS_CAPABILITIES", capabilities)
        .env("CS_ENGINE_IMAGE", engine_image)
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.code().is_some(),
        "the run must exit by itself, never on a signal: {output:?}\n{stderr}"
    );

    let report = std::fs::read_to_string(dir.join(SMOKE_REPORT_FILE))
        .unwrap_or_else(|error| panic!("{} is written: {error}\n{stderr}", SMOKE_REPORT_FILE));
    assert!(
        report.contains(&format!("\"kind\":\"{SMOKE_KIND}\"")),
        "the report names its kind: {report}"
    );
    assert!(
        report.contains("\"human_play\":false"),
        "a scripted run is never human_play: {report}"
    );
    for id in [
        "world_terrain_drawn",
        "world_terrain_textured",
        "world_terrain_lit",
        "world_sea_visible",
        "hud_objective_current",
        "hud_objective_matches",
        "actors_project_and_cover",
        "weapon_guns_fire",
        "weapon_rounds_drawn",
        "weapon_muzzle_flash_drawn",
        "weapon_rocket_launches",
        "audio_radio_queued",
        "audio_engine_queued",
        "smoke_shots_delivered",
    ] {
        assert!(
            report.contains(&format!("\"id\":\"{id}\"")),
            "the report judges {id}: {report}"
        );
    }
    let passed = report
        .rfind("\"passed\":")
        .is_some_and(|index| report[index..].starts_with("\"passed\":true"));
    let failures_empty = report.contains("\"failures\":[]");
    assert_eq!(
        passed, failures_empty,
        "`passed` is the report's own last field and is exactly \"no failures\": {report}"
    );
    let pngs: Vec<_> = std::fs::read_dir(&dir)
        .expect("the capture directory lists")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".png"))
        .collect();
    assert!(
        pngs.len() >= 6,
        "the script asks for the world, the sea (twice), every actor, the HUD, the guns, \
         the rocket and a final frame: {} in {:?}",
        pngs.len(),
        dir
    );
    assert_eq!(
        output.status.success(),
        passed,
        "the process exits non-zero exactly when a check failed; stderr: {stderr}"
    );
    println!(
        "mission smoke artifacts in {}: {} PNGs, report passed = {passed}",
        dir.display(),
        pngs.len()
    );
}
