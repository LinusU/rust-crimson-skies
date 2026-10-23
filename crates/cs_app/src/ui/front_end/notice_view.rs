//! The marked "not implemented" screen a cabin button opens when its screen
//! has not been built (PARITY5-CABIN-INTRO.05).
//!
//! It is a project notice, not an original screen: a dark panel over the cabin
//! naming the button and saying plainly that the screen does not exist yet.
//! It is deliberately unlike the original's art so it cannot be taken for a
//! recreated screen, and it blocks a release claim for the cabin.

use bevy::prelude::{
    Color, Entity, Sprite, Text2d, TextColor, TextFont, Transform, Vec2, World, default,
};
use bevy::text::FontSize;

use super::start_flow::NotImplemented;

/// Above every section sprite (panes 1.x, buttons 10.x, cursor above that).
const Z: f32 = 50.0;

/// The notice's lines for `entry`, top to bottom.
#[must_use]
pub fn lines(entry: &NotImplemented) -> [String; 3] {
    [
        "NOT IMPLEMENTED".to_owned(),
        format!("{} is not built yet.", entry.what),
        "Esc, Return or a click returns to the cabin.".to_owned(),
    ]
}

/// Spawns the notice over a `surface`-sized window and returns its entities.
pub(super) fn spawn_notice(
    world: &mut World,
    entry: &NotImplemented,
    surface: (u32, u32),
) -> Vec<Entity> {
    let mut spawned = vec![
        world
            .spawn((
                Sprite {
                    color: Color::srgba(0.0, 0.0, 0.0, 0.85),
                    custom_size: Some(Vec2::new(surface.0 as f32, surface.1 as f32)),
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, Z),
            ))
            .id(),
    ];
    let height = surface.1 as f32;
    let rows = [
        (height * 0.07, Color::srgb(1.0, 0.25, 0.2), 0.08),
        (height * 0.04, Color::WHITE, 0.0),
        (height * 0.03, Color::srgb(0.8, 0.8, 0.8), -0.07),
    ];
    for (line, (size, color, up)) in lines(entry).into_iter().zip(rows) {
        spawned.push(
            world
                .spawn((
                    Text2d::new(line),
                    TextFont {
                        font_size: FontSize::Px(size),
                        ..default()
                    },
                    TextColor(color),
                    Transform::from_xyz(0.0, height * up, Z + 0.01),
                ))
                .id(),
        );
    }
    spawned
}
