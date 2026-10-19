//! Task #1672 `M01-LC-FC-D`: the owner-command smoke passes through the
//! Flight Check. These tests drive the check judge and the region measurement
//! without a GPU; the run through the real window is the ignored test at the
//! bottom and the review's `cs … --mission M01 --smoke-seconds 45` run.
//! Task test prefix: `accept_m01_lc_fc_d_`.

use cs_app::mission_session::{
    Check, FlightCheckButtonMetrics, FlightCheckMetrics, FlightCheckTextMetrics,
    MIN_FLIGHT_CHECK_BUTTON_PIXELS, MIN_FLIGHT_CHECK_DRAWN_PERMILLE, MIN_FLIGHT_CHECK_TEXT_PIXELS,
    SmokeMeasurements, evaluate_mission_smoke, region_difference,
};

fn text(row: &str, pixels: usize) -> FlightCheckTextMetrics {
    FlightCheckTextMetrics {
        row: row.to_owned(),
        expected: format!("{row} text"),
        drawn: Some(format!("{row} text")),
        visible: true,
        region: [10, 10, 100, 20],
        pixels,
    }
}

fn button(hotspot: &str, pixels: usize) -> FlightCheckButtonMetrics {
    FlightCheckButtonMetrics {
        hotspot: hotspot.to_owned(),
        drawn: true,
        region: [500, 500, 80, 20],
        pixels,
    }
}

fn healthy() -> FlightCheckMetrics {
    FlightCheckMetrics {
        frames_delivered: true,
        frame_size: (2560, 1440),
        drawn_permille: 760,
        texts: vec![text("FC_T_MISSION", 300), text("FC_T_OBJECTIVES", 900)],
        buttons: vec![
            button("FC_B_FLYMISSION", 700),
            button("FC_B_RETURNBRIEF", 650),
        ],
        launched: true,
        refusal: None,
    }
}

fn flight_check_verdicts(metrics: Option<FlightCheckMetrics>) -> Vec<Check> {
    let measurements = SmokeMeasurements {
        flight_check: metrics,
        ..SmokeMeasurements::default()
    };
    evaluate_mission_smoke(&measurements)
        .into_iter()
        .filter(|check| check.id.starts_with("flight_check_"))
        .collect()
}

fn failed(checks: &[Check]) -> Vec<&str> {
    checks
        .iter()
        .filter(|check| check.applies && !check.passed)
        .map(|check| check.id.as_str())
        .collect()
}

#[test]
fn accept_m01_lc_fc_d_a_drawn_flight_check_passes_all_four_checks() {
    let checks = flight_check_verdicts(Some(healthy()));
    let ids: Vec<&str> = checks.iter().map(|check| check.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "flight_check_screen_drawn",
            "flight_check_text_matches",
            "flight_check_buttons_drawn",
            "flight_check_fly_mission"
        ]
    );
    assert!(failed(&checks).is_empty(), "{checks:?}");
}

#[test]
fn accept_m01_lc_fc_d_each_way_the_screen_can_be_wrong_fails_its_own_check() {
    // A run that never reached the screen is four failures, never a skip.
    assert_eq!(failed(&flight_check_verdicts(None)).len(), 4);

    let mut black = healthy();
    black.drawn_permille = MIN_FLIGHT_CHECK_DRAWN_PERMILLE - 1;
    assert_eq!(
        failed(&flight_check_verdicts(Some(black))),
        ["flight_check_screen_drawn"]
    );

    // The drawn line differs from the view's.
    let mut wrong = healthy();
    wrong.texts[1].drawn = Some("something else".to_owned());
    assert_eq!(
        failed(&flight_check_verdicts(Some(wrong))),
        ["flight_check_text_matches"]
    );

    // The row is there but adds no pixels.
    let mut faint = healthy();
    faint.texts[0].pixels = MIN_FLIGHT_CHECK_TEXT_PIXELS - 1;
    assert_eq!(
        failed(&flight_check_verdicts(Some(faint))),
        ["flight_check_text_matches"]
    );

    // The objectives row is missing from the scene altogether.
    let mut missing = healthy();
    missing.texts.pop();
    assert_eq!(
        failed(&flight_check_verdicts(Some(missing))),
        ["flight_check_text_matches"]
    );

    let mut flat = healthy();
    flat.buttons[1].pixels = MIN_FLIGHT_CHECK_BUTTON_PIXELS - 1;
    assert_eq!(
        failed(&flight_check_verdicts(Some(flat))),
        ["flight_check_buttons_drawn"]
    );

    let mut refused = healthy();
    refused.launched = false;
    refused.refusal = Some("no loadout".to_owned());
    let checks = flight_check_verdicts(Some(refused));
    assert_eq!(failed(&checks), ["flight_check_fly_mission"]);
    assert!(checks[3].detail.contains("no loadout"));

    let mut lost = healthy();
    lost.frames_delivered = false;
    assert_eq!(
        failed(&flight_check_verdicts(Some(lost))),
        [
            "flight_check_screen_drawn",
            "flight_check_text_matches",
            "flight_check_buttons_drawn"
        ]
    );
}

#[test]
fn accept_m01_lc_fc_d_the_region_difference_counts_only_changed_pixels_inside() {
    let (width, height) = (8_u32, 4_u32);
    let bare = vec![0x20_u8; (width * height * 4) as usize];
    let mut full = bare.clone();
    // Two pixels inside the region, one outside, one inside that changed by
    // less than the floor, one inside whose only change is its alpha.
    for (x, y, delta) in [(2, 1, 0x80_u8), (3, 1, 0x80), (7, 3, 0x80), (4, 1, 0x04)] {
        full[((y * width + x) * 4) as usize + 1] = 0x20 + delta;
    }
    full[((2 * width + 2) * 4) as usize + 3] = 0xFF;
    assert_eq!(
        region_difference(&full, &bare, width, height, [2, 1, 4, 2]),
        2
    );
    assert_eq!(
        region_difference(&full, &bare, width, height, [0, 0, 8, 4]),
        3
    );
    // A region off the frame is clamped, not a panic.
    assert_eq!(
        region_difference(&full, &bare, width, height, [6, 2, 100, 100]),
        1
    );
    assert_eq!(
        region_difference(&full, &bare, width, height, [50, 50, 4, 4]),
        0
    );
}
