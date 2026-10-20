//! The spyglass: the second camera behind the target indicator's round
//! window (PARITY2-M01-SPYGLASS, #1734).
//!
//! In the original the "circle with a camera view" beside the selected
//! target's label is a picture-in-picture window fed by a second camera that
//! frames the selected target. What was read statically from the engine image
//! (`0x49d940`, the per-frame update; owner statics of 2026-10-20, not
//! `verified_original`) and is implemented here:
//!
//! * the second camera sits at the **player's** position and looks at the
//!   target (pose partly inferred: the look-at construction was not traced,
//!   `docs/findings/2026-10-20-m01-spyglass.md`);
//! * its field of view frames the target's bounding sphere:
//!   `2 * atan(1.1 * radius / distance)`, clamped to `[1.5°, 90°]`, the radius
//!   smoothed so it never grows past its previous value; `3°` with no target;
//! * the window exists only while the target is off screen and within
//!   [`super::actor_census::spyglass_window_threshold`] metres; the camera is
//!   released (here: deactivated) otherwise;
//! * the window's scale comes from a size mode in `{0, 2, 3}` and a global
//!   flag ([`window_scale`]);
//! * `Shift+S` (`MSG_CAM2_TOG`, command 48, packed code `0x41f`) toggles it.
//!
//! Not measured, and so **designed** here: the render-target resolution
//! ([`SPYGLASS_TEXTURE_PX`]), the bounding radius of a target whose mesh has
//! no box ([`FALLBACK_RADIUS_M`]), what changes the size mode, and the
//! transition animation.

use bevy::camera::primitives::Aabb;
use bevy::camera::{RenderTarget, visibility::RenderLayers};
use bevy::image::Image;
use bevy::prelude::{
    App, Assets, ButtonInput, Camera, Camera3d, Children, ClearColorConfig, Commands, Component,
    DistanceFog, Entity, GlobalTransform, Handle, IntoScheduleConfigs, KeyCode, Msaa,
    PerspectiveProjection, Plugin, Projection, Query, Res, ResMut, Resource, Startup, Transform,
    Update, Vec3, With, Without, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

use cs_sim::damage::ActorId;

use crate::mission_actors::{MissionGroundTarget, MissionZeppelinActor, ScriptedAircraftActor};
use crate::targeting::TargetingSession;

use super::compose::MissionPlayerBody;

/// Side of the square render target, texels (designed: the original's texture
/// size is unmeasured; the window is ~100 px across on a 1024×768 screen).
pub const SPYGLASS_TEXTURE_PX: u32 = 256;

/// The measured field-of-view floor, radians (`0x6082e4`).
pub const MIN_FOV: f32 = 0.02618;
/// The measured field-of-view ceiling, radians (π/2).
pub const MAX_FOV: f32 = std::f32::consts::FRAC_PI_2;
/// The measured field of view with no target, radians (`0x3d567750`).
pub const NO_TARGET_FOV: f32 = 0.05236;
/// The measured margin on the bounding radius (`0x6082e8`).
pub const RADIUS_MARGIN: f32 = 1.1;
/// The bounding radius used when the target's entity carries no mesh box,
/// metres (designed).
pub const FALLBACK_RADIUS_M: f32 = 30.0;

/// How far the second camera is stepped from the player's position toward
/// the target, metres (designed: clears the player's own airframe).
pub const EYE_CLEARANCE_M: f32 = 12.0;

/// The measured field of view framing a bounding sphere of `radius` metres at
/// `distance` metres.
#[must_use]
pub fn spyglass_fov(radius: f32, distance: f32) -> f32 {
    (2.0 * (RADIUS_MARGIN * radius / distance.max(1.0)).atan()).clamp(MIN_FOV, MAX_FOV)
}

/// The window's scale from the size mode (`0`, `2` or `3`) and the global
/// flag (`0x440840`–`0x4408fc`): mode 0 → 1.0 / 0.85, mode 2 → 0.75 / 0.6,
/// mode 3 → 0.5 / 0.4. Other modes are unmeasured and read as mode 0.
#[must_use]
pub fn window_scale(mode: u8, flag: bool) -> f32 {
    match (mode, flag) {
        (2, false) => 0.75,
        (2, true) => 0.6,
        (3, false) => 0.5,
        (3, true) => 0.4,
        (_, false) => 1.0,
        (_, true) => 0.85,
    }
}

/// The window's diameter at scale 1.0 on a 768-pixel-high screen (the owner's
/// frames: about 95–100 px).
const WINDOW_DIAMETER_AT_768: f32 = 100.0;

/// The window's diameter in screen pixels for a screen `height` pixels high.
#[must_use]
pub fn window_diameter_px(scale: f32, height: f32) -> f32 {
    WINDOW_DIAMETER_AT_768 * scale * height / 768.0
}

/// The spyglass object (the original's single global at `0x71d284`).
#[derive(Resource, Clone, Debug)]
pub struct SpyglassState {
    /// The user-visible show/hide toggle (`+0xe`), flipped by `Shift+S`.
    pub enabled: bool,
    /// The window currently exists: the target is off screen and in range
    /// (`+0xc` active with a camera, `+0x20c`). Written by the target
    /// indicator each frame.
    pub window_open: bool,
    /// The size mode (`+0x24c`): `0`, `2` or `3`.
    pub size_mode: u8,
    /// The global flag (`0x64f6a8`) that picks the smaller scale of the mode.
    pub size_flag: bool,
    /// The render target the second camera draws into.
    pub image: Handle<Image>,
    /// The smoothed target radius and the target it belongs to (`+0x210`).
    radius: Option<(ActorId, f32)>,
}

impl SpyglassState {
    /// The window's scale for the current mode and flag.
    #[must_use]
    pub fn scale(&self) -> f32 {
        window_scale(self.size_mode, self.size_flag)
    }

    /// Whether the window should be drawn this frame.
    #[must_use]
    pub fn shown(&self) -> bool {
        self.enabled && self.window_open
    }
}

/// Marks the spyglass's second camera.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct SpyglassCamera;

/// Creates the render target, the second camera (inactive until a window
/// opens) and the state resource.
fn spawn_spyglass(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut target = Image::new_fill(
        Extent3d {
            width: SPYGLASS_TEXTURE_PX,
            height: SPYGLASS_TEXTURE_PX,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    );
    target.texture_descriptor.usage =
        TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(target);
    commands.spawn((
        Camera3d::default(),
        Camera {
            // Before the scene camera, so the window's texture is complete
            // when the UI pass samples it.
            order: -1,
            is_active: false,
            clear_color: ClearColorConfig::Default,
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        Projection::Perspective(PerspectiveProjection {
            fov: NO_TARGET_FOV,
            aspect_ratio: 1.0,
            near: 1.0,
            far: 40_000.0,
            ..PerspectiveProjection::default()
        }),
        Msaa::Off,
        RenderLayers::layer(0),
        Transform::default(),
        SpyglassCamera,
        // Not the scene's eye: the world-follow systems read the one camera
        // without this marker.
        crate::render::OverlayCamera,
    ));
    commands.insert_resource(SpyglassState {
        enabled: true,
        window_open: false,
        size_mode: 0,
        size_flag: false,
        image,
        radius: None,
    });
}

/// `Shift+S` toggles the window (`MSG_CAM2_TOG`, packed code `0x41f`). The
/// original matches the modifier exactly, so `Ctrl+Shift+S` does nothing.
fn toggle_spyglass(keys: Option<Res<ButtonInput<KeyCode>>>, state: Option<ResMut<SpyglassState>>) {
    let (Some(keys), Some(mut state)) = (keys, state) else {
        return;
    };
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if shift && !ctrl && keys.just_pressed(KeyCode::KeyS) {
        state.enabled = !state.enabled;
    }
}

/// The bounding radius of the entity's drawn meshes around its origin.
fn bounding_radius(
    root: Entity,
    children: &Query<&Children>,
    boxes: &Query<(&Aabb, &GlobalTransform)>,
    origin: Vec3,
) -> Option<f32> {
    let mut radius: Option<f32> = None;
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if let Ok((aabb, transform)) = boxes.get(entity) {
            let center = transform.transform_point(Vec3::from(aabb.center));
            let extent = Vec3::from(aabb.half_extents) * transform.scale();
            let reach = center.distance(origin) + extent.length();
            radius = Some(radius.map_or(reach, |r| r.max(reach)));
        }
        if let Ok(kids) = children.get(entity) {
            stack.extend(kids.iter());
        }
    }
    radius
}

/// Aims the second camera: at the player's position, looking at the selected
/// target, with the measured field of view; active only while the window is
/// shown.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drive_spyglass_camera(
    mut commands: Commands,
    state: Option<ResMut<SpyglassState>>,
    targeting: Option<Res<TargetingSession>>,
    players: Query<&GlobalTransform, With<MissionPlayerBody>>,
    aircraft: Query<(Entity, &ScriptedAircraftActor)>,
    zeppelins: Query<(Entity, &MissionZeppelinActor)>,
    grounds: Query<(Entity, &MissionGroundTarget)>,
    children: Query<&Children>,
    boxes: Query<(&Aabb, &GlobalTransform)>,
    fogs: Query<&DistanceFog, (With<Camera3d>, Without<SpyglassCamera>)>,
    mut cameras: Query<
        (
            Entity,
            &mut Camera,
            &mut Transform,
            &mut Projection,
            Option<&DistanceFog>,
        ),
        With<SpyglassCamera>,
    >,
) {
    let (Some(mut state), Ok((entity, mut camera, mut transform, mut projection, fog))) =
        (state, cameras.single_mut())
    else {
        return;
    };
    if fog.is_none()
        && let Some(scene_fog) = fogs.iter().next()
    {
        commands.entity(entity).insert(scene_fog.clone());
    }
    let reticle = targeting
        .as_deref()
        .and_then(TargetingSession::last_phase)
        .and_then(|phase| phase.reticle.clone());
    let (Some(reticle), Ok(player)) = (reticle, players.single()) else {
        camera.is_active = false;
        state.radius = None;
        return;
    };
    camera.is_active = state.shown();
    if !camera.is_active {
        return;
    }
    let eye = player.translation();
    let target = Vec3::new(
        reticle.position.x() as f32,
        reticle.position.y() as f32,
        reticle.position.z() as f32,
    );
    let owner = aircraft
        .iter()
        .find(|(_, actor)| actor.damage_actor == reticle.target)
        .map(|(e, _)| e)
        .or_else(|| {
            zeppelins
                .iter()
                .find(|(_, actor)| actor.damage_actor == reticle.target)
                .map(|(e, _)| e)
        })
        .or_else(|| {
            grounds
                .iter()
                .find(|(_, actor)| actor.damage_actor == reticle.target)
                .map(|(e, _)| e)
        });
    let measured = owner
        .and_then(|e| bounding_radius(e, &children, &boxes, target))
        .unwrap_or(FALLBACK_RADIUS_M);
    // The radius never exceeds its previous value for the same target.
    let radius = match state.radius {
        Some((id, previous)) if id == reticle.target => measured.min(previous),
        _ => measured,
    };
    state.radius = Some((reticle.target, radius));
    let distance = eye.distance(target);
    if distance < 1.0e-3 {
        return;
    }
    // The eye sits at the player's position; it is stepped past the player's
    // own airframe (designed, `EYE_CLEARANCE_M`) so the picture is the
    // target and not the back of the player's aircraft.
    let eye = eye + (target - eye).normalize() * EYE_CLEARANCE_M.min(distance * 0.5);
    *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    if let Projection::Perspective(perspective) = &mut *projection {
        perspective.fov = spyglass_fov(radius, distance);
    }
}

/// Installs the spyglass: the second camera, the `Shift+S` toggle and the
/// per-frame aim. The window itself is drawn by the target indicator.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpyglassPlugin;

impl Plugin for SpyglassPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_spyglass)
            .add_systems(Update, (toggle_spyglass, drive_spyglass_camera).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_parity2_m01_spyglass_fov_frames_the_bounding_sphere() {
        // 1.1 * r / d = 1 -> 2 * atan(1) = 90 degrees (the ceiling).
        assert!((spyglass_fov(100.0, 110.0) - MAX_FOV).abs() < 1.0e-4);
        // A small distant sphere hits the floor.
        assert!((spyglass_fov(1.0, 10_000.0) - MIN_FOV).abs() < 1.0e-6);
        // In between it is the telephoto formula.
        let fov = spyglass_fov(20.0, 1000.0);
        assert!((fov - 2.0 * (0.022_f32).atan()).abs() < 1.0e-5);
        // Nearer is wider.
        assert!(spyglass_fov(20.0, 500.0) > fov);
    }

    #[test]
    fn accept_parity2_m01_spyglass_size_table_is_the_measured_one() {
        assert_eq!(window_scale(0, false), 1.0);
        assert_eq!(window_scale(0, true), 0.85);
        assert_eq!(window_scale(2, false), 0.75);
        assert_eq!(window_scale(2, true), 0.6);
        assert_eq!(window_scale(3, false), 0.5);
        assert_eq!(window_scale(3, true), 0.4);
        // About 100 px at 1024x768, scaled with the screen height.
        assert_eq!(window_diameter_px(1.0, 768.0), 100.0);
        assert_eq!(window_diameter_px(0.5, 1536.0), 100.0);
    }

    #[test]
    fn accept_parity2_m01_spyglass_shift_s_toggles_the_window() {
        let mut app = App::new();
        app.insert_resource(ButtonInput::<KeyCode>::default());
        app.insert_resource(SpyglassState {
            enabled: true,
            window_open: true,
            size_mode: 0,
            size_flag: false,
            image: Handle::default(),
            radius: None,
        });
        app.add_systems(Update, toggle_spyglass);
        let press = |app: &mut App, codes: &[KeyCode]| {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release_all();
            keys.clear();
            for code in codes {
                keys.press(*code);
            }
            app.update();
        };
        // Plain S is the pitch key, not the toggle.
        press(&mut app, &[KeyCode::KeyS]);
        assert!(app.world().resource::<SpyglassState>().enabled);
        press(&mut app, &[KeyCode::ShiftLeft, KeyCode::KeyS]);
        let state = app.world().resource::<SpyglassState>();
        assert!(!state.enabled);
        assert!(!state.shown());
        // The original matches modifiers exactly.
        press(
            &mut app,
            &[KeyCode::ShiftLeft, KeyCode::ControlLeft, KeyCode::KeyS],
        );
        assert!(!app.world().resource::<SpyglassState>().enabled);
        press(&mut app, &[KeyCode::ShiftRight, KeyCode::KeyS]);
        assert!(app.world().resource::<SpyglassState>().enabled);
    }
}
