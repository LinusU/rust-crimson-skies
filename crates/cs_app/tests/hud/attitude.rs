use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

use cs_app::ui::hud::attitude;
use cs_types::space::Quaternion;

use crate::{EPS, rot};

#[test]
fn accept_f46_a_level_nose_north_reads_flat_horizon_and_zero_heading() {
    let a = attitude(Quaternion::IDENTITY).unwrap();
    assert!(a.pitch.0.abs() < EPS);
    assert!(a.roll.unwrap().0.abs() < EPS);
    assert!(a.heading.unwrap().0.abs() < EPS);
}

#[test]
fn accept_f46_a_known_attitude_gives_expected_horizon_and_heading() {
    // Yaw 90 degrees right (about -Y): the nose points at +X, heading 90.
    let yaw = attitude(rot([0.0, -1.0, 0.0], FRAC_PI_2)).unwrap();
    assert!((yaw.heading.unwrap().0 - FRAC_PI_2).abs() < EPS);
    assert!(yaw.pitch.0.abs() < EPS);

    // Yaw left 45 degrees wraps to 315, not -45.
    let left = attitude(rot([0.0, 1.0, 0.0], FRAC_PI_4)).unwrap();
    assert!((left.heading.unwrap().0 - 7.0 * FRAC_PI_4).abs() < EPS);

    // Nose up 30 degrees (about +X).
    let up = attitude(rot([1.0, 0.0, 0.0], 30f64.to_radians())).unwrap();
    assert!((up.pitch.0 - 30f64.to_radians()).abs() < EPS);
    assert!(up.heading.unwrap().0.abs() < EPS);
    assert!(up.roll.unwrap().0.abs() < EPS);

    // Right bank 40 degrees (about forward, -Z): roll positive, nose unmoved.
    let bank = attitude(rot([0.0, 0.0, -1.0], 40f64.to_radians())).unwrap();
    assert!((bank.roll.unwrap().0 - 40f64.to_radians()).abs() < EPS);
    assert!(bank.pitch.0.abs() < EPS);
    assert!(bank.heading.unwrap().0.abs() < EPS);

    // Inverted: roll is +/-180, not 0.
    let inverted = attitude(rot([0.0, 0.0, -1.0], PI)).unwrap();
    assert!((inverted.roll.unwrap().0.abs() - PI).abs() < 1e-6);
}

#[test]
fn accept_f46_a_vertical_nose_has_no_heading_and_no_roll() {
    let climb = attitude(rot([1.0, 0.0, 0.0], FRAC_PI_2)).unwrap();
    assert!((climb.pitch.0 - FRAC_PI_2).abs() < EPS);
    assert!(climb.heading.is_none());
    assert!(climb.roll.is_none());
}
