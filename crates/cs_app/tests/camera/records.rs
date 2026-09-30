//! F21-A: the declared camera records in `cs_content::cameras`.
//!
//! These tests exercise the production record constructors and validators:
//! the synthetic fixture, the corrupt-known-value refusals, the set-level
//! rules (unique kinds, present default, camera namespace) and the validated
//! scalar types. Removing a range check or the set validation fails them.

use cs_content::cameras::{
    AspectFraming, AspectRatio, AspectRatioError, CameraModeError, CameraModeKind,
    CameraModesError, DeclaredCameraMode, DeclaredCameraModes, FovAxis, Magnification,
    MagnificationError, ProjectionPolicy, declared_synthetic_camera_modes,
};
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::space::{Meters, Radians};

use crate::common::{claim, known};

/// A designed projection policy, with the fields a caller wants to corrupt
/// overridden.
fn policy(fov: f64) -> ProjectionPolicy {
    ProjectionPolicy {
        fov: known(Radians(fov)),
        fov_axis: known(FovAxis::Vertical),
        reference_aspect: known(AspectRatio::FOUR_THREE),
        framing: known(AspectFraming::PreserveVertical),
        near_m: known(Meters(0.1)),
        far_m: known(Meters(10_000.0)),
    }
}

fn mode(
    kind: CameraModeKind,
    projection: ProjectionPolicy,
    magnification: Magnification,
) -> Result<DeclaredCameraMode, CameraModeError> {
    DeclaredCameraMode::try_new(kind, projection, known(magnification), known(false))
}

fn camera_subject() -> ContentId {
    ContentId::from_source(ContentKind::CameraTrack, "synthetic.camera-modes").expect("valid")
}

/// The value of a `Resolved`, ignoring its provenance, so a fixture value can
/// be compared without restating the claim it was recorded under.
fn known_value<T: Clone>(value: &Resolved<T>) -> Option<T> {
    match value {
        Resolved::Known(known) => Some(known.value.clone()),
        Resolved::Unknown { .. } => None,
    }
}

/// The synthetic fixture is designed content: a camera-namespace subject,
/// `SyntheticFixture` origin, the cockpit as default, and the three modes
/// with their mode-specific fields.
#[test]
fn accept_f21_a_synthetic_fixture_is_designed_and_declares_three_modes() {
    let modes = declared_synthetic_camera_modes();
    assert_eq!(modes.subject().kind(), ContentKind::CameraTrack);
    assert_eq!(
        modes.subject().as_str(),
        "camera_track/synthetic.camera-modes"
    );
    assert_eq!(modes.origin(), &Origin::SyntheticFixture);
    assert_eq!(modes.default_mode(), CameraModeKind::Cockpit);
    assert_eq!(modes.modes().len(), 3);

    let cockpit = modes
        .get(CameraModeKind::Cockpit)
        .expect("cockpit declared");
    assert_eq!(
        known_value(&cockpit.projection().fov_axis),
        Some(FovAxis::Vertical)
    );
    assert_eq!(
        known_value(&cockpit.projection().framing),
        Some(AspectFraming::PreserveVertical)
    );
    assert_eq!(
        known_value(&cockpit.projection().reference_aspect),
        Some(AspectRatio::FOUR_THREE)
    );
    assert_eq!(
        known_value(cockpit.magnification()),
        Some(Magnification::ONE)
    );
    assert_eq!(known_value(cockpit.tracks_target()), Some(false));

    let spyglass = modes
        .get(CameraModeKind::Spyglass)
        .expect("spyglass declared");
    assert_eq!(
        known_value(spyglass.magnification()),
        Some(Magnification::new(4.0).expect("valid"))
    );
    assert_eq!(known_value(spyglass.tracks_target()), Some(true));
    // The spyglass carries its own clipping planes (F21 behavior 3).
    assert_ne!(
        spyglass.projection().near_m,
        cockpit.projection().near_m,
        "the spyglass's own near plane is part of the fixture"
    );

    assert!(modes.get(CameraModeKind::AuthoredSequence).is_none());
}

/// A corrupt *known* value is refused when the mode is built: a field of
/// view outside `(0, π)`, a non-positive near plane, an unordered clipping
/// range, and a magnification on a mode that must not magnify.
#[test]
fn accept_f21_a_declared_mode_refuses_corrupt_known_values() {
    for fov in [0.0, -0.5, std::f64::consts::PI, 4.0] {
        assert_eq!(
            mode(CameraModeKind::Cockpit, policy(fov), Magnification::ONE),
            Err(CameraModeError::FovOutOfRange { radians: fov }),
            "field of view {fov} must be refused"
        );
    }

    let non_positive_near = ProjectionPolicy {
        near_m: known(Meters(0.0)),
        ..policy(1.0)
    };
    assert_eq!(
        mode(
            CameraModeKind::Cockpit,
            non_positive_near,
            Magnification::ONE
        ),
        Err(CameraModeError::NonPositiveNear { meters: 0.0 })
    );

    let unordered = ProjectionPolicy {
        near_m: known(Meters(10.0)),
        far_m: known(Meters(5.0)),
        ..policy(1.0)
    };
    assert_eq!(
        mode(CameraModeKind::Cockpit, unordered, Magnification::ONE),
        Err(CameraModeError::ClippingNotOrdered {
            near_m: 10.0,
            far_m: 5.0,
        })
    );

    assert_eq!(
        mode(
            CameraModeKind::Cockpit,
            policy(1.0),
            Magnification::new(2.0).expect("valid")
        ),
        Err(CameraModeError::UnexpectedMagnification {
            kind: CameraModeKind::Cockpit,
            value: 2.0,
        }),
        "a non-spyglass mode must not magnify"
    );

    // A spyglass may magnify and does validate.
    assert!(
        mode(
            CameraModeKind::Spyglass,
            policy(1.0),
            Magnification::new(2.0).expect("valid")
        )
        .is_ok()
    );
}

/// The set rules are enforced: a non-camera subject, an empty set, a
/// duplicated kind and a default that is not declared are each refused by
/// name.
#[test]
fn accept_f21_a_declared_mode_set_enforces_namespace_kinds_and_default() {
    let cockpit =
        || mode(CameraModeKind::Cockpit, policy(1.0), Magnification::ONE).expect("valid cockpit");
    let external =
        || mode(CameraModeKind::External, policy(1.0), Magnification::ONE).expect("valid external");

    let wrong_subject =
        ContentId::from_source(ContentKind::IaScenario, "synthetic.test").expect("valid id");
    assert_eq!(
        DeclaredCameraModes::try_new(
            wrong_subject.clone(),
            Origin::SyntheticFixture,
            CameraModeKind::Cockpit,
            vec![cockpit()],
            Provenance::designed(claim()),
        ),
        Err(CameraModesError::SubjectKindMismatch {
            subject: wrong_subject,
        })
    );

    assert_eq!(
        DeclaredCameraModes::try_new(
            camera_subject(),
            Origin::SyntheticFixture,
            CameraModeKind::Cockpit,
            Vec::new(),
            Provenance::designed(claim()),
        ),
        Err(CameraModesError::Empty)
    );

    assert_eq!(
        DeclaredCameraModes::try_new(
            camera_subject(),
            Origin::SyntheticFixture,
            CameraModeKind::Cockpit,
            vec![cockpit(), cockpit()],
            Provenance::designed(claim()),
        ),
        Err(CameraModesError::DuplicateKind {
            kind: CameraModeKind::Cockpit,
        })
    );

    assert_eq!(
        DeclaredCameraModes::try_new(
            camera_subject(),
            Origin::SyntheticFixture,
            CameraModeKind::Cockpit,
            vec![external()],
            Provenance::designed(claim()),
        ),
        Err(CameraModesError::MissingDefault {
            default_mode: CameraModeKind::Cockpit,
        }),
        "the default must be one of the declared modes"
    );
}

/// The validated scalar types refuse zero, negative and non-finite values and
/// accept ordinary ones.
#[test]
fn accept_f21_a_aspect_ratio_and_magnification_reject_unusable_values() {
    assert_eq!(
        AspectRatio::new(0.0),
        Err(AspectRatioError::NotPositive { value: 0.0 })
    );
    assert_eq!(
        AspectRatio::new(-1.5),
        Err(AspectRatioError::NotPositive { value: -1.5 })
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(AspectRatio::new(bad), Err(AspectRatioError::NonFinite));
        assert_eq!(Magnification::new(bad), Err(MagnificationError::NonFinite));
    }
    assert_eq!(
        Magnification::new(0.0),
        Err(MagnificationError::NotPositive { value: 0.0 })
    );

    assert_eq!(
        AspectRatio::new(16.0 / 9.0).expect("valid").value(),
        16.0 / 9.0
    );
    assert_eq!(AspectRatio::SIXTEEN_NINE.value(), 16.0 / 9.0);
    assert_eq!(Magnification::new(4.0).expect("valid").value(), 4.0);
    assert!(!AspectFraming::Stretch.is_aspect_correct());
    assert!(AspectFraming::PreserveVertical.is_aspect_correct());
    assert_eq!(CameraModeKind::Spyglass.label(), "spyglass");
}
