//! The camera-mode stage of the owner-command smoke (task #1826
//! `PARITY3-CAMERA-MODES.04`).
//!
//! After the `F1` keymap overlay the scripted run walks the camera keys the
//! owner's original external frames show: `F7`, `F9`-`F12`, the `F8` cockpit
//! cycle, the zoom keys, `K`/`J`/`L` and the numpad look directions. Each
//! stage presses its keys through the same keyboard path the player uses,
//! reads the view, look and zoom state plus the camera's pose relative to the
//! aircraft, and takes a real framebuffer (`frame_*_cam_<stage>.png`).
//!
//! The stage lengths and the pass floors are **designed** and named beside
//! their constants. Nothing here is `verified_original`; the frames are for
//! comparing with the owner's external frames by eye.

use avian3d::prelude::{Position, Rotation};
use bevy::prelude::{ClearColor, Commands, GlobalTransform, KeyCode, Query, Res, ResMut, With};

use super::super::compose::MissionPlayerBody;
use super::super::look_view::{LookMode, MissionLook};
use super::super::view_cycle::{MissionViewMode, MissionZoom};
use super::{
    Check, FrameRecord, KeyOp, KeyQueue, Phase, SmokeConfig, SmokeData, SmokeShots,
    clear_colour_encodings, request_screenshot,
};

/// Frames one stage lasts (about 0.83 s at 60 Hz).
const STAGE_FRAMES: u32 = 50;
/// Frames into a stage at which the held keys are released.
const RELEASE_OFFSET: u32 = 44;
/// Frames into a stage at which the state is read and the frame is taken.
/// **Designed:** the look smoother (rate 3 and 5 per second) is within 10 % of
/// its target about 0.75 s after a key goes down, and the held keys are still
/// down here.
const SAMPLE_OFFSET: u32 = 40;
/// The least distinct luminance levels a camera-mode frame must show.
/// **Designed:** a flat or blank frame has one or two.
pub const MIN_CAMERA_FRAME_LUMINANCE_LEVELS: usize = 16;
/// The least distance, metres, between two external cameras' offsets from the
/// aircraft for the two views to count as different. **Designed.**
pub const MIN_VIEW_SEPARATION_M: f32 = 1.0;
/// The least zoom-out that must show in the chase camera's distance, metres.
/// **Designed:** the full-scale zoom adds 10 m, the held key reaches about a
/// third of it.
pub const MIN_ZOOM_DISTANCE_M: f32 = 0.5;
/// The least look angle, radians, a held snap/smooth direction must reach.
/// **Designed:** snap targets are at least pi/4 and the smoother is 85 % of
/// the way there at the sample.
pub const MIN_LOOK_ANGLE_RAD: f32 = 0.5;

/// What a stage expects the zoom to be doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZoomExpect {
    /// The stage does not touch the zoom.
    Untouched,
    /// The zoom-out key is held: the smoothed zoom has opened.
    Out,
    /// The zoom-in key is held: the target is back at zero and the smoothed
    /// zoom is below the zoom-out stage's.
    In,
}

/// What a stage expects the look to be doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LookExpect {
    /// No expectation (the look is not exercised).
    Any,
    /// The look mode, and no turn at all.
    ModeOnly(LookMode),
    /// The look mode and a pitch above [`MIN_LOOK_ANGLE_RAD`].
    Pitched(LookMode),
    /// The look mode and a yaw beyond [`MIN_LOOK_ANGLE_RAD`] either way.
    Yawed(LookMode),
}

/// One stage of the camera walk.
struct Stage {
    label: &'static str,
    /// The keys pressed at the stage's first frame.
    press: &'static [KeyCode],
    /// Frames they stay down (the rest stay down to [`RELEASE_OFFSET`]).
    hold: u32,
    view: MissionViewMode,
    look: LookExpect,
    zoom: ZoomExpect,
}

/// The walk, in order. `F8` from an external view returns to the remembered
/// cycle mode: the cockpit phase before this stage ends the cycle on the chase
/// view, so that is where it lands; the last stage's `F8` then enters the eye.
const STAGES: &[Stage] = &[
    Stage {
        label: "chase",
        press: &[],
        hold: 0,
        view: MissionViewMode::Chase,
        look: LookExpect::Any,
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "f7_held_point",
        press: &[KeyCode::F7],
        hold: 3,
        view: MissionViewMode::HeldPoint,
        look: LookExpect::Any,
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "f9_down",
        press: &[KeyCode::F9],
        hold: 3,
        view: MissionViewMode::ExternalDown,
        look: LookExpect::Any,
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "f10_back",
        press: &[KeyCode::F10],
        hold: 3,
        view: MissionViewMode::ExternalBack,
        look: LookExpect::Any,
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "f11_left",
        press: &[KeyCode::F11],
        hold: 3,
        view: MissionViewMode::ExternalLeft,
        look: LookExpect::Any,
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "f12_right",
        press: &[KeyCode::F12],
        hold: 3,
        view: MissionViewMode::ExternalRight,
        look: LookExpect::Any,
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "f8_remembered_view",
        press: &[KeyCode::F8],
        hold: 3,
        view: MissionViewMode::Chase,
        look: LookExpect::Any,
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "zoom_out",
        press: &[KeyCode::NumpadSubtract],
        hold: RELEASE_OFFSET,
        view: MissionViewMode::Chase,
        look: LookExpect::Any,
        zoom: ZoomExpect::Out,
    },
    Stage {
        label: "zoom_in",
        press: &[KeyCode::NumpadAdd],
        hold: RELEASE_OFFSET,
        view: MissionViewMode::Chase,
        look: LookExpect::Any,
        zoom: ZoomExpect::In,
    },
    Stage {
        label: "k_snap_look_up",
        press: &[KeyCode::KeyK, KeyCode::Numpad8],
        hold: RELEASE_OFFSET,
        view: MissionViewMode::Chase,
        look: LookExpect::Pitched(LookMode::Snap),
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "k_snap_look_side",
        press: &[KeyCode::Numpad4],
        hold: RELEASE_OFFSET,
        view: MissionViewMode::Chase,
        look: LookExpect::Yawed(LookMode::Snap),
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "j_smooth_look",
        press: &[KeyCode::KeyJ, KeyCode::Numpad6],
        hold: RELEASE_OFFSET,
        view: MissionViewMode::Chase,
        look: LookExpect::Yawed(LookMode::Smooth),
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "l_track_target",
        press: &[KeyCode::KeyL],
        hold: 3,
        view: MissionViewMode::Chase,
        look: LookExpect::ModeOnly(LookMode::Track),
        zoom: ZoomExpect::Untouched,
    },
    Stage {
        label: "eye_snap_look_side",
        press: &[KeyCode::F8, KeyCode::KeyK, KeyCode::Numpad4],
        hold: RELEASE_OFFSET,
        view: MissionViewMode::Cockpit,
        look: LookExpect::Yawed(LookMode::Snap),
        zoom: ZoomExpect::Untouched,
    },
];

/// The shot label of a stage.
fn shot_label(stage: &Stage) -> String {
    format!("cam_{}", stage.label)
}

/// What one stage read at its sample frame.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraModeSample {
    /// The stage's name.
    pub label: String,
    /// The view mode the mission held.
    pub view: MissionViewMode,
    /// The look mode.
    pub look_mode: LookMode,
    /// The smoothed look pitch, radians.
    pub look_pitch: f32,
    /// The smoothed look yaw, radians.
    pub look_yaw: f32,
    /// The zoom target.
    pub zoom_target: f32,
    /// The smoothed zoom.
    pub zoom_smoothed: f32,
    /// The camera's offset from the aircraft in the aircraft's frame, metres.
    pub offset_body_m: [f32; 3],
    /// Distinct luminance levels in the stage's framebuffer (`None`: never
    /// delivered).
    pub distinct_luminance: Option<usize>,
}

impl CameraModeSample {
    fn distance_m(&self) -> f32 {
        let [x, y, z] = self.offset_body_m;
        (x * x + y * y + z * z).sqrt()
    }

    fn separation_m(&self, other: &Self) -> f32 {
        let (a, b) = (self.offset_body_m, other.offset_body_m);
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    }
}

/// Everything the camera stage measured.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraModeMetrics {
    /// The samples, in stage order.
    pub samples: Vec<CameraModeSample>,
}

/// Copies each stage's framebuffer measure onto its sample.
pub(super) fn attach_frames(metrics: &mut CameraModeMetrics, shots: &[FrameRecord]) {
    for sample in &mut metrics.samples {
        let wanted = format!("cam_{}", sample.label);
        sample.distinct_luminance = shots
            .iter()
            .find(|shot| shot.label == wanted)
            .map(|shot| shot.distinct_luminance);
    }
}

/// Runs the camera stage: presses the stage's keys, reads the state, takes
/// the frame. Ends by handing the run to [`Phase::Final`].
#[allow(clippy::too_many_arguments)]
pub(super) fn smoke_camera_modes(
    config: Res<SmokeConfig>,
    mut data: ResMut<SmokeData>,
    mut queue: ResMut<KeyQueue>,
    shots: Res<SmokeShots>,
    mut commands: Commands,
    clear: Option<Res<ClearColor>>,
    view: Option<Res<MissionViewMode>>,
    look: Option<Res<MissionLook>>,
    zoom: Option<Res<MissionZoom>>,
    player: Query<(&Position, &Rotation), With<MissionPlayerBody>>,
    camera: Query<&GlobalTransform, With<crate::playtest::PlaytestCameraMarker>>,
) {
    if data.phase != Phase::Cameras {
        return;
    }
    let frame = data.frame;
    let at = frame.saturating_sub(data.phase_start);
    let index = (at / STAGE_FRAMES) as usize;
    let local = at % STAGE_FRAMES;
    let Some(stage) = STAGES.get(index) else {
        data.phase = Phase::Final;
        data.phase_start = frame + 1;
        return;
    };
    if local == 0 {
        queue
            .0
            .extend(stage.press.iter().map(|key| KeyOp::Press(*key)));
    }
    if stage.hold != 0 && local == stage.hold {
        queue
            .0
            .extend(stage.press.iter().map(|key| KeyOp::Release(*key)));
    }
    if local != SAMPLE_OFFSET {
        return;
    }
    if let (Some(view), Some(look), Some(zoom), Ok((position, rotation)), Ok(eye)) = (
        view.as_deref(),
        look.as_deref(),
        zoom.as_deref(),
        player.single(),
        camera.single(),
    ) {
        let offset = rotation.0.inverse() * (eye.translation() - position.0);
        data.measurements
            .camera_modes
            .samples
            .push(CameraModeSample {
                label: stage.label.to_owned(),
                view: *view,
                look_mode: look.mode,
                look_pitch: look.pitch,
                look_yaw: look.yaw,
                zoom_target: zoom.target,
                zoom_smoothed: zoom.smoothed,
                offset_body_m: offset.to_array(),
                distinct_luminance: None,
            });
    }
    let label = shot_label(stage);
    data.shots_requested += 1;
    data.requested.push(label.clone());
    let path = config
        .request
        .capture_dir
        .join(format!("frame_{frame:05}_{label}.png"));
    let clear_colours = clear.as_deref().map(clear_colour_encodings);
    request_screenshot(
        &mut commands,
        shots.clone(),
        path,
        label,
        clear_colours,
        None,
        None,
    );
}

/// Judges the camera stage: every stage's state, the views' separation and
/// the frames.
pub(super) fn evaluate(m: &super::SmokeMeasurements) -> Vec<Check> {
    let samples = &m.camera_modes.samples;
    let sample = |label: &str| samples.iter().find(|s| s.label == label);
    let mut checks = Vec::new();
    for stage in STAGES {
        let id = format!("camera_{}", stage.label);
        let Some(s) = sample(stage.label) else {
            checks.push(Check {
                id,
                applies: true,
                passed: false,
                detail: "the camera stage never read this view".to_owned(),
            });
            continue;
        };
        let look_ok = match stage.look {
            LookExpect::Any => true,
            LookExpect::ModeOnly(mode) => s.look_mode == mode,
            LookExpect::Pitched(mode) => s.look_mode == mode && s.look_pitch > MIN_LOOK_ANGLE_RAD,
            LookExpect::Yawed(mode) => s.look_mode == mode && s.look_yaw.abs() > MIN_LOOK_ANGLE_RAD,
        };
        let zoom_ok = match stage.zoom {
            ZoomExpect::Untouched => true,
            ZoomExpect::Out => {
                let chase = sample("chase");
                s.zoom_smoothed > 0.05
                    && chase.is_some_and(|c| s.distance_m() > c.distance_m() + MIN_ZOOM_DISTANCE_M)
            }
            ZoomExpect::In => {
                s.zoom_target <= 1e-4
                    && sample("zoom_out").is_some_and(|out| s.zoom_smoothed < out.zoom_smoothed)
            }
        };
        let drawn = s
            .distinct_luminance
            .is_some_and(|levels| levels >= MIN_CAMERA_FRAME_LUMINANCE_LEVELS);
        checks.push(Check {
            id,
            applies: true,
            passed: s.view == stage.view && look_ok && zoom_ok && drawn,
            detail: format!(
                "view {:?} (want {:?}); look {:?} pitch {:.2} yaw {:.2}; zoom target {:.2} smoothed {:.2}; \
                 camera {:.1} m from the aircraft at {:?}; frame luminance levels {:?} (floor {})",
                s.view,
                stage.view,
                s.look_mode,
                s.look_pitch,
                s.look_yaw,
                s.zoom_target,
                s.zoom_smoothed,
                s.distance_m(),
                s.offset_body_m,
                s.distinct_luminance,
                MIN_CAMERA_FRAME_LUMINANCE_LEVELS
            ),
        });
    }
    // The four external cameras and the chase camera sit in different places.
    let views = ["chase", "f9_down", "f10_back", "f11_left", "f12_right"];
    let mut near = Vec::new();
    for (i, a) in views.iter().enumerate() {
        for b in &views[i + 1..] {
            match (sample(a), sample(b)) {
                (Some(sa), Some(sb)) if sa.separation_m(sb) >= MIN_VIEW_SEPARATION_M => {}
                _ => near.push(format!("{a}/{b}")),
            }
        }
    }
    checks.push(Check {
        id: "camera_external_views_differ".to_owned(),
        applies: true,
        passed: near.is_empty(),
        detail: if near.is_empty() {
            format!("every pair of chase, F9, F10, F11 and F12 stands at least {MIN_VIEW_SEPARATION_M} m apart")
        } else {
            format!(
                "views closer than {MIN_VIEW_SEPARATION_M} m or unread: {}",
                near.join(", ")
            )
        },
    });
    checks
}
