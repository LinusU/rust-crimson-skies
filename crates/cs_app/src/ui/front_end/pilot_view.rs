//! The windowed drawing of the pilot page (FE-MENU-3): sprites and text over
//! the `Campaign` section's panes and buttons.
//!
//! Text uses the engine's default face at a size derived from the row
//! height. The original's `FONT_CM_EDIT` is a GDI font id whose face is not
//! in the installation, so this is a stand-in the page's docs name; the
//! colours, geometry and behaviour are the layout's and the script's.

use bevy::prelude::{
    Color, Entity, Sprite, Text2d, TextColor, TextFont, Transform, Vec2, Vec3, World, default,
};
use bevy::sprite::Anchor;
use bevy::text::FontSize;

use cs_content::ui_layout::{AspectFit, Rect};

use super::pilot_page::{HOVER_FRAME, PilotPage, ROW_TEXT, SELECTED_FILL};
use super::shell::{SECTION_SURFACE, ShellRect};

/// Layer of the page's overlay: over the section's panes (1.x), under its
/// buttons (10.x).
const Z: f32 = 5.0;

fn argb(value: u32) -> Color {
    Color::srgba_u8(
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
        (value >> 24) as u8,
    )
}

/// A logical rect on the surface (clipped to the 800x600 image, mapped
/// through the fit), or `None` when nothing of it is visible.
fn mapped(fit: &AspectFit, rect: ShellRect) -> Option<Rect> {
    rect.clip(SECTION_SURFACE)
        .map(|(visible, _)| fit.map_rect(visible))
}

fn centre(rect: Rect, surface: (u32, u32), z: f32) -> Vec3 {
    Vec3::new(
        rect.x as f32 + rect.width as f32 * 0.5 - surface.0 as f32 * 0.5,
        surface.1 as f32 * 0.5 - (rect.y as f32 + rect.height as f32 * 0.5),
        z,
    )
}

fn block(
    world: &mut World,
    fit: &AspectFit,
    surface: (u32, u32),
    rect: ShellRect,
    color: Color,
    z: f32,
) -> Option<Entity> {
    let rect = mapped(fit, rect)?;
    Some(
        world
            .spawn((
                Sprite {
                    color,
                    custom_size: Some(Vec2::new(rect.width as f32, rect.height as f32)),
                    ..default()
                },
                Transform::from_translation(centre(rect, surface, z)),
            ))
            .id(),
    )
}

/// Left-aligned text vertically centred in `rect`.
fn text(
    world: &mut World,
    fit: &AspectFit,
    surface: (u32, u32),
    rect: ShellRect,
    label: String,
    color: Color,
    z: f32,
) -> Option<Entity> {
    let mapped = mapped(fit, rect)?;
    let size = (mapped.height as f32 * 0.72).max(8.0);
    let mut at = centre(mapped, surface, z);
    at.x = mapped.x as f32 - surface.0 as f32 * 0.5 + 3.0;
    Some(
        world
            .spawn((
                Text2d::new(label),
                TextFont {
                    font_size: FontSize::Px(size),
                    ..default()
                },
                TextColor(color),
                Anchor::CENTER_LEFT,
                Transform::from_translation(at),
            ))
            .id(),
    )
}

/// Spawns the page's overlay and returns its entities.
pub(super) fn spawn_page(world: &mut World, page: &PilotPage, surface: (u32, u32)) -> Vec<Entity> {
    let Some(fit) = AspectFit::new(SECTION_SURFACE, surface) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let list = page.list();
    // The list's ground, the script's black fill.
    out.extend(block(world, &fit, surface, list.rect, Color::BLACK, Z));
    let first = page.top();
    for (slot, entry) in page
        .entries()
        .iter()
        .enumerate()
        .skip(first)
        .take(list.visible)
    {
        let row = ShellRect {
            x: list.rect.x,
            y: list.rect.y + ((slot - first) as u32 * list.row_height) as i32,
            width: list.rect.width,
            height: list.row_height,
        };
        if page.selected() == Some(slot) {
            out.extend(block(
                world,
                &fit,
                surface,
                row,
                argb(SELECTED_FILL),
                Z + 0.1,
            ));
        }
        if page.hover() == Some(slot) {
            // The script's one-pixel `ldrawframe` in red.
            let edge = |x, y, width, height| ShellRect {
                x,
                y,
                width,
                height,
            };
            for bar in [
                edge(row.x, row.y, row.width, 1),
                edge(row.x, row.y + row.height as i32 - 1, row.width, 1),
                edge(row.x, row.y, 1, row.height),
                edge(row.x + row.width as i32 - 1, row.y, 1, row.height),
            ] {
                out.extend(block(world, &fit, surface, bar, argb(HOVER_FRAME), Z + 0.2));
            }
        }
        out.extend(text(
            world,
            &fit,
            surface,
            row,
            entry.name.clone(),
            argb(ROW_TEXT),
            Z + 0.3,
        ));
    }
    // The edit box: the typed text and a caret in the text's own stand-in.
    let edit = page.edit();
    out.extend(text(
        world,
        &fit,
        surface,
        edit.rect,
        format!("{}|", page.text()),
        argb(edit.text_color),
        Z + 0.3,
    ));
    if let Some(notice) = page.notice() {
        let below = ShellRect {
            x: list.rect.x,
            y: list.rect.y + list.rect.height as i32 + 8,
            width: list.rect.width,
            height: 22,
        };
        out.extend(text(
            world,
            &fit,
            surface,
            below,
            notice.text(),
            Color::srgb(1.0, 0.85, 0.2),
            Z + 0.3,
        ));
    }
    out
}
