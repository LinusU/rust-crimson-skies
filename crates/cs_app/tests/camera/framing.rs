//! F21-A: aspect-correct framing of an invariant world-space target.
//!
//! This is the stage's minimum scenario: *compare framing at three aspect
//! ratios using an invariant world-space target*. The camera is at the
//! origin looking down canonical forward; the target is a fixed world point.
//! Under preserve-vertical framing the target's **vertical** viewport
//! coordinate is identical at 4:3, 16:9 and ultrawide — no vertical stretch —
//! while the horizontal field of view grows and reveals more world. Under
//! preserve-horizontal framing the roles swap.

use std::f64::consts::PI;

use cs_app::camera::{Framing, FramingError, LoweredProjection, lower_projection};
use cs_content::cameras::{
    AspectFraming, AspectRatio, CameraModeKind, FovAxis, ProjectionPolicy,
    declared_synthetic_camera_modes,
};
use cs_types::space::{Meters, Quaternion, Radians, UnitVec3, WorldPosition};

use crate::common::{assert_close, known, origin_pose};

const ASPECTS: [AspectRatio; 3] = [
    AspectRatio::FOUR_THREE,
    AspectRatio::SIXTEEN_NINE,
    AspectRatio::ULTRAWIDE_64_27,
];

fn base_policy(vertical_fov: f64, framing: AspectFraming) -> ProjectionPolicy {
    ProjectionPolicy {
        fov: known(Radians(vertical_fov)),
        fov_axis: known(FovAxis::Vertical),
        reference_aspect: known(AspectRatio::FOUR_THREE),
        framing: known(framing),
        near_m: known(Meters(0.1)),
        far_m: known(Meters(10_000.0)),
    }
}

fn cockpit_projection() -> LoweredProjection {
    let modes = declared_synthetic_camera_modes();
    let cockpit = modes
        .get(CameraModeKind::Cockpit)
        .expect("cockpit declared");
    lower_projection(cockpit.projection()).expect("the fixture cockpit lowers")
}

fn world(x: f64, y: f64, z: f64) -> WorldPosition {
    WorldPosition::try_new([x, y, z]).expect("finite test point")
}

/// AC01: at 4:3, 16:9 and ultrawide the same world target keeps its vertical
/// position (no stretch), its horizontal position moves monotonically as the
/// horizontal FOV grows, and a point just outside 4:3 is revealed by 16:9.
#[test]
fn accept_f21_a_framing_at_three_aspect_ratios_keeps_vertical_extent_and_reveals_more_world() {
    let projection = cockpit_projection();
    let basis = origin_pose().basis().expect("the identity basis is usable");
    let target = world(30.0, 10.0, -100.0);

    let frames: Vec<Framing> = ASPECTS
        .iter()
        .map(|aspect| {
            projection
                .framing_of(*aspect, basis, target)
                .expect("a target in front frames")
        })
        .collect();

    // The vertical coordinate is the same at every aspect: the authored
    // vertical extent is preserved and the art is not stretched.
    let expected_y = 0.1 / (PI / 6.0).tan();
    for frame in &frames {
        assert_close(frame.y(), expected_y, 1e-12, "vertical viewport coordinate");
        assert!(frame.is_inside(), "the target stays in frame: {frame}");
    }
    assert_close(
        frames[0].y(),
        frames[2].y(),
        1e-15,
        "vertical coordinate is invariant across aspects",
    );

    // The horizontal coordinate shrinks monotonically: wider viewports reveal
    // more world instead of magnifying or cropping it.
    assert!(
        frames[0].x() > frames[1].x() && frames[1].x() > frames[2].x(),
        "horizontal coordinate must decrease as the viewport widens: {:?}",
        frames.iter().map(|frame| frame.x()).collect::<Vec<_>>()
    );

    // The two fields of view tell the same story.
    for aspect in ASPECTS {
        assert_close(
            projection.vertical_fov_at(aspect).0,
            PI / 3.0,
            1e-12,
            "vertical FOV is constant under preserve-vertical",
        );
    }
    let horizontals: Vec<f64> = ASPECTS
        .iter()
        .map(|aspect| projection.horizontal_fov_at(*aspect).0)
        .collect();
    assert!(
        horizontals[0] < horizontals[1] && horizontals[1] < horizontals[2],
        "horizontal FOV must grow with aspect: {horizontals:?}"
    );

    // A world point 45° off-axis is cropped by 4:3 but revealed by 16:9 —
    // the visible world grows, the image does not stretch.
    let off_axis = world(100.0, 0.0, -100.0);
    let cropped = projection
        .framing_of(AspectRatio::FOUR_THREE, basis, off_axis)
        .expect("in front");
    let revealed = projection
        .framing_of(AspectRatio::SIXTEEN_NINE, basis, off_axis)
        .expect("in front");
    assert!(
        !cropped.is_inside(),
        "45° is outside a 4:3 frame: {cropped}"
    );
    assert!(
        revealed.is_inside(),
        "45° is inside a 16:9 frame: {revealed}"
    );
}

/// Framing depends only on the ray, not on how far along it the point is,
/// and a point at or behind the camera is refused instead of projected.
#[test]
fn accept_f21_a_framing_is_distance_independent_and_refuses_points_behind() {
    let projection = cockpit_projection();
    let basis = origin_pose().basis().expect("usable basis");
    let aspect = AspectRatio::SIXTEEN_NINE;

    let near = projection
        .framing_of(aspect, basis, world(30.0, 10.0, -100.0))
        .expect("in front");
    let far = projection
        .framing_of(aspect, basis, world(60.0, 20.0, -200.0))
        .expect("in front");
    assert_close(near.x(), far.x(), 1e-12, "x is distance independent");
    assert_close(near.y(), far.y(), 1e-12, "y is distance independent");

    assert_eq!(
        projection.framing_of(aspect, basis, world(0.0, 0.0, 100.0)),
        Err(FramingError::BehindCamera)
    );
    assert_eq!(
        projection.framing_of(aspect, basis, world(0.0, 0.0, 0.0)),
        Err(FramingError::BehindCamera)
    );
}

/// Framing follows the camera's own basis: a yawed camera frames a point
/// that is off-axis for an unrotated one.
#[test]
fn accept_f21_a_framing_follows_the_camera_basis() {
    let projection = cockpit_projection();
    let yaw = Quaternion::from_axis_angle(UnitVec3::UP, Radians(PI / 2.0)).expect("quarter turn");
    let pose = cs_app::camera::CameraPose::new(world(0.0, 0.0, 0.0), yaw);
    let basis = pose.basis().expect("usable basis");

    // +90° about +Y turns forward from -Z to -X, so a point on -X is centred.
    let centred = projection
        .framing_of(AspectRatio::SIXTEEN_NINE, basis, world(-100.0, 0.0, 0.0))
        .expect("in front of the yawed camera");
    assert_close(centred.x(), 0.0, 1e-12, "centred x");
    assert_close(centred.y(), 0.0, 1e-12, "centred y");
}

/// Preserve-horizontal framing is the mirror policy: the horizontal extent
/// is kept and the vertical extent shrinks as the viewport widens, so the
/// target's horizontal coordinate is constant while its vertical coordinate
/// grows.
#[test]
fn accept_f21_a_preserve_horizontal_keeps_horizontal_extent() {
    let projection = lower_projection(&base_policy(PI / 3.0, AspectFraming::PreserveHorizontal))
        .expect("lowers");
    let basis = origin_pose().basis().expect("usable basis");
    let target = world(30.0, 10.0, -100.0);

    let frames: Vec<Framing> = ASPECTS
        .iter()
        .map(|aspect| {
            projection
                .framing_of(*aspect, basis, target)
                .expect("in front")
        })
        .collect();
    assert_close(frames[0].x(), frames[1].x(), 1e-15, "x invariant");
    assert_close(frames[1].x(), frames[2].x(), 1e-15, "x invariant");
    assert!(
        frames[0].y() < frames[1].y() && frames[1].y() < frames[2].y(),
        "vertical coordinate grows as the viewport widens: {:?}",
        frames.iter().map(|frame| frame.y()).collect::<Vec<_>>()
    );

    let horizontals: Vec<f64> = ASPECTS
        .iter()
        .map(|aspect| projection.horizontal_fov_at(*aspect).0)
        .collect();
    assert_close(
        horizontals[0],
        horizontals[2],
        1e-15,
        "horizontal FOV invariant",
    );
    let verticals: Vec<f64> = ASPECTS
        .iter()
        .map(|aspect| projection.vertical_fov_at(*aspect).0)
        .collect();
    assert!(
        verticals[0] > verticals[1] && verticals[1] > verticals[2],
        "vertical FOV shrinks as the viewport widens: {verticals:?}"
    );
}
