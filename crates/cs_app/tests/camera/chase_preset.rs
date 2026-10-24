//! PARITY-M01-PLAYER-VIEW-A (#1666): the mission window's chase view, fitted
//! to the owner's capture of the original.
//!
//! The values are **designed** (the original's are unrecovered, see
//! `docs/findings/2026-10-18-parity-m01-player-chase-camera.md`); these tests
//! pin what the capture fixes: the aircraft sits astern of the eye, level, and
//! its body origin drops below the view axis by the capture's fraction of the
//! image height. The numbers come out of the production rig and its
//! projection, not a formula of the test's own.

use std::time::Duration;

use cs_app::camera::chase_preset::{
    CAPTURE_FUSELAGE_DROP_FRACTION, CHASE_ABOVE_M, CHASE_ASTERN_M,
    declared_capture_fitted_chase_modes,
};
use cs_app::camera::{CameraRig, RigInputs, ViewRig, lower_camera_modes};
use cs_app::origin::OriginChange;
use cs_content::cameras::{
    AspectRatio, CameraModeKind, DeclaredPlacement, declared_synthetic_camera_modes,
};
use cs_sim::damage::ActorId;
use cs_types::Tick;
use cs_types::content::Resolved;
use cs_types::evidence::ClaimStatus;
use cs_types::net::SessionId;
use cs_types::space::Quaternion;

use crate::camera::common::{aircraft_pose, assert_close, assert_close_position, world};

fn frame() -> cs_app::camera::RigFrame {
    let mut rig = CameraRig::new(
        lower_camera_modes(&declared_capture_fitted_chase_modes()).expect("the preset lowers"),
    )
    .expect("the preset's default mode has a rig");
    rig.set_rig(ViewRig::Chase).expect("the chase rig exists");
    rig.resolve(&RigInputs {
        at: Tick(10),
        subject: ActorId {
            session: SessionId::new(7).expect("a nonzero session generation"),
            serial: 1,
        },
        aircraft: aircraft_pose([0.0, 0.0, 0.0], Quaternion::IDENTITY),
        aspect: AspectRatio::SIXTEEN_NINE,
        elapsed: Duration::from_millis(16),
        look: None,
        spyglass: None,
        origin_change: OriginChange::Teleport,
    })
    .expect("the chase resolves")
}

#[test]
fn accept_parity_m01_player_view_a_eye_is_astern_and_above_at_the_fitted_offset() {
    let frame = frame();
    assert_eq!(frame.mode, CameraModeKind::External);
    assert_close_position(
        frame.pose.position(),
        [0.0, CHASE_ABOVE_M, CHASE_ASTERN_M],
        1e-9,
        "the eye is the fitted offset behind the level aircraft",
    );
    let basis = frame.pose.basis().expect("a usable basis");
    assert_close_position(
        world(basis.forward().to_array()),
        [0.0, 0.0, -1.0],
        1e-12,
        "the view is horizon-level along the aircraft's forward axis",
    );
}

#[test]
fn accept_parity_m01_player_view_a_body_origin_drops_by_the_captures_fraction() {
    let frame = frame();
    let framing = frame
        .framing_of(world([0.0, 0.0, 0.0]))
        .expect("the aircraft is in front of the eye");
    // Normalised y runs from -1 (bottom) to +1 (top): a drop of `f` of the
    // image height is y = -2f. Stated tolerance: 0.03 of the image height.
    assert_close(
        framing.y(),
        -2.0 * CAPTURE_FUSELAGE_DROP_FRACTION,
        0.06,
        "the body origin sits where the capture's fuselage does",
    );
    assert_close(framing.x(), 0.0, 1e-9, "and dead centre horizontally");
}

#[test]
fn accept_parity_m01_player_view_a_the_synthetic_fixture_keeps_its_12_by_3_chase() {
    let fitted = declared_capture_fitted_chase_modes();
    let fixture = declared_synthetic_camera_modes();
    let chase = |set: &cs_content::cameras::DeclaredCameraModes| match set
        .get(CameraModeKind::External)
        .expect("an external mode")
        .placement()
    {
        DeclaredPlacement::BodyOffset(offset) => *offset,
        DeclaredPlacement::Cockpit(_) => unreachable!("the external mode is a body offset"),
    };
    assert_ne!(chase(&fitted), chase(&fixture));
    assert_eq!(chase(&fixture).forward_m().0, -12.0);
    assert_eq!(fitted.origin(), fixture.origin(), "still labelled designed");
}

#[test]
fn accept_parity2_m01_chase_camera_fov_is_the_measured_vertical_angle() {
    let modes = declared_capture_fitted_chase_modes();
    let chase = modes
        .get(CameraModeKind::External)
        .expect("an external mode");
    let Resolved::Known(fov) = chase.projection().fov.clone() else {
        panic!("the chase field of view is known");
    };
    // 60 degrees horizontal on the fixed 4:3 frame: 2 atan(0.75 tan 30 deg).
    assert!(
        (fov.value.0.to_degrees() - 46.827).abs() < 1e-2,
        "{}",
        fov.value.0.to_degrees()
    );
    assert_eq!(fov.provenance.class, ClaimStatus::ObservedTool);
    let fixture = declared_synthetic_camera_modes();
    let designed = fixture
        .get(CameraModeKind::External)
        .expect("an external mode")
        .projection()
        .fov
        .clone()
        .known()
        .expect("known");
    assert_ne!(fov.value, designed, "no longer the designed 55 degrees");
}
