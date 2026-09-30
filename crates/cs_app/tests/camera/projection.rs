//! F21-A: lowering the declared projection policy and the vertical vs
//! horizontal field-of-view conversion.
//!
//! These tests call `cs_app::camera::lower_projection` and the lowered
//! record's own conversion methods. The vertical↔horizontal formulas are the
//! standard perspective identity, re-derived here as an independent oracle;
//! removing the conversion or a refusal fails the test.

use std::f64::consts::PI;

use cs_app::camera::{ProjectionLowerError, lower_projection};
use cs_content::cameras::{
    AspectFraming, AspectRatio, CameraModeKind, FovAxis, ProjectionPolicy,
    declared_synthetic_camera_modes,
};
use cs_types::content::Resolved;
use cs_types::space::{Meters, Radians};

use crate::common::{assert_close, claim, known};

fn policy(fov: f64, axis: FovAxis) -> ProjectionPolicy {
    ProjectionPolicy {
        fov: known(Radians(fov)),
        fov_axis: known(axis),
        reference_aspect: known(AspectRatio::FOUR_THREE),
        framing: known(AspectFraming::PreserveVertical),
        near_m: known(Meters(0.1)),
        far_m: known(Meters(10_000.0)),
    }
}

fn unknown<T>(reason: &str) -> Resolved<T> {
    Resolved::unknown(claim(), reason).expect("nonempty reason")
}

fn horizontal_from_vertical(vertical: f64, aspect: f64) -> f64 {
    2.0 * ((vertical * 0.5).tan() * aspect).atan()
}

fn vertical_from_horizontal(horizontal: f64, aspect: f64) -> f64 {
    2.0 * ((horizontal * 0.5).tan() / aspect).atan()
}

/// The fixture cockpit lowers unchanged: a 60° vertical field of view at
/// 4:3, preserve-vertical framing and its own clipping planes.
#[test]
fn accept_f21_a_lower_projection_keeps_the_declared_vertical_policy() {
    let modes = declared_synthetic_camera_modes();
    let cockpit = modes
        .get(CameraModeKind::Cockpit)
        .expect("cockpit declared");
    let lowered = lower_projection(cockpit.projection()).expect("the fixture cockpit lowers");

    assert_close(lowered.vertical_fov().0, PI / 3.0, 1e-12, "vertical FOV");
    assert_eq!(lowered.reference_aspect(), AspectRatio::FOUR_THREE);
    assert_eq!(lowered.framing_policy(), AspectFraming::PreserveVertical);
    assert_close(lowered.near_m().0, 0.1, 1e-12, "near plane");
    assert_close(lowered.far_m().0, 10_000.0, 1e-12, "far plane");
}

/// A horizontal declaration is normalized to the vertical axis at the
/// reference aspect, and the conversion is its own inverse.
#[test]
fn accept_f21_a_lower_projection_converts_horizontal_to_vertical_and_back() {
    let reference = AspectRatio::FOUR_THREE.value();
    let vertical = PI / 3.0;
    let horizontal = horizontal_from_vertical(vertical, reference);
    let lowered = lower_projection(&policy(horizontal, FovAxis::Horizontal))
        .expect("a horizontal declaration lowers");

    assert_close(
        lowered.vertical_fov().0,
        vertical,
        1e-12,
        "horizontal declaration normalized to vertical",
    );
    assert_close(
        lowered.horizontal_fov_at(AspectRatio::FOUR_THREE).0,
        horizontal,
        1e-12,
        "the authored horizontal FOV round-trips at the reference aspect",
    );

    // The same identity the other way: vertical in, horizontal out.
    let widened = lower_projection(&policy(vertical, FovAxis::Vertical)).expect("lowers");
    assert_close(
        widened.horizontal_fov_at(AspectRatio::FOUR_THREE).0,
        horizontal_from_vertical(vertical, reference),
        1e-12,
        "vertical-to-horizontal conversion",
    );
    // And the inverse holds to round-trip precision.
    assert_close(
        vertical_from_horizontal(
            widened.horizontal_fov_at(AspectRatio::SIXTEEN_NINE).0,
            16.0 / 9.0,
        ),
        vertical,
        1e-12,
        "vertical -> horizontal -> vertical is the identity",
    );
}

/// Every unknown refuses by field name, and a declared stretch framing rule
/// is refused outright — stretching art is forbidden (F21 behavior 2). A
/// corrupt known value is refused too.
#[test]
fn accept_f21_a_lower_projection_refuses_unknowns_stretch_and_corruption() {
    let unknown_framing = ProjectionPolicy {
        framing: unknown("unmeasured in the original config"),
        ..policy(1.0, FovAxis::Vertical)
    };
    assert_eq!(
        lower_projection(&unknown_framing),
        Err(ProjectionLowerError::UnknownField {
            field: "framing",
            claim_id: claim(),
            reason: "unmeasured in the original config".to_owned(),
        })
    );

    let unknown_fov = ProjectionPolicy {
        fov: unknown("no original field of view was read"),
        ..policy(1.0, FovAxis::Vertical)
    };
    assert_eq!(
        lower_projection(&unknown_fov),
        Err(ProjectionLowerError::UnknownField {
            field: "fov",
            claim_id: claim(),
            reason: "no original field of view was read".to_owned(),
        })
    );

    let stretched = ProjectionPolicy {
        framing: known(AspectFraming::Stretch),
        ..policy(1.0, FovAxis::Vertical)
    };
    assert_eq!(
        lower_projection(&stretched),
        Err(ProjectionLowerError::StretchFraming),
        "a stretch rule must be refused, not honoured"
    );

    let out_of_range = ProjectionPolicy {
        fov: known(Radians(PI)),
        ..policy(1.0, FovAxis::Vertical)
    };
    assert_eq!(
        lower_projection(&out_of_range),
        Err(ProjectionLowerError::FovOutOfRange { radians: PI })
    );

    let unordered = ProjectionPolicy {
        near_m: known(Meters(50.0)),
        far_m: known(Meters(10.0)),
        ..policy(1.0, FovAxis::Vertical)
    };
    assert_eq!(
        lower_projection(&unordered),
        Err(ProjectionLowerError::ClippingNotOrdered {
            near_m: 50.0,
            far_m: 10.0,
        })
    );
}
