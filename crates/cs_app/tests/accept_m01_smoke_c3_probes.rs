//! Task #1591 `M01-SMOKE-C3-PROBES` acceptance tests.
//! Prefix: `accept_m01_smoke_c3_probes_`.
//!
//! On ZBD/C3/M01 the owner smoke's `world_terrain_lit` sampled open water (the
//! frame's bottom third) and `hud_objective_matches` read no objective (the
//! probe queried `GlobalTransform`, which a UI node does not carry, and
//! compared the widget's caption-prefixed text with the bare model line).
//! These tests drive the pure measurement and the judge; the windowed half is
//! the ignored run at the bottom.

use std::path::PathBuf;

use cs_app::mission_session::{
    HudMetrics, MIN_OBJECTIVE_TEXT_PIXELS, MIN_TERRAIN_LUMINANCE_SPREAD, near_terrain_luminance,
    objective_line_of,
};
use cs_app::ui::hud::OBJECTIVE_HUD_CAPTION;

const W: usize = 256;
const H: usize = 192;

/// A synthetic probe frame: flat sea everywhere, plus a terrain block over the
/// given rows. `shaded` gives the block a checker texture spanning luminance
/// 40..120; otherwise it is one flat value.
struct Frame {
    luminance: Vec<u8>,
    detail: Vec<bool>,
}

fn frame(terrain_rows: std::ops::Range<usize>, shaded: bool) -> Frame {
    let mut luminance = vec![44_u8; W * H];
    let mut detail = vec![false; W * H];
    for y in terrain_rows {
        for x in 16..W - 16 {
            let index = y * W + x;
            if shaded {
                let high = (x / 2 + y / 2) % 2 == 0;
                luminance[index] = if high { 120 } else { 40 };
                detail[index] = true;
            } else {
                luminance[index] = 82;
            }
        }
    }
    Frame { luminance, detail }
}

fn spread(frame: &Frame) -> Option<u8> {
    near_terrain_luminance(W, &frame.luminance, &frame.detail).map(|(min, max)| max - min)
}

#[test]
fn accept_m01_smoke_c3_probes_terrain_far_up_the_frame_is_measured_not_the_sea_below() {
    // Terrain only in the upper half; the bottom third is flat sea (spread 0),
    // which is what the old bottom-third probe measured.
    let shaded = frame(40..90, true);
    let spread = spread(&shaded).expect("the frame shows terrain");
    assert!(
        spread >= MIN_TERRAIN_LUMINANCE_SPREAD,
        "shaded terrain over a sea must read as lit, spread {spread}"
    );
}

#[test]
fn accept_m01_smoke_c3_probes_unlit_terrain_fails() {
    // The same island with one flat value: no detail, so no terrain tile and
    // nothing to call lit.
    let flat = frame(40..90, false);
    assert!(
        spread(&flat).is_none_or(|spread| spread < MIN_TERRAIN_LUMINANCE_SPREAD),
        "an unlit flat island must not pass: {:?}",
        spread(&flat)
    );
}

#[test]
fn accept_m01_smoke_c3_probes_a_frame_of_only_sea_has_no_terrain_to_call_lit() {
    let sea = frame(0..0, true);
    assert_eq!(spread(&sea), None);
}

#[test]
fn accept_m01_smoke_c3_probes_a_coastline_alone_is_not_terrain() {
    // One textured row (a coastline's edge) is under a tile's detail share.
    let mut edge = frame(0..0, false);
    for x in 16..W - 16 {
        edge.luminance[100 * W + x] = 200;
        edge.detail[100 * W + x] = true;
    }
    assert_eq!(spread(&edge), None);
}

#[test]
fn accept_m01_smoke_c3_probes_the_nearest_terrain_wins_over_a_cloud_bank_above() {
    // A high-contrast textured bank at the top, sea between, and a dull
    // island lower (texture 80/90): the near field is the island, so the bank
    // must not rescue the check.
    let mut both = frame(8..40, true);
    for y in 120..170 {
        for x in 16..W - 16 {
            let index = y * W + x;
            both.luminance[index] = if (x / 2 + y / 2) % 2 == 0 { 90 } else { 80 };
            both.detail[index] = true;
        }
    }
    let near =
        near_terrain_luminance(W, &both.luminance, &both.detail).expect("the island is terrain");
    assert_eq!(near, (80, 90));
}

#[test]
fn accept_m01_smoke_c3_probes_the_widget_text_loses_its_caption() {
    let drawn = format!("{OBJECTIVE_HUD_CAPTION}\n1) Find the main treasure site.");
    assert_eq!(objective_line_of(&drawn), "1) Find the main treasure site.");
    assert_eq!(objective_line_of("bare"), "bare");
    assert_eq!(objective_line_of(""), "");
}

#[test]
fn accept_m01_smoke_c3_probes_a_missing_objective_line_fails_the_judge() {
    use cs_app::mission_session::{SmokeMeasurements, evaluate_mission_smoke};
    let line = "1) Find the main treasure site.".to_owned();
    let judged = |hud: HudMetrics| {
        let mut m = SmokeMeasurements::default();
        m.approach.objective_woke = true;
        m.hud = hud;
        evaluate_mission_smoke(&m)
            .into_iter()
            .find(|check| check.id == "hud_objective_matches")
            .expect("the judge reports the check")
    };
    let drawn = judged(HudMetrics {
        model_line: Some(line.clone()),
        drawn_line: Some(line.clone()),
        drawn_visible: true,
        region_text_pixels: Some(MIN_OBJECTIVE_TEXT_PIXELS + 1),
        overlay_hidden: false,
    });
    assert!(drawn.passed, "{}", drawn.detail);
    let missing = judged(HudMetrics {
        model_line: Some(line),
        drawn_line: None,
        drawn_visible: false,
        region_text_pixels: None,
        overlay_hidden: false,
    });
    assert!(
        !missing.passed,
        "a current objective with no drawn line must fail"
    );
}

/// The windowed run on C3/M01: both fixed checks pass and name their evidence.
#[test]
#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed binary"]
fn accept_m01_smoke_c3_probes_the_windowed_smoke_passes_both_checks() {
    let game_dir = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is unset");
    let capabilities = std::env::var("CS_CAPABILITIES").expect("CS_CAPABILITIES is unset");
    let engine_image = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE is unset");
    let bin = std::env::var("CS_MISSION_BIN").map_or_else(
        |_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/release/cs"),
        PathBuf::from,
    );
    let dir = std::env::temp_dir().join("m01-smoke-c3-probes");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("capture directory");
    let output = std::process::Command::new(bin)
        .args(["--cs-path", &game_dir, "--mission", "M01"])
        .args(["--smoke-seconds", "45", "--capture-dir"])
        .arg(&dir)
        .env("CS_CAPABILITIES", capabilities)
        .env("CS_ENGINE_IMAGE", engine_image)
        .output()
        .expect("the cs binary runs");
    let report = std::fs::read_to_string(dir.join("report.json")).unwrap_or_else(|error| {
        panic!(
            "report.json is written: {error}\n{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    for id in ["world_terrain_lit", "hud_objective_matches"] {
        let needle = format!("\"id\":\"{id}\",\"applies\":true,\"passed\":true");
        assert!(report.contains(&needle), "{id} must pass: {report}");
    }
}
