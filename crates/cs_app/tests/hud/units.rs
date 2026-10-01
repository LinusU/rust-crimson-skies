use cs_app::ui::hud::{Hud, HudError};
use cs_content::hud::{
    AltitudeDatum, AltitudeUnit, Declared, HudPolicy, HudPolicyError, SpeedReference, SpeedUnit,
};

use crate::{actor, level_sample, session};

fn hud(policy: HudPolicy) -> Hud {
    let mut hud = Hud::new(policy).unwrap();
    hud.bind(session(1), actor(1, 1)).unwrap();
    hud
}

#[test]
fn accept_f46_a_speeds_convert_from_si_and_split_air_from_ground() {
    let mut policy = HudPolicy::designed();
    policy.speed_unit = Declared::designed(SpeedUnit::KilometersPerHour);
    let mut h = hud(policy);
    let mut sample = level_sample(1, 1);
    // 30 m/s west over the ground while climbing at 40 m/s, in a 30 m/s west wind.
    sample.velocity_mps = [-30.0, 40.0, 0.0];
    sample.wind_mps = [-30.0, 0.0, 0.0];
    let out = h.project(&sample).unwrap();
    assert!(
        (out.ground_speed - 30.0 * 3.6).abs() < 1e-9,
        "horizontal only"
    );
    assert!(
        (out.air_speed - 40.0 * 3.6).abs() < 1e-9,
        "wind removed, climb kept"
    );
    assert_eq!(out.gauge_speed, out.air_speed);

    let mut policy = HudPolicy::designed();
    policy.airspeed_reference = Declared::designed(SpeedReference::Ground);
    let out = hud(policy).project(&sample).unwrap();
    assert_eq!(out.gauge_speed, out.ground_speed);
}

#[test]
fn accept_f46_a_altitude_uses_the_declared_datum_and_unit() {
    let mut sample = level_sample(1, 1);
    sample.height_m = 1000.0;
    sample.ground_height_m = Some(400.0);

    let out = hud(HudPolicy::designed()).project(&sample).unwrap();
    assert!((out.altitude - 600.0).abs() < 1e-9, "terrain datum");

    let mut policy = HudPolicy::designed();
    policy.altitude_datum = Declared::designed(AltitudeDatum::WorldOrigin);
    policy.altitude_unit = Declared::designed(AltitudeUnit::Feet);
    let out = hud(policy).project(&sample).unwrap();
    assert!(
        (out.altitude - 1000.0 / 0.3048).abs() < 1e-9,
        "origin datum in feet"
    );

    sample.ground_height_m = None;
    assert_eq!(
        hud(HudPolicy::designed()).project(&sample).unwrap_err(),
        HudError::MissingGroundHeight
    );
}

#[test]
fn accept_f46_a_low_altitude_warning_has_hysteresis() {
    let mut h = hud(HudPolicy::designed()); // warn < 100 m, clear > 150 m
    let mut at = |height: f64| {
        let mut s = level_sample(1, 1);
        s.height_m = height;
        h.project(&s).unwrap().low_altitude_warning
    };
    assert!(!at(500.0));
    assert!(at(99.0), "sets below warn");
    assert!(at(120.0), "stays set between thresholds");
    assert!(!at(151.0), "clears above clear");
    assert!(!at(120.0), "stays clear between thresholds");
}

#[test]
fn accept_f46_a_policy_rejects_chatter_and_bad_thresholds_and_tracks_basis() {
    let mut policy = HudPolicy::designed();
    assert!(
        !policy.fully_verified(),
        "designed defaults are never verified"
    );
    policy.low_altitude_clear_m = Declared::designed(100.0);
    assert!(matches!(
        Hud::new(policy.clone()).unwrap_err(),
        HudError::Policy(HudPolicyError::NoHysteresis { .. })
    ));
    policy.low_altitude_warn_m = Declared::designed(f64::NAN);
    assert!(matches!(
        policy.validate().unwrap_err(),
        HudPolicyError::BadThreshold { .. }
    ));
}

#[test]
fn accept_f46_a_non_finite_samples_are_refused() {
    let mut h = hud(HudPolicy::designed());
    let mut s = level_sample(1, 1);
    s.velocity_mps[1] = f64::INFINITY;
    assert_eq!(
        h.project(&s).unwrap_err(),
        HudError::NonFinite("velocity_mps")
    );
}
