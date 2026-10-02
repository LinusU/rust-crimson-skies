//! Shared helpers for the F21 camera acceptance tests.
//!
//! Everything here is authored test scaffolding, never production logic:
//! the `known`/`claim` helpers build designed provenance exactly the way
//! the fixtures do, `origin_pose` is the identity camera the framing
//! scenario uses, and `assert_close` is the tolerance assertion shared by
//! the projection tests. The F21-B half adds the declared-placement helpers
//! the rig tests build their records with, so the cockpit/chase/spyglass
//! tests state a placement in metres instead of restating a record
//! constructor at every call site.

use cs_app::camera::{CameraPose, CameraRig, LookOffset, LoweredCameraModes, lower_camera_modes};
use cs_content::cameras::{
    AspectFraming, AspectRatio, BodyOffset, CameraModeKind, CockpitBindingSource, CockpitViewpoint,
    DeclaredCameraMode, DeclaredCameraModes, DeclaredPlacement, FovAxis, LookLimits, Magnification,
    ProjectionPolicy,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Meters, Quaternion, Radians, WorldPosition};

/// The claim id every designed test value is recorded under.
pub fn claim() -> ClaimId {
    ClaimId::new("f21a.camera-test").expect("valid claim id")
}

/// Wraps a test value with designed provenance.
pub fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// The synthetic airframe a declared test mode set is owned by.
pub fn owner_subject() -> ContentId {
    ContentId::from_source(ContentKind::Airframe, "synthetic.test-plane").expect("a valid id")
}

/// A designed projection policy for a mode kind, with the same values the
/// synthetic fixture declares so a test can compare against the fixture.
pub fn projection_for(kind: CameraModeKind) -> ProjectionPolicy {
    let (fov_deg, near_m, far_m) = match kind {
        CameraModeKind::Cockpit => (60.0, 0.1, 10_000.0),
        CameraModeKind::External => (55.0, 0.1, 10_000.0),
        CameraModeKind::Spyglass => (20.0, 1.0, 20_000.0),
        CameraModeKind::AuthoredSequence => (45.0, 0.1, 10_000.0),
    };
    ProjectionPolicy {
        fov: known(Radians(fov_deg_f64(fov_deg))),
        fov_axis: known(FovAxis::Vertical),
        reference_aspect: known(AspectRatio::FOUR_THREE),
        framing: known(AspectFraming::PreserveVertical),
        near_m: known(Meters(near_m)),
        far_m: known(Meters(far_m)),
    }
}

/// `degrees` in radians, spelled out so a test states an angle in degrees the
/// way a config file would.
pub fn fov_deg_f64(degrees: f64) -> f64 {
    degrees * std::f64::consts::PI / 180.0
}

/// A designed mode of `kind`, with the fixture's placement and look limits.
pub fn declared_mode(kind: CameraModeKind) -> DeclaredCameraMode {
    declared_mode_with(kind, placement(kind), kind == CameraModeKind::Spyglass)
}

/// A designed mode of `kind` with an explicit placement and tracking flag.
pub fn declared_mode_with(
    kind: CameraModeKind,
    placement: DeclaredPlacement,
    tracks_target: bool,
) -> DeclaredCameraMode {
    DeclaredCameraMode::try_new(
        kind,
        projection_for(kind),
        known_magnification(if kind == CameraModeKind::Spyglass {
            4.0
        } else {
            1.0
        }),
        known(tracks_target),
        placement,
        known_look_limits(),
    )
    .expect("the designed mode is valid")
}

/// A designed mode set for [`owner_subject`].
pub fn declared_set(
    modes: Vec<DeclaredCameraMode>,
    default: CameraModeKind,
) -> DeclaredCameraModes {
    DeclaredCameraModes::try_new(
        owner_subject(),
        Origin::SyntheticFixture,
        default,
        modes,
        Provenance::designed(claim()),
    )
    .expect("the designed mode set is valid")
}

/// The lowered form of [`declared_set`].
pub fn lowered_set(modes: Vec<DeclaredCameraMode>, default: CameraModeKind) -> LoweredCameraModes {
    lower_camera_modes(&declared_set(modes, default)).expect("the designed set lowers")
}

/// A rig over the synthetic fixture's own declared mode set.
pub fn fixture_rig() -> CameraRig {
    CameraRig::new(
        lower_camera_modes(&cs_content::cameras::declared_synthetic_camera_modes())
            .expect("the fixture lowers"),
    )
    .expect("the fixture's default mode has a rig")
}

/// A camera at the world origin, looking along canonical forward (`-Z`)
/// with `+Y` up and `+X` right — the canonical pose of F16.
pub fn origin_pose() -> CameraPose {
    CameraPose::new(
        WorldPosition::try_new([0.0, 0.0, 0.0]).expect("the origin is finite"),
        Quaternion::IDENTITY,
    )
}

/// Asserts two `f64` values agree within `tolerance`.
pub fn assert_close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

/// Asserts two world positions agree component-wise within `tolerance`.
pub fn assert_close_position(
    actual: WorldPosition,
    expected: [f64; 3],
    tolerance: f64,
    what: &str,
) {
    for (axis, (value, want)) in actual.to_array().into_iter().zip(expected).enumerate() {
        assert_close(value, want, tolerance, &format!("{what} on axis {axis}"));
    }
}

/// A world position from three metres.
pub fn world([x, y, z]: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new([x, y, z]).expect("the fixture position is finite")
}

/// An aircraft pose at `position` with `rotation`.
pub fn aircraft_pose(position: [f64; 3], rotation: Quaternion) -> CameraPose {
    CameraPose::new(world(position), rotation)
}

/// A body-frame offset in metres along the body's right, up and forward axes.
pub fn body_offset(right: f64, up: f64, forward: f64) -> BodyOffset {
    BodyOffset::new(Meters(right), Meters(up), Meters(forward))
        .expect("the fixture body offset is finite")
}

/// The placement a mode of `kind` may declare, bound to a named node.
///
/// The cockpit's binding name is a fixture key; it is not a node of any
/// original model.
pub fn placement(kind: CameraModeKind) -> DeclaredPlacement {
    match kind {
        CameraModeKind::Cockpit => DeclaredPlacement::at_cockpit(
            CockpitViewpoint::try_new(
                CockpitBindingSource::ModelNode {
                    node: "synthetic.test_eye".to_owned(),
                },
                body_offset(0.0, 1.0, 1.0),
                known(Radians(0.0)),
                known(Radians(0.0)),
            )
            .expect("the fixture cockpit viewpoint is valid"),
        ),
        _ => DeclaredPlacement::BodyOffset(BodyOffset::ZERO),
    }
}

/// The declared free-look limits used by the rig tests: 120° of yaw and 60°
/// of pitch, which is what the synthetic airframe declares.
pub fn look_limits() -> LookLimits {
    LookLimits::new(
        Radians(120.0_f64.to_radians()),
        Radians(60.0_f64.to_radians()),
    )
    .expect("the fixture look limits are in range")
}

/// The declared look limits, with designed provenance.
pub fn known_look_limits() -> Resolved<LookLimits> {
    known(look_limits())
}

/// A free-look offset in radians.
pub fn look(yaw: f64, pitch: f64) -> LookOffset {
    LookOffset::new(Radians(yaw), Radians(pitch)).expect("the fixture look offset is finite")
}

/// The magnification a mode declares, as a resolved value.
pub fn known_magnification(factor: f64) -> Resolved<Magnification> {
    known(Magnification::new(factor).expect("the fixture magnification is positive"))
}
