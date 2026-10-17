//! Task #1572 `M01-SMOKE-NO-TELEPORT` acceptance tests.
//! Prefix: `accept_m01_smoke_no_teleport_`.
//!
//! The mode never moving the player is proved next to the systems it drives
//! (`mission_session::smoke::tests`, same prefix). These tests cover the CLI,
//! the check judge and the windowed run on the owner's command.

use std::path::PathBuf;

use cs_app::cli::{self, CliRequest, parse};
use cs_app::mission_session::{
    Check, MAX_WINGMAN_DISTANCE_M, SMOKE_REPORT_FILE, SmokeMeasurements, WINGMAN_SAMPLE_SECONDS,
    WingmanSample, evaluate_mission_smoke,
};

fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn check<'a>(checks: &'a [Check], id: &str) -> &'a Check {
    checks
        .iter()
        .find(|check| check.id == id)
        .unwrap_or_else(|| panic!("the smoke reports a {id} check: {checks:?}"))
}

fn measured(last_metres: &[f64]) -> SmokeMeasurements {
    let mut m = SmokeMeasurements {
        no_teleport: true,
        ..SmokeMeasurements::default()
    };
    for second in WINGMAN_SAMPLE_SECONDS {
        let metres = if second == 30 { last_metres } else { &[900.0] };
        m.wingmen.push(WingmanSample {
            second,
            distances_m: metres
                .iter()
                .enumerate()
                .map(|(i, d)| (format!("wingman{i}"), *d))
                .collect(),
        });
    }
    m
}

/// **The flag travels on the mission request and nowhere else.**
#[test]
fn accept_m01_smoke_no_teleport_the_cli_takes_the_flag_only_with_a_mission_smoke() {
    let ok = parse(args(&[
        "--cs-path",
        "/i",
        "--mission",
        "M01",
        "--smoke-seconds",
        "45",
        "--smoke-no-teleport",
    ]));
    let CliRequest::Mission(request) = ok else {
        panic!("parses as a mission request: {ok:?}");
    };
    assert!(request.smoke_no_teleport && request.smoke.is_some());

    let default = parse(args(&[
        "--cs-path",
        "/i",
        "--mission",
        "M01",
        "--smoke-seconds",
        "45",
    ]));
    let CliRequest::Mission(request) = default else {
        panic!("parses as a mission request: {default:?}");
    };
    assert!(!request.smoke_no_teleport, "the default smoke is unchanged");

    for bad in [
        args(&["--cs-path", "/i", "--mission", "M01", "--smoke-no-teleport"]),
        args(&["--playtest", "--smoke-seconds", "20", "--smoke-no-teleport"]),
        args(&["--smoke-no-teleport"]),
    ] {
        assert!(
            matches!(parse(bad.clone()), CliRequest::Invalid { .. }),
            "{bad:?} is refused"
        );
    }
    assert!(cli::HELP_TEXT.contains("--smoke-no-teleport"));
}

/// **The check judges the last sample against the stated threshold, and
/// reports nothing else.**
#[test]
fn accept_m01_smoke_no_teleport_wingmen_near_player_judges_the_threshold() {
    let near = evaluate_mission_smoke(&measured(&[300.0, MAX_WINGMAN_DISTANCE_M]));
    assert!(check(&near, "wingmen_near_player").passed, "{near:?}");
    assert!(check(&near, "wingmen_near_player").detail.contains("t+30s"));
    assert!(
        near.iter().all(|c| matches!(
            c.id.as_str(),
            "wingmen_near_player" | "smoke_shots_delivered"
        )),
        "the full checklist does not apply to a run that never moved"
    );

    let far = evaluate_mission_smoke(&measured(&[300.0, MAX_WINGMAN_DISTANCE_M + 1.0]));
    assert!(!check(&far, "wingmen_near_player").passed);

    let none = evaluate_mission_smoke(&measured(&[]));
    assert!(!check(&none, "wingmen_near_player").passed);

    let mut partial = measured(&[100.0]);
    partial.wingmen.pop();
    assert!(!check(&evaluate_mission_smoke(&partial), "wingmen_near_player").passed);
}

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

/// **The windowed run on the owner's command, never moving the player.**
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_m01_smoke_no_teleport_the_windowed_launch_reports_wingman_distances() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the installation");
    let capabilities = std::env::var("CS_CAPABILITIES").expect("CS_CAPABILITIES lists gpu,audio");
    let engine_image = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE is set");
    let dir = std::env::var("CS_M01_NO_TELEPORT_OUT").map_or_else(
        |_| std::env::temp_dir().join("m01-smoke-no-teleport"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let output = std::process::Command::new(mission_bin())
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .args([
            "--smoke-seconds",
            "45",
            "--smoke-no-teleport",
            "--capture-dir",
        ])
        .arg(&dir)
        .env("CS_CAPABILITIES", capabilities)
        .env("CS_ENGINE_IMAGE", engine_image)
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.code().is_some(), "{output:?}\n{stderr}");
    let report = std::fs::read_to_string(dir.join(SMOKE_REPORT_FILE))
        .unwrap_or_else(|error| panic!("report.json is written: {error}\n{stderr}"));
    for needle in [
        "\"mode\":\"no_teleport\"",
        "\"player_teleported\":false",
        "\"id\":\"wingmen_near_player\"",
        "\"human_play\":false",
        "\"placements\":[]",
    ] {
        assert!(report.contains(needle), "{needle} in {report}");
    }
    for second in WINGMAN_SAMPLE_SECONDS {
        assert!(
            report.contains(&format!("\"second\":{second},")),
            "{report}"
        );
    }
    println!("{report}");
}
