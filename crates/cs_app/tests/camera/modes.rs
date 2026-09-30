//! F21-A: lowering declared camera modes into runtime records.
//!
//! These tests call `cs_app::camera::lower_camera_mode(s)`. They prove the
//! fixture lowers field-for-field and that an unknown projection,
//! magnification or target-tracking flag refuses by name instead of
//! defaulting.

use cs_app::camera::{
    CameraLowerError, ProjectionLowerError, lower_camera_mode, lower_camera_modes,
};
use cs_content::cameras::{
    AspectFraming, AspectRatio, CameraModeKind, FovAxis, Magnification, ProjectionPolicy,
    declared_synthetic_camera_modes,
};
use cs_types::content::Resolved;
use cs_types::space::{Meters, Radians};

use crate::common::{assert_close, claim, known};

fn projection(fov: f64) -> ProjectionPolicy {
    ProjectionPolicy {
        fov: known(Radians(fov)),
        fov_axis: known(FovAxis::Vertical),
        reference_aspect: known(AspectRatio::FOUR_THREE),
        framing: known(AspectFraming::PreserveVertical),
        near_m: known(Meters(0.1)),
        far_m: known(Meters(10_000.0)),
    }
}

/// The fixture set lowers with its default, order and mode-specific fields
/// intact.
#[test]
fn accept_f21_a_lower_camera_modes_preserves_the_fixture() {
    let declared = declared_synthetic_camera_modes();
    let lowered = lower_camera_modes(&declared).expect("the fixture lowers");

    assert_eq!(lowered.default_mode(), CameraModeKind::Cockpit);
    assert_eq!(lowered.len(), 3);
    assert!(!lowered.is_empty());
    assert!(lowered.get(CameraModeKind::Cockpit).is_some());

    let spyglass = lowered
        .get(CameraModeKind::Spyglass)
        .expect("spyglass lowers");
    assert_eq!(spyglass.magnification().value(), 4.0);
    assert!(spyglass.tracks_target());
    assert_close(
        spyglass.projection().vertical_fov().0,
        std::f64::consts::PI / 9.0,
        1e-12,
        "spyglass vertical FOV",
    );

    let external = lowered
        .get(CameraModeKind::External)
        .expect("external lowers");
    assert_eq!(external.magnification(), Magnification::ONE);
    assert!(!external.tracks_target());
    assert!(lowered.get(CameraModeKind::AuthoredSequence).is_none());
}

/// An unknown behavior flag refuses by mode and field; an unknown projection
/// field refuses through the projection error.
#[test]
fn accept_f21_a_unknown_mode_fields_refuse_to_lower() {
    let unknown_magnification = cs_content::cameras::DeclaredCameraMode::try_new(
        CameraModeKind::Spyglass,
        projection(1.0),
        Resolved::unknown(claim(), "the original magnification is unmeasured").expect("reason"),
        known(true),
    )
    .expect("unknown values are valid declared content");
    assert_eq!(
        lower_camera_mode(&unknown_magnification),
        Err(CameraLowerError::UnknownField {
            mode: CameraModeKind::Spyglass,
            field: "magnification",
            claim_id: claim(),
            reason: "the original magnification is unmeasured".to_owned(),
        })
    );

    let unknown_tracking = cs_content::cameras::DeclaredCameraMode::try_new(
        CameraModeKind::Cockpit,
        projection(1.0),
        known(Magnification::ONE),
        Resolved::unknown(claim(), "no original tracking behavior was read").expect("reason"),
    )
    .expect("unknown values are valid declared content");
    assert_eq!(
        lower_camera_mode(&unknown_tracking),
        Err(CameraLowerError::UnknownField {
            mode: CameraModeKind::Cockpit,
            field: "tracks_target",
            claim_id: claim(),
            reason: "no original tracking behavior was read".to_owned(),
        })
    );

    let unknown_fov_policy = ProjectionPolicy {
        fov: Resolved::unknown(claim(), "the original field of view is unmeasured")
            .expect("reason"),
        ..projection(1.0)
    };
    let unknown_fov = cs_content::cameras::DeclaredCameraMode::try_new(
        CameraModeKind::Cockpit,
        unknown_fov_policy,
        known(Magnification::ONE),
        known(false),
    )
    .expect("unknown values are valid declared content");
    assert_eq!(
        lower_camera_mode(&unknown_fov),
        Err(CameraLowerError::Projection(
            ProjectionLowerError::UnknownField {
                field: "fov",
                claim_id: claim(),
                reason: "the original field of view is unmeasured".to_owned(),
            }
        ))
    );
}
