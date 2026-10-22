//! Acceptance for PARITY4-LENS-FLARE-C (#1860): the owner-command smoke's
//! sun-aim step. Task test prefix: `accept_parity4_lens_flare_c_`.
//!
//! The unit half lives in `crates/cs_app/src/mission_session/smoke.rs` (the
//! aim solve, the comparison probe, the phase walk and the check judge). The
//! file's own test is the ignored windowed run: the owner's own command
//! `cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir
//! <dir>` freezes the flight on the `Menu` hold (never the `PlayerRequest`
//! hold, so the Esc map stays shut), aims the nose so the camera-anchored
//! `sun` sits on the owner capture's frame fraction, and writes `lens_flare`
//! and `lens_flare_hidden` framebuffer PNGs the report's per-quad comparison
//! is measured from. It needs the installation, the GPU and the audio
//! device.

use std::path::PathBuf;

use cs_app::mission_session::{
    MIN_FLARE_QUAD_PIXELS, SMOKE_KIND, SMOKE_REPORT_FILE, SUN_SCREEN_FRACTION,
};

/// The windowed binary the ignored run drives: `CS_MISSION_BIN` when the
/// operator names one, else the release build, else the test harness's own
/// binary — the same pick `accept_m01_owner_smoke` makes.
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

/// **The windowed run photographs the sun and its flare chain.**
///
/// The smoke's own `lens_flare` object is the assertion surface: the source
/// is the `sun` node's stored light, the aimed placement was written, the
/// measured sun position is the designed fraction of the viewport (the
/// composition of the owner's capture), all four `lflare` quads drew, and
/// every one added real pixels over the quads-hidden comparison frame.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_parity4_lens_flare_c_the_windowed_smoke_photographs_the_sun_and_its_rings() {
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
    let dir = std::env::var("CS_LENS_FLARE_C_SMOKE_OUT").map_or_else(
        |_| std::env::temp_dir().join("parity4-lens-flare-c-smoke"),
        PathBuf::from,
    );
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the capture directory is created");
    let output = std::process::Command::new(mission_bin())
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .args(["--smoke-seconds", "45", "--capture-dir"])
        .arg(&dir)
        .env("CS_CAPABILITIES", &capabilities)
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
        "lens_flare_sun_in_view",
        "lens_flare_quads_drawn",
        "smoke_shots_delivered",
    ] {
        assert!(
            report.contains(&format!("\"id\":\"{id}\"")),
            "the report judges {id}: {report}"
        );
    }
    // The phase's own serialisation: the sun source, the written placement,
    // the measured screen position on the designed fraction, all four quads
    // drawn, the raw pair delivered and every quad over its pixel floor.
    let flare = report
        .split_once("\"lens_flare\":{")
        .and_then(|(_, rest)| {
            rest.split_once("]},\"actors\"")
                .map(|(block, _)| block.to_owned())
        })
        .unwrap_or_else(|| panic!("the report serialises the lens_flare measure: {report}"));
    assert!(
        flare.contains("\"source\":\"sun\""),
        "the aimed source is the sun node: {flare}"
    );
    assert!(
        flare.contains("\"placed\":true"),
        "the aimed placement was written: {flare}"
    );
    assert!(
        flare.contains("\"frames_delivered\":true"),
        "the raw frame pair arrived: {flare}"
    );
    assert_eq!(
        flare.matches("\"drawn\":true").count(),
        4,
        "all four lflare quads drew: {flare}"
    );
    for quad in flare.split('{').skip(1) {
        let pixels: Option<usize> = quad.split_once("\"pixels_added\":").and_then(|(_, rest)| {
            rest.trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        });
        assert!(
            pixels.is_some_and(|added| added >= MIN_FLARE_QUAD_PIXELS),
            "each quad added at least {MIN_FLARE_QUAD_PIXELS} pixels over the hidden frame: {flare}"
        );
    }
    // The sun sat where the aim put it: the designed fraction of the
    // viewport, the composition of the owner's capture.
    let sun_px = flare
        .split_once("\"sun_px\":[")
        .and_then(|(_, rest)| rest.split_once(']').map(|(pair, _)| pair.to_owned()))
        .and_then(|pair| {
            let mut parts = pair.split(',').map(|part| part.parse::<f32>());
            Some((parts.next()?.ok()?, parts.next()?.ok()?))
        })
        .expect("the report names the sun's measured position");
    let viewport = flare
        .split_once("\"viewport_px\":[")
        .and_then(|(_, rest)| rest.split_once(']').map(|(pair, _)| pair.to_owned()))
        .and_then(|pair| {
            let mut parts = pair.split(',').map(|part| part.parse::<f32>());
            Some((parts.next()?.ok()?, parts.next()?.ok()?))
        })
        .expect("the report names the viewport the sun was measured in");
    let fraction = (sun_px.0 / viewport.0, sun_px.1 / viewport.1);
    assert!(
        (fraction.0 - SUN_SCREEN_FRACTION[0]).abs() <= 0.05
            && (fraction.1 - SUN_SCREEN_FRACTION[1]).abs() <= 0.05,
        "the sun sat at {fraction:?} of {viewport:?}, not {SUN_SCREEN_FRACTION:?}"
    );
    // The framebuffer pair itself.
    let png = |label: &str| -> PathBuf {
        std::fs::read_dir(&dir)
            .expect("the capture directory lists")
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().ends_with(&format!("_{label}.png")))
            })
            .unwrap_or_else(|| panic!("a frame_*_{label}.png exists in {}", dir.display()))
    };
    for label in ["lens_flare", "lens_flare_hidden"] {
        let path = png(label);
        assert!(
            path.metadata().is_ok_and(|meta| meta.len() > 0),
            "{} is a framebuffer PNG: {}",
            label,
            path.display()
        );
    }
    let passed = report
        .rfind("\"passed\":")
        .is_some_and(|index| report[index..].starts_with("\"passed\":true"));
    assert_eq!(
        output.status.success(),
        passed,
        "the process exits non-zero exactly when a check failed; stderr: {stderr}"
    );
    println!(
        "lens-flare smoke artifacts in {}: the sun sat at {fraction:?}, report passed = {passed}",
        dir.display()
    );
}
