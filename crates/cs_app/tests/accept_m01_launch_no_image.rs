//! M01-LAUNCH-NO-IMAGE-REGRESSION (#1534): `cs --cs-path <retail> --mission
//! M01` launches with only `CS_CAPABILITIES=retail,gpu,audio`. The early
//! environment check of #1431 once refused whenever `CS_ENGINE_IMAGE` was
//! unset, before the committed owner-static table (#1432) could apply.

use std::process::Command;

use cs_app::mission_launch::LaunchPreflightError;

/// The preflight alone: an unset or empty image passes, a set but missing
/// one is still refused, and a missing `audio` capability is still exit 4.
#[test]
fn accept_m01_launch_no_image_preflight_does_not_require_the_image() {
    use std::ffi::OsStr;
    LaunchPreflightError::check(None, Some("retail,gpu,audio")).expect("unset image passes");
    LaunchPreflightError::check(Some(OsStr::new("")), Some("retail,gpu,audio"))
        .expect("empty image passes");
    let missing = LaunchPreflightError::check(
        Some(OsStr::new("/nonexistent/crimson.decrypted.exe")),
        Some("retail,gpu,audio"),
    )
    .expect_err("a named but missing image is refused");
    assert!(
        missing.action.starts_with("point CS_ENGINE_IMAGE"),
        "{missing}"
    );
    let mute = LaunchPreflightError::check(None, Some("retail,gpu")).expect_err("no audio");
    assert_eq!(mute.exit_code, 4);
}

/// The real binary, `CS_ENGINE_IMAGE` removed, on the owner's installation.
/// A start override without `CS_MISSION_CAPTURE=1` is refused by
/// `run_windowed` before any window opens, but only after the environment
/// check, the closure measurement and the launchable verdict have all passed
/// — so that refusal, and not an image or closure refusal, is the expected
/// outcome.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_launch_no_image_binary_gets_past_the_launch_checks() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set");
    let output = Command::new(env!("CARGO_BIN_EXE_cs"))
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .env_remove("CS_ENGINE_IMAGE")
        .env_remove("CS_MISSION_CAPTURE")
        .env("CS_CAPABILITIES", "retail,gpu,audio")
        .env("CS_MISSION_START_M", "0,0,0")
        .env("CS_MISSION_START_HEADING_RAD", "0")
        .output()
        .expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stderr}");
    assert!(
        stderr.contains("needs CS_MISSION_CAPTURE=1"),
        "the launch must reach the windowed composition: {stderr}"
    );
    assert!(!stderr.contains("set CS_ENGINE_IMAGE"), "{stderr}");
    assert!(!stderr.contains("cannot launch"), "{stderr}");
}
