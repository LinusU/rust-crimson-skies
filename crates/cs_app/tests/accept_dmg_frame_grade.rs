//! Acceptance for Rally #1966: the smoke's damage-phase frame is not dimmed.
//! The dimming was no grade of that phase: the flak phase's test-woken turret
//! kept bursting round the player, and every close burst starts a full-window
//! flicker overlay (alpha 0.2-0.4) that was still running at the shot. The
//! smoke now stands the turret down, and `damage_frame_ungraded` judges that
//! no flicker overlay is alive on a damage-phase shot frame.
//!
//! The synthetic test runs in CI against the smoke's production judge; the
//! windowed test needs `retail`, `gpu` and `audio` (`--include-ignored`).
//! Nothing here is `verified_original`.

use std::path::PathBuf;

use cs_app::mission_session::{
    DamageTrailMetrics, SMOKE_REPORT_FILE, SmokeMeasurements, evaluate_mission_smoke,
};

fn verdict(trail: DamageTrailMetrics) -> (bool, bool) {
    let checks = evaluate_mission_smoke(&SmokeMeasurements {
        damage_trail: trail,
        ..SmokeMeasurements::default()
    });
    let check = checks
        .iter()
        .find(|check| check.id == "damage_frame_ungraded")
        .expect("the smoke judges damage_frame_ungraded");
    (check.applies, check.passed)
}

#[test]
fn accept_dmg_frame_grade_the_judge_fails_a_frame_under_a_flak_flicker() {
    let clean = DamageTrailMetrics {
        hit_applied: true,
        flak_stood_down: true,
        ..DamageTrailMetrics::default()
    };
    assert_eq!(verdict(clean.clone()), (true, true));
    let tinted = DamageTrailMetrics {
        flak_flickers_at_shots: 1,
        ..clean
    };
    assert_eq!(verdict(tinted), (true, false));
    // No damage phase, no judgement.
    assert_eq!(verdict(DamageTrailMetrics::default()), (false, true));
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

#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_dmg_frame_grade_the_windowed_smoke_frame_has_no_flak_flicker() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is unset");
    let capabilities = std::env::var("CS_CAPABILITIES").expect("CS_CAPABILITIES is unset");
    let engine_image = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE is unset");
    let dir = std::env::var("CS_DMG_FRAME_GRADE_OUT").map_or_else(
        |_| std::env::temp_dir().join("dmg-frame-grade"),
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
    let report = std::fs::read_to_string(dir.join(SMOKE_REPORT_FILE)).unwrap_or_else(|error| {
        panic!(
            "report is written: {error}\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let at = report
        .find("\"id\":\"damage_frame_ungraded\"")
        .expect("the report judges damage_frame_ungraded");
    assert!(
        report[at..].contains("\"passed\":true"),
        "no flicker tints the damage frame: {}",
        &report[at..at + 400.min(report.len() - at)]
    );
}
