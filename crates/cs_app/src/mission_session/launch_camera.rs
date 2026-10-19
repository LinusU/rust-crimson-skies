//! The launch cinematic's camera in the session (task #1665
//! `PARITY-M01-LC-CAMERA`, slice iv of #1642).
//!
//! While [`super::launch_cinematic::LaunchCinematic`] plays, the window's
//! camera shows the measured `gi_cam*` script
//! ([`crate::cinematics::launch_camera`]): the pose of cinematic time
//! `elapsed_ticks / BASELINE_FIXED_HZ`, in the frame of the Pandora
//! (`piratezep`), put into the world through the Pandora's live
//! `GlobalTransform`. Control returns to the chase rig on the tick the phase
//! ends; the rig kept resolving underneath, so there is no swing.
//!
//! The camera's forward axis is `-Z` and its rotation is the part's unit
//! quaternion as is: measured on the data (the forward vector points at the
//! Pandora's origin for the scenes that look at it,
//! `docs/findings/2026-10-19-m01-launch-camera.md`). The field of view is
//! **not** measured and the camera keeps the window's own projection.

use bevy::app::{App, PostUpdate};
use bevy::prelude::{
    GlobalTransform, IntoScheduleConfigs, Query, Res, ResMut, Resource, Transform, With,
};
use bevy::transform::TransformSystems;

use super::launch_cinematic::LaunchCinematic;
use crate::cinematics::launch_camera::LaunchCameraScript;
use crate::mission_actors::MissionZeppelinActor;
use crate::physics::BASELINE_FIXED_HZ;
use crate::playtest::PlaytestCameraMarker;
use crate::world_actors::WorldActorBinding;

/// The node the camera's frame is.
pub const CAMERA_FRAME_NODE: &str = crate::cinematics::launch_parent::PANDORA_NODE;

/// The script that plays, and what the run saw of it.
#[derive(Resource, Clone, Debug)]
pub struct LaunchCameraPlayback {
    script: LaunchCameraScript,
    frames_driven: u64,
    frames_without_frame_node: u64,
    scenes_seen: Vec<usize>,
}

/// What a playback measured, for the smoke's check.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LaunchCameraStats {
    /// Rendered frames the camera was driven by the script.
    pub frames_driven: u64,
    /// Frames the phase played but the Pandora's entity was not found.
    pub frames_without_frame_node: u64,
    /// Scenes of the script that were on screen at least one frame.
    pub scenes_seen: usize,
    /// Scenes the script has.
    pub scenes_total: usize,
}

impl LaunchCameraPlayback {
    /// A playback of `script`.
    #[must_use]
    pub fn new(script: LaunchCameraScript) -> Self {
        Self {
            script,
            frames_driven: 0,
            frames_without_frame_node: 0,
            scenes_seen: Vec::new(),
        }
    }

    /// The script.
    #[must_use]
    pub fn script(&self) -> &LaunchCameraScript {
        &self.script
    }

    /// What the run saw.
    #[must_use]
    pub fn stats(&self) -> LaunchCameraStats {
        LaunchCameraStats {
            frames_driven: self.frames_driven,
            frames_without_frame_node: self.frames_without_frame_node,
            scenes_seen: self.scenes_seen.len(),
            scenes_total: self.script.scenes().len(),
        }
    }
}

/// Puts `local` (a pose in `frame`'s space) into the world. The frame's scale
/// is dropped: a camera is a rigid pose.
#[must_use]
pub fn camera_in_world(
    frame: &GlobalTransform,
    position: [f32; 3],
    quaternion: [f64; 4],
) -> Transform {
    let (_, rotation, translation) = frame.to_scale_rotation_translation();
    let [w, x, y, z] = quaternion;
    let local = bevy::math::Quat::from_xyzw(x as f32, y as f32, z as f32, w as f32).normalize();
    Transform {
        translation: translation + rotation * bevy::math::Vec3::from_array(position),
        rotation: rotation * local,
        ..Transform::IDENTITY
    }
}

/// Installs the playback after the phase is installed.
pub fn install_launch_camera(app: &mut App, script: LaunchCameraScript) {
    app.insert_resource(LaunchCameraPlayback::new(script));
    app.add_systems(
        PostUpdate,
        play_launch_camera.before(TransformSystems::Propagate),
    );
}

fn play_launch_camera(
    phase: Option<Res<LaunchCinematic>>,
    playback: Option<ResMut<LaunchCameraPlayback>>,
    frames: Query<(&MissionZeppelinActor, &WorldActorBinding, &GlobalTransform)>,
    mut cameras: Query<&mut Transform, With<PlaytestCameraMarker>>,
) {
    let (Some(phase), Some(mut playback)) = (phase, playback) else {
        return;
    };
    if !phase.is_playing() {
        return;
    }
    let seconds = phase.elapsed_ticks() as f64 / f64::from(BASELINE_FIXED_HZ);
    let Some(pose) = playback.script.pose_at(seconds) else {
        return;
    };
    let Some(frame) = frames.iter().find_map(|(_, binding, global)| {
        (binding.subject.key().rsplit('.').next() == Some(CAMERA_FRAME_NODE)).then_some(global)
    }) else {
        playback.frames_without_frame_node += 1;
        return;
    };
    let world = camera_in_world(frame, pose.position, pose.quaternion);
    for mut transform in &mut cameras {
        *transform = world;
    }
    playback.frames_driven += 1;
    if !playback.scenes_seen.contains(&pose.scene) {
        playback.scenes_seen.push(pose.scene);
    }
}
