//! What the player sees: the synthetic scene's meshes, the lights, the window
//! camera and the on-screen readout. Needs a renderer; the core in
//! [`super`] does not.
//!
//! Everything is attached to the entities the core spawns (`Added<…>` of the
//! scene markers), so a reset's fresh aircraft gets its visual the same frame
//! and the visuals own no game state.

use bevy::camera::{PerspectiveProjection, Projection};
use bevy::mesh::{Mesh, Mesh3d};
use bevy::prelude::{
    Added, AlignItems, App, Assets, BackgroundColor, Bundle, Camera3d, ChildOf, ClearColor, Color,
    Commands, Component, Cuboid, DirectionalLight, Entity, GlobalAmbientLight, JustifyContent,
    Local, MeshMaterial3d, Node, Plugin, PositionType, Query, Res, ResMut, StandardMaterial,
    Startup, Text, TextColor, TextFont, Transform, UiRect, Update, Val, Visibility, Window, With,
    default,
};
use bevy::render::view::Msaa;
use bevy::text::FontSize;
use cs_content::cameras::AspectRatio;

use super::retail::RetailContent;
use super::scene::{PlaytestAircraft, PlaytestGround, PlaytestObstacle};
use super::{PlaytestCamera, PlaytestCameraMarker, PlaytestState};

/// The camera's far plane: the ground slab is 20 km across.
const FAR_PLANE_M: f32 = 40_000.0;

/// The on-screen controls reminder.
pub const CONTROLS_TEXT: &str = "W/S pitch   Q/E roll   A/D yaw   Left Shift / F throttle up / down   1 idle   4 full\n\
L Level-Off (original law)   R reset   Esc pause / resume   F10 quit (or close the window)";

/// Adds the scene meshes, lights, window camera and HUD.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlaytestVisualsPlugin;

impl Plugin for PlaytestVisualsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::srgb(0.45, 0.65, 0.9)))
            .insert_resource(GlobalAmbientLight {
                color: Color::WHITE,
                brightness: 400.0,
                ..default()
            })
            .add_systems(Startup, spawn_lights_and_hud)
            .add_systems(
                Update,
                (
                    attach_camera,
                    attach_ground,
                    attach_obstacle,
                    attach_aircraft,
                    update_aspect,
                    update_hud,
                    apply_overlay_visibility,
                ),
            );
    }
}

#[derive(Component)]
struct HudStatus;

/// Whether the developer overlay (banner, telemetry block, key help, contact
/// list, objective caption and weapon notice) is drawn. The playtest draws it
/// always; the composed mission inserts `DeveloperOverlay(false)` and shows
/// the original-style cockpit HUD instead (#1641), unless the owner passes
/// `--dev-overlay`.
#[derive(bevy::prelude::Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeveloperOverlay(pub bool);

/// Marks a root UI node that belongs to the developer overlay: it is hidden
/// while [`DeveloperOverlay`] is `false`.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct DeveloperOverlayNode;

/// Whether the overlay is on: absent resource means on (the playtest).
#[must_use]
pub fn overlay_visible(world_flag: Option<&DeveloperOverlay>) -> bool {
    world_flag.is_none_or(|flag| flag.0)
}

/// Applies [`DeveloperOverlay`] to every overlay root.
pub fn apply_overlay_visibility(
    flag: Option<Res<DeveloperOverlay>>,
    mut nodes: Query<&mut bevy::prelude::Visibility, With<DeveloperOverlayNode>>,
) {
    // Only the hidden state is forced: while the overlay is on, each widget
    // keeps managing its own visibility (the objective line hides itself when
    // it has no text).
    if overlay_visible(flag.as_deref()) {
        return;
    }
    for mut visibility in &mut nodes {
        if *visibility != bevy::prelude::Visibility::Hidden {
            *visibility = bevy::prelude::Visibility::Hidden;
        }
    }
}

fn spawn_lights_and_hud(mut commands: Commands, state: Res<PlaytestState>) {
    let label = state.label;
    commands.spawn((
        DirectionalLight {
            illuminance: 12_000.0,
            ..default()
        },
        Transform::from_xyz(300.0, 800.0, 200.0)
            .looking_at(bevy::prelude::Vec3::ZERO, bevy::prelude::Vec3::Y),
    ));

    let font = |size: f32| TextFont {
        font_size: FontSize::Px(size),
        ..default()
    };
    // Banner.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(6.0),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            DeveloperOverlayNode,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new(label),
                font(22.0),
                TextColor(Color::srgb(1.0, 0.85, 0.2)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            ));
        });
    // Flight readout.
    commands.spawn((
        Text::new(""),
        font(18.0),
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(44.0),
            left: Val::Px(8.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        HudStatus,
        DeveloperOverlayNode,
    ));
    // Controls.
    commands.spawn((
        Text::new(CONTROLS_TEXT),
        font(16.0),
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(8.0),
            left: Val::Px(8.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        DeveloperOverlayNode,
    ));
}

/// The multisampling the interactive playtest window renders with.
///
/// Decision (#1458): this camera is human-facing, not a comparison capture,
/// so it is **not** pinned to the fidelity baseline (`Msaa::Off`, one sample
/// per pixel) and keeps four-sample antialiasing — a designed improvement
/// (spec F17 non-negotiable 5), never evidence of original-renderer parity.
/// It is stated here rather than left to `CameraPlugin`'s required-component
/// default so that the choice is explicit and a Bevy default change cannot
/// alter it silently. The window camera takes no tone-curve decision: it
/// keeps `Camera3d`'s own `Tonemapping`.
const WINDOW_MSAA: Msaa = Msaa::Sample4;

fn window_camera() -> impl Bundle {
    (
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            near: 0.5,
            far: FAR_PLANE_M,
            ..PerspectiveProjection::default()
        }),
        WINDOW_MSAA,
        // Overlay passes (the mission HUD art camera, order 1) would otherwise
        // become Bevy's default UI camera — the highest-order camera on the
        // primary window — and take the letterbox bars, the flight-check panel
        // and the HUD text down with them whenever the pass is held.
        bevy::ui::IsDefaultUiCamera,
    )
}

fn attach_camera(mut commands: Commands, cameras: Query<Entity, Added<PlaytestCameraMarker>>) {
    for entity in &cameras {
        commands.entity(entity).insert(window_camera());
    }
}

fn attach_ground(
    mut commands: Commands,
    grounds: Query<(Entity, &PlaytestGround), Added<PlaytestGround>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, ground) in &grounds {
        let [hx, hy, hz] = ground.half_extents_m;
        let slab = meshes.add(Cuboid::new(hx * 2.0, hy * 2.0, hz * 2.0));
        let grass = materials.add(StandardMaterial {
            base_color: Color::srgb(0.28, 0.45, 0.22),
            perceptual_roughness: 1.0,
            ..default()
        });
        let line = materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.9, 0.85),
            unlit: true,
            ..default()
        });
        commands
            .entity(entity)
            .insert((Mesh3d(slab), MeshMaterial3d(grass)));
        // Flat reference lines every 500 m so speed and height read against
        // the ground. Visual only: they have no collider.
        let along_x = meshes.add(Cuboid::new(hx * 2.0, 0.2, 4.0));
        let along_z = meshes.add(Cuboid::new(4.0, 0.2, hz * 2.0));
        for step in -20..=20 {
            let offset = step as f32 * 500.0;
            for (mesh, translation) in [
                (&along_x, bevy::prelude::Vec3::new(0.0, hy + 0.1, offset)),
                (&along_z, bevy::prelude::Vec3::new(offset, hy + 0.1, 0.0)),
            ] {
                commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(line.clone()),
                    Transform::from_translation(translation),
                    ChildOf(entity),
                ));
            }
        }
    }
}

fn attach_obstacle(
    mut commands: Commands,
    obstacles: Query<(Entity, &PlaytestObstacle), Added<PlaytestObstacle>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (entity, obstacle) in &obstacles {
        let [hx, hy, hz] = obstacle.half_extents_m;
        let wall = meshes.add(Cuboid::new(hx * 2.0, hy * 2.0, hz * 2.0));
        let paint = materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.35, 0.1),
            perceptual_roughness: 0.8,
            ..default()
        });
        commands
            .entity(entity)
            .insert((Mesh3d(wall), MeshMaterial3d(paint)));
    }
}

fn attach_aircraft(
    mut commands: Commands,
    aircraft: Query<Entity, Added<PlaytestAircraft>>,
    retail: Option<Res<RetailContent>>,
    mission: Option<Res<super::AircraftSpawner>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &aircraft {
        // The body has no mesh of its own; its parts inherit its visibility.
        commands.entity(entity).insert(Visibility::Inherited);
        if retail.is_some() || mission.is_some() {
            // Over original content the airframe's parts are children of the
            // body from the moment the scene spawns it, and over a mission
            // composition the player airframe's visual is a separate stage
            // (#1216) — the playtest's synthetic parts are never drawn over
            // an original mission's body.
            continue;
        }
        let yellow = materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.85, 0.15),
            ..default()
        });
        let red = materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.1, 0.1),
            ..default()
        });
        // Forward is -Z. Fuselage, wing, tailplane and fin: enough to read
        // attitude from behind.
        let parts = [
            (Cuboid::new(1.6, 1.6, 9.0), [0.0, 0.0, 0.0], &yellow),
            (Cuboid::new(12.0, 0.25, 2.4), [0.0, 0.0, -0.5], &red),
            (Cuboid::new(4.5, 0.2, 1.4), [0.0, 0.2, 4.0], &red),
            (Cuboid::new(0.25, 2.2, 1.6), [0.0, 1.2, 4.0], &red),
        ];
        for (shape, [x, y, z], material) in parts {
            commands.spawn((
                Mesh3d(meshes.add(shape)),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(x, y, z),
                ChildOf(entity),
            ));
        }
    }
}

fn update_aspect(windows: Query<&Window>, mut camera: ResMut<PlaytestCamera>) {
    if let Ok(window) = windows.single()
        && window.height() > 0.0
        && let Ok(aspect) = AspectRatio::new(f64::from(window.width() / window.height()))
    {
        camera.aspect = aspect;
    }
}

fn update_hud(
    state: Res<PlaytestState>,
    mut hud: Query<&mut Text, With<HudStatus>>,
    mut last: Local<String>,
) {
    let t = state.telemetry;
    let status = match (state.paused, state.pause_reason) {
        (true, Some(reason)) => format!("PAUSED ({reason}) — Esc to resume"),
        (true, None) => "PAUSED — Esc to resume".to_owned(),
        _ => "FLYING".to_owned(),
    };
    let collision = match state.first_obstacle_contact_tick {
        Some(tick) => format!("COLLISION with obstacle (first at tick {tick})"),
        None if state.ground_contacts > 0 => "COLLISION with ground".to_owned(),
        None => "no collision".to_owned(),
    };
    let text = format!(
        "SPEED {:5.1} m/s   ALT {:6.1} m   HDG {:05.1}   PITCH {:+5.1}   BANK {:+5.1}\n\
         THROTTLE {:3.0}%   {status}\n\
         {collision}   (obstacle {} / ground {})   resets {}",
        t.speed_m_s,
        t.altitude_m,
        t.heading_deg.rem_euclid(360.0),
        t.pitch_deg,
        t.roll_deg,
        state.command.throttle * 100.0,
        state.obstacle_contacts,
        state.ground_contacts,
        state.resets,
    );
    if *last != text {
        for mut node in &mut hud {
            node.0.clone_from(&text);
        }
        *last = text;
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::world::World;

    use super::*;

    /// The window camera's antialiasing is an explicit component, not the
    /// renderer's required-component default.
    #[test]
    fn accept_f17_e_the_playtest_window_camera_pins_its_antialiasing_explicitly() {
        let mut world = World::new();
        let entity = world.spawn(window_camera()).id();
        assert_eq!(world.get::<Msaa>(entity), Some(&Msaa::Sample4));
    }

    /// The developer overlay follows the `DeveloperOverlay` resource: shown by
    /// default (the playtest), hidden when the mission composition says so.
    #[test]
    fn accept_parity_m01_hud_developer_overlay_is_hidden_by_resource() {
        use bevy::prelude::{IntoScheduleConfigs as _, Schedule, Visibility};
        let mut world = World::new();
        let node = world
            .spawn((Visibility::Inherited, DeveloperOverlayNode))
            .id();
        let mut schedule = Schedule::default();
        schedule.add_systems(apply_overlay_visibility.into_configs());
        schedule.run(&mut world);
        assert_eq!(world.get::<Visibility>(node), Some(&Visibility::Inherited));
        world.insert_resource(DeveloperOverlay(false));
        schedule.run(&mut world);
        assert_eq!(world.get::<Visibility>(node), Some(&Visibility::Hidden));
    }
}
