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
    Added, AlignItems, App, Assets, BackgroundColor, Camera3d, ChildOf, ClearColor, Color,
    Commands, Component, Cuboid, DirectionalLight, Entity, GlobalAmbientLight, JustifyContent,
    Local, MeshMaterial3d, Node, Plugin, PositionType, Query, Res, ResMut, StandardMaterial,
    Startup, Text, TextColor, TextFont, Transform, UiRect, Update, Val, Visibility, Window, With,
    default,
};
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
                ),
            );
    }
}

#[derive(Component)]
struct HudStatus;

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
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: Val::Px(6.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        })
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
    ));
}

fn attach_camera(mut commands: Commands, cameras: Query<Entity, Added<PlaytestCameraMarker>>) {
    for entity in &cameras {
        commands.entity(entity).insert((
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                near: 0.5,
                far: FAR_PLANE_M,
                ..PerspectiveProjection::default()
            }),
        ));
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
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for entity in &aircraft {
        // The body has no mesh of its own; its parts inherit its visibility.
        commands.entity(entity).insert(Visibility::Inherited);
        if retail.is_some() {
            // The original airframe's parts are children of the body from the
            // moment the scene spawns it (`scene::spawn_aircraft`), so the
            // headless and windowed apps hold the same entities.
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
