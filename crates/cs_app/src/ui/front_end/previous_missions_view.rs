//! The windowed drawing of the Previous Missions page (task #1956): the list's
//! rows with their selection and hover, the `T` captions and the buttons'
//! captions over the `ScrapBook_TOC` section's panes and buttons.
//!
//! Text uses the engine's default face at the size the `[FONT]` tag names
//! (`TREB9B` → 9 px); the original's faces are commercial fonts that are not
//! in the installation's public data, so this is a stand-in. The colours,
//! fills, frames and row geometry are the script's.

use std::collections::BTreeMap;

use bevy::asset::Handle;
use bevy::prelude::{
    Color, Entity, Image, Sprite, Text2d, TextColor, TextFont, Transform, Vec2, Vec3, World,
    default,
};
use bevy::sprite::Anchor;
use bevy::text::{FontSize, Justify, TextBounds, TextLayout};

use cs_content::ui_layout::{AspectFit, Rect};

use super::previous_missions::{
    FRAME, HOVER_FILL, ICON_ART, ICON_FRAMES, LINE_STEP, LINE_TOP, PreviousMissionsPage, ROW_TEXT,
    SELECTED_FILL,
};
use super::shell::{SECTION_SURFACE, ShellRect};

/// Layer of the page's overlay: over the section's panes (1.x), under its
/// buttons (10.x).
const Z: f32 = 5.0;
/// The face's size when the `[FONT]` tag names none.
const DEFAULT_SIZE: f32 = 9.0;
/// The buttons' captions sit over the buttons (10.x).
const CAPTION_Z: f32 = 11.0;

fn argb(value: u32) -> Color {
    Color::srgba_u8(
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
        (value >> 24) as u8,
    )
}

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

struct Canvas<'a> {
    world: &'a mut World,
    fit: AspectFit,
    surface: (u32, u32),
    scale: f32,
    out: Vec<Entity>,
}

impl Canvas<'_> {
    fn block(&mut self, rect: ShellRect, color: Color, z: f32) {
        let Some(rect) = mapped(&self.fit, rect) else {
            return;
        };
        let entity = self
            .world
            .spawn((
                Sprite {
                    color,
                    custom_size: Some(Vec2::new(rect.width as f32, rect.height as f32)),
                    ..default()
                },
                Transform::from_translation(centre(rect, self.surface, z)),
            ))
            .id();
        self.out.push(entity);
    }

    fn frame(&mut self, rect: ShellRect, color: Color, z: f32) {
        let edge = |x, y, width, height| ShellRect {
            x,
            y,
            width,
            height,
        };
        for bar in [
            edge(rect.x, rect.y, rect.width, 1),
            edge(rect.x, rect.y + rect.height as i32 - 1, rect.width, 1),
            edge(rect.x, rect.y, 1, rect.height),
            edge(rect.x + rect.width as i32 - 1, rect.y, 1, rect.height),
        ] {
            self.block(bar, color, z);
        }
    }

    /// One frame of a vertical strip at a logical top-left.
    fn strip_frame(
        &mut self,
        image: &Handle<Image>,
        at: (i32, i32),
        frame: (u32, u32, u32),
        z: f32,
    ) {
        let (index, width, height) = frame;
        let logical = ShellRect {
            x: at.0,
            y: at.1,
            width,
            height,
        };
        let Some(rect) = mapped(&self.fit, logical) else {
            return;
        };
        // The presented texture may be an HD replacement: scale the crop.
        let texel = self
            .world
            .get_resource::<bevy::asset::Assets<Image>>()
            .and_then(|images| images.get(image))
            .map_or(1.0, |texture| {
                texture.height() as f32 / (height * ICON_FRAMES) as f32
            });
        let (width_t, height_t) = (width as f32 * texel, height as f32 * texel);
        let top = index as f32 * height_t;
        let entity = self
            .world
            .spawn((
                Sprite {
                    image: image.clone(),
                    rect: Some(bevy::math::Rect::new(0.0, top, width_t, top + height_t)),
                    custom_size: Some(Vec2::new(rect.width as f32, rect.height as f32)),
                    ..default()
                },
                Transform::from_translation(centre(rect, self.surface, z)),
            ))
            .id();
        self.out.push(entity);
    }

    /// Text in a logical box: `justify` 0 left, 1 centre, 2 right; a box
    /// width of 0 means auto-size.
    fn text(
        &mut self,
        rect: ShellRect,
        label: &str,
        size: f32,
        color: Color,
        justify: u32,
        z: f32,
    ) {
        let (anchor, at_x, layout) = match justify {
            1 => (
                Anchor::TOP_CENTER,
                rect.x as f32 + rect.width as f32 * 0.5,
                Justify::Center,
            ),
            2 => (
                Anchor::TOP_RIGHT,
                rect.x as f32 + rect.width as f32,
                Justify::Right,
            ),
            _ => (Anchor::TOP_LEFT, rect.x as f32, Justify::Left),
        };
        let origin = self.fit.map_rect(Rect {
            x: at_x.max(0.0) as u32,
            y: rect.y.max(0) as u32,
            width: 1,
            height: 1,
        });
        let position = Vec3::new(
            origin.x as f32 - self.surface.0 as f32 * 0.5,
            self.surface.1 as f32 * 0.5 - origin.y as f32,
            z,
        );
        let bounds = if rect.width == 0 {
            TextBounds::UNBOUNDED
        } else {
            TextBounds::new_horizontal(rect.width as f32 * self.scale)
        };
        let entity = self
            .world
            .spawn((
                Text2d::new(label),
                TextFont {
                    font_size: FontSize::Px(size * self.scale),
                    ..default()
                },
                TextColor(color),
                TextLayout::justify(layout),
                bounds,
                anchor,
                Transform::from_translation(position),
            ))
            .id();
        self.out.push(entity);
    }
}

/// Spawns the page's overlay and returns its entities. `art` is the
/// presenter's decoded art by lower-cased name; `buttons` the section's
/// buttons' rects by upper-cased id, which the captions sit centred on.
pub(super) fn spawn_page(
    world: &mut World,
    page: &PreviousMissionsPage,
    art: &BTreeMap<String, Handle<Image>>,
    buttons: &[(String, ShellRect)],
    surface: (u32, u32),
) -> Vec<Entity> {
    let Some(fit) = AspectFit::new(SECTION_SURFACE, surface) else {
        return Vec::new();
    };
    let scale = fit.image_rect().width as f32 / SECTION_SURFACE.0 as f32;
    let mut canvas = Canvas {
        world,
        fit,
        surface,
        scale,
        out: Vec::new(),
    };
    for label in page.labels() {
        canvas.text(
            label.rect,
            &label.text,
            label.size.map_or(DEFAULT_SIZE, |size| size as f32),
            argb(label.color),
            label.justify,
            Z + 0.5,
        );
    }
    for caption in page.captions() {
        let Some((_, rect)) = buttons.iter().find(|(id, _)| *id == caption.button) else {
            continue;
        };
        let size = caption.size.map_or(DEFAULT_SIZE, |size| size as f32);
        let line = ShellRect {
            x: rect.x,
            y: rect.y + (rect.height as f32 * 0.5 - size * 0.6) as i32,
            width: rect.width,
            height: rect.height,
        };
        canvas.text(line, &caption.text, size, argb(ROW_TEXT), 1, CAPTION_Z);
    }
    let list = page.list();
    let icons = art.get(ICON_ART);
    let text_x = list.rect.x + page.text_offset() as i32;
    for (slot, row) in page
        .rows()
        .iter()
        .enumerate()
        .skip(page.top())
        .take(list.visible)
    {
        let top = list.rect.y + ((slot - page.top()) as u32 * list.row_height) as i32;
        let bounds = ShellRect {
            x: list.rect.x,
            y: top,
            width: list.rect.width,
            height: list.row_height,
        };
        if page.hover() == Some(slot) {
            canvas.block(bounds, argb(HOVER_FILL), Z + 0.1);
            canvas.frame(bounds, argb(FRAME), Z + 0.2);
        }
        if row.index == page.selected() {
            canvas.block(bounds, argb(SELECTED_FILL), Z + 0.1);
            canvas.frame(bounds, argb(FRAME), Z + 0.2);
        }
        if let (Some(strip), Some(frame), Some((width, height))) =
            (icons, row.icon, page.icon_frame())
        {
            canvas.strip_frame(
                strip,
                (list.rect.x + 2, top),
                (frame.min(ICON_FRAMES - 1), width, height),
                Z + 0.3,
            );
        }
        for (n, text) in row.lines.iter().enumerate() {
            if let Some(text) = text {
                canvas.text(
                    ShellRect {
                        x: text_x,
                        y: top + (LINE_TOP + LINE_STEP * n as u32) as i32,
                        width: list.rect.width.saturating_sub(page.text_offset()),
                        height: LINE_STEP,
                    },
                    text,
                    DEFAULT_SIZE,
                    argb(ROW_TEXT),
                    0,
                    Z + 0.5,
                );
            }
        }
    }
    canvas.out
}
