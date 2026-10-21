//! Task #1813 `PARITY3-COCKPIT-VIEW.05` acceptance tests.
//! Prefix: `accept_parity3_cockpit_view_05_`.
//!
//! The owner-command smoke's cockpit phase presses `F8` three times through
//! the production key path (0 -> 7 -> 6 -> 0), reads the view at each stop and
//! photographs it. The judge tests below run without a GPU against the
//! measurements the phase writes; the windowed test at the bottom runs the
//! real command and is run with `--include-ignored` on the commit under
//! review.

use std::path::PathBuf;

use cs_app::mission_session::{
    Check, CockpitViewMetrics, CockpitViewSample, SMOKE_REPORT_FILE, SmokeMeasurements,
    evaluate_mission_smoke,
};

const COCKPIT_CHECKS: [&str; 4] = [
    "cockpit_view_cycles",
    "cockpit_eye_placement",
    "cockpit_dials_drawn",
    "cockpit_interior_drawn",
];

fn pilot(mode: u32) -> CockpitViewSample {
    CockpitViewSample {
        original_mode: mode,
        camera_from_eye_m: 0.0,
        body_hidden: true,
        dials_drawn: 39,
        dials_total: 39,
        interior_drawn: 12,
        interior_total: 12,
    }
}

/// What a correct cockpit phase measures: modes 7, 6, 0, the camera at the
/// eye with the body hidden at the first two, the panel and interior drawn
/// there and gone in the chase view.
pub(crate) fn healthy_cockpit() -> CockpitViewMetrics {
    CockpitViewMetrics {
        ran: true,
        cockpit: Some(pilot(7)),
        second_eye: Some(pilot(6)),
        chase_after: Some(CockpitViewSample {
            original_mode: 0,
            camera_from_eye_m: 14.0,
            body_hidden: false,
            dials_drawn: 0,
            dials_total: 39,
            interior_drawn: 0,
            interior_total: 12,
        }),
    }
}

fn verdicts(cockpit: CockpitViewMetrics) -> Vec<Check> {
    let checks = evaluate_mission_smoke(&SmokeMeasurements {
        cockpit,
        ..SmokeMeasurements::default()
    });
    COCKPIT_CHECKS
        .iter()
        .map(|id| {
            checks
                .iter()
                .find(|check| check.id == *id)
                .unwrap_or_else(|| panic!("the smoke judges {id}"))
                .clone()
        })
        .collect()
}

#[test]
fn accept_parity3_cockpit_view_05_the_judge_passes_the_cycle_the_original_makes() {
    for check in verdicts(healthy_cockpit()) {
        assert!(
            check.applies && check.passed,
            "{}: {}",
            check.id,
            check.detail
        );
    }
}

/// With the phase removed the measurements stay empty and every cockpit
/// check fails by name.
#[test]
fn accept_parity3_cockpit_view_05_a_run_without_the_phase_fails_every_cockpit_check() {
    for check in verdicts(CockpitViewMetrics::default()) {
        assert!(
            check.applies && !check.passed,
            "{}: {}",
            check.id,
            check.detail
        );
    }
}

#[test]
fn accept_parity3_cockpit_view_05_each_defect_fails_its_own_check() {
    let failing = |cockpit: CockpitViewMetrics| -> Vec<String> {
        verdicts(cockpit)
            .into_iter()
            .filter(|check| !check.passed)
            .map(|check| check.id)
            .collect()
    };

    let mut wrong_cycle = healthy_cockpit();
    wrong_cycle.second_eye = Some(pilot(7));
    assert_eq!(failing(wrong_cycle), ["cockpit_view_cycles"]);

    let mut chase_eye = healthy_cockpit();
    if let Some(sample) = chase_eye.cockpit.as_mut() {
        sample.camera_from_eye_m = 14.0;
    }
    assert_eq!(failing(chase_eye), ["cockpit_eye_placement"]);

    let mut no_dials = healthy_cockpit();
    if let Some(sample) = no_dials.second_eye.as_mut() {
        sample.dials_drawn = 38;
    }
    assert_eq!(failing(no_dials), ["cockpit_dials_drawn"]);

    let mut leaked_interior = healthy_cockpit();
    if let Some(sample) = leaked_interior.chase_after.as_mut() {
        sample.interior_drawn = 3;
    }
    assert_eq!(failing(leaked_interior), ["cockpit_interior_drawn"]);
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

/// **The owner's own command, cockpit phase included.** The report carries
/// the four cockpit checks, every one passed, and the three framebuffer
/// screenshots arrived and are not empty.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu and a built windowed binary"]
fn accept_parity3_cockpit_view_05_the_windowed_smoke_photographs_the_cockpit_cycle() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is unset");
    let capabilities = std::env::var("CS_CAPABILITIES").expect("CS_CAPABILITIES is unset");
    assert!(
        capabilities.split(',').any(|part| part.trim() == "gpu"),
        "CS_CAPABILITIES must list gpu: {capabilities}"
    );
    let engine_image = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE is unset");
    let dir = std::env::var("CS_COCKPIT_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("cockpit-view-smoke"),
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
    let report = std::fs::read_to_string(dir.join(SMOKE_REPORT_FILE))
        .unwrap_or_else(|error| panic!("{SMOKE_REPORT_FILE} is written: {error}\n{stderr}"));
    for id in COCKPIT_CHECKS {
        let marker = format!("\"id\":\"{id}\",\"applies\":true,\"passed\":");
        let at = report
            .find(&marker)
            .unwrap_or_else(|| panic!("the report judges {id}: {report}"));
        assert!(
            report[at + marker.len()..].starts_with("true"),
            "{id} failed: {}",
            &report[at..(at + 400).min(report.len())]
        );
    }
    for label in [
        "cockpit_view",
        "cockpit_view_second_eye",
        "chase_after_cockpit",
    ] {
        let png = std::fs::read_dir(&dir)
            .expect("the capture directory lists")
            .filter_map(Result::ok)
            .find(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.ends_with(&format!("_{label}.png"))
            })
            .unwrap_or_else(|| panic!("the {label} screenshot arrived in {}", dir.display()));
        assert!(png.metadata().is_ok_and(|meta| meta.len() > 1_000));
    }
}
