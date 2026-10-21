//! The cockpit eye camera (PARITY3-COCKPIT-VIEW.02, Rally #1810).
//!
//! While [`MissionViewMode`] is a pilot-eye view (original modes 7 and 6, see
//! [`super::view_cycle`]) the mission camera sits at the player body's
//! `cockpit_camera` node and looks along the body's forward with the body's
//! roll, with no chase smoothing. The original reads the node's stored local
//! translation into its eye globals at spawn and transforms it by the
//! aircraft's world matrix each frame (`0x473480`, `0x4d1cc0`, `0x42d980`;
//! `docs/findings/2026-10-11-t542-cockpit-camera-eye-placement.md`), so the
//! eye is `body.pose * stored translation`.
//!
//! **Designed, not measured:** the stored triple's axis map and unit. They are
//! read the way the exterior meshes are: one stored unit is one metre and the
//! axes are the stored ones, turned by the same [`nose_mapping`] yaw
//! ([`crate::playtest_retail::PLAYTEST_UNIT_IS_DESIGNED`]); a stored `-Z` nose
//! and a forward eye offset such as `player_pfighter`'s `z = -0.2` agree. The
//! vertical field of view is not recovered either: the camera keeps the chase
//! view's designed projection. See
//! `docs/findings/2026-10-21-parity3-cockpit-view-02-eye-camera.md`.
//!
//! The exterior airframe is hidden in these views by hiding the body (its
//! parts inherit); the chase rig keeps resolving underneath and is re-seated
//! on the frame the view leaves the cockpit, so it snaps instead of swinging.

use avian3d::prelude::{Position, Rotation};
use bevy::app::{App, PostUpdate};
use bevy::prelude::{
    IntoScheduleConfigs, Local, Quat, Query, Res, ResMut, Resource, Transform, Vec3, Visibility,
    With,
};
use bevy::transform::TransformSystems;
use cs_formats::gamez::GameZNodes;

use super::compose::{MissionCompositionError, MissionPlayerBody};
use super::launch_cinematic::LaunchCinematic;
use super::player_visual::{MISSION_PLAYER_AIRCRAFT_ROOT_NAME, PlayerAirframeSource};
use super::view_cycle::MissionViewMode;
use crate::playtest::{PlaytestCamera, PlaytestCameraMarker};
use crate::playtest_retail::{STORED_AIRCRAFT_NOSE_AXIS, nose_mapping};

/// The airframe node whose stored translation is the pilot's eye.
pub const COCKPIT_EYE_NODE_NAME: &str = "cockpit_camera";

/// The pilot's eye in the player body's frame.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct MissionCockpitEye {
    stored: [f32; 3],
    nose: Quat,
}

impl MissionCockpitEye {
    /// An eye from a stored translation (source units) and the nose mapping.
    #[must_use]
    pub const fn new(stored: [f32; 3], nose: Quat) -> Self {
        Self { stored, nose }
    }

    /// The stored translation, verbatim.
    #[must_use]
    pub const fn stored(&self) -> [f32; 3] {
        self.stored
    }

    /// The eye's offset from the body origin in the body frame, metres.
    #[must_use]
    pub fn body_offset(&self) -> Vec3 {
        self.nose * Vec3::from_array(self.stored)
    }

    /// The camera pose for a body at `position` with `rotation`:
    /// `body.pose * cockpit_camera translation`, looking along the body's
    /// forward with its roll.
    #[must_use]
    pub fn pose(&self, position: Vec3, rotation: Quat) -> Transform {
        Transform {
            translation: position + rotation * self.body_offset(),
            rotation,
            ..Transform::IDENTITY
        }
    }
}

/// Finds the one `cockpit_camera` object node under the root named
/// `root_name` and returns its stored translation.
///
/// # Errors
///
/// [`MissionCompositionError::AirframeVisual`] when the root, the node or an
/// object record is missing, or when several nodes share the name (the
/// original's last-wins tiebreak is not claimed).
pub fn find_cockpit_eye(
    nodes: &GameZNodes,
    root_name: &str,
) -> Result<[f32; 3], MissionCompositionError> {
    let refuse = |reason: String| MissionCompositionError::AirframeVisual(reason);
    let root = nodes
        .roots()
        .find(|node| node.name == root_name)
        .ok_or_else(|| refuse(format!("no airframe root named {root_name}")))?;
    let mut found = Vec::new();
    let mut stack = vec![root.index];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(index) = stack.pop() {
        if !seen.insert(index) {
            continue;
        }
        let Some(node) = nodes.get(index) else {
            continue;
        };
        if node.name == COCKPIT_EYE_NODE_NAME {
            found.push(index);
        }
        stack.extend(node.children.iter().copied());
    }
    match found.as_slice() {
        [] => Err(refuse(format!(
            "{root_name} holds no {COCKPIT_EYE_NODE_NAME} node"
        ))),
        [only] => nodes
            .get(*only)
            .and_then(|node| node.object3d())
            .map(|data| data.translation)
            .ok_or_else(|| refuse(format!("{COCKPIT_EYE_NODE_NAME} is not an object node"))),
        many => Err(refuse(format!(
            "{root_name} holds {} {COCKPIT_EYE_NODE_NAME} nodes",
            many.len()
        ))),
    }
}

/// The player airframe's eye, read from its source container.
///
/// # Errors
///
/// As [`find_cockpit_eye`], or when the nose axis would not map.
pub fn resolve_player_cockpit_eye(
    source: &PlayerAirframeSource,
) -> Result<MissionCockpitEye, MissionCompositionError> {
    let stored = find_cockpit_eye(
        source.sources().aircraft().nodes(),
        MISSION_PLAYER_AIRCRAFT_ROOT_NAME,
    )?;
    let nose = nose_mapping(STORED_AIRCRAFT_NOSE_AXIS).map_err(|error| {
        MissionCompositionError::AirframeVisual(format!(
            "the measured nose axis would not map: {error}"
        ))
    })?;
    Ok(MissionCockpitEye::new(stored, nose))
}

/// Installs the cockpit camera after the launch camera's slot: it runs in
/// `PostUpdate` before transform propagation.
pub fn install_cockpit_camera(app: &mut App) {
    app.init_resource::<MissionViewMode>();
    app.add_systems(
        PostUpdate,
        drive_cockpit_camera.before(TransformSystems::Propagate),
    );
}

/// Places the camera at the eye and hides the body while a pilot-eye view is
/// on; re-seats the chase rig on the frame the view leaves it.
pub fn drive_cockpit_camera(
    view: Res<MissionViewMode>,
    eye: Option<Res<MissionCockpitEye>>,
    cinematic: Option<Res<LaunchCinematic>>,
    camera: Option<ResMut<PlaytestCamera>>,
    bodies: Query<(&Position, &Rotation, &mut Visibility), With<MissionPlayerBody>>,
    mut cameras: Query<&mut Transform, With<PlaytestCameraMarker>>,
    mut in_cockpit: Local<bool>,
) {
    let mut bodies = bodies;
    // The launch cinematic owns the camera while it plays.
    let cinematic_plays = cinematic.is_some_and(|phase| phase.is_playing());
    let active = view.at_pilot_eye() && eye.is_some() && !cinematic_plays;
    let wanted = if active {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for (position, rotation, mut visibility) in &mut bodies {
        if *visibility != wanted {
            *visibility = wanted;
        }
        if let (true, Some(eye)) = (active, eye.as_deref()) {
            let pose = eye.pose(position.0, rotation.0);
            for mut transform in &mut cameras {
                *transform = pose;
            }
        }
    }
    if *in_cockpit
        && !active
        && let Some(mut camera) = camera
    {
        camera.reseat();
    }
    *in_cockpit = active;
}
