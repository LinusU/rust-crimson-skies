//! The windowed drawing of the Plane Construction hub (task #2042): the `T`
//! texts, the tabs' captions, the plane-name edit box and the marked
//! not-built panel line over the `PlaneConstruction` section's panes and
//! buttons.
//!
//! Text uses the engine's default face at the size the `[FONT]` tag names; the
//! original's faces are not in the installation's public data, so this is a
//! stand-in. The colours and geometry are the layout's.

use bevy::prelude::{Entity, World};

use cs_content::ui_layout::AspectFit;

use super::construction_hub::HubPage;
use super::previous_missions_view::{CAPTION_Z, Canvas, DEFAULT_SIZE, Z, argb};
use super::shell::{SECTION_SURFACE, ShellRect};

/// The marked line that stands in for a panel nobody has built.
const PANEL_NOTE_COLOUR: u32 = 0xFFFF_FF00;
/// The cancel prompt's colour.
const PROMPT_COLOUR: u32 = 0xFFFF_FFFF;
/// Where the panel area is: the line sits in the middle of it.
const PANEL_AREA: ShellRect = ShellRect {
    x: 0,
    y: 230,
    width: 800,
    height: 24,
};
/// The cancel prompt's box, over the panel area.
const PROMPT_AREA: ShellRect = ShellRect {
    x: 100,
    y: 280,
    width: 600,
    height: 40,
};
const PANEL_NOTE_SIZE: f32 = 14.0;

/// Spawns the page's overlay and returns its entities. `buttons` are the
/// section's buttons' rects by upper-cased id, which the captions sit
/// centred on.
pub(super) fn spawn_page(
    world: &mut World,
    page: &HubPage,
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
    for text in page.texts() {
        canvas.text(
            text.rect,
            &text.text,
            text.size.map_or(DEFAULT_SIZE, |size| size as f32),
            argb(text.color),
            text.justify,
            Z + 0.5,
        );
    }
    for tab in page.tabs() {
        let Some((_, rect)) = buttons.iter().find(|(id, _)| *id == tab.button) else {
            continue;
        };
        let size = tab.size.map_or(DEFAULT_SIZE, |size| size as f32);
        // The selected tab is held down: the strip's depressed colour.
        let selected = tab.button.eq_ignore_ascii_case(page.selected().button());
        let color = tab.colors[if selected { 3 } else { 1 }];
        let line = ShellRect {
            x: rect.x,
            y: rect.y + (rect.height as f32 * 0.5 - size * 0.6) as i32,
            width: rect.width,
            height: rect.height,
        };
        canvas.text(line, &tab.text, size, argb(color), 1, CAPTION_Z);
    }
    let edit = page.edit();
    if let Some(frame) = edit.frame_color {
        canvas.frame(edit.rect, argb(frame), Z + 0.4);
    }
    canvas.text(
        edit.rect,
        &format!("{}|", page.text()),
        edit.rect.height as f32,
        argb(edit.text_color),
        0,
        Z + 0.5,
    );
    if let Some(prompt) = page.prompt() {
        canvas.text(
            PROMPT_AREA,
            prompt,
            PANEL_NOTE_SIZE,
            argb(PROMPT_COLOUR),
            1,
            Z + 0.6,
        );
        canvas.text(
            ShellRect {
                y: PROMPT_AREA.y + 24,
                ..PROMPT_AREA
            },
            "Y / Enter: cancel the build     N / Esc: keep building",
            DEFAULT_SIZE + 2.0,
            argb(PANEL_NOTE_COLOUR),
            1,
            Z + 0.6,
        );
    } else {
        let (notice, task) = page.selected().not_built();
        canvas.text(
            PANEL_AREA,
            &format!("Not implemented: {} (task #{task})", notice.what),
            PANEL_NOTE_SIZE,
            argb(PANEL_NOTE_COLOUR),
            1,
            Z + 0.6,
        );
    }
    canvas.out
}
