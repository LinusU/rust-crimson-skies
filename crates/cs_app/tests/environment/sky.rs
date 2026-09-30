//! `accept_f19_a_` tests for F19 non-negotiable behavior 3 and AC01: the
//! sky frame's relationship with camera translation, world orientation and a
//! rebased origin.
//!
//! The production code under test is `cs_app::environment::frame`
//! (`SkyFrame`) together with the `cs_app::origin` rebase machinery it
//! consumes. Nothing here computes a sky direction of its own: every value
//! the assertions compare comes out of the fixtures or out of
//! `SkyFrame`.

use cs_app::environment::{SKY_CENTERING_TOLERANCE_M, SkyFrame};
use cs_app::origin::{OriginEpoch, OriginShift, SpatialAnchor, WorldOrigin};
use cs_types::content::Resolved;
use cs_types::space::WorldPosition;

use crate::common;

/// Where the camera starts, in world meters. Every component is dyadic, so
/// the f32 local cache holds the position exactly and the assertions below
/// compare real conversions rather than rounding noise.
const CAMERA_START: [f64; 3] = [4096.0, 512.0, -2048.0];

/// The origin's offset from the camera at the rebase, in meters. Non-zero
/// on two axes, and dyadic, so the converted local coordinates are both
/// non-trivial and exact.
const REBASE_OFFSET: [f64; 3] = [512.0, 0.0, 0.25];

/// How far the camera flies between two captures, in meters.
const CAMERA_STEP_M: [f32; 3] = [8.0, -4.0, 2.0];

fn distance_m(a: WorldPosition, b: WorldPosition) -> f64 {
    let [ax, ay, az] = a.to_array();
    let [bx, by, bz] = b.to_array();
    ((ax - bx).powi(2) + (ay - by).powi(2) + (az - bz).powi(2)).sqrt()
}

/// AC01's minimum scenario: rebase the world under a fixed horizon and the
/// sky and sun direction stay stable.
///
/// What makes this discriminating: the rebase is *not* a no-op (the camera's
/// local coordinate changes by hundreds of metres), the frame is captured
/// through the same production path on both sides, and the dome is compared
/// in **world** metres — an implementation that parked the dome in the local
/// frame would move it by the origin offset, five orders of magnitude above
/// [`SKY_CENTERING_TOLERANCE_M`].
#[test]
fn accept_f19_a_rebase_keeps_sky_and_sun_direction_stable() {
    let environment = common::clear();
    let origin = WorldOrigin::new(OriginEpoch(0), common::world([0.0, 0.0, 0.0]));
    let camera_world = common::world(CAMERA_START);

    let camera = SpatialAnchor::new(&origin, camera_world).expect("the position is finite");
    let local_before = camera.local();
    let before = SkyFrame::capture(&environment, &camera);
    assert_eq!(before.epoch(), OriginEpoch(0));

    // The origin jumps away from the camera and every anchor converts into
    // the new frame before anything observes it.
    let target = common::world([
        CAMERA_START[0] + REBASE_OFFSET[0],
        CAMERA_START[1] + REBASE_OFFSET[1],
        CAMERA_START[2] + REBASE_OFFSET[2],
    ]);
    let shift = OriginShift::rebase(origin, target).expect("the epoch can rebase");
    let mut anchors = [camera];
    shift.apply(&mut anchors).expect("the camera converts");
    let camera = anchors[0];
    let origin = shift.to();
    assert_eq!(
        origin.epoch(),
        OriginEpoch(1),
        "the scenario really rebased"
    );

    // The local coordinate moved by the origin offset: if the frame below
    // reads it as a world position, the dome jumps with it.
    let local_after = camera.local();
    assert_ne!(
        local_before.to_array(),
        local_after.to_array(),
        "the rebase must actually rewrite the local coordinate"
    );

    let after = SkyFrame::capture(&environment, &camera);
    assert_eq!(after.epoch(), OriginEpoch(1), "the frame reports its epoch");

    // A fixed horizon: the orientation and the sun are world values, copied
    // verbatim, so they are *exactly* equal across the rebase.
    assert_eq!(
        before.sky_orientation(),
        after.sky_orientation(),
        "a rebase must not rotate the sky"
    );
    assert_eq!(
        before.sun_direction(),
        after.sun_direction(),
        "a rebase must not move the sun"
    );
    assert_eq!(
        before.horizon_normal(),
        after.horizon_normal(),
        "the horizon normal must not move"
    );
    assert!(before.sun_direction().is_known());
    assert!(after.known_sun_direction().is_some());

    // The dome did not pop: it still sits on the camera, in world metres.
    assert!(
        after.is_centered_on(camera.world(), SKY_CENTERING_TOLERANCE_M),
        "the dome must sit on the camera after the rebase"
    );
    assert!(
        distance_m(before.dome_position(), after.dome_position()) <= SKY_CENTERING_TOLERANCE_M,
        "the dome's world position must not move at the rebase: {:?} vs {:?}",
        before.dome_position(),
        after.dome_position()
    );

    // A frame captured *before* the rebase is still valid afterwards: a
    // rebase alone never invalidates the sky (this is the "cannot pop the
    // sky" half of behavior 3).
    assert!(
        before.is_centered_on(camera.world(), SKY_CENTERING_TOLERANCE_M),
        "a frame held across a rebase must still be centred"
    );

    // Capturing through the *local* coordinate — the constructor that would
    // fail visibly if it skipped the conversion — agrees with the captured
    // frame.
    let via_local = SkyFrame::from_local(&environment, &origin, camera.local())
        .expect("the local position converts");
    assert!(
        via_local.is_centered_on(camera.world(), SKY_CENTERING_TOLERANCE_M),
        "the dome built from the local coordinate must land on the camera in world metres: {:?} vs {:?}",
        via_local.dome_position(),
        camera.world()
    );
    assert_eq!(via_local.sun_direction(), before.sun_direction());
    assert_eq!(via_local.sky_orientation(), before.sky_orientation());
    assert_eq!(via_local.epoch(), origin.epoch());
}

/// The other half of behavior 3: the dome follows camera *translation* and
/// nothing else, and a frame that the camera has moved away from is
/// detectably stale instead of silently wrong.
#[test]
fn accept_f19_a_sky_frame_follows_the_camera_and_detects_a_stale_frame() {
    let environment = common::clear();
    let origin = WorldOrigin::new(OriginEpoch(0), common::world([0.0, 0.0, 0.0]));
    let camera_world = common::world(CAMERA_START);
    let mut camera = SpatialAnchor::new(&origin, camera_world).expect("the position is finite");

    let first = SkyFrame::capture(&environment, &camera);
    assert!(
        first.is_centered_on(camera.world(), SKY_CENTERING_TOLERANCE_M),
        "a frame is centred at capture time"
    );

    camera
        .advance_local(&origin, CAMERA_STEP_M)
        .expect("the camera can move in this frame");
    let second = SkyFrame::capture(&environment, &camera);

    // The dome followed the camera, in world metres.
    assert!(second.is_centered_on(camera.world(), SKY_CENTERING_TOLERANCE_M));
    assert!(
        distance_m(first.dome_position(), second.dome_position()) > SKY_CENTERING_TOLERANCE_M,
        "the dome must move with the camera"
    );

    // The old frame did not: a renderer holding it must recapture rather
    // than draw a dome that lagged behind.
    assert!(
        !first.is_centered_on(camera.world(), SKY_CENTERING_TOLERANCE_M),
        "a frame captured before the camera moved must be reported as stale"
    );

    // Camera translation changes neither the orientation nor the sun.
    assert_eq!(first.sky_orientation(), second.sky_orientation());
    assert_eq!(first.sun_direction(), second.sun_direction());
    for (axis, step) in CAMERA_STEP_M.iter().enumerate() {
        let moved =
            second.dome_position().to_array()[axis] - first.dome_position().to_array()[axis];
        assert!(
            (moved - f64::from(*step)).abs() <= SKY_CENTERING_TOLERANCE_M,
            "the dome's axis {axis} must track the camera by exactly {step} m, moved {moved}"
        );
    }
}

/// The sky honors world orientation: the frame carries the authored
/// orientation verbatim — a heading no identity matrix could produce — and
/// the horizon is the plane perpendicular to it.
#[test]
fn accept_f19_a_sky_frame_carries_the_authored_world_orientation() {
    let environment = common::clear();
    let origin = WorldOrigin::new(OriginEpoch(0), common::world([0.0, 0.0, 0.0]));
    let camera = SpatialAnchor::new(&origin, common::world(CAMERA_START)).expect("finite");

    let frame = SkyFrame::capture(&environment, &camera);
    let orientation = match frame.sky_orientation() {
        Resolved::Known(known) => &known.value,
        Resolved::Unknown { .. } => panic!("the clear-sky fixture authors an orientation"),
    };

    let up = orientation.up();
    let heading = orientation.heading();
    assert_eq!(up.to_array(), [0.0, 1.0, 0.0], "the horizon's normal is up");
    assert_eq!(
        frame.horizon_normal(),
        Some(up),
        "the horizon normal comes from the authored orientation"
    );

    // A 3-4-5 heading: neither axis-aligned nor a unit-length accident, so
    // an implementation that defaulted the orientation to an identity matrix
    // (or copied the sun direction) fails here.
    assert!((heading.x() - 0.6).abs() < 1e-12, "heading.x");
    assert!(
        (heading.y()).abs() < 1e-12,
        "heading.y stays on the horizon plane"
    );
    assert!((heading.z() - 0.8).abs() < 1e-12, "heading.z");

    let sun = frame
        .known_sun_direction()
        .expect("the clear-sky fixture authors a sun");
    assert_ne!(
        [sun.x(), sun.y(), sun.z()],
        [heading.x(), heading.y(), heading.z()],
        "the sun direction is not the sky heading"
    );

    // The orientation is data, not a constant: the storm fixture authors a
    // different heading and its frame reports that one.
    let storm_environment = common::storm();
    let storm_frame = SkyFrame::capture(&storm_environment, &camera);
    let storm_orientation = match storm_frame.sky_orientation() {
        Resolved::Known(known) => &known.value,
        Resolved::Unknown { .. } => panic!("the storm fixture authors an orientation"),
    };
    assert_eq!(storm_orientation.up().to_array(), up.to_array());
    assert_ne!(
        storm_orientation.heading().to_array(),
        heading.to_array(),
        "each environment carries its own authored heading"
    );
}

/// An unresolved value stays unresolved in the frame: an unknown sun is
/// reported with its claim and reason, never replaced by a default, while
/// the independently resolved orientation still arrives.
#[test]
fn accept_f19_a_unknown_sun_stays_unknown_in_the_frame() {
    let environment = common::storm();
    let origin = WorldOrigin::new(OriginEpoch(0), common::world([0.0, 0.0, 0.0]));
    let camera = SpatialAnchor::new(&origin, common::world(CAMERA_START)).expect("finite");

    let frame = SkyFrame::capture(&environment, &camera);
    assert_eq!(
        common::unknown_claim(frame.sun_direction()),
        Some((
            "f19a.fixture.storm.sun",
            "the storm fixture authors no sun direction"
        )),
        "the frame carries the record's unknown verbatim"
    );
    assert_eq!(
        frame.known_sun_direction(),
        None,
        "no default sun is invented"
    );

    // The orientation resolves independently, so an unknown sun does not
    // take the horizon with it.
    assert!(frame.sky_orientation().is_known());
    assert_eq!(
        frame.horizon_normal().map(|normal| normal.to_array()),
        Some([0.0, 1.0, 0.0])
    );

    // The dome is still placed: a missing sun does not stop the sky from
    // being centred on the camera.
    assert!(frame.is_centered_on(camera.world(), SKY_CENTERING_TOLERANCE_M));
}

/// The clear-sky fixture really is the authored record the other tests
/// assume: its sun, orientation and sky texture are known values with
/// designed provenance, and its environment id is the declared key.
#[test]
fn accept_f19_a_clear_sky_fixture_is_fully_authored() {
    let environment = common::clear();
    assert_eq!(environment.id().as_str(), "fixture.clear-sky");
    assert!(environment.sun_direction().is_known());
    assert!(environment.sky_orientation().is_known());
    assert!(environment.sky().texture().is_known());
    assert_eq!(
        environment.origin().label(),
        cs_types::content::Origin::SyntheticFixture.label(),
        "the fixture is development content, never original data"
    );
}
