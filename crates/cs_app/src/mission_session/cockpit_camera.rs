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
//! field of view is measured ([`cockpit_vertical_fov`]): the original's 60
//! degrees is horizontal and its vertical angle follows from the fixed 4:3
//! screen factor. The stored-unit scale stays designed. See
//! `docs/findings/2026-10-21-parity3-cockpit-view-02-eye-camera.md` and
//! `docs/findings/2026-10-24-parity3-cockpit-fov.md`.
//!
//! The exterior airframe is hidden in these views by hiding the body (its
//! parts inherit); the chase rig keeps resolving underneath and is re-seated
//! on the frame the view leaves the cockpit, so it snaps instead of swinging.

use avian3d::prelude::{Position, Rotation};
use bevy::app::{App, PostUpdate};
use bevy::camera::Projection;
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

/// The horizontal angle `0x42b660` returns for the cockpit modes, degrees
/// (`fov.default`, VA `0x0060409c`).
pub const COCKPIT_HORIZONTAL_FOV_DEGREES: f32 = 60.0;

/// The screen factor `[0x71b3f8]`: `1.0 / (4/3)`, written once at start-up
/// from two image constants (`fov.screen_factor_*`, VA `0x004603f0`), so the
/// vertical angle does not depend on the window.
pub const COCKPIT_SCREEN_FACTOR: f32 = 0.75;

/// The vertical field of view in radians for a horizontal angle `horizontal`
/// (radians): `2 * atan(factor * tan(horizontal / 2))`, as `0x42b570`
/// derives it (`fov.vertical_derivation`).
#[must_use]
pub fn cockpit_vertical_fov(horizontal: f32) -> f32 {
    2.0 * (COCKPIT_SCREEN_FACTOR * (horizontal * 0.5).tan()).atan()
}

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
    mut cameras: Query<(&mut Transform, Option<&mut Projection>), With<PlaytestCameraMarker>>,
    mut state: Local<(bool, Option<f32>)>,
) {
    let (in_cockpit, chase_fov) = &mut *state;
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
            let fov = cockpit_vertical_fov(COCKPIT_HORIZONTAL_FOV_DEGREES.to_radians());
            for (mut transform, projection) in &mut cameras {
                *transform = pose;
                if let Some(mut projection) = projection
                    && let Projection::Perspective(perspective) = &mut *projection
                    && perspective.fov != fov
                {
                    chase_fov.get_or_insert(perspective.fov);
                    perspective.fov = fov;
                }
            }
        }
    }
    if !active && let Some(fov) = chase_fov.take() {
        for (_, projection) in &mut cameras {
            if let Some(mut projection) = projection
                && let Projection::Perspective(perspective) = &mut *projection
            {
                perspective.fov = fov;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playtest::PlaytestCamera;
    use bevy::app::Update;
    use bevy::camera::PerspectiveProjection;
    use bevy::prelude::Quat;

    #[test]
    fn accept_parity3_cockpit_fov_vertical_follows_the_fixed_four_by_three_factor() {
        let vertical = cockpit_vertical_fov(COCKPIT_HORIZONTAL_FOV_DEGREES.to_radians());
        assert!((vertical.to_degrees() - 46.827).abs() < 1e-2, "{vertical}");
        // The factor is 1.0 / (4/3), the two constants the writer divides.
        assert!((COCKPIT_SCREEN_FACTOR - 1.0 / (4.0_f32 / 3.0)).abs() < 1e-6);
    }

    #[test]
    fn accept_parity3_cockpit_fov_is_applied_in_the_cockpit_and_restored_in_chase() {
        let mut app = App::new();
        app.insert_resource(MissionViewMode::default());
        app.insert_resource(PlaytestCamera::capture_fitted());
        app.insert_resource(MissionCockpitEye::new([0.0, 0.75, -0.2], Quat::IDENTITY));
        app.add_systems(Update, drive_cockpit_camera);
        app.world_mut().spawn((
            MissionPlayerBody,
            Position(Vec3::ZERO),
            Rotation(Quat::IDENTITY),
            Visibility::Inherited,
        ));
        let chase = 0.9;
        let camera = app
            .world_mut()
            .spawn((
                PlaytestCameraMarker,
                Transform::IDENTITY,
                Projection::Perspective(PerspectiveProjection {
                    fov: chase,
                    ..PerspectiveProjection::default()
                }),
            ))
            .id();
        let fov = |app: &App| match app.world().get::<Projection>(camera) {
            Some(Projection::Perspective(p)) => p.fov,
            _ => panic!("perspective camera"),
        };
        app.update();
        assert_eq!(fov(&app), chase, "chase view keeps its projection");
        let mut view = MissionViewMode::default();
        while !view.at_pilot_eye() {
            view = view.next();
        }
        app.insert_resource(view);
        app.update();
        let expected = cockpit_vertical_fov(60.0_f32.to_radians());
        assert!((fov(&app) - expected).abs() < 1e-6);
        app.insert_resource(MissionViewMode::default());
        app.update();
        assert_eq!(
            fov(&app),
            chase,
            "leaving the cockpit restores the chase fov"
        );
    }
}
