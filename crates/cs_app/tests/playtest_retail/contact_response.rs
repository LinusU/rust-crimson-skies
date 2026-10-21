//! Task #1849 acceptance tests. Prefix: `accept_parity4_m01_contact_`.
//!
//! The retail aircraft carries the contact coefficients the original's response
//! uses (`bounce_factor` of the `player.zrd` `crash` record as restitution, no
//! friction), and a body with them rebounds at that fraction of its approach
//! speed instead of Avian's default (none). The retail half is `#[ignore]`d
//! (`requires CS_GAME_DIR`) and fails without it.

use std::path::PathBuf;

use avian3d::prelude::{CoefficientCombine, Friction, Restitution};
use bevy::prelude::{App, With};
use cs_app::playtest::headless_app_with;
use cs_app::playtest::retail::{self, RetailFlight, RetailRequest};
use cs_app::playtest::scene::{PlaytestAircraft, contact_coefficients};

#[test]
fn accept_parity4_m01_contact_coefficients_are_bounce_and_no_friction() {
    let (restitution, friction) = contact_coefficients(0.6);
    assert_eq!(restitution.coefficient, 0.6);
    assert_eq!(restitution.combine_rule, CoefficientCombine::Max);
    assert_eq!(friction.dynamic_coefficient, 0.0);
    assert_eq!(friction.static_coefficient, 0.0);
    assert_eq!(friction.combine_rule, CoefficientCombine::Min);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity4_m01_contact_retail_aircraft_carries_the_crash_record_bounce() {
    let install = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    );
    let request =
        RetailRequest::new(install, None, None).expect("the documented defaults are valid");
    let sources = retail::read_sources(&request).expect("the installation reads");
    let mut app: App = headless_app_with(|app| {
        retail::install(app, &sources, &request).expect("the original area installs");
    });
    app.update();
    let bounce = app.world().resource::<RetailFlight>().crash.bounce_factor;
    assert_eq!(bounce, f64::from(0.6_f32));
    let world = app.world_mut();
    let (restitution, friction) = world
        .query_filtered::<(&Restitution, &Friction), With<PlaytestAircraft>>()
        .single(world)
        .expect("the retail aircraft carries the coefficients");
    assert_eq!(f64::from(restitution.coefficient), bounce);
    assert_eq!(friction.dynamic_coefficient, 0.0);
}
