//! The external camera views and the zoom (PARITY3-CAMERA-MODES.02, Rally
//! #1824).
//!
//! [`MissionViewMode`] holds the view the view keys selected
//! ([`super::view_cycle`]); this module places the camera for the external
//! ones and applies the chase zoom. The measured facts are in
//! `docs/findings/2026-10-21-parity3-camera-modes-01-measured.md` and the
//! cited rows of `data/owner-static/engine-facts.toml`; provenance `static`,
//! never `verified_original`.
//!
//! # Measured and used
//!
//! * Which key selects which mode ([`MissionViewMode::for_command`]), the keep
//!   table, mode 0's zoom of `smoothed * 10` extra distance and the zoom's
//!   rates and clamp ([`MissionZoom`]); entering the chase view clears the zoom.
//! * Modes 0 and 9 are different modes: `Chase view` (`F7`) selects the
//!   held-point mode 9, not the ordinary behind-the-plane camera.
//!
//! # Designed, not measured
//!
//! The original's distances live in `camparam.zrd`, which nothing in the
//! repository reads yet, and the placement maths of modes 1, 3, 4 and 9 is
//! untraced. Until a loader exists the placements are this project's own,
//! sized from the capture-fitted chase view ([`EXTERNAL_DISTANCE_M`]):
//!
//! * **Down (1):** above and a little astern, looking at the aircraft.
//! * **Back (2):** astern on the body axis, level, looking at the aircraft.
//! * **Left / Right (3, 4):** abeam on the body's own side, looking at the
//!   aircraft. The original flips a side flag with heading changes; not
//!   modelled.
//! * **Held point (9):** the camera stays where it was when the mode was
//!   entered and looks at the aircraft. The original's timer and fly-by
//!   parameters are not modelled.
//!
//! The distance unit is read as metres and the zoom only moves the chase view
//! (mode 0), as measured.

use avian3d::prelude::{Position, Rotation};
use bevy::app::{App, PostUpdate, Update};
use bevy::prelude::{
    IntoScheduleConfigs, Quat, Query, Res, ResMut, Resource, Time, Transform, Vec3, With,
};
use bevy::time::Real;
use bevy::transform::TransformSystems;

use super::compose::MissionPlayerBody;
use super::launch_cinematic::LaunchCinematic;
use super::view_cycle::{CycleMemory, MissionViewMode, MissionZoom};
use crate::camera::chase_preset::CHASE_ASTERN_M;
use crate::playtest::PlaytestCameraMarker;

/// How far the side views sit from the aircraft, metres. **Designed**: the
/// capture-fitted chase distance; the original's `dist` is in `camparam.zrd`.
pub const EXTERNAL_DISTANCE_M: f32 = CHASE_ASTERN_M as f32;

/// How far astern the back view sits, metres. **Designed**: the measured
/// back distance exceeds `dist` (15.5 against 13.0 in the default record);
/// the ratio is kept, the values are not copied.
pub const BACK_DISTANCE_M: f32 = EXTERNAL_DISTANCE_M * (15.5 / 13.0);

/// How far above the aircraft the down view sits, metres. **Designed**.
pub const DOWN_HEIGHT_M: f32 = EXTERNAL_DISTANCE_M * 0.9;

/// How far astern the down view sits, metres. **Designed**.
pub const DOWN_ASTERN_M: f32 = EXTERNAL_DISTANCE_M * 0.3;

/// Where the held-point camera (mode 9) stays: the world point it had when the
/// mode was entered. Cleared whenever the view is not mode 9.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct HeldPoint(pub Option<Vec3>);

/// The camera position for an external view of a body at `position` turned by
/// `rotation`, `None` for a view this function does not place (the chase,
/// the cockpit and the held point).
#[must_use]
pub fn external_eye(view: MissionViewMode, position: Vec3, rotation: Quat) -> Option<Vec3> {
    let forward = rotation * Vec3::NEG_Z;
    let right = rotation * Vec3::X;
    let flat_back = Vec3::new(-forward.x, 0.0, -forward.z).normalize_or(-forward);
    Some(match view {
        MissionViewMode::ExternalDown => {
            position + Vec3::Y * DOWN_HEIGHT_M + flat_back * DOWN_ASTERN_M
        }
        MissionViewMode::ExternalBack => position + flat_back * BACK_DISTANCE_M,
        MissionViewMode::ExternalLeft => position - right * EXTERNAL_DISTANCE_M,
        MissionViewMode::ExternalRight => position + right * EXTERNAL_DISTANCE_M,
        _ => return None,
    })
}

/// A camera at `eye` looking at `target`, with the world's up.
#[must_use]
pub fn look_at(eye: Vec3, target: Vec3) -> Transform {
    let mut transform = Transform::from_translation(eye);
    if (target - eye).length_squared() > 1e-6 {
        let up = if (target - eye).normalize().dot(Vec3::Y).abs() > 0.999 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        transform.look_at(target, up);
    }
    transform
}

/// Steps the zoom by the real frame time.
pub fn step_zoom(time: Res<Time<Real>>, mut zoom: ResMut<MissionZoom>) {
    zoom.step(time.delta_secs());
}

/// Installs the zoom and the external camera driver.
pub fn install_external_view(app: &mut App) {
    app.init_resource::<MissionViewMode>()
        .init_resource::<CycleMemory>()
        .init_resource::<MissionZoom>()
        .init_resource::<HeldPoint>();
    app.add_systems(Update, step_zoom);
    app.add_systems(
        PostUpdate,
        drive_external_camera
            .before(TransformSystems::Propagate)
            // The crash camera overrides a pilot-eye pose set in the frame
            // the crash happens.
            .after(super::cockpit_camera::drive_cockpit_camera),
    );
}

/// Places the camera for the external views and pulls the chase view out by
/// the zoom; after a fatal crash it holds the detached crash camera
/// (`CrashSequence`). The chase rig (`follow_camera`) has already written the camera
/// for the frame; the zoom scales that frame's offset from the aircraft, so it
/// never accumulates. With no player body (destroyed, being swapped) nothing
/// is touched.
pub fn drive_external_camera(
    view: Res<MissionViewMode>,
    zoom: Res<MissionZoom>,
    cinematic: Option<Res<LaunchCinematic>>,
    crash: Option<Res<crate::playtest::fatal_contact::CrashSequence>>,
    mut held: ResMut<HeldPoint>,
    bodies: Query<(&Position, &Rotation), With<MissionPlayerBody>>,
    mut cameras: Query<&mut Transform, With<PlaytestCameraMarker>>,
) {
    if *view != MissionViewMode::HeldPoint {
        held.0 = None;
    }
    if cinematic.is_some_and(|phase| phase.is_playing()) {
        return;
    }
    let Ok((position, rotation)) = bodies.single() else {
        return;
    };
    let (position, rotation) = (position.0, rotation.0);
    for mut transform in &mut cameras {
        // The crash camera is held at one point and looks at the wreck; the
        // view keys are inert from the crash on.
        if let Some(crash) = crash.as_deref() {
            *transform = look_at(crash.camera_m, position);
            continue;
        }
        match *view {
            MissionViewMode::Chase => {
                let extra = zoom.extra_distance_m();
                let offset = transform.translation - position;
                let length = offset.length();
                if extra > 0.0 && length > 1e-3 {
                    transform.translation = position + offset * ((length + extra) / length);
                }
            }
            MissionViewMode::HeldPoint => {
                let point = *held.0.get_or_insert(transform.translation);
                *transform = look_at(point, position);
            }
            external if external.is_external_camera() => {
                if let Some(eye) = external_eye(external, position, rotation) {
                    *transform = look_at(eye, position);
                }
            }
            _ => {}
        }
    }
}
