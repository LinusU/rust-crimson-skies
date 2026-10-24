//! The chase view the mission window flies with, fitted to the owner's capture
//! (`PARITY-M01-PLAYER-VIEW-A`, #1666).
//!
//! **The placement is not an original value; the field of view is.** The original's chase distance,
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
//! height below the horizon-level view axis. The projection is the original's
//! measured 46.83° vertical field of view ([`chase_vertical_fov`]),
//! `PreserveVertical` framing, so a fraction of the image *height* is
//! independent of the window's aspect and is what is compared. The distance and
//! height were fitted at the fixture's 55° (10.1 m / 2.1 m) and rescaled by the
//! tangent ratio of the half angles (0.832, height by its square) so the
//! fractions below hold at the measured angle. The fixture's 12 m / 3 m put the same wing at
//! 0.83 and the fuselage at 0.24 of the height, and the first fit (10.9 m /
//! 2.5 m) measured 0.85 in the owner-command smoke. The astern distance is
//! scaled by `0.85 / 0.92` and the height set so the body origin drops by the
//! capture's 0.20 of the height at that distance: the constants below.

use cs_content::cameras::{
    BodyOffset, CameraModeKind, DeclaredCameraMode, DeclaredCameraModes, DeclaredPlacement,
    ProjectionPolicy, declared_synthetic_camera_modes,
};
use cs_types::content::{Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::space::{Meters, Radians};

use crate::mission_session::{COCKPIT_HORIZONTAL_FOV_DEGREES, cockpit_vertical_fov};

/// How far astern of the body origin the chase eye sits, metres. **Designed**,
/// fitted to the owner's capture; the original's value is unknown.
pub const CHASE_ASTERN_M: f64 = 8.4;

/// How far above the body origin the chase eye sits, metres. **Designed**,
/// fitted to the owner's capture; the original's value is unknown.
pub const CHASE_ABOVE_M: f64 = 1.45;

/// The claim the chase view's field of view cites (finding
/// `docs/findings/2026-10-24-parity2-m01-chase-camera-fov.md`).
const CHASE_FOV_CLAIM: &str = "OWNER-STATIC-2026-10-21";

/// The chase view's vertical field of view, radians: the original's 60 degree
/// horizontal angle (`fov.default`, `0x42b660`, every mode but 6) on its fixed
/// 4:3 frame (`fov.vertical_derivation`), 46.83 degrees. **Measured** (static).
#[must_use]
pub fn chase_vertical_fov() -> Radians {
    Radians(f64::from(cockpit_vertical_fov(
        COCKPIT_HORIZONTAL_FOV_DEGREES.to_radians(),
    )))
}

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
                    measured_projection(mode.projection()),
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

/// `projection` with its field of view replaced by the measured one.
fn measured_projection(projection: &ProjectionPolicy) -> ProjectionPolicy {
    let provenance = Provenance::new(
        ClaimId::new(CHASE_FOV_CLAIM).expect("a valid claim id"),
        ClaimStatus::ObservedTool,
        None,
    )
    .expect("an observed-tool provenance needs no source span");
    ProjectionPolicy {
        fov: Resolved::Known(Known::new(chase_vertical_fov(), provenance)),
        ..projection.clone()
    }
}
