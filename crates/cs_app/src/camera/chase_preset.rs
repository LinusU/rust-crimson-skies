//! The chase view the mission window flies with, fitted to the owner's capture
//! (`PARITY-M01-PLAYER-VIEW-A`, #1666).
//!
//! **No original value is recovered here.** The original's chase distance,
//! height, field of view and follow lag are in no file the repository reads
//! and in no row of `data/owner-static/engine-facts.toml`; the finding
//! `docs/findings/2026-10-18-parity-m01-player-chase-camera.md` records each as
//! unknown. What this module declares is a **designed** external placement
//! whose only authority is the owner's capture of the original at +25 s: the
//! capture is the visual target, and the numbers below are the smallest change
//! to the synthetic fixture's 12 m / 3 m that puts the player aircraft in frame
//! the way the capture does.
//!
//! # The fit
//!
//! The capture is 1024×768 and the aircraft's lower wing spans about 705 px,
//! 0.92 of the image height, with the fuselage centre about 0.20 of the image
//! height below the horizon-level view axis. The projection stays the
//! fixture's designed 55° vertical field of view, `PreserveVertical` framing,
//! so a fraction of the image *height* is independent of the window's aspect
//! and is what is compared. The fixture's 12 m / 3 m put the same wing at
//! 0.83 and the fuselage at 0.24 of the height, and the first fit (10.9 m /
//! 2.5 m) measured 0.85 in the owner-command smoke. The astern distance is
//! scaled by `0.85 / 0.92` and the height set so the body origin drops by the
//! capture's 0.20 of the height at that distance: the constants below.

use cs_content::cameras::{
    BodyOffset, CameraModeKind, DeclaredCameraMode, DeclaredCameraModes, DeclaredPlacement,
    declared_synthetic_camera_modes,
};
use cs_types::space::Meters;

/// How far astern of the body origin the chase eye sits, metres. **Designed**,
/// fitted to the owner's capture; the original's value is unknown.
pub const CHASE_ASTERN_M: f64 = 10.1;

/// How far above the body origin the chase eye sits, metres. **Designed**,
/// fitted to the owner's capture; the original's value is unknown.
pub const CHASE_ABOVE_M: f64 = 2.1;

/// The fraction of the image height the aircraft's lower wing spans in the
/// owner's capture of the original (705 px of 768).
pub const CAPTURE_WING_SPAN_FRACTION: f64 = 705.0 / 768.0;

/// The fraction of the image height the fuselage centre sits below the view
/// axis in the capture (≈ 540 px of 768 against the 384 px axis).
pub const CAPTURE_FUSELAGE_DROP_FRACTION: f64 = (540.0 - 384.0) / 768.0;

/// The synthetic mode set with its external (chase) placement replaced by the
/// capture-fitted one. Every other mode, the projection, the clipping planes
/// and the provenance are the fixture's, still labelled designed.
///
/// # Panics
///
/// Never in practice: the fixture is valid and a finite offset stays valid.
#[must_use]
pub fn declared_capture_fitted_chase_modes() -> DeclaredCameraModes {
    let fixture = declared_synthetic_camera_modes();
    let modes = fixture
        .modes()
        .iter()
        .map(|mode| {
            if mode.kind() == CameraModeKind::External {
                DeclaredCameraMode::try_new(
                    mode.kind(),
                    mode.projection().clone(),
                    mode.magnification().clone(),
                    mode.tracks_target().clone(),
                    DeclaredPlacement::BodyOffset(
                        BodyOffset::new(
                            Meters(0.0),
                            Meters(CHASE_ABOVE_M),
                            Meters(-CHASE_ASTERN_M),
                        )
                        .expect("the capture-fitted chase offset is finite"),
                    ),
                    mode.look_limits().clone(),
                )
                .expect("the fixture's external mode stays valid with another offset")
            } else {
                mode.clone()
            }
        })
        .collect();
    DeclaredCameraModes::try_new(
        fixture.subject().clone(),
        fixture.origin().clone(),
        fixture.default_mode(),
        modes,
        fixture.provenance().clone(),
    )
    .expect("the fixture's mode set stays valid with another external offset")
}
