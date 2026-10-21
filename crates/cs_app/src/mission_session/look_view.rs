//! The look state: snap look `K`, smooth look `J`, track target `L` and the
//! numpad look directions (PARITY3-CAMERA-MODES.03, Rally #1825).
//!
//! The measured behaviour is in
//! `docs/findings/2026-10-21-parity3-camera-modes-01-measured.md` ("Look");
//! provenance `static`, never `verified_original`. What is used as measured:
//!
//! * Commands 55 / 57 / 56 set the look mode: snap (`0`), smooth (`1`, look
//!   state zeroed) and track (`2`, a toggle with snap).
//! * Commands 58-66 (numpad 1-9) are polled while held; `x` is `+1` to the
//!   left, `y` is `+1` upward ([`look_direction`]).
//! * **Snap:** the yaw target is `atan2(x, y)`; the pitch target is pi/2 for
//!   straight up, pi/4 for the four diagonals and 0 otherwise; no key, both 0.
//! * **Smooth:** the targets move at `2 rad/s` along `(x, y)`; the pitch is
//!   clamped to pi/2 and the yaw wrapped.
//! * Both modes smooth the angles the camera reads, `v = t + (v - t) *
//!   exp(-rate * dt)`, pitch rate 3.0 and yaw rate 5.0 (the `exp` reading of
//!   the original's `0x53e2e0` is the finding's, not traced).
//! * **Track:** no selected target, both targets zero; otherwise the vector to
//!   the target in the body frame gives pitch `atan2(y, hypot(x, z))` and yaw
//!   `atan2(-x, -z)`. The target is read from the targeting session's
//!   published reticle; this module never selects, drops or fires at one.
//! * Entering the chase view clears the look, entering a pilot-eye view clears
//!   it too ([`super::view_cycle::select_view`]).
//!
//! **Designed, not measured:** several held direction keys add their vectors
//! (the original reads each key separately); numpad 5 (command 62) held in
//! track mode recentres (both targets zero), the finding records only that the
//! track branch reads it; the joystick hat (`0xffff` centred, else hundredths
//! of a degree) is not read because the input layer has no hat path; the look
//! is applied to the chase view by orbiting the camera about the aircraft and
//! to the pilot-eye views by turning the camera, and not to the external
//! cameras, which the original's look reader is not measured to feed.

use avian3d::prelude::{Position, Rotation};
use bevy::app::{App, PostUpdate, Update};
use bevy::prelude::{
    IntoScheduleConfigs, Quat, Query, Res, ResMut, Resource, Time, Transform, Vec3, With,
};
use bevy::time::Real;
use bevy::transform::TransformSystems;

use super::cockpit_camera::drive_cockpit_camera;
use super::compose::MissionPlayerBody;
use super::external_view::drive_external_camera;
use super::launch_cinematic::LaunchCinematic;
use super::view_cycle::MissionViewMode;
use crate::playtest::PlaytestCameraMarker;
use crate::targeting::TargetingSession;

/// `Snap look` (`K`).
pub const SNAP_LOOK_COMMAND_ID: u16 = 55;
/// `Track target` (`L`).
pub const TRACK_TARGET_COMMAND_ID: u16 = 56;
/// `Smooth look` (`J`).
pub const SMOOTH_LOOK_COMMAND_ID: u16 = 57;
/// The first look-direction command (numpad 1, down-left).
pub const LOOK_FIRST_COMMAND_ID: u16 = 58;
/// The middle look command (numpad 5).
pub const LOOK_MIDDLE_COMMAND_ID: u16 = 62;
/// The last look-direction command (numpad 9, up-right).
pub const LOOK_LAST_COMMAND_ID: u16 = 66;

/// Smooth-look rate of the targets, radians per second (`0x42d010`).
pub const SMOOTH_LOOK_RATE: f32 = 2.0;
/// Rate of the smoothed pitch towards its target (`0x42d827`).
pub const PITCH_SMOOTH_RATE: f32 = 3.0;
/// Rate of the smoothed yaw towards its target (`0x42d867`).
pub const YAW_SMOOTH_RATE: f32 = 5.0;
/// The pitch limit the chase and eye callers pass, radians.
pub const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2;

/// The original's look mode word (`[0x64ef68]`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LookMode {
    /// `0`: the direction keys set the targets outright.
    #[default]
    Snap,
    /// `1`: the direction keys move the targets.
    Smooth,
    /// `2`: the targets follow the selected target.
    Track,
}

/// The `(x, y)` of a look command, `x` `+1` left and `y` `+1` up; `None` for a
/// command that is not a direction. The middle key is `(0, 0)`.
#[must_use]
pub const fn look_direction(command_id: u16) -> Option<(i8, i8)> {
    Some(match command_id {
        58 => (1, -1),
        59 => (0, -1),
        60 => (-1, -1),
        61 => (1, 0),
        62 => (0, 0),
        63 => (-1, 0),
        64 => (1, 1),
        65 => (0, 1),
        66 => (-1, 1),
        _ => return None,
    })
}

/// What one frame of look reads besides the keys: the vector to the selected
/// target in the body frame (`x` right, `y` up, `-z` forward), `None` with no
/// target.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LookTarget(pub Option<Vec3>);

/// The look state the cameras read.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct MissionLook {
    /// The look mode.
    pub mode: LookMode,
    /// The pitch target, radians, up positive.
    pub pitch_target: f32,
    /// The yaw target, radians, left positive.
    pub yaw_target: f32,
    /// The smoothed pitch the camera reads.
    pub pitch: f32,
    /// The smoothed yaw the camera reads.
    pub yaw: f32,
    /// The summed `(x, y)` of the direction keys held this frame.
    pub held: (i8, i8),
    /// Whether the middle key is held this frame.
    pub centre_held: bool,
}

impl MissionLook {
    /// Applies a look-mode key (`55`, `56`, `57`); other commands do nothing.
    pub fn press(&mut self, command_id: u16) {
        match command_id {
            SNAP_LOOK_COMMAND_ID => self.mode = LookMode::Snap,
            SMOOTH_LOOK_COMMAND_ID => {
                self.mode = LookMode::Smooth;
                self.clear();
            }
            TRACK_TARGET_COMMAND_ID => {
                self.mode = if self.mode == LookMode::Track {
                    LookMode::Snap
                } else {
                    LookMode::Track
                };
            }
            _ => {}
        }
    }

    /// Zeroes the targets and the smoothed angles (the mode is kept).
    pub fn clear(&mut self) {
        self.pitch_target = 0.0;
        self.yaw_target = 0.0;
        self.pitch = 0.0;
        self.yaw = 0.0;
    }

    /// Whether the camera has anything to turn by.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.pitch.abs() > 1e-6 || self.yaw.abs() > 1e-6
    }

    /// The camera's look rotation in the body frame: yaw about up, then pitch
    /// about right.
    #[must_use]
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch)
    }

    /// One frame of `dt` seconds: moves the targets by the mode, then smooths.
    pub fn step(&mut self, dt: f32, target: LookTarget) {
        let (x, y) = (f32::from(self.held.0), f32::from(self.held.1));
        match self.mode {
            LookMode::Snap => {
                if self.held == (0, 0) {
                    self.yaw_target = 0.0;
                    self.pitch_target = 0.0;
                } else {
                    self.yaw_target = x.atan2(y);
                    self.pitch_target = match (self.held.0 != 0, self.held.1) {
                        (false, 1) => PITCH_LIMIT,
                        (true, 1 | -1) => PITCH_LIMIT / 2.0,
                        _ => 0.0,
                    };
                }
            }
            LookMode::Smooth => {
                self.pitch_target = (self.pitch_target + SMOOTH_LOOK_RATE * dt * y)
                    .clamp(-PITCH_LIMIT, PITCH_LIMIT);
                self.yaw_target = wrap_pi(self.yaw_target + SMOOTH_LOOK_RATE * dt * x);
            }
            LookMode::Track => match target.0 {
                Some(to) if !self.centre_held => {
                    self.pitch_target =
                        to.y.atan2(to.x.hypot(to.z))
                            .clamp(-PITCH_LIMIT, PITCH_LIMIT);
                    self.yaw_target = (-to.x).atan2(-to.z);
                }
                _ => {
                    self.pitch_target = 0.0;
                    self.yaw_target = 0.0;
                }
            },
        }
        self.pitch = smooth(self.pitch, self.pitch_target, PITCH_SMOOTH_RATE, dt);
        self.yaw = smooth(self.yaw, self.yaw_target, YAW_SMOOTH_RATE, dt);
    }
}

/// `v = target + (v - target) * exp(-rate * dt)`.
fn smooth(value: f32, target: f32, rate: f32, dt: f32) -> f32 {
    target + (value - target) * (-rate * dt).exp()
}

/// Wraps an angle to `(-pi, pi]`.
fn wrap_pi(angle: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let wrapped = angle.rem_euclid(tau);
    if wrapped > std::f32::consts::PI {
        wrapped - tau
    } else {
        wrapped
    }
}

/// The vector from the player to the session's selected target, in the body
/// frame; `None` when the session publishes no reticle. Read-only.
#[must_use]
pub fn selected_target_in_body_frame(
    targeting: Option<&TargetingSession>,
    position: Vec3,
    rotation: Quat,
) -> LookTarget {
    let reticle = targeting
        .and_then(TargetingSession::last_phase)
        .and_then(|phase| phase.reticle.as_ref());
    LookTarget(reticle.map(|reticle| {
        let world = Vec3::new(
            reticle.position.x() as f32,
            reticle.position.y() as f32,
            reticle.position.z() as f32,
        );
        rotation.inverse() * (world - position)
    }))
}

/// Steps the look by the real frame time.
pub fn step_look(
    time: Res<Time<Real>>,
    targeting: Option<Res<TargetingSession>>,
    bodies: Query<(&Position, &Rotation), With<MissionPlayerBody>>,
    mut look: ResMut<MissionLook>,
) {
    let target = bodies
        .single()
        .map(|(position, rotation)| {
            selected_target_in_body_frame(targeting.as_deref(), position.0, rotation.0)
        })
        .unwrap_or_default();
    look.step(time.delta_secs(), target);
}

/// Turns the camera by the look: the chase view orbits about the aircraft, a
/// pilot-eye view turns in place. Runs after the views placed the camera, so
/// the offset never accumulates.
pub fn apply_look(
    view: Res<MissionViewMode>,
    look: Res<MissionLook>,
    cinematic: Option<Res<LaunchCinematic>>,
    bodies: Query<(&Position, &Rotation), With<MissionPlayerBody>>,
    mut cameras: Query<&mut Transform, With<PlaytestCameraMarker>>,
) {
    if !look.is_active() || cinematic.is_some_and(|phase| phase.is_playing()) {
        return;
    }
    let Ok((position, rotation)) = bodies.single() else {
        return;
    };
    let (position, rotation) = (position.0, rotation.0);
    let turn = look.rotation();
    for mut transform in &mut cameras {
        match *view {
            MissionViewMode::Chase => {
                let offset = transform.translation - position;
                let eye = position + rotation * (turn * (rotation.inverse() * offset));
                *transform = super::external_view::look_at(eye, position);
            }
            eye_view if eye_view.at_pilot_eye() => {
                transform.rotation = rotation * turn;
            }
            _ => {}
        }
    }
}

/// Installs the look state and its two systems.
pub fn install_look_view(app: &mut App) {
    app.init_resource::<MissionLook>();
    app.add_systems(Update, step_look);
    app.add_systems(
        PostUpdate,
        apply_look
            .after(drive_cockpit_camera)
            .after(drive_external_camera)
            .before(TransformSystems::Propagate),
    );
}
