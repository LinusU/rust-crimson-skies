//! Acceptance scenario F24-C for the named handling profiles: the declared
//! producer selects a profile explicitly, so an improved record is never
//! silently the fidelity profile a reference trace is compared against.
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stage `### F24-C`. Task test prefix: `accept_f24_c_`.
//!
//! These tests use only [`cs_content`]'s public API, so they fail to compile if
//! the profile producer is removed and fail at run time if an undeclared profile
//! starts falling back to the fidelity record.

use cs_content::flight_tuning::{
    FIDELITY_PROFILE, IMPROVED_PROFILE, TuningSchema, declared_synthetic_airframe,
    declared_synthetic_airframe_for, declared_synthetic_improved_airframe,
};
use cs_types::content::Origin;

/// The declared producer answers only for the named profiles and refuses an
/// unknown label instead of falling back to fidelity.
#[test]
fn accept_f24_c_profile_selection_is_explicit_and_never_a_fallback() {
    let schema = TuningSchema::fixed_wing();

    let fidelity = declared_synthetic_airframe_for(FIDELITY_PROFILE)
        .expect("the fidelity profile is declared");
    assert_eq!(fidelity.profile, FIDELITY_PROFILE);
    assert_eq!(fidelity.validate(&schema), Ok(()));
    assert!(!fidelity.assists_enabled);

    let improved = declared_synthetic_airframe_for(IMPROVED_PROFILE)
        .expect("the improved profile is declared");
    assert_eq!(improved.profile, IMPROVED_PROFILE);
    assert_eq!(improved.validate(&schema), Ok(()));
    assert!(improved.assists_enabled);

    assert_ne!(fidelity, improved);
    assert_eq!(declared_synthetic_airframe_for("autogyro"), None);
    assert_eq!(declared_synthetic_airframe(), fidelity);
}

/// The improved record changes the controller and assist only: the airframe
/// fixtures (mass, engine, drag, lift, area) stay the synthetic airframe's, and
/// the record is explicitly synthetic, never an original airframe.
#[test]
fn accept_f24_c_improved_profile_changes_only_handling() {
    let schema = TuningSchema::fixed_wing();
    let fidelity = declared_synthetic_airframe();
    let improved = declared_synthetic_improved_airframe();

    for field in [
        "mass.mass_kg",
        "mass.inertia_kg_m2[0]",
        "engine.max_thrust_n",
        "drag.zero_lift_coefficient",
        "lift.lift_slope_per_rad",
        "reference_area_m2",
    ] {
        assert_eq!(
            fidelity.known_value(field),
            improved.known_value(field),
            "{field} must not change between profiles"
        );
    }

    assert_ne!(
        fidelity.known_value("angular.rate_gain_per_s"),
        improved.known_value("angular.rate_gain_per_s")
    );
    assert_eq!(improved.validate(&schema), Ok(()));
    assert_eq!(improved.origin, Origin::SyntheticFixture);
    assert!(!improved.origin.is_original());
    assert_eq!(
        improved.known_value("assists.bank_level_gain_nm_per_rad"),
        Some(8_000.0)
    );
}
