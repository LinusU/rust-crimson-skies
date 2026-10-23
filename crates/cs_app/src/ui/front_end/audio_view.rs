//! The windowed drawing of the Audio page (task #1996): the labels, the three
//! sliders, the drop-down and the focus frame over the `Audio` section's panes
//! and buttons.
//!
//! Text uses the engine's default face at the size the `[FONT]` tag names
//! (`COUR12B` → 12 px). The original's faces (`COUR12B.ttf`, `TREB9B.ttf`)
//! are commercial fonts not in the installation's public data, so this is a
//! stand-in the page's docs name; the strings, colours, boxes and behaviour
//! are the layout's and the script's. The drop-down's ground, frame and
//! highlight colours (`ground`/`SELECTED` below) are unread — the original
//! draws them in a control the executable owns — and are designed stand-ins,
//! like the pilot list's.

use std::collections::BTreeMap;

use bevy::asset::Handle;
use bevy::prelude::{
    Color, Entity, Image, Sprite, Text2d, TextColor, TextFont, Transform, Vec2, Vec3, World,
    default,
};
use bevy::sprite::Anchor;
use bevy::text::{FontSize, Justify, TextBounds, TextLayout};

use cs_content::ui_layout::{AspectFit, Rect};

use super::audio_page::{AudioControl, AudioPage};
use super::shell::{SECTION_SURFACE, ShellRect};

/// Layer of the page's overlay: over the section's panes (1.x), under its
/// buttons (10.x).
const Z: f32 = 5.0;
/// The open list sits over the buttons.
const LIST_Z: f32 = 20.0;
/// The face's size when the `[FONT]` tag names none.
const DEFAULT_SIZE: f32 = 12.0;
/// The drop-down's ground and frame: unread, designed.
const GROUND: u32 = 0xFF00_0000;
/// The highlighted item and the focus frame: the colours `CAMPAIGN.SCRIPT`
/// draws its list's selection and hover with, borrowed for the same job.
const SELECTED: u32 = 0xFF80_0000;
const FRAME: u32 = 0xFFFF_0000;

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

    /// An art image (whole) at a logical top-left.
    fn image(&mut self, image: &Handle<Image>, at: (i32, i32), extent: (u32, u32), z: f32) {
        let logical = ShellRect {
            x: at.0,
            y: at.1,
            width: extent.0,
            height: extent.1,
        };
        let Some(rect) = mapped(&self.fit, logical) else {
            return;
        };
        let entity = self
            .world
            .spawn((
                Sprite {
                    image: image.clone(),
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
        let scale = self.scale;
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
            TextBounds::new_horizontal(rect.width as f32 * scale)
        };
        let entity = self
            .world
            .spawn((
                Text2d::new(label),
                TextFont {
                    font_size: FontSize::Px(size * scale),
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
/// presenter's decoded art by lower-cased name.
pub(super) fn spawn_page(
    world: &mut World,
    page: &AudioPage,
    art: &BTreeMap<String, Handle<Image>>,
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
        let size = label.size.map_or(DEFAULT_SIZE, |size| size as f32);
        canvas.text(
            label.rect,
            &label.text,
            size,
            argb(label.color),
            label.justify,
            Z,
        );
    }
    for (control, slider) in [
        AudioControl::MusicVolume,
        AudioControl::EffectsVolume,
        AudioControl::VoiceVolume,
    ]
    .into_iter()
    .filter_map(|control| page.slider_of(control).map(|slider| (control, slider)))
    {
        if let Some(slot) = art.get(&slider.slot_art.to_ascii_lowercase()) {
            canvas.image(slot, (slider.x, slider.y), slider.slot, Z + 0.1);
        }
        let value = page.volume(control).unwrap_or(slider.initial);
        if let Some(knob) = art.get(&slider.knob_art.to_ascii_lowercase()) {
            canvas.image(knob, slider.knob_at(value), slider.knob, Z + 0.2);
        }
        if page.focus() == Some(control) {
            canvas.frame(slider.hit_rect(), argb(FRAME), Z + 0.3);
        }
    }
    let dropdown = page.dropdown();
    if !page.items().is_empty() {
        canvas.block(dropdown.rect, argb(GROUND), Z + 0.1);
        let current = page
            .items()
            .iter()
            .find(|(quality, _)| *quality == page.staged().quality)
            .map(|(_, label)| label.clone());
        let tint = page
            .labels()
            .first()
            .map_or(Color::WHITE, |label| argb(label.color));
        if let Some(current) = current {
            canvas.text(
                ShellRect {
                    x: dropdown.rect.x + 4,
                    y: dropdown.rect.y + 2,
                    width: dropdown.rect.width.saturating_sub(8),
                    height: dropdown.rect.height,
                },
                &current,
                DEFAULT_SIZE,
                tint,
                0,
                Z + 0.2,
            );
        }
        if let Some(arrow) = art.get(&dropdown.arrow_art.to_ascii_lowercase()) {
            // The arrow's extent is the art's; the box's height is the
            // measure available here, so it sits on the right edge, square.
            let side = dropdown.rect.height;
            canvas.image(
                arrow,
                (
                    dropdown.rect.x + dropdown.rect.width as i32 - side as i32,
                    dropdown.rect.y,
                ),
                (side, side),
                Z + 0.2,
            );
        }
        if page.focus() == Some(AudioControl::Quality) {
            canvas.frame(dropdown.rect, argb(FRAME), Z + 0.3);
        }
        if page.is_open() {
            for (index, (quality, label)) in page.items().iter().enumerate() {
                let row = page.item_rect(index);
                canvas.block(row, argb(GROUND), LIST_Z);
                if *quality == page.staged().quality {
                    canvas.block(row, argb(SELECTED), LIST_Z + 0.1);
                }
                canvas.text(
                    ShellRect {
                        x: row.x + 4,
                        y: row.y + 2,
                        width: row.width.saturating_sub(8),
                        height: row.height,
                    },
                    label,
                    DEFAULT_SIZE,
                    tint,
                    0,
                    LIST_Z + 0.2,
                );
            }
            canvas.frame(
                ShellRect {
                    x: dropdown.rect.x,
                    y: dropdown.rect.y + dropdown.rect.height as i32,
                    width: dropdown.rect.width,
                    height: dropdown.rect.height * page.items().len() as u32,
                },
                argb(FRAME),
                LIST_Z + 0.3,
            );
        }
    }
    canvas.out
}
