//! The windowed drawing of the Plane Name page (task #1958): the `T`
//! captions, the buttons' captions and the name's edit box over the
//! `PlaneName` section's panes and buttons.
//!
//! Text uses the engine's default face at the size the `[FONT]` tag names; the
//! original's faces are not in the installation's public data, so this is a
//! stand-in. The colours and geometry are the layout's.

use bevy::prelude::{Entity, World};

use cs_content::ui_layout::AspectFit;

use super::plane_name::PlaneNamePage;
use super::previous_missions_view::{CAPTION_Z, Canvas, DEFAULT_SIZE, Z, argb};
use super::shell::{SECTION_SURFACE, ShellRect};

/// The captions' colour: the layout gives the buttons' text colours as
/// state tints, which this stand-in does not use; black reads on the plates.
const CAPTION_COLOUR: u32 = 0xFF00_0000;
/// The refusal's text.
const NOTICE_COLOUR: u32 = 0xFFFF_FF00;

/// Spawns the page's overlay and returns its entities. `buttons` are the
/// section's buttons' rects by upper-cased id, which the captions sit
/// centred on.
pub(super) fn spawn_page(
    world: &mut World,
    page: &PlaneNamePage,
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
        canvas.text(
            line,
            &caption.text,
            size,
            argb(CAPTION_COLOUR),
            1,
            CAPTION_Z,
        );
    }
    let edit = page.edit();
    canvas.text(
        edit.rect,
        &format!("{}|", page.text()),
        edit.rect.height as f32,
        argb(edit.text_color),
        0,
        Z + 0.5,
    );
    if let Some(notice) = page.notice() {
        let below = ShellRect {
            x: edit.rect.x,
            y: edit.rect.y + edit.rect.height as i32 + 4,
            width: edit.rect.width,
            height: 20,
        };
        canvas.text(below, notice, DEFAULT_SIZE, argb(NOTICE_COLOUR), 0, Z + 0.6);
    }
    canvas.out
}
