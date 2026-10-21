//! PARITY3-CAMERA-MODES.04 (#1826): the owner-command smoke proves every
//! camera mode with frames.
//!
//! The judge half is pure and runs everywhere: [`evaluate_mission_smoke`]
//! reads the camera stage's samples. The windowed half, ignored in CI, drives
//! the exact owner command (`cs --cs-path <retail> --mission M01
//! --smoke-seconds <n> --capture-dir <dir>`) and asserts the camera frames
//! arrive and every camera check is judged.

use std::path::PathBuf;

use cs_app::mission_session::{
    CameraModeMetrics, CameraModeSample, Check, LookMode, MissionViewMode, SMOKE_REPORT_FILE,
    SmokeMeasurements, evaluate_mission_smoke,
};

fn sample(label: &str, view: MissionViewMode, offset: [f32; 3]) -> CameraModeSample {
    CameraModeSample {
        label: label.to_owned(),
        view,
        look_mode: LookMode::Snap,
        look_pitch: 0.0,
        look_yaw: 0.0,
        zoom_target: 0.0,
        zoom_smoothed: 0.0,
        offset_body_m: offset,
        distinct_luminance: Some(180),
    }
}

/// The camera stage of a healthy run, one sample per stage.
pub fn healthy_camera_modes() -> CameraModeMetrics {
    use MissionViewMode as V;
    let chase = |label, view, offset| sample(label, view, offset);
    let mut zoom_out = chase("zoom_out", V::Chase, [0.0, 3.0, 17.0]);
    zoom_out.zoom_target = 1.0;
    zoom_out.zoom_smoothed = 0.4;
    let mut zoom_in = chase("zoom_in", V::Chase, [0.0, 3.0, 13.5]);
    zoom_in.zoom_smoothed = 0.1;
    let mut up = chase("k_snap_look_up", V::Chase, [0.0, 12.0, 6.0]);
    up.look_pitch = 1.3;
    let mut side = chase("k_snap_look_side", V::Chase, [-10.0, 3.0, 8.0]);
    side.look_yaw = 1.4;
    let mut smooth = chase("j_smooth_look", V::Chase, [8.0, 3.0, 10.0]);
    smooth.look_mode = LookMode::Smooth;
    smooth.look_yaw = -1.1;
    let mut track = chase("l_track_target", V::Chase, [0.0, 3.0, 13.0]);
    track.look_mode = LookMode::Track;
    let mut eye = chase("eye_snap_look_side", V::Cockpit, [0.0, 0.7, -2.0]);
    eye.look_yaw = 1.4;
    CameraModeMetrics {
        samples: vec![
            chase("chase", V::Chase, [0.0, 3.0, 13.0]),
            chase("f7_held_point", V::HeldPoint, [0.0, 3.0, 13.0]),
            chase("f9_down", V::ExternalDown, [0.0, 12.0, 4.0]),
            chase("f10_back", V::ExternalBack, [0.0, 3.0, 15.5]),
            chase("f11_left", V::ExternalLeft, [-13.0, 0.0, 0.0]),
            chase("f12_right", V::ExternalRight, [13.0, 0.0, 0.0]),
            chase("f8_remembered_eye", V::SecondEye, [0.0, 0.7, -2.0]),
            chase("f8_back_to_chase", V::Chase, [0.0, 3.0, 13.0]),
            zoom_out,
            zoom_in,
            up,
            side,
            smooth,
            track,
            eye,
        ],
    }
}

fn judged(metrics: CameraModeMetrics) -> Vec<Check> {
    evaluate_mission_smoke(&SmokeMeasurements {
        mission: "M01".to_owned(),
        camera_modes: metrics,
        ..SmokeMeasurements::default()
    })
    .into_iter()
    .filter(|check| check.id.starts_with("camera_"))
    .collect()
}

fn failing(checks: &[Check]) -> Vec<&str> {
    checks
        .iter()
        .filter(|check| check.applies && !check.passed)
        .map(|check| check.id.as_str())
        .collect()
}

#[test]
fn accept_parity3_camera_modes_04_the_judge_passes_a_run_that_showed_every_mode() {
    let checks = judged(healthy_camera_modes());
    assert!(failing(&checks).is_empty(), "{:?}", failing(&checks));
    for id in [
        "camera_f7_held_point",
        "camera_f9_down",
        "camera_f10_back",
        "camera_f11_left",
        "camera_f12_right",
        "camera_zoom_out",
        "camera_zoom_in",
        "camera_k_snap_look_up",
        "camera_j_smooth_look",
        "camera_l_track_target",
        "camera_external_views_differ",
    ] {
        assert!(checks.iter().any(|check| check.id == id), "no {id} check");
    }
}

#[test]
fn accept_parity3_camera_modes_04_a_run_without_the_stage_fails_every_mode_by_name() {
    let checks = judged(CameraModeMetrics::default());
    let failed = failing(&checks);
    assert!(failed.contains(&"camera_f9_down"), "{failed:?}");
    assert!(
        failed.contains(&"camera_external_views_differ"),
        "{failed:?}"
    );
    assert_eq!(failed.len(), checks.len(), "nothing passes unmeasured");
}

fn mutate(label: &str, edit: impl FnOnce(&mut CameraModeSample)) -> Vec<String> {
    let mut metrics = healthy_camera_modes();
    let sample = metrics
        .samples
        .iter_mut()
        .find(|sample| sample.label == label)
        .expect("the healthy fixture has the stage");
    edit(sample);
    failing(&judged(metrics))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

#[test]
fn accept_parity3_camera_modes_04_the_judge_fails_what_the_owner_would_see_wrong() {
    assert_eq!(
        mutate("f10_back", |s| s.view = MissionViewMode::Chase),
        ["camera_f10_back"]
    );
    assert_eq!(
        mutate("f11_left", |s| s.distinct_luminance = Some(1)),
        ["camera_f11_left"],
        "a flat frame is not a picture"
    );
    assert_eq!(
        mutate("f12_right", |s| s.distinct_luminance = None),
        ["camera_f12_right"],
        "an undelivered frame is not a picture"
    );
    assert_eq!(
        mutate("zoom_out", |s| {
            s.zoom_smoothed = 0.0;
            s.offset_body_m = [0.0, 3.0, 13.0];
        }),
        ["camera_zoom_out", "camera_zoom_in"],
        "no zoom shows in the camera (and zoom-in has nothing to be below)"
    );
    assert_eq!(
        mutate("k_snap_look_up", |s| s.look_pitch = 0.0),
        ["camera_k_snap_look_up"]
    );
    assert_eq!(
        mutate("j_smooth_look", |s| s.look_mode = LookMode::Snap),
        ["camera_j_smooth_look"]
    );
    assert_eq!(
        mutate("l_track_target", |s| s.look_mode = LookMode::Snap),
        ["camera_l_track_target"]
    );
    assert_eq!(
        mutate("f9_down", |s| s.offset_body_m = [0.0, 3.0, 13.0]),
        ["camera_external_views_differ"],
        "an F9 camera standing where the chase camera stands is not another view"
    );
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

/// **The owner command, with every camera mode photographed.**
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_parity3_camera_modes_04_the_owner_command_photographs_every_camera_mode() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is unset");
    let capabilities = std::env::var("CS_CAPABILITIES").expect("CS_CAPABILITIES is unset");
    for capability in ["gpu", "audio"] {
        assert!(
            capabilities
                .split(',')
                .any(|part| part.trim() == capability),
            "CS_CAPABILITIES must list {capability}: {capabilities}"
        );
    }
    let dir = std::env::var("CS_M01_CAMERA_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("m01-camera-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let mut command = std::process::Command::new(mission_bin());
    command
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .args(["--smoke-seconds", "60", "--capture-dir"])
        .arg(&dir)
        .env("CS_CAPABILITIES", capabilities);
    if let Ok(image) = std::env::var("CS_ENGINE_IMAGE") {
        command.env("CS_ENGINE_IMAGE", image);
    }
    let output = command.output().expect("the cs binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report = std::fs::read_to_string(dir.join(SMOKE_REPORT_FILE))
        .unwrap_or_else(|error| panic!("{SMOKE_REPORT_FILE} is written: {error}\n{stderr}"));
    for stage in [
        "chase",
        "f7_held_point",
        "f9_down",
        "f10_back",
        "f11_left",
        "f12_right",
        "f8_remembered_eye",
        "f8_back_to_chase",
        "zoom_out",
        "zoom_in",
        "k_snap_look_up",
        "k_snap_look_side",
        "j_smooth_look",
        "l_track_target",
        "eye_snap_look_side",
    ] {
        assert!(
            report.contains(&format!("\"id\":\"camera_{stage}\"")),
            "the report judges camera_{stage}: {report}"
        );
        let frame = std::fs::read_dir(&dir)
            .expect("the capture directory lists")
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with(&format!("_cam_{stage}.png"))
            });
        assert!(frame, "a framebuffer for {stage} is written in {dir:?}");
    }
    assert!(
        report.contains("\"id\":\"camera_external_views_differ\""),
        "the report judges the views' separation"
    );
    let camera_failures: Vec<&str> = report
        .split("\"id\":\"camera_")
        .skip(1)
        .filter(|rest| {
            let end = rest.find('}').unwrap_or(rest.len());
            rest[..end].contains("\"passed\":false")
        })
        .collect();
    assert!(
        camera_failures.is_empty(),
        "camera checks failed: {camera_failures:?}\n{stderr}"
    );
    println!("camera smoke frames in {}", dir.display());
}
