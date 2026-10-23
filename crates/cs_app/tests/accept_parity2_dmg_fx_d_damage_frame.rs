//! Acceptance for Rally #1808 `PARITY2-DMG-FX-D`: the owner-command smoke's
//! damage phase hits the player through the session damage authority and
//! photographs the damaged airframe (`frame_*_damage_trail.png`), the frame
//! `docs/findings/2026-10-23-parity2-dmg-fx-d-damage-frame-beside-the-original.md`
//! sets beside the owner's `orig70.png`.
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
        .find(|check| check.id == "damage_trail_drawn")
        .expect("the smoke judges damage_trail_drawn");
    (check.applies, check.passed)
}

#[test]
fn accept_parity2_dmg_fx_d_the_judge_needs_a_hit_through_the_authority_and_drawn_puffs() {
    let drawn = DamageTrailMetrics {
        hit_applied: true,
        smoke_spawned: 10,
        fire_spawned: 20,
        passes_burning: 5,
        ..DamageTrailMetrics::default()
    };
    assert_eq!(verdict(drawn.clone()), (true, true));
    // No puffs after the hit: the frame would show an undamaged airframe.
    let mut quiet = drawn.clone();
    quiet.fire_spawned = 0;
    assert_eq!(verdict(quiet), (true, false));
    // The authority refused the hit: the check applies and fails by name.
    let refused = DamageTrailMetrics {
        refusal: Some("no session damage authority".to_owned()),
        ..DamageTrailMetrics::default()
    };
    assert_eq!(verdict(refused), (true, false));
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
fn accept_parity2_dmg_fx_d_the_windowed_smoke_photographs_the_damaged_airframe() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is unset");
    let capabilities = std::env::var("CS_CAPABILITIES").expect("CS_CAPABILITIES is unset");
    let engine_image = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE is unset");
    let dir = std::env::var("CS_DMG_FX_D_OUT").map_or_else(
        |_| std::env::temp_dir().join("parity2-dmg-fx-d"),
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
        .find("\"id\":\"damage_trail_drawn\"")
        .expect("the report judges damage_trail_drawn");
    assert!(
        report[at..].contains("\"passed\":true"),
        "the damage check passes: {}",
        &report[at..at + 400.min(report.len() - at)]
    );
    let photographed = std::fs::read_dir(&dir)
        .expect("the capture directory lists")
        .filter_map(|entry| entry.ok())
        .any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("frame_") && name.ends_with("_damage_trail.png")
        });
    assert!(photographed, "the damaged-airframe framebuffer arrived");
}
